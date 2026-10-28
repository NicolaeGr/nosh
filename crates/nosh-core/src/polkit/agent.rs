//! The polkit agent over D-Bus.
//!
//! The shell registers as an authentication agent on the system bus and exports
//! `org.freedesktop.PolicyKit1.AuthenticationAgent`. Each `BeginAuthentication`
//! call drives one helper session and stays open until the helper reports a
//! result, because polkitd reads the outcome when the method returns.

use std::collections::HashMap;

use async_channel::{Receiver, Sender};
use async_trait::async_trait;
use tokio::sync::OnceCell;
use zbus::fdo::DBusProxy;
use zbus::message::Header;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{interface, Proxy};

use crate::auth::model::Secret;
use crate::error::BackendError;
use crate::polkit::backend::PolkitBackend;
use crate::polkit::helper::{current_uid, username_for, HelperMessage, HelperSession};
use crate::polkit::model::{AgentRequest, Identity, PolkitPrompt};

pub const POLKIT_SERVICE: &str = "org.freedesktop.PolicyKit1";
pub const POLKIT_AUTHORITY_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
pub const POLKIT_AUTHORITY_INTERFACE: &str = "org.freedesktop.PolicyKit1.Authority";
pub const POLKIT_AGENT_INTERFACE: &str = "org.freedesktop.PolicyKit1.AuthenticationAgent";
pub const POLKIT_AGENT_PATH: &str = "/org/nosh/PolicyKit1/AuthenticationAgent";

pub struct PolkitAgentBackend {
    connection: OnceCell<zbus::Connection>,
}

impl PolkitAgentBackend {
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

    async fn authority(&self) -> Result<Proxy<'_>, BackendError> {
        let connection = self.connection().await?;
        Proxy::new(
            connection,
            POLKIT_SERVICE,
            POLKIT_AUTHORITY_PATH,
            POLKIT_AUTHORITY_INTERFACE,
        )
        .await
        .map_err(dbus_error)
    }
}

impl Default for PolkitAgentBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PolkitBackend for PolkitAgentBackend {
    fn name(&self) -> &'static str {
        "polkit"
    }

    async fn register(
        &self,
        path: &str,
        locale: &str,
        requests: Sender<AgentRequest>,
    ) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let exported = connection
            .object_server()
            .at(
                path,
                AuthenticationAgentInterface {
                    connection: connection.clone(),
                    requests,
                },
            )
            .await
            .map_err(dbus_error)?;
        if !exported {
            return Err(BackendError::Unavailable(
                "the polkit agent object is already exported".to_string(),
            ));
        }

        let authority = self.authority().await?;
        if let Err(error) = authority
            .call::<_, _, ()>("RegisterAuthenticationAgent", &(subject(), locale, path))
            .await
        {
            let _ = connection
                .object_server()
                .remove::<AuthenticationAgentInterface, _>(path)
                .await;
            return Err(dbus_error(error));
        }

        Ok(())
    }

    async fn unregister(&self, path: &str) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let authority = self.authority().await?;
        let result = authority
            .call::<_, _, ()>("UnregisterAuthenticationAgent", &(subject(), path))
            .await;
        let _ = connection
            .object_server()
            .remove::<AuthenticationAgentInterface, _>(path)
            .await;
        result.map_err(dbus_error)
    }
}

/// The subject the agent registers for.
///
/// A session subject covers every process in the user's session, which is where
/// `systemctl`, `pkexec`, and graphical applications live. When the session id
/// is missing the process subject is the only fallback.
fn subject() -> (String, HashMap<String, OwnedValue>) {
    let mut details = HashMap::new();
    if let Ok(session) = std::env::var("XDG_SESSION_ID") {
        if !session.is_empty() {
            details.insert("session-id".to_string(), string_value(session));
            return ("unix-session".to_string(), details);
        }
    }

    details.insert("pid".to_string(), OwnedValue::from(std::process::id()));
    ("unix-process".to_string(), details)
}

fn string_value(value: String) -> OwnedValue {
    OwnedValue::try_from(Value::from(value)).expect("a string is a valid variant")
}

struct AuthenticationAgentInterface {
    connection: zbus::Connection,
    requests: Sender<AgentRequest>,
}

#[interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl AuthenticationAgentInterface {
    // The arguments are the D-Bus method signature and cannot be grouped.
    #[allow(clippy::too_many_arguments)]
    async fn begin_authentication(
        &self,
        action_id: String,
        message: String,
        icon_name: String,
        details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedValue>)>,
        #[zbus(header)] header: Header<'_>,
    ) {
        // Only polkitd may ask for a password. Without this check any process
        // on the system bus could pop the card and feed a cookie of its choice
        // to the privileged helper.
        if !sender_is_root(&self.connection, &header).await {
            tracing::warn!("ignoring a polkit authentication request from a non-root sender");
            return;
        }

        let identities: Vec<Identity> = identities
            .into_iter()
            .map(|(kind, details)| Identity {
                uid: details
                    .get("uid")
                    .and_then(|value| u32::try_from(value).ok()),
                kind,
            })
            .collect();
        let Some(username) = identity_username(&identities) else {
            tracing::warn!("the polkit request named no resolvable user");
            let _ = self
                .requests
                .send(AgentRequest::End {
                    cookie,
                    success: false,
                    message: Some("The requested user is not known".to_string()),
                })
                .await;
            return;
        };

        let mut details: Vec<(String, String)> = details.into_iter().collect();
        details.sort();

        let prompt = PolkitPrompt {
            cookie: cookie.clone(),
            action_id,
            message,
            icon_name,
            details,
        };

        let mut helper = match HelperSession::start(&username, &cookie).await {
            Ok(helper) => helper,
            Err(error) => {
                tracing::warn!(%error, "the polkit helper could not be started");
                let _ = self
                    .requests
                    .send(AgentRequest::End {
                        cookie,
                        success: false,
                        message: Some("The authentication helper is not available".to_string()),
                    })
                    .await;
                return;
            }
        };

        let (responses, passwords) = async_channel::bounded(1);
        if self
            .requests
            .send(AgentRequest::Begin {
                prompt,
                identities,
                responses,
            })
            .await
            .is_err()
        {
            return;
        }

        let (success, message) = drive(&mut helper, &passwords).await;
        let _ = self
            .requests
            .send(AgentRequest::End {
                cookie,
                success,
                message,
            })
            .await;
    }

    async fn cancel_authentication(&self, cookie: String) {
        let _ = self.requests.send(AgentRequest::Cancel { cookie }).await;
    }
}

/// Feeds the helper until it reports a result or the UI stops answering.
async fn drive(helper: &mut HelperSession, passwords: &Receiver<Secret>) -> (bool, Option<String>) {
    loop {
        let message = match helper.next().await {
            Some(message) => message,
            None => return (false, Some("The authentication helper stopped".to_string())),
        };
        match message {
            HelperMessage::Prompt { .. } => {
                // Wait for the password, but notice a helper that dies before
                // the user answers. PAM blocks for the answer, so a message
                // here means the session is already over.
                let password = tokio::select! {
                    answer = passwords.recv() => match answer {
                        Ok(password) => password,
                        // The service closed the channel because the user cancelled.
                        Err(_) => return (false, None),
                    },
                    message = helper.next() => {
                        return match message {
                            Some(HelperMessage::Success) => (true, None),
                            Some(HelperMessage::Failure) => {
                                (false, Some("Authentication failed".to_string()))
                            }
                            _ => (false, Some("The authentication helper stopped".to_string())),
                        };
                    }
                };
                if helper.respond(password.as_bytes()).await.is_err() {
                    return (false, Some("The authentication helper stopped".to_string()));
                }
            }
            HelperMessage::Info(_) | HelperMessage::Error(_) => {}
            HelperMessage::Success => return (true, None),
            HelperMessage::Failure => return (false, Some("Authentication failed".to_string())),
        }
    }
}

/// Whether the caller of a D-Bus method is the root user.
async fn sender_is_root(connection: &zbus::Connection, header: &Header<'_>) -> bool {
    let Some(sender) = header.sender() else {
        return false;
    };
    let bus = match DBusProxy::new(connection).await {
        Ok(bus) => bus,
        Err(error) => {
            tracing::warn!(%error, "the polkit caller could not be looked up");
            return false;
        }
    };
    match bus.get_connection_unix_user(sender.clone().into()).await {
        Ok(uid) => uid == 0,
        Err(error) => {
            tracing::warn!(%error, "the uid of the polkit caller could not be read");
            false
        }
    }
}

/// The name PAM authenticates for an identity list.
///
/// The `$USER` environment variable is never used, because a process can
/// change it. The uid from the request, or the agent's own uid, is resolved
/// through NSS the same way polkit resolves it.
fn identity_username(identities: &[Identity]) -> Option<String> {
    identities
        .iter()
        .filter(|identity| identity.kind == "unix-user")
        .find_map(|identity| identity.uid)
        .or_else(|| Some(current_uid()))
        .and_then(username_for)
}

fn dbus_error(error: zbus::Error) -> BackendError {
    if let zbus::Error::MethodError(name, _, _) = &error {
        if name.as_str() == "org.freedesktop.DBus.Error.AccessDenied" {
            return BackendError::Message(
                "Another authentication agent already handles this session".to_string(),
            );
        }
    }
    BackendError::Unavailable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_subject_uses_the_session_id() {
        // The test process may run outside a session, so only assert the shape
        // that does not depend on the environment.
        let (kind, details) = subject();
        match kind.as_str() {
            "unix-session" => assert!(details.contains_key("session-id")),
            "unix-process" => assert!(details.contains_key("pid")),
            other => panic!("unexpected subject kind {other}"),
        }
    }

    #[test]
    fn a_string_value_carries_a_string_variant() {
        let value = string_value("3".to_string());
        assert_eq!(<&str>::try_from(&value).unwrap(), "3");
    }

    #[test]
    fn a_request_without_a_user_identity_names_the_process_user() {
        let identities = [Identity {
            kind: "unix-session".to_string(),
            uid: None,
        }];
        assert_eq!(identity_username(&identities), username_for(current_uid()));
    }

    #[test]
    fn the_first_unix_user_identity_names_the_user() {
        let identities = [
            Identity {
                kind: "unix-session".to_string(),
                uid: None,
            },
            Identity::unix_user(current_uid()),
        ];
        assert_eq!(identity_username(&identities), username_for(current_uid()));
    }
}
