//! The session lock service.
//!
//! Resolves the caller's logind session once, then forwards `Lock` and
//! `Unlock` to the UI. A missing logind does not stop the shell, because the
//! UI can still lock itself.

use std::sync::Arc;

use async_channel::{Receiver, Sender};

use crate::event::{Event, Service, SessionLockEvent};
use crate::session_lock::backend::SessionLockBackendRef;
use crate::session_lock::logind::LogindSessionLockBackend;

pub struct SessionLockService {
    backend: SessionLockBackendRef,
}

impl SessionLockService {
    pub fn from_env() -> Self {
        Self {
            backend: Arc::new(LogindSessionLockBackend::new()),
        }
    }

    pub fn with_backend(backend: SessionLockBackendRef) -> Self {
        Self { backend }
    }

    /// Runs until the event receiver closes or logind disconnects.
    pub async fn run(self, events: Sender<Event>) {
        let available = match self.backend.probe().await {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(
                    backend = self.backend.name(),
                    %error,
                    "logind lock requests are unavailable"
                );
                false
            }
        };

        if events
            .send(Event::ServiceAvailability(Service::SessionLock, available))
            .await
            .is_err()
        {
            return;
        }
        if !available {
            if events
                .send(Event::SessionLock(SessionLockEvent::Unavailable))
                .await
                .is_err()
            {
                return;
            }
            return;
        }

        let (sender, receiver) = async_channel::unbounded();
        let backend = Arc::clone(&self.backend);
        let watcher = tokio::spawn(async move { backend.watch(sender).await });

        forward(receiver, events).await;

        // The watcher can sit idle on a signal stream, so stopping the
        // forwarder is not enough to let the runtime finish.
        watcher.abort();
        match watcher.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "the logind lock watch stopped"),
            Err(error) if !error.is_cancelled() => {
                tracing::warn!(%error, "the logind lock watch panicked")
            }
            Err(_) => {}
        }
    }
}

async fn forward(events: Receiver<SessionLockEvent>, public: Sender<Event>) {
    loop {
        tokio::select! {
            event = events.recv() => {
                let Ok(event) = event else {
                    return;
                };
                if public.send(Event::SessionLock(event)).await.is_err() {
                    return;
                }
            }
            _ = public.closed() => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::BackendError;
    use crate::session_lock::backend::SessionLockBackend;
    use std::sync::Mutex;

    struct FakeBackend {
        events: Mutex<Option<Sender<SessionLockEvent>>>,
        available: bool,
    }

    impl FakeBackend {
        fn new(available: bool) -> Arc<Self> {
            Arc::new(Self {
                events: Mutex::new(None),
                available,
            })
        }

        async fn push(&self, event: SessionLockEvent) {
            let sender = self.events.lock().unwrap().clone();
            if let Some(sender) = sender {
                sender.send(event).await.unwrap();
            }
        }
    }

    #[async_trait::async_trait]
    impl SessionLockBackend for FakeBackend {
        fn name(&self) -> &'static str {
            "fake"
        }

        async fn probe(&self) -> Result<(), BackendError> {
            if self.available {
                Ok(())
            } else {
                Err(BackendError::Unavailable("no logind".to_string()))
            }
        }

        async fn watch(&self, events: Sender<SessionLockEvent>) -> Result<(), BackendError> {
            *self.events.lock().unwrap() = Some(events);
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn next_lock(rx: &mut Receiver<Event>) -> SessionLockEvent {
        loop {
            if let Event::SessionLock(event) = rx.recv().await.unwrap() {
                return event;
            }
        }
    }

    #[tokio::test]
    async fn logind_requests_reach_the_event_stream() {
        let backend = FakeBackend::new(true);
        let (event_tx, mut event_rx) = async_channel::unbounded();
        tokio::spawn(SessionLockService::with_backend(backend.clone()).run(event_tx));

        // The service probes before it watches, so the fake sender appears
        // after the availability event.
        loop {
            if let Event::ServiceAvailability(Service::SessionLock, true) =
                event_rx.recv().await.unwrap()
            {
                break;
            }
        }
        while backend.events.lock().unwrap().is_none() {
            tokio::task::yield_now().await;
        }

        backend.push(SessionLockEvent::Lock).await;
        assert_eq!(next_lock(&mut event_rx).await, SessionLockEvent::Lock);
    }

    #[tokio::test]
    async fn a_missing_logind_reports_unavailable() {
        let (event_tx, mut event_rx) = async_channel::unbounded();
        tokio::spawn(SessionLockService::with_backend(FakeBackend::new(false)).run(event_tx));

        loop {
            match event_rx.recv().await.unwrap() {
                Event::ServiceAvailability(Service::SessionLock, false) => break,
                _ => continue,
            }
        }
        assert_eq!(
            next_lock(&mut event_rx).await,
            SessionLockEvent::Unavailable
        );
    }
}
