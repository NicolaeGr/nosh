//! The logind D-Bus backend.
//!
//! Reads the `Can*` answers and calls the matching manager method. Logout
//! terminates the session object at `/session/auto`, which logind resolves to
//! the caller's session.

use tokio::sync::OnceCell;

use crate::error::BackendError;
use crate::power::backend::PowerBackend;
use crate::power::model::{PowerAction, PowerModel};

pub const LOGIND_SERVICE: &str = "org.freedesktop.login1";
pub const LOGIND_PATH: &str = "/org/freedesktop/login1";
pub const LOGIND_MANAGER: &str = "org.freedesktop.login1.Manager";
pub const LOGIND_SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";
pub const LOGIND_SESSION_PATH: &str = "/org/freedesktop/login1/session/auto";

/// The answer to a logind `Can*` question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capability {
    Yes,
    /// logind accepts the action only after another authentication step.
    Challenge,
    No,
}

pub struct LogindBackend {
    connection: OnceCell<zbus::Connection>,
}

impl LogindBackend {
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

    async fn manager(&self) -> Result<zbus::Proxy<'_>, BackendError> {
        let connection = self.connection().await?;
        zbus::Proxy::new(connection, LOGIND_SERVICE, LOGIND_PATH, LOGIND_MANAGER)
            .await
            .map_err(dbus_error)
    }

    async fn can(&self, method: &str) -> Result<bool, BackendError> {
        Ok(self.capability(method).await? != Capability::No)
    }

    async fn capability(&self, method: &str) -> Result<Capability, BackendError> {
        let proxy = self.manager().await?;
        let answer: String = proxy.call(method, &()).await.map_err(dbus_error)?;
        Ok(match answer.as_str() {
            "yes" => Capability::Yes,
            "challenge" => Capability::Challenge,
            _ => Capability::No,
        })
    }

    async fn call(&self, method: &str) -> Result<(), BackendError> {
        let proxy = self.manager().await?;
        proxy
            .call::<_, _, ()>(method, &(true,))
            .await
            .map_err(dbus_error)
    }

    async fn terminate_session(&self) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let proxy = zbus::Proxy::new(
            connection,
            LOGIND_SERVICE,
            LOGIND_SESSION_PATH,
            LOGIND_SESSION_INTERFACE,
        )
        .await
        .map_err(dbus_error)?;
        proxy
            .call::<_, _, ()>("Terminate", &())
            .await
            .map_err(dbus_error)
    }
}

impl Default for LogindBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl PowerBackend for LogindBackend {
    fn name(&self) -> &'static str {
        "logind"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        // One denied capability does not mean the menu is unusable. Only a
        // logind that answers nothing disables the actions, including Logout.
        let mut last = None;
        for method in ["CanPowerOff", "CanReboot", "CanSuspend", "CanHibernate"] {
            match self.can(method).await {
                Ok(_) => return Ok(()),
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(|| BackendError::Unavailable("logind did not answer".to_string())))
    }

    async fn read(&self) -> Result<PowerModel, BackendError> {
        let mut supported = Vec::new();
        let mut requires_auth = Vec::new();
        let mut answered = false;
        let mut last_error = None;

        for (action, method) in [
            (PowerAction::Shutdown, "CanPowerOff"),
            (PowerAction::Reboot, "CanReboot"),
            (PowerAction::Suspend, "CanSuspend"),
            (PowerAction::Hibernate, "CanHibernate"),
        ] {
            let capability = match self.capability(method).await {
                Ok(capability) => {
                    answered = true;
                    capability
                }
                Err(error) => {
                    // One unanswered capability drops one action, not the menu.
                    tracing::warn!(%error, method, "logind did not answer, dropping the action");
                    last_error = Some(error);
                    continue;
                }
            };
            match capability {
                Capability::No => {}
                Capability::Challenge => {
                    supported.push(action);
                    requires_auth.push(action);
                }
                Capability::Yes => supported.push(action),
            }
        }

        if !answered {
            if let Some(error) = last_error {
                return Err(error);
            }
        }

        // Logout only needs a session object, which the caller always has.
        supported.push(PowerAction::Logout);

        Ok(PowerModel {
            supported,
            requires_auth,
        })
    }

    async fn execute(&self, action: PowerAction) -> Result<(), BackendError> {
        match action {
            PowerAction::Shutdown => self.call("PowerOff").await,
            PowerAction::Reboot => self.call("Reboot").await,
            PowerAction::Suspend => self.call("Suspend").await,
            PowerAction::Hibernate => self.call("Hibernate").await,
            PowerAction::Logout => self.terminate_session().await,
        }
    }
}

fn dbus_error(error: zbus::Error) -> BackendError {
    let text = error.to_string();
    if let zbus::Error::MethodError(name, _, _) = &error {
        match name.as_str() {
            "org.freedesktop.login1.NotInControl"
            | "org.freedesktop.login1.AccessDenied"
            | "org.freedesktop.DBus.Error.AccessDenied" => {
                return BackendError::Message("The system did not allow this action".to_string());
            }
            "org.freedesktop.login1.InteractiveAuthenticationRequired" => {
                return BackendError::Message(
                    "The system requires another form of authentication".to_string(),
                );
            }
            _ => {}
        }
    }
    BackendError::Unavailable(text)
}
