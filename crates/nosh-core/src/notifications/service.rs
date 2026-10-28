//! The notification service.
//!
//! Owns the daemon role, the store, DND, daemon timeouts, and persistence. The
//! UI receives typed events and never sees D-Bus.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};
use zbus::Connection;

use crate::backoff::{Backoff, RetryPolicy};
use crate::command::Command;
use crate::event::{Event, NotificationEvent, Service, Snapshot};
use crate::notifications::backend::{
    default_candidates, DaemonRole, IncomingRequest, NotificationBackendRef, NotificationCandidate,
    NotificationSession, NotifyRequest, NOTIFICATION_INTERFACE, NOTIFICATION_PATH,
};
use crate::notifications::image::normalize_image_hints;
use crate::notifications::model::{
    parse_actions, sanitize_body, sanitize_label, ClosedReason, Notification, NotificationModel,
    NotificationSettings, NotificationState, ACTION_DISMISS, ACTION_EXPIRE, HINT_ICON_DATA,
    HINT_IMAGE_DATA_LEGACY,
};
use crate::notifications::persistence;
use crate::notifications::store::NotificationStore;
use crate::paths::NoshPaths;

/// The debounce for state writes.
pub const FLUSH_DELAY: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub struct NotificationServiceConfig {
    pub retry: RetryPolicy,
    /// A session that lasted this long resets the backoff.
    pub stable_connection: Duration,
    /// How long to wait after a change before writing the state file.
    pub flush_delay: Duration,
    pub settings: NotificationSettings,
    /// Resolved config and state paths. `None` disables persistence and the
    /// image cache.
    pub paths: Option<NoshPaths>,
}

impl Default for NotificationServiceConfig {
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            stable_connection: Duration::from_secs(5),
            flush_delay: FLUSH_DELAY,
            settings: NotificationSettings::default(),
            paths: None,
        }
    }
}

pub struct NotificationService {
    candidates: Vec<NotificationCandidate>,
    config: NotificationServiceConfig,
}

impl NotificationService {
    pub fn from_env() -> Self {
        Self::from_env_with(NotificationServiceConfig::default())
    }

    pub fn from_env_with(mut config: NotificationServiceConfig) -> Self {
        if config.paths.is_none() {
            config.paths = NoshPaths::from_env().ok();
        }
        Self::with_candidates(default_candidates(), config)
    }

    pub fn with_candidates(
        candidates: Vec<NotificationCandidate>,
        config: NotificationServiceConfig,
    ) -> Self {
        Self { candidates, config }
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let mut store = self.load_store();
        let mut settings = self.config.settings.clone();
        let mut backoff = Backoff::new(self.config.retry, seed());

        loop {
            if commands.is_closed() || events.is_closed() {
                return;
            }

            let Some(session) = self.start_session().await else {
                tracing::debug!("no notification daemon role is available");
                if report_unavailable(&events).await.is_err() {
                    return;
                }
                let delay = backoff.next_delay();
                if wait_or_closed(&commands, delay).await {
                    return;
                }
                continue;
            };

            if announce(&events, session.name, &store, &settings)
                .await
                .is_err()
            {
                return;
            }

            let connected_at = Instant::now();
            let outcome = run_session(
                session,
                store,
                settings,
                &commands,
                &events,
                self.config.paths.as_ref(),
                self.config.flush_delay,
            )
            .await;
            store = outcome.store;
            settings = outcome.settings;

            if outcome.shutdown {
                return;
            }

            if report_unavailable(&events).await.is_err() {
                return;
            }

            if connected_at.elapsed() >= self.config.stable_connection {
                backoff.reset();
            }
            let delay = backoff.next_delay();
            tracing::debug!(?delay, "reconnecting the notification daemon");
            if wait_or_closed(&commands, delay).await {
                return;
            }
        }
    }

    async fn start_session(&self) -> Option<NotificationSession> {
        for candidate in &self.candidates {
            let backend: NotificationBackendRef = candidate.create();
            if let Err(error) = backend.probe().await {
                tracing::debug!(
                    backend = candidate.name,
                    %error,
                    "notification backend probe failed"
                );
                continue;
            }

            match backend.start().await {
                Ok(session) => {
                    tracing::info!(backend = session.name, "selected notification backend");
                    return Some(session);
                }
                Err(error) => {
                    tracing::debug!(
                        backend = candidate.name,
                        %error,
                        "notification backend failed to start"
                    );
                }
            }
        }

        None
    }

    fn load_store(&self) -> NotificationStore {
        let mut store = NotificationStore::new();
        let Some(paths) = &self.config.paths else {
            return store;
        };
        match persistence::load(&paths.notifications_store()) {
            Ok(state) => persistence::restore(&mut store, state),
            Err(error) => tracing::warn!(%error, "the notification state could not be loaded"),
        }
        store
    }
}

enum Wake {
    Expire(u32),
}

struct SessionOutcome {
    store: NotificationStore,
    settings: NotificationSettings,
    shutdown: bool,
}

struct SessionRunner {
    connection: Option<Connection>,
    role: DaemonRole,
    events: Sender<Event>,
    settings: NotificationSettings,
    store: NotificationStore,
    cache_dir: Option<PathBuf>,
    store_path: Option<PathBuf>,
    wake: Sender<Wake>,
    timers: HashMap<u32, tokio::task::JoinHandle<()>>,
    flush_dirty: bool,
    closed: bool,
}

impl SessionRunner {
    async fn handle_request(&mut self, request: IncomingRequest) {
        match request {
            IncomingRequest::Notify(request) => self.notify(request).await,
            IncomingRequest::Close(id) => self.close(id, ClosedReason::Closed).await,
            IncomingRequest::ExternalClosed { id, reason } => {
                let reason = ClosedReason::from_u32(reason);
                self.cancel_timer(id);
                if self.store.remove(id).is_some() {
                    self.emit(NotificationEvent::Closed { id, reason }).await;
                    self.flush_dirty = true;
                }
            }
            IncomingRequest::ExternalAction { id, action } => {
                self.emit(NotificationEvent::Invoked { id, action }).await;
            }
        }
    }

    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::NotificationDismiss(id) => self.close(id, ClosedReason::DismissedByUser).await,
            Command::NotificationExpire(id) => self.close(id, ClosedReason::Expired).await,
            Command::NotificationInvoke { id, action } => self.invoke(id, action).await,
            Command::NotificationSetDnd(enabled) if self.settings.dont_disturb != enabled => {
                self.settings.dont_disturb = enabled;
                self.emit(NotificationEvent::DndChanged { enabled }).await;
            }
            _ => {}
        }
    }

    async fn notify(&mut self, request: NotifyRequest) {
        let timeout = self.settings.timeout_for(request.expire_timeout);
        let mut hints = request.hints;
        if let Some(cache_dir) = self.cache_dir.clone() {
            hints = tokio::task::spawn_blocking(move || {
                normalize_image_hints(&mut hints, &cache_dir);
                hints
            })
            .await
            .unwrap_or_default();
        }
        hints.remove(HINT_IMAGE_DATA_LEGACY);
        hints.remove(HINT_ICON_DATA);

        let replaced = request.replaces_id != 0 && self.store.contains(request.replaces_id);
        let id = if replaced {
            request.replaces_id
        } else {
            self.store.allocate_id()
        };
        let notification = Notification {
            id,
            app_name: sanitize_label(&request.app_name),
            app_icon: sanitize_label(&request.app_icon),
            summary: sanitize_label(&request.summary),
            body: sanitize_body(&request.body),
            actions: parse_actions(&request.actions),
            hints,
            expire_timeout: request.expire_timeout,
            time: now(),
            state: NotificationState::Received,
        };

        self.store.insert(notification.clone());
        tracing::info!(
            id,
            app = %notification.app_name,
            summary = %notification.summary,
            replaced,
            "notification received"
        );

        self.emit(NotificationEvent::Received {
            notification,
            replaced,
            suppress_popup: self.settings.dont_disturb,
        })
        .await;

        self.cancel_timer(id);
        if !self.settings.ignore_timeout && timeout > 0 {
            self.schedule_expiry(id, timeout);
        }
        self.flush_dirty = true;

        let _ = request.reply.send(id);
    }

    async fn close(&mut self, id: u32, reason: ClosedReason) {
        let removed = self.store.remove(id).is_some();
        self.cancel_timer(id);

        if removed {
            if matches!(reason, ClosedReason::DismissedByUser) {
                self.forward_dismiss(id).await;
            }
            self.emit_closed(id, reason).await;
            self.emit(NotificationEvent::Closed { id, reason }).await;
            self.flush_dirty = true;
        } else if matches!(reason, ClosedReason::DismissedByUser) {
            // A proxied daemon may hold notifications this process never saw.
            self.forward_dismiss(id).await;
        }
    }

    async fn expire(&mut self, id: u32) {
        if self.store.contains(id) {
            self.close(id, ClosedReason::Expired).await;
        }
    }

    async fn invoke(&mut self, id: u32, action: String) {
        let Some(notification) = self.store.get(id).cloned() else {
            return;
        };
        let resident = notification.is_resident();

        if matches!(self.role, DaemonRole::Owner) {
            self.emit_action(id, &action).await;
        }
        self.emit(NotificationEvent::Invoked {
            id,
            action: action.clone(),
        })
        .await;

        let reason = match action.as_str() {
            ACTION_DISMISS => Some(ClosedReason::DismissedByUser),
            ACTION_EXPIRE => Some(ClosedReason::Expired),
            _ if resident => None,
            _ => Some(ClosedReason::Closed),
        };
        if let Some(reason) = reason {
            self.close(id, reason).await;
        }
    }

    fn schedule_expiry(&mut self, id: u32, timeout_ms: i32) {
        let wake = self.wake.clone();
        let duration = Duration::from_millis(timeout_ms as u64);
        let handle = tokio::spawn(async move {
            tokio::time::sleep(duration).await;
            let _ = wake.send(Wake::Expire(id)).await;
        });
        self.timers.insert(id, handle);
    }

    fn cancel_timer(&mut self, id: u32) {
        if let Some(handle) = self.timers.remove(&id) {
            handle.abort();
        }
    }

    fn cancel_all_timers(&mut self) {
        for (_, handle) in self.timers.drain() {
            handle.abort();
        }
    }

    async fn emit(&mut self, event: NotificationEvent) {
        if self.events.send(Event::Notifications(event)).await.is_err() {
            self.closed = true;
        }
    }

    async fn emit_closed(&mut self, id: u32, reason: ClosedReason) {
        if !matches!(self.role, DaemonRole::Owner) {
            return;
        }
        let Some(connection) = &self.connection else {
            return;
        };
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            connection.emit_signal(
                None::<&str>,
                NOTIFICATION_PATH,
                NOTIFICATION_INTERFACE,
                "NotificationClosed",
                &(id, reason.as_u32()),
            ),
        )
        .await;
        if let Ok(Err(error)) = result {
            tracing::debug!(%error, "the notification close signal was not delivered");
        }
    }

    async fn emit_action(&mut self, id: u32, action: &str) {
        let Some(connection) = &self.connection else {
            return;
        };
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            connection.emit_signal(
                None::<&str>,
                NOTIFICATION_PATH,
                NOTIFICATION_INTERFACE,
                "ActionInvoked",
                &(id, action),
            ),
        )
        .await;
        if let Ok(Err(error)) = result {
            tracing::debug!(%error, "the notification action signal was not delivered");
        }
    }

    async fn forward_dismiss(&mut self, id: u32) {
        let DaemonRole::Proxy(proxy) = &self.role else {
            return;
        };
        let _ = tokio::time::timeout(
            Duration::from_secs(1),
            proxy.call_method("CloseNotification", &(id,)),
        )
        .await;
    }

    async fn persist(&mut self) {
        let Some(path) = self.store_path.clone() else {
            return;
        };
        let store = self.store.clone();
        match tokio::task::spawn_blocking(move || persistence::save(&path, &store)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "the notification state could not be saved"),
            Err(error) => tracing::warn!(%error, "the notification state write panicked"),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_session(
    session: NotificationSession,
    store: NotificationStore,
    settings: NotificationSettings,
    commands: &Receiver<Command>,
    events: &Sender<Event>,
    paths: Option<&NoshPaths>,
    flush_delay: Duration,
) -> SessionOutcome {
    let NotificationSession {
        name: _,
        connection,
        incoming,
        role,
        task,
        keepalive,
        ..
    } = session;
    let mut task = task;
    let keepalive = keepalive;
    let (wake, wake_events) = async_channel::unbounded();
    let mut runner = SessionRunner {
        connection,
        role,
        events: events.clone(),
        settings,
        store,
        cache_dir: paths.map(NoshPaths::notifications_cache),
        store_path: paths.map(NoshPaths::notifications_store),
        wake,
        timers: HashMap::new(),
        flush_dirty: false,
        closed: false,
    };

    let mut flush = Box::pin(tokio::time::sleep(Duration::from_secs(86_400)));
    let mut flush_armed = false;

    let shutdown = loop {
        tokio::select! {
            result = commands.recv() => {
                let Ok(command) = result else { break true };
                runner.handle_command(command).await;
            }
            request = incoming.recv() => {
                let Ok(request) = request else { break false };
                runner.handle_request(request).await;
            }
            wake = wake_events.recv() => {
                if let Ok(Wake::Expire(id)) = wake {
                    runner.expire(id).await;
                }
            }
            _ = &mut flush, if flush_armed => {
                flush_armed = false;
                runner.persist().await;
                runner.flush_dirty = false;
            }
        }

        if runner.closed {
            break true;
        }
        if runner.flush_dirty && !flush_armed {
            flush
                .as_mut()
                .reset(tokio::time::Instant::now() + flush_delay);
            flush_armed = true;
        }
    };

    if let Some(task) = task.take() {
        task.abort();
    }
    runner.cancel_all_timers();
    if runner.flush_dirty {
        runner.persist().await;
    }
    drop(keepalive);

    SessionOutcome {
        store: runner.store,
        settings: runner.settings,
        shutdown,
    }
}

async fn announce(
    events: &Sender<Event>,
    backend: &'static str,
    store: &NotificationStore,
    settings: &NotificationSettings,
) -> Result<(), ()> {
    let model = NotificationModel {
        notifications: store.snapshot(),
        settings: settings.clone(),
    };
    if events
        .send(Event::Notifications(NotificationEvent::Snapshot(
            model.clone(),
        )))
        .await
        .is_err()
    {
        return Err(());
    }
    let snapshot = Snapshot {
        notifications: Some(model),
        notification_backend: Some(backend),
        ..Snapshot::default()
    };
    if events.send(Event::Snapshot(snapshot)).await.is_err() {
        return Err(());
    }
    if events
        .send(Event::ServiceAvailability(Service::Notifications, true))
        .await
        .is_err()
    {
        return Err(());
    }
    Ok(())
}

async fn report_unavailable(events: &Sender<Event>) -> Result<(), ()> {
    if events
        .send(Event::Notifications(NotificationEvent::Unavailable))
        .await
        .is_err()
    {
        return Err(());
    }
    if events
        .send(Event::ServiceAvailability(Service::Notifications, false))
        .await
        .is_err()
    {
        return Err(());
    }
    Ok(())
}

async fn wait_or_closed(commands: &Receiver<Command>, duration: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => false,
        result = commands.recv() => {
            if result.is_ok() {
                tracing::debug!("dropping a command while the notification daemon is unavailable");
            }
            result.is_err()
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::oneshot;
    use tokio::sync::Mutex;

    use crate::error::BackendError;
    use crate::notifications::backend::{NotificationBackend, NotificationBackendRef};
    use crate::notifications::model::Urgency;

    struct Harness {
        requests: Sender<IncomingRequest>,
        commands: Sender<Command>,
        events: Receiver<Event>,
        _temp: tempfile::TempDir,
        handle: tokio::task::JoinHandle<()>,
    }

    impl Harness {
        fn start(settings: NotificationSettings) -> Self {
            let (requests, incoming) = async_channel::unbounded();
            let slot = Arc::new(Mutex::new(Some(incoming)));
            let make_slot = Arc::clone(&slot);
            let candidates = vec![NotificationCandidate::new("fake", move || {
                let incoming = Arc::clone(&make_slot);
                Arc::new(FactoryBackend { incoming }) as NotificationBackendRef
            })];
            let temp = tempfile::tempdir().unwrap();
            let paths = NoshPaths {
                config_dir: temp.path().join("config"),
                data_dir: temp.path().join("data"),
                cache_dir: temp.path().join("cache"),
                state_dir: temp.path().join("state"),
                runtime_dir: None,
            };
            let config = NotificationServiceConfig {
                settings,
                paths: Some(paths),
                flush_delay: Duration::from_millis(10),
                ..NotificationServiceConfig::default()
            };
            let service = NotificationService::with_candidates(candidates, config);
            let (commands, command_rx) = async_channel::unbounded();
            let (event_tx, events) = async_channel::unbounded();
            let handle = tokio::spawn(service.run(command_rx, event_tx));
            Self {
                requests,
                commands,
                events,
                _temp: temp,
                handle,
            }
        }

        async fn notify(&self, summary: &str, replaces_id: u32, timeout: i32) -> u32 {
            let (reply, response) = oneshot::channel();
            self.requests
                .send(IncomingRequest::Notify(NotifyRequest {
                    app_name: "test".to_string(),
                    replaces_id,
                    app_icon: "icon".to_string(),
                    summary: summary.to_string(),
                    body: "<b>body</b>".to_string(),
                    actions: vec!["default".to_string(), "Open".to_string()],
                    hints: HashMap::new(),
                    expire_timeout: timeout,
                    reply,
                }))
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(1), response)
                .await
                .unwrap()
                .unwrap()
        }

        async fn next_notification(&self) -> NotificationEvent {
            loop {
                let event = tokio::time::timeout(Duration::from_secs(2), self.events.recv())
                    .await
                    .expect("timed out waiting for an event")
                    .expect("the event channel closed");
                if let Event::Notifications(event) = event {
                    return event;
                }
            }
        }

        async fn close(self) {
            self.commands.close();
            let _ = tokio::time::timeout(Duration::from_secs(1), self.handle).await;
        }
    }

    struct FactoryBackend {
        incoming: Arc<Mutex<Option<Receiver<IncomingRequest>>>>,
    }

    #[async_trait]
    impl NotificationBackend for FactoryBackend {
        fn name(&self) -> &'static str {
            "fake"
        }

        async fn probe(&self) -> Result<(), BackendError> {
            Ok(())
        }

        async fn start(&self) -> Result<NotificationSession, BackendError> {
            let incoming = self.incoming.lock().await.take().ok_or_else(|| {
                BackendError::Unavailable("the fake backend was already started".to_string())
            })?;
            Ok(NotificationSession {
                name: self.name(),
                connection: None,
                incoming,
                role: DaemonRole::Null,
                capabilities: Arc::new(tokio::sync::RwLock::new(Vec::new())),
                task: None,
                keepalive: None,
            })
        }
    }

    fn received(event: NotificationEvent) -> (Notification, bool, bool) {
        match event {
            NotificationEvent::Received {
                notification,
                replaced,
                suppress_popup,
            } => (notification, replaced, suppress_popup),
            other => panic!("expected a received event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn notify_stores_the_payload_and_returns_an_id() {
        let harness = Harness::start(NotificationSettings::default());
        let snapshot = harness.next_notification().await;
        assert!(matches!(snapshot, NotificationEvent::Snapshot(_)));

        let id = harness.notify("hello", 0, 0).await;
        let (notification, replaced, suppress) = received(harness.next_notification().await);

        assert_eq!(id, 1);
        assert_eq!(notification.id, 1);
        assert_eq!(notification.summary, "hello");
        assert_eq!(notification.body, "<b>body</b>");
        assert_eq!(notification.actions[0].label, "Open");
        assert!(!replaced);
        assert!(!suppress);
        assert_eq!(notification.urgency(), Urgency::Normal);

        harness.close().await;
    }

    #[tokio::test]
    async fn a_replacement_reuses_the_id() {
        let harness = Harness::start(NotificationSettings::default());
        let _ = harness.next_notification().await;

        assert_eq!(harness.notify("first", 0, 0).await, 1);
        let _ = received(harness.next_notification().await);

        assert_eq!(harness.notify("second", 1, 0).await, 1);
        let (notification, replaced, _) = received(harness.next_notification().await);
        assert!(replaced);
        assert_eq!(notification.summary, "second");

        harness.close().await;
    }

    #[tokio::test]
    async fn dnd_suppresses_the_popup_and_stores_the_notification() {
        let settings = NotificationSettings {
            dont_disturb: true,
            ..NotificationSettings::default()
        };
        let harness = Harness::start(settings);
        let _ = harness.next_notification().await;

        harness.notify("quiet", 0, 0).await;
        let (_, _, suppress) = received(harness.next_notification().await);
        assert!(suppress);

        harness.close().await;
    }

    #[tokio::test]
    async fn the_dnd_command_emits_a_change() {
        let harness = Harness::start(NotificationSettings::default());
        let _ = harness.next_notification().await;

        harness
            .commands
            .send(Command::NotificationSetDnd(true))
            .await
            .unwrap();
        assert_eq!(
            harness.next_notification().await,
            NotificationEvent::DndChanged { enabled: true }
        );

        harness.close().await;
    }

    #[tokio::test]
    async fn dismiss_removes_the_notification() {
        let harness = Harness::start(NotificationSettings::default());
        let _ = harness.next_notification().await;
        let id = harness.notify("dismiss me", 0, 0).await;
        let _ = received(harness.next_notification().await);

        harness
            .commands
            .send(Command::NotificationDismiss(id))
            .await
            .unwrap();

        assert_eq!(
            harness.next_notification().await,
            NotificationEvent::Closed {
                id,
                reason: ClosedReason::DismissedByUser,
            }
        );

        harness.close().await;
    }

    #[tokio::test]
    async fn a_daemon_timeout_expires_the_notification() {
        let harness = Harness::start(NotificationSettings::default());
        let _ = harness.next_notification().await;
        let id = harness.notify("short", 0, 20).await;
        let _ = received(harness.next_notification().await);

        assert_eq!(
            harness.next_notification().await,
            NotificationEvent::Closed {
                id,
                reason: ClosedReason::Expired,
            }
        );

        harness.close().await;
    }

    #[tokio::test]
    async fn ignore_timeout_disables_daemon_expiry() {
        let settings = NotificationSettings {
            ignore_timeout: true,
            ..NotificationSettings::default()
        };
        let harness = Harness::start(settings);
        let _ = harness.next_notification().await;
        let _ = harness.notify("stays", 0, 10).await;
        let _ = received(harness.next_notification().await);

        let event = tokio::time::timeout(Duration::from_millis(100), harness.events.recv()).await;
        assert!(event.is_err());

        harness.close().await;
    }

    #[tokio::test]
    async fn invoking_an_action_closes_a_non_resident_notification() {
        let harness = Harness::start(NotificationSettings::default());
        let _ = harness.next_notification().await;
        let id = harness.notify("action", 0, 0).await;
        let _ = received(harness.next_notification().await);

        harness
            .commands
            .send(Command::NotificationInvoke {
                id,
                action: "default".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(
            harness.next_notification().await,
            NotificationEvent::Invoked {
                id,
                action: "default".to_string(),
            }
        );
        assert_eq!(
            harness.next_notification().await,
            NotificationEvent::Closed {
                id,
                reason: ClosedReason::Closed,
            }
        );

        harness.close().await;
    }

    #[tokio::test]
    async fn a_resident_notification_survives_an_action() {
        let settings = NotificationSettings {
            ignore_timeout: true,
            ..NotificationSettings::default()
        };
        let harness = Harness::start(settings);
        let _ = harness.next_notification().await;

        let (reply, response) = oneshot::channel();
        let mut hints = HashMap::new();
        hints.insert(
            crate::notifications::model::HINT_RESIDENT.to_string(),
            zbus::zvariant::OwnedValue::from(true),
        );
        harness
            .requests
            .send(IncomingRequest::Notify(NotifyRequest {
                app_name: "test".to_string(),
                replaces_id: 0,
                app_icon: String::new(),
                summary: "resident".to_string(),
                body: String::new(),
                actions: Vec::new(),
                hints,
                expire_timeout: 0,
                reply,
            }))
            .await
            .unwrap();
        let id = response.await.unwrap();
        let _ = received(harness.next_notification().await);

        harness
            .commands
            .send(Command::NotificationInvoke {
                id,
                action: "default".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(
            harness.next_notification().await,
            NotificationEvent::Invoked {
                id,
                action: "default".to_string(),
            }
        );
        let event = tokio::time::timeout(Duration::from_millis(100), harness.events.recv()).await;
        assert!(event.is_err());

        harness.close().await;
    }
}
