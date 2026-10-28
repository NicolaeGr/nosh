//! The power service.
//!
//! Reads the available actions at startup, then forwards one action per
//! command after the UI has authenticated the user.

use async_channel::{Receiver, Sender};

use crate::command::Command;
use crate::event::{Event, PowerEvent, Service, Snapshot};
use crate::power::backend::PowerBackendRef;
use crate::power::logind::LogindBackend;

#[derive(Debug, Clone, Default)]
pub struct PowerServiceConfig;

pub struct PowerService {
    backend: PowerBackendRef,
}

impl PowerService {
    pub fn from_env_with(_config: PowerServiceConfig) -> Self {
        Self {
            backend: std::sync::Arc::new(LogindBackend::new()),
        }
    }

    pub fn with_backend(backend: PowerBackendRef) -> Self {
        Self { backend }
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let available = match self.backend.probe().await {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(
                    backend = self.backend.name(),
                    %error,
                    "session actions are unavailable"
                );
                if events
                    .send(Event::ServiceAvailability(Service::Power, false))
                    .await
                    .is_err()
                {
                    return;
                }
                if events
                    .send(Event::Power(PowerEvent::Unavailable))
                    .await
                    .is_err()
                {
                    return;
                }
                false
            }
        };

        if available {
            if events
                .send(Event::ServiceAvailability(Service::Power, true))
                .await
                .is_err()
            {
                return;
            }

            match self.backend.read().await {
                Ok(model) => {
                    let snapshot = Snapshot {
                        power: Some(model.clone()),
                        ..Snapshot::default()
                    };
                    if events
                        .send(Event::Power(PowerEvent::Snapshot(model)))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    if events.send(Event::Snapshot(snapshot)).await.is_err() {
                        return;
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        backend = self.backend.name(),
                        %error,
                        "the logind capabilities could not be read"
                    );
                    if events
                        .send(Event::Power(PowerEvent::Unavailable))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }

        while let Ok(command) = commands.recv().await {
            let Command::PowerAction(action) = command else {
                continue;
            };

            if !available {
                if events
                    .send(Event::Power(PowerEvent::ActionFailed {
                        action,
                        message: "Session actions are unavailable".to_string(),
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
                continue;
            }

            if events
                .send(Event::Power(PowerEvent::ActionStarted(action)))
                .await
                .is_err()
            {
                return;
            }

            match self.backend.execute(action).await {
                Ok(()) => {
                    tracing::info!(action = action.as_str(), "session action accepted");
                    if events
                        .send(Event::Power(PowerEvent::ActionCompleted(action)))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        action = action.as_str(),
                        %error,
                        "the session action failed"
                    );
                    if events
                        .send(Event::Power(PowerEvent::ActionFailed {
                            action,
                            message: error.to_string(),
                        }))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::BackendError;
    use crate::power::backend::PowerBackend;
    use crate::power::model::{PowerAction, PowerModel};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    struct FakeBackend {
        supported: Vec<PowerAction>,
        fail_probe: bool,
        executed: Mutex<Vec<PowerAction>>,
        fail: AtomicBool,
    }

    impl FakeBackend {
        fn new(supported: Vec<PowerAction>) -> Arc<Self> {
            Arc::new(Self {
                supported,
                fail_probe: false,
                executed: Mutex::new(Vec::new()),
                fail: AtomicBool::new(false),
            })
        }

        fn broken() -> Arc<Self> {
            Arc::new(Self {
                supported: Vec::new(),
                fail_probe: true,
                executed: Mutex::new(Vec::new()),
                fail: AtomicBool::new(false),
            })
        }
    }

    #[async_trait::async_trait]
    impl PowerBackend for FakeBackend {
        fn name(&self) -> &'static str {
            "fake"
        }

        async fn probe(&self) -> Result<(), BackendError> {
            if self.fail_probe {
                return Err(BackendError::Unavailable("no logind".to_string()));
            }
            Ok(())
        }

        async fn read(&self) -> Result<PowerModel, BackendError> {
            Ok(PowerModel {
                supported: self.supported.clone(),
                ..PowerModel::default()
            })
        }

        async fn execute(&self, action: PowerAction) -> Result<(), BackendError> {
            self.executed.lock().unwrap().push(action);
            if self.fail.load(Ordering::SeqCst) {
                return Err(BackendError::Message("denied".to_string()));
            }
            Ok(())
        }
    }

    async fn next_power(rx: &mut Receiver<Event>) -> PowerEvent {
        loop {
            if let Event::Power(event) = rx.recv().await.unwrap() {
                return event;
            }
        }
    }

    #[tokio::test]
    async fn startup_publishes_the_model() {
        let backend = FakeBackend::new(vec![PowerAction::Shutdown, PowerAction::Logout]);
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, mut event_rx) = async_channel::unbounded();
        tokio::spawn(PowerService::with_backend(backend).run(command_rx, event_tx));

        let event = next_power(&mut event_rx).await;
        let PowerEvent::Snapshot(model) = event else {
            panic!("expected a snapshot");
        };
        assert!(model.supports(PowerAction::Shutdown));
        assert!(!model.supports(PowerAction::Hibernate));
        drop(commands);
    }

    #[tokio::test]
    async fn an_action_reaches_the_backend_and_completes() {
        let backend = FakeBackend::new(vec![PowerAction::Suspend]);
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, mut event_rx) = async_channel::unbounded();
        let backend_ref: crate::power::backend::PowerBackendRef = backend.clone();
        tokio::spawn(PowerService::with_backend(backend_ref).run(command_rx, event_tx));

        commands
            .send(Command::PowerAction(PowerAction::Suspend))
            .await
            .unwrap();

        loop {
            if let PowerEvent::ActionCompleted(action) = next_power(&mut event_rx).await {
                assert_eq!(action, PowerAction::Suspend);
                break;
            }
        }
        assert_eq!(
            backend.executed.lock().unwrap().as_slice(),
            &[PowerAction::Suspend]
        );
    }

    #[tokio::test]
    async fn a_backend_error_is_reported() {
        let backend = FakeBackend::new(vec![PowerAction::Reboot]);
        backend.fail.store(true, Ordering::SeqCst);
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, mut event_rx) = async_channel::unbounded();
        tokio::spawn(PowerService::with_backend(backend).run(command_rx, event_tx));

        commands
            .send(Command::PowerAction(PowerAction::Reboot))
            .await
            .unwrap();

        loop {
            if let PowerEvent::ActionFailed { message, .. } = next_power(&mut event_rx).await {
                assert_eq!(message, "denied");
                break;
            }
        }
    }

    #[tokio::test]
    async fn a_missing_logind_reports_unavailable() {
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, mut event_rx) = async_channel::unbounded();
        tokio::spawn(PowerService::with_backend(FakeBackend::broken()).run(command_rx, event_tx));

        commands
            .send(Command::PowerAction(PowerAction::Shutdown))
            .await
            .unwrap();
        loop {
            if let PowerEvent::ActionFailed { message, .. } = next_power(&mut event_rx).await {
                assert!(message.contains("unavailable"));
                break;
            }
        }
    }
}
