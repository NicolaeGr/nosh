//! The logind session lock watcher.
//!
//! `loginctl lock-session` emits `Lock` on the caller's session object. The
//! object path is resolved through `GetSessionByPID`, because `session/auto`
//! is a method alias and does not carry the signals of the real session.

use async_channel::Sender;
use futures_util::StreamExt;
use tokio::sync::OnceCell;
use zbus::zvariant::OwnedObjectPath;

use crate::error::BackendError;
use crate::event::SessionLockEvent;
use crate::power::logind::{LOGIND_MANAGER, LOGIND_PATH, LOGIND_SERVICE, LOGIND_SESSION_INTERFACE};
use crate::session_lock::backend::SessionLockBackend;

pub struct LogindSessionLockBackend {
    connection: OnceCell<zbus::Connection>,
}

impl LogindSessionLockBackend {
    pub fn new() -> Self {
        Self {
            connection: OnceCell::new(),
        }
    }

    async fn connection(&self) -> Result<&zbus::Connection, BackendError> {
        self.connection
            .get_or_try_init(|| async {
                zbus::Connection::system()
                    .await
                    .map_err(|error| BackendError::Unavailable(error.to_string()))
            })
            .await
    }

    async fn session_path(&self) -> Result<OwnedObjectPath, BackendError> {
        let connection = self.connection().await?;
        let manager = zbus::Proxy::new(connection, LOGIND_SERVICE, LOGIND_PATH, LOGIND_MANAGER)
            .await
            .map_err(dbus_error)?;

        // logind resolves the pid through the session cgroup. A shell started
        // outside the session (a transient unit, for example) still has
        // XDG_SESSION_ID, and GetSession applies the object path escaping.
        match manager
            .call("GetSessionByPID", &(std::process::id(),))
            .await
        {
            Ok(path) => Ok(path),
            Err(error) => {
                let id = std::env::var("XDG_SESSION_ID").map_err(|_| {
                    BackendError::Unavailable(format!("the session is unknown: {error}"))
                })?;
                manager.call("GetSession", &(id,)).await.map_err(dbus_error)
            }
        }
    }

    async fn watch_session(&self, events: &Sender<SessionLockEvent>) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let path = self.session_path().await?;
        let session = zbus::Proxy::new(
            connection,
            LOGIND_SERVICE,
            path.as_str(),
            LOGIND_SESSION_INTERFACE,
        )
        .await
        .map_err(dbus_error)?;

        let mut lock = session.receive_signal("Lock").await.map_err(dbus_error)?;
        let mut unlock = session.receive_signal("Unlock").await.map_err(dbus_error)?;

        loop {
            tokio::select! {
                signal = lock.next() => {
                    if signal.is_none() {
                        return Ok(());
                    }
                    if events.send(SessionLockEvent::Lock).await.is_err() {
                        return Ok(());
                    }
                }
                signal = unlock.next() => {
                    if signal.is_none() {
                        return Ok(());
                    }
                    if events.send(SessionLockEvent::Unlock).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

impl Default for LogindSessionLockBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SessionLockBackend for LogindSessionLockBackend {
    fn name(&self) -> &'static str {
        "logind"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        self.session_path().await.map(|_| ())
    }

    async fn watch(&self, events: Sender<SessionLockEvent>) -> Result<(), BackendError> {
        let result = self.watch_session(&events).await;
        if result.is_err() {
            let _ = events.send(SessionLockEvent::Unavailable).await;
        }
        result
    }
}

fn dbus_error(error: zbus::Error) -> BackendError {
    BackendError::Unavailable(error.to_string())
}
