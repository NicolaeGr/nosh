//! Shared connection to the running Hyprland instance.
//!
//! One process-wide [`Hyprland`] owns the command client, the provider cache,
//! the transaction lock, and the single event reader. Every caller that detects
//! the same socket paths receives the same `Arc`.

use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};
use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::timeout;

use crate::backoff::{Backoff, RetryPolicy};
use crate::error::{BackendError, PathError};
use crate::hyprland::client::HyprlandClient;
use crate::hyprland::dispatch::{detect_provider, format_dispatch, ConfigProvider, Dispatch};
use crate::hyprland::events::{parse_event, HyprlandEvent};
use crate::hyprland::json::{HyprBind, HyprStatus, HyprVersion};
use crate::paths::HyprlandPaths;

const EVENT_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const EVENT_CHANNEL_CAPACITY: usize = 256;
const BACKOFF_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

static CONNECTION: OnceLock<StdMutex<Option<Arc<Hyprland>>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HyprlandState {
    Connecting,
    Ready,
    Lost,
}

pub struct Hyprland {
    paths: HyprlandPaths,
    client: HyprlandClient,
    transaction: tokio::sync::Mutex<()>,
    provider: tokio::sync::RwLock<Option<ConfigProvider>>,
    state: tokio::sync::watch::Sender<HyprlandState>,
    events: tokio::sync::broadcast::Sender<HyprlandEvent>,
    started: std::sync::Mutex<bool>,
}

impl Hyprland {
    /// Returns the cached connection for the current socket paths.
    ///
    /// This is the only place that reads `HyprlandPaths::from_env`. A different
    /// path set replaces the cached connection.
    pub fn detect() -> Result<Option<Arc<Self>>, PathError> {
        let Some(paths) = HyprlandPaths::from_env()? else {
            return Ok(None);
        };

        let cache = CONNECTION.get_or_init(|| StdMutex::new(None));
        let mut cached = cache.lock().unwrap_or_else(|error| error.into_inner());

        let connection = match cached.as_ref() {
            Some(existing) if existing.paths == paths => Arc::clone(existing),
            _ => {
                let connection = Self::connect(paths);
                *cached = Some(Arc::clone(&connection));
                connection
            }
        };

        Ok(Some(connection))
    }

    pub fn connect(paths: HyprlandPaths) -> Arc<Self> {
        let client = HyprlandClient::new(paths.command_socket.clone());
        let (state, _) = tokio::sync::watch::channel(HyprlandState::Connecting);
        let (events, _) = tokio::sync::broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let connection = Arc::new(Self {
            paths,
            client,
            transaction: tokio::sync::Mutex::new(()),
            provider: tokio::sync::RwLock::new(None),
            state,
            events,
            started: std::sync::Mutex::new(false),
        });
        connection.start();
        connection
    }

    /// Starts the event reader once. Without a Tokio runtime the reader is not
    /// started, and [`Self::start`] can be called again later.
    pub fn start(self: &Arc<Self>) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::debug!("no Tokio runtime: Hyprland event reader not started");
            return;
        };

        {
            let mut started = self
                .started
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if *started {
                return;
            }
            *started = true;
        }

        handle.spawn(read_events(Arc::downgrade(self)));
    }

    pub fn paths(&self) -> &HyprlandPaths {
        &self.paths
    }

    pub fn state(&self) -> tokio::sync::watch::Receiver<HyprlandState> {
        self.state.subscribe()
    }

    pub fn events(&self) -> tokio::sync::broadcast::Receiver<HyprlandEvent> {
        self.events.subscribe()
    }

    pub async fn refresh_provider(&self) -> ConfigProvider {
        let provider = self.query_provider().await;
        *self.provider.write().await = Some(provider);
        provider
    }

    pub async fn invalidate_provider(&self) {
        *self.provider.write().await = None;
    }

    pub async fn provider(&self) -> ConfigProvider {
        if let Some(provider) = *self.provider.read().await {
            return provider;
        }

        self.refresh_provider().await
    }

    async fn query_provider(&self) -> ConfigProvider {
        let status: Option<HyprStatus> = match self.client.query_json("j/status").await {
            Ok(status) => Some(status),
            Err(error) => {
                tracing::debug!(%error, "Hyprland status query failed");
                None
            }
        };
        let version: Option<HyprVersion> = match self.client.query_json("j/version").await {
            Ok(version) => Some(version),
            Err(error) => {
                tracing::debug!(%error, "Hyprland version query failed");
                None
            }
        };

        let provider = detect_provider(
            status
                .as_ref()
                .and_then(|status| status.config_provider.as_deref()),
            version.as_ref().map(|version| version.tag.as_str()),
        );
        tracing::debug!(?provider, "detected Hyprland config provider");
        provider
    }

    pub async fn request(&self, request: &str) -> Result<String, BackendError> {
        let _transaction = self.transaction.lock().await;
        self.client.query(request).await
    }

    pub async fn query_json<T>(&self, request: &str) -> Result<T, BackendError>
    where
        T: DeserializeOwned,
    {
        let response = self.request(request).await?;
        serde_json::from_str(&response).map_err(|error| BackendError::Json(error.to_string()))
    }

    pub async fn dispatch(&self, action: Dispatch<'_>) -> Result<(), BackendError> {
        let _transaction = self.transaction.lock().await;
        let provider = self.provider().await;
        let request = format_dispatch(provider, action);
        let response = self.client.query(&request).await?;
        expect_ok(&request, response)
    }

    /// Sends each request verbatim through one transaction.
    pub async fn requests(&self, requests: &[String]) -> Result<(), BackendError> {
        let _transaction = self.transaction.lock().await;
        for request in requests {
            let response = self.client.query(request).await?;
            expect_ok(request, response)?;
        }
        Ok(())
    }

    pub async fn binds(&self) -> Result<Vec<HyprBind>, BackendError> {
        self.query_json("j/binds").await
    }
}

fn expect_ok(request: &str, response: String) -> Result<(), BackendError> {
    if response.trim().eq_ignore_ascii_case("ok") {
        Ok(())
    } else {
        Err(BackendError::Request(format!(
            "Hyprland rejected `{request}`: {}",
            response.trim()
        )))
    }
}

async fn read_events(connection: Weak<Hyprland>) {
    let mut backoff = Backoff::new(RetryPolicy::default(), BACKOFF_SEED);

    loop {
        let Some(connection) = connection.upgrade() else {
            return;
        };

        connection.state.send_replace(HyprlandState::Connecting);
        let socket = connection.paths.event_socket.clone();

        let stream = match timeout(EVENT_CONNECT_TIMEOUT, UnixStream::connect(&socket)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => {
                tracing::debug!(%error, socket = %socket.display(), "cannot connect to the Hyprland event socket");
                connection.state.send_replace(HyprlandState::Lost);
                drop(connection);
                tokio::time::sleep(backoff.next_delay()).await;
                continue;
            }
            Err(_) => {
                tracing::debug!(socket = %socket.display(), "timed out connecting to the Hyprland event socket");
                connection.state.send_replace(HyprlandState::Lost);
                drop(connection);
                tokio::time::sleep(backoff.next_delay()).await;
                continue;
            }
        };

        connection.state.send_replace(HyprlandState::Ready);
        backoff.reset();
        let _ = connection.events.send(HyprlandEvent::Connected);

        let mut lines = BufReader::new(stream).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }

                    let event = match parse_event(&line) {
                        Ok(event) => event,
                        Err(error) => {
                            tracing::debug!(%error, line, "ignoring malformed Hyprland event");
                            continue;
                        }
                    };

                    if matches!(event, HyprlandEvent::ConfigReloaded) {
                        connection.invalidate_provider().await;
                    }

                    let _ = connection.events.send(event);
                }
                Ok(None) => {
                    tracing::debug!("Hyprland event socket closed");
                    connection.state.send_replace(HyprlandState::Lost);
                    break;
                }
                Err(error) => {
                    tracing::debug!(%error, "cannot read the Hyprland event socket");
                    connection.state.send_replace(HyprlandState::Lost);
                    break;
                }
            }
        }

        drop(connection);
        tokio::time::sleep(backoff.next_delay()).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixListener;

    use super::*;

    fn paths(directory: &tempfile::TempDir) -> HyprlandPaths {
        HyprlandPaths {
            command_socket: directory.path().join("command.sock"),
            event_socket: directory.path().join("event.sock"),
        }
    }

    async fn next_event(
        receiver: &mut tokio::sync::broadcast::Receiver<HyprlandEvent>,
    ) -> HyprlandEvent {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
                .await
                .unwrap()
                .unwrap();
            if event != HyprlandEvent::Connected {
                return event;
            }
        }
    }

    #[tokio::test]
    async fn two_subscribers_receive_the_same_event_from_one_reader() {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(&paths(&directory).event_socket).unwrap();
        let connection = Hyprland::connect(paths(&directory));

        let mut first = connection.events();
        let mut second = connection.events();

        let (mut stream, _) = listener.accept().await.unwrap();
        stream.write_all(b"workspacev2>>1,1\n").await.unwrap();
        stream.flush().await.unwrap();

        let expected = HyprlandEvent::Workspace {
            id: 1,
            name: "1".to_string(),
        };
        assert_eq!(next_event(&mut first).await, expected);
        assert_eq!(next_event(&mut second).await, expected);
    }

    #[tokio::test]
    async fn the_state_goes_ready_then_lost_when_the_peer_drops() {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(&paths(&directory).event_socket).unwrap();
        let connection = Hyprland::connect(paths(&directory));
        let mut state = connection.state();

        let (stream, _) = listener.accept().await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while *state.borrow_and_update() != HyprlandState::Ready {
                state.changed().await.unwrap();
            }
        })
        .await
        .unwrap();

        drop(listener);
        drop(stream);

        tokio::time::timeout(Duration::from_secs(2), async {
            while *state.borrow_and_update() != HyprlandState::Lost {
                state.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_missing_event_socket_reports_lost_within_two_seconds() {
        let directory = tempfile::tempdir().unwrap();
        let connection = Hyprland::connect(paths(&directory));
        let mut state = connection.state();

        tokio::time::timeout(Duration::from_secs(2), async {
            while *state.borrow_and_update() != HyprlandState::Lost {
                state.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    }
}
