//! The polkit agent service.
//!
//! Registers one agent with polkitd, forwards prompts to the UI, and relays
//! passwords to the helper session that owns the dialog.

use std::collections::HashMap;
use std::sync::Arc;

use async_channel::{Receiver, Sender};

use crate::auth::model::Secret;
use crate::command::Command;
use crate::event::{Event, Service};
use crate::polkit::agent::{PolkitAgentBackend, POLKIT_AGENT_PATH};
use crate::polkit::backend::PolkitBackendRef;
use crate::polkit::model::{AgentRequest, PolkitEvent};

pub struct PolkitService {
    backend: PolkitBackendRef,
}

impl PolkitService {
    pub fn from_env() -> Self {
        Self {
            backend: Arc::new(PolkitAgentBackend::new()),
        }
    }

    pub fn with_backend(backend: PolkitBackendRef) -> Self {
        Self { backend }
    }

    /// Runs until the command receiver closes or the agent request channel
    /// closes.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let (requests_tx, requests) = async_channel::unbounded();
        if let Err(error) = self
            .backend
            .register(POLKIT_AGENT_PATH, &locale(), requests_tx)
            .await
        {
            tracing::warn!(
                backend = self.backend.name(),
                %error,
                "the polkit authentication agent is unavailable"
            );
            let _ = events
                .send(Event::ServiceAvailability(Service::Polkit, false))
                .await;
            let _ = events.send(Event::Polkit(PolkitEvent::Unavailable)).await;
            return;
        }

        if events
            .send(Event::ServiceAvailability(Service::Polkit, true))
            .await
            .is_err()
        {
            let _ = self.backend.unregister(POLKIT_AGENT_PATH).await;
            return;
        }

        // One open helper session per cookie.
        let mut pending: HashMap<String, Sender<Secret>> = HashMap::new();
        'service: loop {
            tokio::select! {
                request = requests.recv() => {
                    match request {
                        Ok(AgentRequest::Begin { prompt, responses, .. }) => {
                            // The UI shows one card. Drop any older session
                            // first. Dropping its password channel makes its
                            // helper stop, and the agent ignores the late End.
                            let stale: Vec<String> = pending.keys().cloned().collect();
                            for cookie in stale {
                                pending.remove(&cookie);
                                if events
                                    .send(Event::Polkit(PolkitEvent::Cancelled { cookie }))
                                    .await
                                    .is_err()
                                {
                                    break 'service;
                                }
                            }
                            pending.insert(prompt.cookie.clone(), responses);
                            if events
                                .send(Event::Polkit(PolkitEvent::Prompt(prompt)))
                                .await
                                .is_err()
                            {
                                break 'service;
                            }
                        }
                        Ok(AgentRequest::End { cookie, success, message }) => {
                            if pending.remove(&cookie).is_none() {
                                continue;
                            }
                            let event = if success {
                                PolkitEvent::Finished { cookie }
                            } else {
                                PolkitEvent::Failed {
                                    cookie,
                                    message: message.unwrap_or_else(|| {
                                        "Authentication failed".to_string()
                                    }),
                                }
                            };
                            if events.send(Event::Polkit(event)).await.is_err() {
                                break 'service;
                            }
                        }
                        Ok(AgentRequest::Cancel { cookie }) => {
                            // Dropping the sender makes the helper session stop.
                            pending.remove(&cookie);
                            if events
                                .send(Event::Polkit(PolkitEvent::Cancelled { cookie }))
                                .await
                                .is_err()
                            {
                                break 'service;
                            }
                        }
                        Err(_) => break 'service,
                    }
                }
                command = commands.recv() => {
                    match command {
                        Ok(Command::PolkitRespond { cookie, password }) => {
                            if let Some(responses) = pending.get(&cookie) {
                                if responses.send(password).await.is_err() {
                                    pending.remove(&cookie);
                                }
                            }
                        }
                        Ok(Command::PolkitCancel { cookie }) => {
                            pending.remove(&cookie);
                            if events
                                .send(Event::Polkit(PolkitEvent::Cancelled { cookie }))
                                .await
                                .is_err()
                            {
                                break 'service;
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break 'service,
                    }
                }
            }
        }

        if let Err(error) = self.backend.unregister(POLKIT_AGENT_PATH).await {
            tracing::warn!(%error, "the polkit agent was not unregistered");
        }
    }
}

fn locale() -> String {
    std::env::var("LC_MESSAGES")
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "C".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::BackendError;
    use crate::polkit::model::{Identity, PolkitPrompt};
    use std::sync::Mutex;

    struct FakeBackend {
        requests: Mutex<Option<Sender<AgentRequest>>>,
        fail_register: bool,
    }

    impl FakeBackend {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                requests: Mutex::new(None),
                fail_register: false,
            })
        }

        fn broken() -> Arc<Self> {
            Arc::new(Self {
                requests: Mutex::new(None),
                fail_register: true,
            })
        }

        fn sender(&self) -> Sender<AgentRequest> {
            self.requests
                .lock()
                .unwrap()
                .clone()
                .expect("the service did not register")
        }
    }

    #[async_trait::async_trait]
    impl crate::polkit::backend::PolkitBackend for FakeBackend {
        fn name(&self) -> &'static str {
            "fake"
        }

        async fn register(
            &self,
            _path: &str,
            _locale: &str,
            requests: Sender<AgentRequest>,
        ) -> Result<(), BackendError> {
            if self.fail_register {
                return Err(BackendError::Unavailable("no polkit".to_string()));
            }
            *self.requests.lock().unwrap() = Some(requests);
            Ok(())
        }

        async fn unregister(&self, _path: &str) -> Result<(), BackendError> {
            Ok(())
        }
    }

    fn begin(cookie: &str) -> (AgentRequest, Receiver<Secret>) {
        let (responses, passwords) = async_channel::bounded(1);
        let request = AgentRequest::Begin {
            prompt: PolkitPrompt {
                cookie: cookie.to_string(),
                action_id: "org.freedesktop.systemd1.manage-units".to_string(),
                message: "Authentication is required".to_string(),
                icon_name: "dialog-password".to_string(),
                details: Vec::new(),
            },
            identities: vec![Identity::unix_user(1000)],
            responses,
        };
        (request, passwords)
    }

    async fn next_polkit(rx: &mut Receiver<Event>) -> PolkitEvent {
        loop {
            if let Event::Polkit(event) = rx.recv().await.unwrap() {
                return event;
            }
        }
    }

    async fn started() -> (Arc<FakeBackend>, Receiver<Event>, Sender<Command>) {
        let backend = FakeBackend::new();
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        tokio::spawn(PolkitService::with_backend(backend.clone()).run(command_rx, event_tx));

        // The availability event is sent after registration.
        loop {
            match event_rx.recv().await.unwrap() {
                Event::ServiceAvailability(Service::Polkit, true) => break,
                Event::ServiceAvailability(Service::Polkit, false) => {
                    panic!("the service did not register")
                }
                _ => {}
            }
        }

        (backend, event_rx, commands)
    }

    #[tokio::test]
    async fn a_prompt_reaches_the_ui() {
        let (backend, mut events, _commands) = started().await;
        let (request, _passwords) = begin("cookie");
        backend.sender().send(request).await.unwrap();

        match next_polkit(&mut events).await {
            PolkitEvent::Prompt(prompt) => {
                assert_eq!(prompt.cookie, "cookie");
                assert_eq!(prompt.action_id, "org.freedesktop.systemd1.manage-units");
            }
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_second_prompt_cancels_the_first() {
        let (backend, mut events, _commands) = started().await;
        let (first, first_passwords) = begin("first");
        backend.sender().send(first).await.unwrap();
        let _ = next_polkit(&mut events).await;

        let (second, _second_passwords) = begin("second");
        backend.sender().send(second).await.unwrap();

        assert!(matches!(
            next_polkit(&mut events).await,
            PolkitEvent::Cancelled { cookie } if cookie == "first"
        ));
        assert!(matches!(
            next_polkit(&mut events).await,
            PolkitEvent::Prompt(prompt) if prompt.cookie == "second"
        ));
        // The older helper session must be told to stop.
        assert!(first_passwords.recv().await.is_err());
    }

    #[tokio::test]
    async fn a_password_reaches_the_helper_session() {
        let (backend, mut events, commands) = started().await;
        let (request, passwords) = begin("cookie");
        backend.sender().send(request).await.unwrap();
        let _ = next_polkit(&mut events).await;

        commands
            .send(Command::PolkitRespond {
                cookie: "cookie".to_string(),
                password: Secret::new("correct"),
            })
            .await
            .unwrap();

        let password = passwords.recv().await.unwrap();
        assert_eq!(password.as_bytes(), b"correct");
    }

    #[tokio::test]
    async fn a_successful_session_closes_the_prompt() {
        let (backend, mut events, _commands) = started().await;
        let (request, _passwords) = begin("cookie");
        backend.sender().send(request).await.unwrap();
        let _ = next_polkit(&mut events).await;

        backend
            .sender()
            .send(AgentRequest::End {
                cookie: "cookie".to_string(),
                success: true,
                message: None,
            })
            .await
            .unwrap();

        assert!(matches!(
            next_polkit(&mut events).await,
            PolkitEvent::Finished { .. }
        ));
    }

    #[tokio::test]
    async fn a_failed_session_reports_the_error() {
        let (backend, mut events, _commands) = started().await;
        let (request, _passwords) = begin("cookie");
        backend.sender().send(request).await.unwrap();
        let _ = next_polkit(&mut events).await;

        backend
            .sender()
            .send(AgentRequest::End {
                cookie: "cookie".to_string(),
                success: false,
                message: Some("Authentication failed".to_string()),
            })
            .await
            .unwrap();

        match next_polkit(&mut events).await {
            PolkitEvent::Failed { cookie, message } => {
                assert_eq!(cookie, "cookie");
                assert_eq!(message, "Authentication failed");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_cancel_closes_the_helper_session() {
        let (backend, mut events, commands) = started().await;
        let (request, passwords) = begin("cookie");
        backend.sender().send(request).await.unwrap();
        let _ = next_polkit(&mut events).await;

        commands
            .send(Command::PolkitCancel {
                cookie: "cookie".to_string(),
            })
            .await
            .unwrap();

        assert!(matches!(
            next_polkit(&mut events).await,
            PolkitEvent::Cancelled { .. }
        ));
        assert!(passwords.recv().await.is_err());
    }

    #[tokio::test]
    async fn polkit_can_cancel_a_prompt() {
        let (backend, mut events, _commands) = started().await;
        let (request, passwords) = begin("cookie");
        backend.sender().send(request).await.unwrap();
        let _ = next_polkit(&mut events).await;

        backend
            .sender()
            .send(AgentRequest::Cancel {
                cookie: "cookie".to_string(),
            })
            .await
            .unwrap();

        assert!(matches!(
            next_polkit(&mut events).await,
            PolkitEvent::Cancelled { .. }
        ));
        assert!(passwords.recv().await.is_err());
    }

    #[tokio::test]
    async fn a_missing_polkit_reports_unavailable() {
        let (_commands, command_rx) = async_channel::unbounded();
        let (event_tx, mut event_rx) = async_channel::unbounded();
        tokio::spawn(PolkitService::with_backend(FakeBackend::broken()).run(command_rx, event_tx));

        assert!(matches!(
            next_polkit(&mut event_rx).await,
            PolkitEvent::Unavailable
        ));
    }
}
