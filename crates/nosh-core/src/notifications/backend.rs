//! The daemon-role try tree.
//!
//! Own the freedesktop notification name, proxy another daemon, or fall back to
//! a null role that logs and displays nothing. The service receives the same
//! request enum in the first two cases.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::names::WellKnownName;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;
use zbus::{interface, Connection, Proxy};

use crate::error::BackendError;
use crate::notifications::model::{
    DEFAULT_CAPABILITIES, SERVER_NAME, SERVER_VENDOR, SERVER_VERSION, SPEC_VERSION,
};

pub const NOTIFICATION_SERVICE: &str = "org.freedesktop.Notifications";
pub const NOTIFICATION_PATH: &str = "/org/freedesktop/Notifications";
pub const NOTIFICATION_INTERFACE: &str = "org.freedesktop.Notifications";

/// The longest a probe or a proxy call may take.
const CALL_TIMEOUT: Duration = Duration::from_secs(2);

/// A `Notify` call waiting for an ID from the service.
pub struct NotifyRequest {
    pub app_name: String,
    pub replaces_id: u32,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<String>,
    pub hints: HashMap<String, OwnedValue>,
    pub expire_timeout: i32,
    pub reply: oneshot::Sender<u32>,
}

/// A request entering the service from the daemon role.
pub enum IncomingRequest {
    Notify(NotifyRequest),
    /// A client called `CloseNotification`.
    Close(u32),
    /// A proxied daemon reported a close.
    ExternalClosed {
        id: u32,
        reason: u32,
    },
    /// A proxied daemon reported an action.
    ExternalAction {
        id: u32,
        action: String,
    },
}

/// Who answers `org.freedesktop.Notifications`.
pub enum DaemonRole {
    Owner,
    Proxy(Proxy<'static>),
    Null,
}

/// A running daemon role.
pub struct NotificationSession {
    pub name: &'static str,
    pub connection: Option<Connection>,
    pub incoming: Receiver<IncomingRequest>,
    pub role: DaemonRole,
    pub capabilities: Arc<RwLock<Vec<String>>>,
    pub task: Option<tokio::task::JoinHandle<()>>,
    /// Keeps the request channel open when the backend has no task.
    pub keepalive: Option<Sender<IncomingRequest>>,
}

#[async_trait]
pub trait NotificationBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Checks whether the role can start. No bus name is claimed here.
    async fn probe(&self) -> Result<(), BackendError>;

    /// Claims the role. A failure lets the resolver try the next candidate.
    async fn start(&self) -> Result<NotificationSession, BackendError>;
}

pub type NotificationBackendRef = Arc<dyn NotificationBackend>;

type BackendFactory = Arc<dyn Fn() -> NotificationBackendRef + Send + Sync>;

/// A candidate in the daemon try tree.
#[derive(Clone)]
pub struct NotificationCandidate {
    pub name: &'static str,
    make: BackendFactory,
}

impl NotificationCandidate {
    pub fn new<F>(name: &'static str, make: F) -> Self
    where
        F: Fn() -> NotificationBackendRef + Send + Sync + 'static,
    {
        Self {
            name,
            make: Arc::new(make),
        }
    }

    pub fn create(&self) -> NotificationBackendRef {
        (self.make)()
    }
}

/// The default candidate list, most specific first.
pub fn default_candidates() -> Vec<NotificationCandidate> {
    vec![
        NotificationCandidate::new("own-daemon", || Arc::new(OwnDaemonBackend::new())),
        NotificationCandidate::new("existing-daemon-proxy", || {
            Arc::new(ExistingDaemonProxyBackend)
        }),
        NotificationCandidate::new("null-notification", || Arc::new(NullNotificationBackend)),
    ]
}

/// Owns the notification name and implements the freedesktop daemon.
pub struct OwnDaemonBackend {
    capabilities: Arc<RwLock<Vec<String>>>,
}

impl OwnDaemonBackend {
    pub fn new() -> Self {
        Self {
            capabilities: default_capabilities(),
        }
    }
}

impl Default for OwnDaemonBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationBackend for OwnDaemonBackend {
    fn name(&self) -> &'static str {
        "own-daemon"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        if name_has_owner(NOTIFICATION_SERVICE).await? {
            return Err(BackendError::Unavailable(
                "another process owns org.freedesktop.Notifications".to_string(),
            ));
        }
        Ok(())
    }

    async fn start(&self) -> Result<NotificationSession, BackendError> {
        let connection = session_connection().await?;
        let dbus = DBusProxy::new(&connection).await.map_err(unavailable)?;
        let name = WellKnownName::try_from(NOTIFICATION_SERVICE).map_err(invalid_name)?;
        let reply = dbus
            .request_name(name, RequestNameFlags::DoNotQueue.into())
            .await
            .map_err(unavailable)?;

        if !matches!(
            reply,
            RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner
        ) {
            return Err(BackendError::Unavailable(format!(
                "cannot own {NOTIFICATION_SERVICE}: {reply}"
            )));
        }

        let (requests_tx, incoming) = async_channel::unbounded();
        let interface = NotificationsInterface {
            requests: requests_tx,
            capabilities: Arc::clone(&self.capabilities),
        };
        if !connection
            .object_server()
            .at(NOTIFICATION_PATH, interface)
            .await
            .map_err(unavailable)?
        {
            return Err(BackendError::Unavailable(
                "the notification object path is already exported".to_string(),
            ));
        }

        Ok(NotificationSession {
            name: self.name(),
            connection: Some(connection),
            incoming,
            role: DaemonRole::Owner,
            capabilities: Arc::clone(&self.capabilities),
            task: None,
            keepalive: None,
        })
    }
}

/// Reads another process' notification daemon.
///
/// The freedesktop interface has no signal for new notifications, so this role
/// mirrors the close and action signals and forwards dismissal commands. The UI
/// receives the same request type when the other daemon exposes its own signals.
pub struct ExistingDaemonProxyBackend;

#[async_trait]
impl NotificationBackend for ExistingDaemonProxyBackend {
    fn name(&self) -> &'static str {
        "existing-daemon-proxy"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        if !name_has_owner(NOTIFICATION_SERVICE).await? {
            return Err(BackendError::Unavailable(
                "no notification daemon is running".to_string(),
            ));
        }
        Ok(())
    }

    async fn start(&self) -> Result<NotificationSession, BackendError> {
        let connection = session_connection().await?;
        let proxy = Proxy::new_owned(
            connection.clone(),
            NOTIFICATION_SERVICE.to_string(),
            NOTIFICATION_PATH.to_string(),
            NOTIFICATION_INTERFACE.to_string(),
        )
        .await
        .map_err(unavailable)?;

        let info =
            tokio::time::timeout(CALL_TIMEOUT, proxy.call_method("GetServerInformation", &()))
                .await
                .map_err(|_| BackendError::Unavailable("the notification daemon timed out".into()))?
                .map_err(unavailable)?;
        if info
            .body()
            .deserialize::<(String, String, String, String)>()
            .is_err()
        {
            return Err(BackendError::Unavailable(
                "the notification daemon returned invalid server information".to_string(),
            ));
        }

        let mut closed = proxy
            .receive_signal("NotificationClosed")
            .await
            .map_err(unavailable)?;
        let mut invoked = proxy
            .receive_signal("ActionInvoked")
            .await
            .map_err(unavailable)?;
        let mut owner_changed = proxy.receive_owner_changed().await.map_err(unavailable)?;

        let (requests_tx, incoming) = async_channel::unbounded();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    message = closed.next() => {
                        let Some(message) = message else { break };
                        let Ok((id, reason)) = message.body().deserialize::<(u32, u32)>() else {
                            continue;
                        };
                        if requests_tx
                            .send(IncomingRequest::ExternalClosed { id, reason })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    message = invoked.next() => {
                        let Some(message) = message else { break };
                        let Ok((id, action)) = message.body().deserialize::<(u32, String)>() else {
                            continue;
                        };
                        if requests_tx
                            .send(IncomingRequest::ExternalAction { id, action })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    owner = owner_changed.next() => {
                        match owner {
                            Some(Some(_)) => {}
                            Some(None) | None => break,
                        }
                    }
                }
            }
        });

        Ok(NotificationSession {
            name: self.name(),
            connection: Some(connection),
            incoming,
            role: DaemonRole::Proxy(proxy),
            capabilities: default_capabilities(),
            task: Some(task),
            keepalive: None,
        })
    }
}

/// Keeps the shell alive when no daemon role is available. Requests cannot
/// arrive because no name is owned.
pub struct NullNotificationBackend;

#[async_trait]
impl NotificationBackend for NullNotificationBackend {
    fn name(&self) -> &'static str {
        "null-notification"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        Ok(())
    }

    async fn start(&self) -> Result<NotificationSession, BackendError> {
        let (requests_tx, incoming) = async_channel::unbounded();
        tracing::info!("no notification daemon role is available; notifications are logged only");
        Ok(NotificationSession {
            name: self.name(),
            connection: None,
            incoming,
            role: DaemonRole::Null,
            capabilities: default_capabilities(),
            task: None,
            keepalive: Some(requests_tx),
        })
    }
}

struct NotificationsInterface {
    requests: Sender<IncomingRequest>,
    capabilities: Arc<RwLock<Vec<String>>>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationsInterface {
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let (reply, response) = oneshot::channel();
        let request = NotifyRequest {
            app_name,
            replaces_id,
            app_icon,
            summary,
            body,
            actions,
            hints,
            expire_timeout,
            reply,
        };
        if self
            .requests
            .send(IncomingRequest::Notify(request))
            .await
            .is_err()
        {
            return 0;
        }
        response.await.unwrap_or(0)
    }

    async fn close_notification(&self, id: u32) {
        let _ = self.requests.send(IncomingRequest::Close(id)).await;
    }

    async fn get_capabilities(&self) -> Vec<String> {
        self.capabilities.read().await.clone()
    }

    async fn get_server_information(&self) -> (String, String, String, String) {
        (
            SERVER_NAME.to_string(),
            SERVER_VENDOR.to_string(),
            SERVER_VERSION.to_string(),
            SPEC_VERSION.to_string(),
        )
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action: &str)
        -> zbus::Result<()>;

    #[zbus(signal)]
    async fn activation_token(
        emitter: &SignalEmitter<'_>,
        id: u32,
        token: &str,
    ) -> zbus::Result<()>;
}

fn default_capabilities() -> Arc<RwLock<Vec<String>>> {
    Arc::new(RwLock::new(
        DEFAULT_CAPABILITIES
            .iter()
            .map(|value| value.to_string())
            .collect(),
    ))
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

    #[test]
    fn the_default_try_tree_is_specific_first() {
        let candidates = default_candidates();
        let names: Vec<&str> = candidates.iter().map(|candidate| candidate.name).collect();

        assert_eq!(
            names,
            ["own-daemon", "existing-daemon-proxy", "null-notification"]
        );
    }

    #[tokio::test]
    async fn the_null_backend_always_starts_and_keeps_the_channel_open() {
        let session = NullNotificationBackend.start().await.unwrap();

        assert_eq!(session.name, "null-notification");
        assert!(session.connection.is_none());
        assert!(matches!(session.role, DaemonRole::Null));
        assert!(session.keepalive.is_some());
        assert!(session.incoming.is_empty());
    }
}
