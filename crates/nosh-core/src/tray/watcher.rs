//! The StatusNotifierWatcher role.
//!
//! The try tree is specific first: own the watcher name, proxy another process'
//! watcher as a host, or report no tray. Probing reads the name owner only.

use std::sync::Arc;

use async_channel::{Receiver, Sender};
use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::sync::Mutex;
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::message::Header;
use zbus::names::WellKnownName;
use zbus::object_server::SignalEmitter;
use zbus::{interface, Connection};

use crate::error::BackendError;
use crate::tray::registration::Registration;

pub const WATCHER_SERVICE: &str = "org.kde.StatusNotifierWatcher";
pub const WATCHER_PATH: &str = "/StatusNotifierWatcher";
pub const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";

/// The largest number of registrations kept. A runaway client cannot grow the
/// list without bound.
pub const MAX_REGISTRATIONS: usize = 256;

/// The longest a watcher call may take before the backend gives up.
const WATCHER_CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// One change in the set of registered items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatcherChange {
    Registered(Registration),
    Unregistered(String),
}

/// A running watcher role.
pub struct WatcherSession {
    pub name: &'static str,
    pub connection: Connection,
    pub changes: Receiver<WatcherChange>,
    pub task: tokio::task::JoinHandle<()>,
}

/// Implements one watcher role.
#[async_trait]
pub trait WatcherBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Checks whether this role can start. No bus name is claimed here.
    async fn probe(&self) -> Result<(), BackendError>;

    /// Claims the role and returns a session. A failure lets the resolver try
    /// the next candidate.
    async fn start(&self) -> Result<WatcherSession, BackendError>;
}

pub type WatcherBackendRef = Arc<dyn WatcherBackend>;

type BackendFactory = Arc<dyn Fn() -> WatcherBackendRef + Send + Sync>;

/// A candidate in the watcher try tree.
#[derive(Clone)]
pub struct TrayCandidate {
    pub name: &'static str,
    make: BackendFactory,
}

impl TrayCandidate {
    pub fn new<F>(name: &'static str, make: F) -> Self
    where
        F: Fn() -> WatcherBackendRef + Send + Sync + 'static,
    {
        Self {
            name,
            make: Arc::new(make),
        }
    }

    pub fn create(&self) -> WatcherBackendRef {
        (self.make)()
    }
}

/// The default candidate list, most specific first.
pub fn default_candidates() -> Vec<TrayCandidate> {
    vec![
        TrayCandidate::new("own-watcher", || Arc::new(OwnWatcherBackend)),
        TrayCandidate::new("proxy-watcher", || Arc::new(ProxyWatcherBackend)),
        TrayCandidate::new("no-tray", || Arc::new(NoTrayBackend)),
    ]
}

/// Owns `org.kde.StatusNotifierWatcher` and implements the full watcher.
pub struct OwnWatcherBackend;

#[async_trait]
impl WatcherBackend for OwnWatcherBackend {
    fn name(&self) -> &'static str {
        "own-watcher"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        if name_has_owner(WATCHER_SERVICE).await? {
            return Err(BackendError::Unavailable(
                "another process owns the tray watcher".to_string(),
            ));
        }
        Ok(())
    }

    async fn start(&self) -> Result<WatcherSession, BackendError> {
        let connection = session_connection().await?;
        let dbus = DBusProxy::new(&connection).await.map_err(unavailable)?;
        let name = WellKnownName::try_from(WATCHER_SERVICE).map_err(invalid_name)?;
        let reply = dbus
            .request_name(name, RequestNameFlags::DoNotQueue.into())
            .await
            .map_err(unavailable)?;

        if !matches!(
            reply,
            RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner
        ) {
            return Err(BackendError::Unavailable(format!(
                "cannot own {WATCHER_SERVICE}: {reply}"
            )));
        }

        let (changes_tx, changes) = async_channel::unbounded();
        let registrations = Arc::new(Mutex::new(Vec::<Registration>::new()));
        let interface = WatcherInterface {
            registrations: Arc::clone(&registrations),
            changes: changes_tx.clone(),
        };

        if !connection
            .object_server()
            .at(WATCHER_PATH, interface)
            .await
            .map_err(unavailable)?
        {
            return Err(BackendError::Unavailable(
                "the tray watcher path is already exported".to_string(),
            ));
        }

        let task = tokio::spawn(watch_item_owners(
            connection.clone(),
            registrations,
            changes_tx,
        ));

        Ok(WatcherSession {
            name: self.name(),
            connection,
            changes,
            task,
        })
    }
}

/// Proxies another process' watcher and registers this process as a host.
pub struct ProxyWatcherBackend;

#[async_trait]
impl WatcherBackend for ProxyWatcherBackend {
    fn name(&self) -> &'static str {
        "proxy-watcher"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        if name_has_owner(WATCHER_SERVICE).await? {
            Ok(())
        } else {
            Err(BackendError::Unavailable(
                "no tray watcher is running".to_string(),
            ))
        }
    }

    async fn start(&self) -> Result<WatcherSession, BackendError> {
        let connection = session_connection().await?;
        let proxy = zbus::Proxy::new_owned(
            connection.clone(),
            WATCHER_SERVICE.to_string(),
            WATCHER_PATH.to_string(),
            WATCHER_INTERFACE.to_string(),
        )
        .await
        .map_err(unavailable)?;

        let unique = connection
            .unique_name()
            .map(|name| name.as_str().to_string())
            .unwrap_or_default();
        tokio::time::timeout(
            WATCHER_CALL_TIMEOUT,
            proxy.call_method("RegisterStatusNotifierHost", &(unique.as_str(),)),
        )
        .await
        .map_err(|_| BackendError::Unavailable("registering the tray host timed out".into()))?
        .map_err(unavailable)?;

        let (changes_tx, changes) = async_channel::unbounded();

        if let Ok(Ok(items)) = tokio::time::timeout(
            WATCHER_CALL_TIMEOUT,
            proxy.get_property::<Vec<String>>("RegisteredStatusNotifierItems"),
        )
        .await
        {
            for raw in items {
                if let Some(registration) = Registration::parse(&raw, None) {
                    let _ = changes_tx
                        .try_send(WatcherChange::Registered(registration))
                        .is_ok();
                }
            }
        }

        let mut registered = proxy
            .receive_signal("StatusNotifierItemRegistered")
            .await
            .map_err(unavailable)?;
        let mut unregistered = proxy
            .receive_signal("StatusNotifierItemUnregistered")
            .await
            .map_err(unavailable)?;
        let mut owner_changed = proxy.receive_owner_changed().await.map_err(unavailable)?;

        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    message = registered.next() => {
                        let Some(message) = message else { break };
                        let Ok(raw) = message.body().deserialize::<String>() else { continue };
                        if let Some(registration) = Registration::parse(&raw, None) {
                            if changes_tx.send(WatcherChange::Registered(registration)).await.is_err() {
                                break;
                            }
                        }
                    }
                    message = unregistered.next() => {
                        let Some(message) = message else { break };
                        let Ok(raw) = message.body().deserialize::<String>() else { continue };
                        if let Some(registration) = Registration::parse(&raw, None) {
                            if changes_tx
                                .send(WatcherChange::Unregistered(registration.item_id()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    owner = owner_changed.next() => {
                        // A new owner is fine. No owner means the watcher left
                        // the bus, so this session is over.
                        match owner {
                            Some(Some(_)) => continue,
                            Some(None) | None => break,
                        }
                    }
                }
            }
        });

        Ok(WatcherSession {
            name: self.name(),
            connection,
            changes,
            task,
        })
    }
}

/// The last candidate. It always reports no tray.
pub struct NoTrayBackend;

#[async_trait]
impl WatcherBackend for NoTrayBackend {
    fn name(&self) -> &'static str {
        "no-tray"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        Err(BackendError::NoBackend)
    }

    async fn start(&self) -> Result<WatcherSession, BackendError> {
        Err(BackendError::NoBackend)
    }
}

struct WatcherInterface {
    registrations: Arc<Mutex<Vec<Registration>>>,
    changes: Sender<WatcherChange>,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl WatcherInterface {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let sender = header.sender().map(|name| name.as_str().to_string());
        let Some(registration) = Registration::parse(service, sender.as_deref()) else {
            tracing::debug!(service, "ignoring an invalid tray registration");
            return;
        };

        let item_id = registration.item_id();
        {
            let mut registrations = self.registrations.lock().await;
            if registrations.len() >= MAX_REGISTRATIONS {
                tracing::warn!(service, "the tray watcher registration list is full");
                return;
            }
            if registrations
                .iter()
                .any(|existing| existing.item_id() == item_id)
            {
                return;
            }
            registrations.push(registration.clone());
        }

        let _ = self
            .changes
            .send(WatcherChange::Registered(registration))
            .await;
        let _ = emitter.status_notifier_item_registered(service).await;
    }

    async fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    async fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registrations
            .lock()
            .await
            .iter()
            .map(|registration| registration.item_id())
            .collect()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
}

/// Removes items whose bus name leaves the bus.
async fn watch_item_owners(
    connection: Connection,
    registrations: Arc<Mutex<Vec<Registration>>>,
    changes: Sender<WatcherChange>,
) {
    let proxy = match zbus::Proxy::new_owned(
        connection.clone(),
        "org.freedesktop.DBus".to_string(),
        "/org/freedesktop/DBus".to_string(),
        "org.freedesktop.DBus".to_string(),
    )
    .await
    {
        Ok(proxy) => proxy,
        Err(error) => {
            tracing::warn!(%error, "cannot watch tray item owners");
            return;
        }
    };

    let mut owner_changed = match proxy.receive_signal("NameOwnerChanged").await {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!(%error, "cannot watch tray item owners");
            return;
        }
    };

    while let Some(message) = owner_changed.next().await {
        let Ok((name, _old_owner, new_owner)) =
            message.body().deserialize::<(String, String, String)>()
        else {
            continue;
        };
        if !new_owner.is_empty() {
            continue;
        }

        let removed = {
            let mut registrations = registrations.lock().await;
            let mut removed = Vec::new();
            let mut index = 0;
            while index < registrations.len() {
                if registrations[index].bus_name == name {
                    removed.push(registrations.remove(index));
                } else {
                    index += 1;
                }
            }
            removed
        };

        for registration in removed {
            let item_id = registration.item_id();
            let _ = connection
                .emit_signal(
                    None::<&str>,
                    WATCHER_PATH,
                    WATCHER_INTERFACE,
                    "StatusNotifierItemUnregistered",
                    &(item_id.as_str(),),
                )
                .await;
            if changes
                .send(WatcherChange::Unregistered(item_id))
                .await
                .is_err()
            {
                return;
            }
        }
    }
}

async fn session_connection() -> Result<Connection, BackendError> {
    Connection::session().await.map_err(unavailable)
}

async fn name_has_owner(name: &str) -> Result<bool, BackendError> {
    let connection = session_connection().await?;
    let dbus = DBusProxy::new(&connection).await.map_err(unavailable)?;
    let name = WellKnownName::try_from(name).map_err(invalid_name)?;
    dbus.name_has_owner(name.into()).await.map_err(unavailable)
}

fn unavailable(error: impl std::fmt::Display) -> BackendError {
    BackendError::Unavailable(error.to_string())
}

fn invalid_name(error: impl std::fmt::Display) -> BackendError {
    BackendError::Unavailable(format!("invalid bus name: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_no_tray_backend_reports_no_backend() {
        let backend = NoTrayBackend;
        assert_eq!(backend.name(), "no-tray");
        assert!(matches!(
            backend.probe().await,
            Err(BackendError::NoBackend)
        ));
        assert!(matches!(
            backend.start().await,
            Err(BackendError::NoBackend)
        ));
    }

    #[test]
    fn the_default_try_tree_is_specific_first() {
        let candidates = default_candidates();
        let names: Vec<&str> = candidates.iter().map(|candidate| candidate.name).collect();
        assert_eq!(names, ["own-watcher", "proxy-watcher", "no-tray"]);
    }
}
