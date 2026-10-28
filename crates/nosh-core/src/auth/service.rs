//! The authentication service.
//!
//! The service answers one password check at a time. PAM runs on a blocking
//! thread, and a failed attempt rate limit keeps a script from guessing.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};

use crate::auth::backend::{AuthBackendRef, PamBackend};
use crate::auth::model::{AuthRequest, AuthResult};
use crate::command::Command;
use crate::event::{AuthEvent, Event, Service};

#[derive(Debug, Clone)]
pub struct AuthServiceConfig {
    /// The PAM service stack. See `docs/features/auth.md`.
    pub service: String,
    /// Use the `login` stack when the configured service file is missing.
    pub allow_fallback: bool,
    /// Failed attempts before the next try is rejected.
    pub lockout_attempts: u32,
    /// How long the lockout lasts.
    pub lockout: Duration,
}

impl Default for AuthServiceConfig {
    fn default() -> Self {
        Self {
            service: "nosh-auth".to_string(),
            allow_fallback: true,
            lockout_attempts: 5,
            lockout: Duration::from_secs(30),
        }
    }
}

pub struct AuthService {
    backend: AuthBackendRef,
    config: AuthServiceConfig,
}

impl AuthService {
    pub fn from_env_with(config: AuthServiceConfig) -> Self {
        let backend = Arc::new(PamBackend::new(&config.service, config.allow_fallback));
        Self { backend, config }
    }

    pub fn with_backend(backend: AuthBackendRef, config: AuthServiceConfig) -> Self {
        Self { backend, config }
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let available = match self.backend.probe() {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(
                    backend = self.backend.name(),
                    %error,
                    "password prompts are unavailable"
                );
                false
            }
        };

        if events
            .send(Event::ServiceAvailability(Service::Auth, available))
            .await
            .is_err()
        {
            return;
        }

        let mut attempts = Attempts::new(self.config.lockout_attempts, self.config.lockout);

        while let Ok(command) = commands.recv().await {
            let Command::AuthAuthenticate {
                request_id,
                username,
                password,
            } = command
            else {
                continue;
            };

            let result = if !available {
                AuthResult::failed(request_id, "Password prompts are unavailable")
            } else if let Some(remaining) = attempts.remaining() {
                AuthResult::failed(
                    request_id,
                    format!(
                        "Too many attempts. Try again in {} seconds.",
                        remaining.as_secs().max(1)
                    ),
                )
            } else {
                let backend = Arc::clone(&self.backend);
                let request = AuthRequest { username, password };
                let outcome =
                    tokio::task::spawn_blocking(move || backend.authenticate(&request)).await;

                match outcome {
                    Ok(Ok(())) => {
                        attempts.succeeded();
                        AuthResult::success(request_id)
                    }
                    Ok(Err(error)) => {
                        attempts.failed();
                        AuthResult::failed(request_id, error.to_string())
                    }
                    Err(error) => {
                        tracing::warn!(%error, "the PAM worker stopped");
                        AuthResult::failed(request_id, "The password check did not finish")
                    }
                }
            };

            if events
                .send(Event::Auth(AuthEvent::Finished(result)))
                .await
                .is_err()
            {
                return;
            }
        }
    }
}

/// Failed-attempt accounting.
struct Attempts {
    failures: u32,
    limit: u32,
    lockout: Duration,
    locked_until: Option<Instant>,
}

impl Attempts {
    fn new(limit: u32, lockout: Duration) -> Self {
        Self {
            failures: 0,
            limit,
            lockout,
            locked_until: None,
        }
    }

    fn succeeded(&mut self) {
        self.failures = 0;
        self.locked_until = None;
    }

    fn failed(&mut self) {
        self.failures += 1;
        if self.limit > 0 && self.failures >= self.limit {
            self.failures = 0;
            self.locked_until = Some(Instant::now() + self.lockout);
        }
    }

    /// The remaining lockout, or `None` when another attempt is allowed.
    fn remaining(&mut self) -> Option<Duration> {
        let until = self.locked_until?;
        let now = Instant::now();
        if now >= until {
            self.locked_until = None;
            return None;
        }
        Some(until - now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::model::Secret;
    use crate::event::AuthEvent;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct FakeBackend {
        password: Mutex<String>,
        calls: AtomicUsize,
    }

    impl FakeBackend {
        fn new(password: &str) -> Arc<Self> {
            Arc::new(Self {
                password: Mutex::new(password.to_string()),
                calls: AtomicUsize::new(0),
            })
        }
    }

    impl crate::auth::backend::AuthBackend for FakeBackend {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn probe(&self) -> Result<(), crate::error::BackendError> {
            Ok(())
        }

        fn authenticate(&self, request: &AuthRequest) -> Result<(), crate::error::BackendError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if request.password.as_bytes() == self.password.lock().unwrap().as_bytes() {
                Ok(())
            } else {
                Err(crate::error::BackendError::Message(
                    "Authentication failure".to_string(),
                ))
            }
        }
    }

    fn config() -> AuthServiceConfig {
        AuthServiceConfig {
            lockout_attempts: 3,
            lockout: Duration::from_secs(30),
            ..AuthServiceConfig::default()
        }
    }

    fn request(id: u64, password: &str) -> Command {
        Command::AuthAuthenticate {
            request_id: id,
            username: "user".to_string(),
            password: Secret::new(password),
        }
    }

    async fn next_result(rx: &mut Receiver<Event>) -> AuthResult {
        loop {
            if let Event::Auth(AuthEvent::Finished(result)) = rx.recv().await.unwrap() {
                return result;
            }
        }
    }

    #[tokio::test]
    async fn a_correct_password_succeeds() {
        let backend = FakeBackend::new("secret");
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, mut event_rx) = async_channel::unbounded();
        tokio::spawn(AuthService::with_backend(backend, config()).run(command_rx, event_tx));

        commands.send(request(7, "secret")).await.unwrap();
        let result = next_result(&mut event_rx).await;
        assert!(result.is_success());
        assert_eq!(result.request_id, 7);
    }

    #[tokio::test]
    async fn a_wrong_password_reports_the_backend_message() {
        let backend = FakeBackend::new("secret");
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, mut event_rx) = async_channel::unbounded();
        tokio::spawn(AuthService::with_backend(backend, config()).run(command_rx, event_tx));

        commands.send(request(1, "wrong")).await.unwrap();
        let result = next_result(&mut event_rx).await;
        assert!(!result.is_success());
        assert_eq!(result.error.as_deref(), Some("Authentication failure"));
    }

    #[tokio::test]
    async fn repeated_failures_lock_the_service() {
        let backend = FakeBackend::new("secret");
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, mut event_rx) = async_channel::unbounded();
        let backend_ref: crate::auth::backend::AuthBackendRef = backend.clone();
        tokio::spawn(AuthService::with_backend(backend_ref, config()).run(command_rx, event_tx));

        for id in 0..3 {
            commands.send(request(id, "wrong")).await.unwrap();
            let _ = next_result(&mut event_rx).await;
        }

        commands.send(request(3, "secret")).await.unwrap();
        let result = next_result(&mut event_rx).await;
        assert!(!result.is_success());
        assert_eq!(backend.calls.load(Ordering::SeqCst), 3);
        assert!(result.error.unwrap().contains("Too many attempts"));
    }

    #[tokio::test]
    async fn an_unavailable_backend_fails_every_request() {
        struct Missing;

        impl crate::auth::backend::AuthBackend for Missing {
            fn name(&self) -> &'static str {
                "missing"
            }

            fn probe(&self) -> Result<(), crate::error::BackendError> {
                Err(crate::error::BackendError::Unavailable("gone".to_string()))
            }

            fn authenticate(
                &self,
                _request: &AuthRequest,
            ) -> Result<(), crate::error::BackendError> {
                panic!("the backend must not be called when it is unavailable");
            }
        }

        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, mut event_rx) = async_channel::unbounded();
        tokio::spawn(
            AuthService::with_backend(Arc::new(Missing), config()).run(command_rx, event_tx),
        );

        commands.send(request(9, "secret")).await.unwrap();
        let result = next_result(&mut event_rx).await;
        assert_eq!(
            result.error.as_deref(),
            Some("Password prompts are unavailable")
        );
    }
}
