//! The tray service.
//!
//! Resolves the watcher role, keeps one proxy per item, and owns every
//! DBusMenu call. Every blocking risk is bounded: menu calls have a timeout and
//! property refreshes are debounced.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};
use futures_util::stream::{select_all, SelectAll};
use futures_util::StreamExt;
use zbus::proxy::OwnerChangedStream;
use zbus::proxy::SignalStream;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{Connection, Proxy};

use crate::backoff::{Backoff, RetryPolicy};
use crate::command::Command;
use crate::error::BackendError;
use crate::event::{Event, Service, Snapshot, TrayEvent};
use crate::tray::focus::{HyprlandFocus, WindowFocus, WindowFocusRef, WindowHint};
use crate::tray::icon::{largest_pixmap, IconCache};
use crate::tray::menu::{DbusMenuClient, MENU_TIMEOUT};
use crate::tray::model::{get, TrayCategory, TrayItemModel, TrayMenu, TrayStatus};
use crate::tray::registration::Registration;
use crate::tray::watcher::{
    default_candidates, TrayCandidate, WatcherBackendRef, WatcherChange, WatcherSession,
};

pub const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
pub const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
pub const DBUS_SERVICE: &str = "org.freedesktop.DBus";
pub const DBUS_PATH: &str = "/org/freedesktop/DBus";
pub const DBUS_INTERFACE: &str = "org.freedesktop.DBus";

/// The number of item commands that may wait before new ones are dropped.
const COMMAND_CAPACITY: usize = 32;

/// The longest a focus lookup may take before the click falls back to
/// `Activate`.
const FOCUS_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct TrayServiceConfig {
    pub retry: RetryPolicy,
    /// A session that lasted this long resets the backoff.
    pub stable_connection: Duration,
    /// How long to wait after a `New*` signal before re-reading properties.
    pub debounce: Duration,
    /// The longest one item property read or activation call may take.
    pub item_timeout: Duration,
    pub menu_timeout: Duration,
}

impl Default for TrayServiceConfig {
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            stable_connection: Duration::from_secs(5),
            debounce: Duration::from_millis(50),
            item_timeout: Duration::from_secs(2),
            menu_timeout: MENU_TIMEOUT,
        }
    }
}

pub struct TrayService {
    candidates: Vec<TrayCandidate>,
    config: TrayServiceConfig,
    focus: Option<WindowFocusRef>,
}

impl TrayService {
    pub fn from_env() -> Self {
        Self::from_env_with(TrayServiceConfig::default())
    }

    pub fn from_env_with(config: TrayServiceConfig) -> Self {
        let mut service = Self::with_candidates(default_candidates(), config);
        service.focus = HyprlandFocus::from_env().map(|focus| Arc::new(focus) as WindowFocusRef);
        service
    }

    pub fn with_candidates(candidates: Vec<TrayCandidate>, config: TrayServiceConfig) -> Self {
        Self {
            candidates,
            config,
            focus: None,
        }
    }

    /// Replaces the window focus backend. `None` disables focus resolution and
    /// makes every activation go straight to the item.
    pub fn with_focus(mut self, focus: Option<WindowFocusRef>) -> Self {
        self.focus = focus;
        self
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let mut backoff = Backoff::new(self.config.retry, seed());
        let focus = self.focus.clone();

        loop {
            if commands.is_closed() || events.is_closed() {
                return;
            }

            let Some(session) = self.start_session().await else {
                tracing::debug!("no tray watcher backend is available");
                if report_unavailable(&events).await.is_err() {
                    return;
                }
                let delay = backoff.next_delay();
                if wait_or_closed(&commands, delay).await {
                    return;
                }
                continue;
            };

            if announce_session(&events, session.name).await.is_err() {
                return;
            }

            let connected_at = Instant::now();
            if run_session(session, &commands, &events, &self.config, focus.clone())
                .await
                .is_err()
            {
                return;
            }

            if report_unavailable(&events).await.is_err() {
                return;
            }

            if connected_at.elapsed() >= self.config.stable_connection {
                backoff.reset();
            }
            let delay = backoff.next_delay();
            tracing::debug!(?delay, "reconnecting the tray watcher");
            if wait_or_closed(&commands, delay).await {
                return;
            }
        }
    }

    async fn start_session(&self) -> Option<WatcherSession> {
        for candidate in &self.candidates {
            let backend: WatcherBackendRef = candidate.create();
            if let Err(error) = backend.probe().await {
                tracing::debug!(
                    backend = candidate.name,
                    %error,
                    "tray backend probe failed"
                );
                continue;
            }

            match backend.start().await {
                Ok(session) => {
                    tracing::info!(backend = session.name, "selected tray backend");
                    return Some(session);
                }
                Err(error) => {
                    tracing::debug!(
                        backend = candidate.name,
                        %error,
                        "tray backend failed to start"
                    );
                }
            }
        }

        None
    }
}

async fn run_session(
    session: WatcherSession,
    commands: &Receiver<Command>,
    events: &Sender<Event>,
    config: &TrayServiceConfig,
    focus: Option<WindowFocusRef>,
) -> Result<(), ()> {
    let WatcherSession {
        name: backend,
        connection,
        changes,
        mut task,
    } = session;

    let (item_events_tx, item_events) = async_channel::unbounded();
    let icons = Arc::new(IconCache::default());
    let mut items: BTreeMap<String, ItemRuntime> = BTreeMap::new();
    let mut models: Vec<TrayItemModel> = Vec::new();
    let mut menus: HashMap<String, TrayMenu> = HashMap::new();

    let mut owner_changes = watch_name_owner_changes(&connection).await;

    loop {
        tokio::select! {
            change = changes.recv() => {
                match change {
                    Ok(WatcherChange::Registered(registration)) => {
                        let item_id = registration.item_id();
                        if items.contains_key(&item_id) {
                            continue;
                        }

                        // An item can register once with its well-known name and
                        // once with its unique name plus a path. Resolve both to
                        // the same owner so only one button appears.
                        let dedupe_key = tokio::time::timeout(
                            config.item_timeout,
                            registration_key(&connection, &registration),
                        )
                        .await
                        .unwrap_or_else(|_| dedupe_key(None, &registration));
                        if items
                            .values()
                            .any(|runtime| runtime.dedupe_key == dedupe_key)
                        {
                            tracing::debug!(item_id, "ignoring a duplicate tray registration");
                            continue;
                        }

                        let (command_tx, command_rx) = async_channel::bounded(COMMAND_CAPACITY);
                        let item_task = tokio::spawn(run_item(
                            connection.clone(),
                            registration.clone(),
                            command_rx,
                            item_events_tx.clone(),
                            config.clone(),
                            Arc::clone(&icons),
                            focus.clone(),
                        ));
                        items.insert(
                            item_id,
                            ItemRuntime {
                                bus_name: registration.bus_name,
                                dedupe_key,
                                command_tx,
                                task: item_task,
                            },
                        );
                    }
                    Ok(WatcherChange::Unregistered(item_id)) => {
                        remove_item(
                            &item_id,
                            &mut items,
                            &mut models,
                            &mut menus,
                            events,
                            backend,
                        )
                        .await?;
                    }
                    Err(_) => break,
                }
            }
            command = commands.recv() => {
                match command {
                    Ok(command) => route_command(command, &items),
                    Err(_) => return Err(()),
                }
            }
            event = item_events.recv() => {
                let Ok(event) = event else { break };
                apply_item_event(
                    event,
                    &mut items,
                    &mut models,
                    &mut menus,
                    events,
                    backend,
                )
                .await?;
            }
            _ = &mut task => {
                tracing::debug!(backend, "the tray watcher session ended");
                break;
            }
            owner = next_name_owner_changed(&mut owner_changes) => {
                let Some((name, _, new_owner)) = owner else { break };
                if !new_owner.is_empty() {
                    continue;
                }
                // A bus name lost its owner. Remove every item it owned.
                let removed: Vec<String> = items
                    .iter()
                    .filter(|(_, runtime)| runtime.bus_name == name)
                    .map(|(item_id, _)| item_id.clone())
                    .collect();
                for item_id in removed {
                    remove_item(
                        &item_id,
                        &mut items,
                        &mut models,
                        &mut menus,
                        events,
                        backend,
                    )
                    .await?;
                }
            }
        }
    }

    for (_, runtime) in std::mem::take(&mut items) {
        runtime.task.abort();
    }

    Ok(())
}

async fn apply_item_event(
    event: TrayItemEvent,
    items: &mut BTreeMap<String, ItemRuntime>,
    models: &mut Vec<TrayItemModel>,
    menus: &mut HashMap<String, TrayMenu>,
    events: &Sender<Event>,
    backend: &'static str,
) -> Result<(), ()> {
    match event {
        TrayItemEvent::Added { item, menu } => {
            let item_id = item.item_id.clone();
            upsert_model(models, item.clone());
            match menu.clone() {
                Some(menu) => {
                    menus.insert(item_id, menu);
                }
                None => {
                    menus.remove(&item_id);
                }
            }
            if events
                .send(Event::Tray(TrayEvent::ItemAdded { item, menu }))
                .await
                .is_err()
            {
                return Err(());
            }
            emit_snapshot(events, models, backend).await?;
        }
        TrayItemEvent::Updated(item) => {
            upsert_model(models, item.clone());
            if events
                .send(Event::Tray(TrayEvent::ItemUpdated { item }))
                .await
                .is_err()
            {
                return Err(());
            }
        }
        TrayItemEvent::Menu { item_id, menu } => {
            match menu.clone() {
                Some(menu) => {
                    menus.insert(item_id.clone(), menu);
                }
                None => {
                    menus.remove(&item_id);
                }
            }
            if events
                .send(Event::Tray(TrayEvent::MenuUpdated { item_id, menu }))
                .await
                .is_err()
            {
                return Err(());
            }
        }
        TrayItemEvent::Failed { item_id, error } => {
            tracing::debug!(item_id, %error, "a tray item could not be read");
            if let Some(runtime) = items.remove(&item_id) {
                runtime.task.abort();
            }
            models.retain(|model| model.item_id != item_id);
            menus.remove(&item_id);
        }
        TrayItemEvent::Removed(item_id) => {
            if let Some(runtime) = items.remove(&item_id) {
                runtime.task.abort();
            }
            if models.iter().any(|model| model.item_id == item_id) {
                models.retain(|model| model.item_id != item_id);
                menus.remove(&item_id);
                if events
                    .send(Event::Tray(TrayEvent::ItemRemoved {
                        item_id: item_id.clone(),
                    }))
                    .await
                    .is_err()
                {
                    return Err(());
                }
                emit_snapshot(events, models, backend).await?;
            }
        }
    }

    Ok(())
}

async fn remove_item(
    item_id: &str,
    items: &mut BTreeMap<String, ItemRuntime>,
    models: &mut Vec<TrayItemModel>,
    menus: &mut HashMap<String, TrayMenu>,
    events: &Sender<Event>,
    backend: &'static str,
) -> Result<(), ()> {
    if let Some(runtime) = items.remove(item_id) {
        runtime.task.abort();
    }

    let was_visible = models.iter().any(|model| model.item_id == item_id);
    models.retain(|model| model.item_id != item_id);
    menus.remove(item_id);

    if was_visible {
        if events
            .send(Event::Tray(TrayEvent::ItemRemoved {
                item_id: item_id.to_string(),
            }))
            .await
            .is_err()
        {
            return Err(());
        }
        emit_snapshot(events, models, backend).await?;
    }

    Ok(())
}

fn route_command(command: Command, items: &BTreeMap<String, ItemRuntime>) {
    let (item_id, item_command) = match command {
        Command::TrayActivate { item_id, x, y } => (item_id, TrayItemCommand::Activate { x, y }),
        Command::TraySecondaryActivate { item_id, x, y } => {
            (item_id, TrayItemCommand::SecondaryActivate { x, y })
        }
        Command::TrayScroll {
            item_id,
            delta,
            horizontal,
        } => (item_id, TrayItemCommand::Scroll { delta, horizontal }),
        Command::TrayMenuAboutToShow { item_id } => (item_id, TrayItemCommand::MenuAboutToShow),
        Command::TrayMenuEvent {
            item_id,
            menu_id,
            event,
        } => (item_id, TrayItemCommand::MenuEvent { menu_id, event }),
        _ => return,
    };

    let Some(runtime) = items.get(&item_id) else {
        tracing::debug!(item_id, "a tray command arrived for a missing item");
        return;
    };
    if let Err(error) = runtime.command_tx.try_send(item_command) {
        tracing::debug!(%error, item_id, "a tray item command was dropped");
    }
}

fn upsert_model(models: &mut Vec<TrayItemModel>, item: TrayItemModel) {
    if let Some(existing) = models
        .iter_mut()
        .find(|existing| existing.item_id == item.item_id)
    {
        *existing = item;
    } else {
        models.push(item);
    }
}

async fn announce_session(events: &Sender<Event>, backend: &'static str) -> Result<(), ()> {
    if events
        .send(Event::ServiceAvailability(Service::Tray, true))
        .await
        .is_err()
    {
        return Err(());
    }
    if events
        .send(Event::Tray(TrayEvent::BackendChanged { backend }))
        .await
        .is_err()
    {
        return Err(());
    }
    emit_snapshot(events, &[], backend).await
}

async fn emit_snapshot(
    events: &Sender<Event>,
    models: &[TrayItemModel],
    backend: &'static str,
) -> Result<(), ()> {
    let snapshot = Snapshot {
        tray: Some(models.to_vec()),
        tray_backend: Some(backend),
        ..Snapshot::default()
    };
    if events.send(Event::Snapshot(snapshot)).await.is_err() {
        return Err(());
    }
    Ok(())
}

async fn report_unavailable(events: &Sender<Event>) -> Result<(), ()> {
    if events
        .send(Event::Tray(TrayEvent::Unavailable))
        .await
        .is_err()
    {
        return Err(());
    }
    if events
        .send(Event::ServiceAvailability(Service::Tray, false))
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
                tracing::debug!("dropping a command while the tray is disconnected");
            }
            result.is_err()
        }
    }
}

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
}

struct ItemRuntime {
    bus_name: String,
    /// The unique owner plus object path. Equivalent registrations share it.
    dedupe_key: String,
    command_tx: Sender<TrayItemCommand>,
    task: tokio::task::JoinHandle<()>,
}

/// Resolves a registration to its unique bus owner and object path.
async fn registration_key(connection: &Connection, registration: &Registration) -> String {
    let owner = match zbus::fdo::DBusProxy::new(connection).await {
        Ok(dbus) => match zbus::names::BusName::try_from(registration.bus_name.clone()) {
            Ok(name) => dbus
                .get_name_owner(name)
                .await
                .ok()
                .map(|owner| owner.as_str().to_string()),
            Err(_) => None,
        },
        Err(_) => None,
    };

    dedupe_key(owner.as_deref(), registration)
}

fn dedupe_key(owner: Option<&str>, registration: &Registration) -> String {
    format!(
        "{}{}",
        owner.unwrap_or(&registration.bus_name),
        registration.object_path
    )
}

#[derive(Debug, Clone)]
enum TrayItemCommand {
    Activate { x: i32, y: i32 },
    SecondaryActivate { x: i32, y: i32 },
    Scroll { delta: i32, horizontal: bool },
    MenuAboutToShow,
    MenuEvent { menu_id: i32, event: String },
}

#[derive(Debug)]
enum TrayItemEvent {
    Added {
        item: TrayItemModel,
        menu: Option<TrayMenu>,
    },
    Updated(TrayItemModel),
    Menu {
        item_id: String,
        menu: Option<TrayMenu>,
    },
    Failed {
        item_id: String,
        error: String,
    },
    Removed(String),
}

/// Runs one item proxy until the item leaves the bus or the service stops.
async fn run_item(
    connection: Connection,
    registration: Registration,
    commands: Receiver<TrayItemCommand>,
    events: Sender<TrayItemEvent>,
    config: TrayServiceConfig,
    icons: Arc<IconCache>,
    focus: Option<WindowFocusRef>,
) {
    let item_id = registration.item_id();

    let proxy = match Proxy::new_owned(
        connection.clone(),
        registration.bus_name.clone(),
        registration.object_path.clone(),
        ITEM_INTERFACE.to_string(),
    )
    .await
    {
        Ok(proxy) => proxy,
        Err(error) => {
            let _ = events
                .send(TrayItemEvent::Failed {
                    item_id,
                    error: error.to_string(),
                })
                .await;
            return;
        }
    };

    let signal_list = item_signal_streams(&proxy).await;
    let mut signals = (!signal_list.is_empty()).then(|| select_all(signal_list));
    let mut owner_changed = proxy.receive_owner_changed().await.ok();

    let mut model = match read_model(&connection, &registration, &icons, config.item_timeout).await
    {
        Ok(model) => model,
        Err(error) => {
            let _ = events
                .send(TrayItemEvent::Failed {
                    item_id,
                    error: error.to_string(),
                })
                .await;
            return;
        }
    };

    let menu_client = match model.menu_path.as_deref() {
        Some(path) if !path.is_empty() && path != "/" => DbusMenuClient::new(
            &connection,
            &registration.bus_name,
            path,
            config.menu_timeout,
        )
        .await
        .ok(),
        _ => None,
    };

    let mut menu = match &menu_client {
        Some(client) => {
            client.about_to_show().await;
            client.layout().await
        }
        None => None,
    };

    let mut menu_signals = match &menu_client {
        Some(client) => {
            let list = menu_signal_streams(client).await;
            (!list.is_empty()).then(|| select_all(list))
        }
        None => None,
    };

    if events
        .send(TrayItemEvent::Added {
            item: model.clone(),
            menu: menu.clone(),
        })
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    Ok(command) => {
                        handle_item_command(
                            command,
                            ItemContext {
                                proxy: &proxy,
                                menu_client: &menu_client,
                                menu: &mut menu,
                                item_id: &item_id,
                                events: &events,
                                model: &model,
                                focus: focus.as_deref(),
                                timeout: config.item_timeout,
                            },
                        )
                        .await;
                    }
                    Err(_) => break,
                }
            }
            signal = next_message(&mut signals) => {
                let Some(_) = signal else { break };
                tokio::time::sleep(config.debounce).await;
                match read_model(&connection, &registration, &icons, config.item_timeout).await {
                    Ok(updated) => {
                        if updated != model {
                            model = updated;
                            if events
                                .send(TrayItemEvent::Updated(model.clone()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    Err(error) => {
                        tracing::debug!(item_id, %error, "a tray item read failed");
                        break;
                    }
                }
            }
            owner = next_owner(&mut owner_changed) => {
                match owner {
                    Some(Some(_)) => {}
                    Some(None) | None => break,
                }
            }
            message = next_message(&mut menu_signals) => {
                let Some(message) = message else { break };
                handle_menu_message(
                    &message,
                    &menu_client,
                    &mut menu,
                    &item_id,
                    &events,
                )
                .await;
            }
        }
    }

    let _ = events.send(TrayItemEvent::Removed(item_id)).await;
}

struct ItemContext<'a> {
    proxy: &'a Proxy<'static>,
    menu_client: &'a Option<DbusMenuClient>,
    menu: &'a mut Option<TrayMenu>,
    item_id: &'a str,
    events: &'a Sender<TrayItemEvent>,
    model: &'a TrayItemModel,
    focus: Option<&'a dyn WindowFocus>,
    timeout: Duration,
}

async fn handle_item_command(command: TrayItemCommand, context: ItemContext<'_>) {
    let ItemContext {
        proxy,
        menu_client,
        menu,
        item_id,
        events,
        model,
        focus,
        timeout,
    } = context;

    match command {
        TrayItemCommand::Activate { x, y } => {
            // Focus an existing window; otherwise `Activate` launches the
            // application.
            if let Some(focus) = focus {
                if focus_window(focus, proxy.connection(), model, timeout).await {
                    return;
                }
            }

            match tokio::time::timeout(timeout, proxy.call_method("Activate", &(x, y))).await {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    tracing::debug!(%error, item_id, "a tray item Activate call failed")
                }
                Err(_) => tracing::debug!(item_id, "a tray item Activate call timed out"),
            }
        }
        TrayItemCommand::SecondaryActivate { x, y } => {
            match tokio::time::timeout(timeout, proxy.call_method("SecondaryActivate", &(x, y)))
                .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    tracing::debug!(%error, item_id, "a tray item SecondaryActivate call failed")
                }
                Err(_) => {
                    tracing::debug!(item_id, "a tray item SecondaryActivate call timed out")
                }
            }
        }
        TrayItemCommand::Scroll { delta, horizontal } => {
            let orientation = if horizontal { "horizontal" } else { "vertical" };
            match tokio::time::timeout(timeout, proxy.call_method("Scroll", &(delta, orientation)))
                .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    tracing::debug!(%error, item_id, "a tray item Scroll call failed")
                }
                Err(_) => tracing::debug!(item_id, "a tray item Scroll call timed out"),
            }
        }
        TrayItemCommand::MenuAboutToShow => {
            if let Some(client) = menu_client {
                client.about_to_show().await;
                if let Some(fresh) = client.layout().await {
                    *menu = Some(fresh.clone());
                    let _ = events
                        .send(TrayItemEvent::Menu {
                            item_id: item_id.to_string(),
                            menu: Some(fresh),
                        })
                        .await;
                }
            }
        }
        TrayItemCommand::MenuEvent { menu_id, event } => {
            if let Some(client) = menu_client {
                client.send_event(menu_id, &event).await;
            }
        }
    }
}

async fn handle_menu_message(
    message: &zbus::Message,
    menu_client: &Option<DbusMenuClient>,
    menu: &mut Option<TrayMenu>,
    item_id: &str,
    events: &Sender<TrayItemEvent>,
) {
    let member = message
        .header()
        .member()
        .map(|member| member.as_str().to_string())
        .unwrap_or_default();

    match member.as_str() {
        "LayoutUpdated" => {
            let Some(client) = menu_client else { return };
            if let Some(fresh) = client.layout().await {
                *menu = Some(fresh.clone());
                let _ = events
                    .send(TrayItemEvent::Menu {
                        item_id: item_id.to_string(),
                        menu: Some(fresh),
                    })
                    .await;
            }
        }
        "ItemsPropertiesUpdated" => {
            let Ok((updated, removed)) = message.body().deserialize::<(
                Vec<(i32, HashMap<String, OwnedValue>)>,
                Vec<(i32, Vec<String>)>,
            )>() else {
                return;
            };

            if let Some(current) = menu.as_mut() {
                let changed = current.apply_updates(&updated) | current.apply_removed(&removed);
                if changed {
                    let snapshot = current.clone();
                    let _ = events
                        .send(TrayItemEvent::Menu {
                            item_id: item_id.to_string(),
                            menu: Some(snapshot),
                        })
                        .await;
                }
            } else if let Some(client) = menu_client {
                if let Some(fresh) = client.layout().await {
                    *menu = Some(fresh.clone());
                    let _ = events
                        .send(TrayItemEvent::Menu {
                            item_id: item_id.to_string(),
                            menu: Some(fresh),
                        })
                        .await;
                }
            }
        }
        _ => {}
    }
}

async fn read_model(
    connection: &Connection,
    registration: &Registration,
    icons: &IconCache,
    timeout: Duration,
) -> Result<TrayItemModel, BackendError> {
    let read = async {
        let properties = Proxy::new_owned(
            connection.clone(),
            registration.bus_name.clone(),
            registration.object_path.clone(),
            PROPERTIES_INTERFACE.to_string(),
        )
        .await
        .map_err(unavailable)?;

        let values: HashMap<String, OwnedValue> = properties
            .call("GetAll", &(ITEM_INTERFACE,))
            .await
            .map_err(unavailable)?;

        Ok::<_, BackendError>(values)
    };

    let values = tokio::time::timeout(timeout, read)
        .await
        .map_err(|_| BackendError::Unavailable("a tray item property read timed out".into()))??;

    Ok(model_from_properties(registration, &values, icons))
}

/// Focuses a window for an item. A failure returns `false`, so the caller
/// falls back to `Activate`.
async fn focus_window(
    focus: &dyn WindowFocus,
    connection: &Connection,
    model: &TrayItemModel,
    timeout: Duration,
) -> bool {
    let timeout = timeout.min(FOCUS_TIMEOUT);
    let pid = connection_pid(connection, &model.bus_name, timeout).await;
    let hint = WindowHint::from_model(model, pid);

    match tokio::time::timeout(timeout, focus.focus(&hint)).await {
        Ok(Ok(focused)) => focused,
        Ok(Err(error)) => {
            tracing::debug!(%error, "a tray window lookup failed");
            false
        }
        Err(_) => {
            tracing::debug!("a tray window lookup timed out");
            false
        }
    }
}

/// The process that owns the item's bus name, used to match a window that has
/// no recognizable name.
async fn connection_pid(connection: &Connection, bus_name: &str, timeout: Duration) -> Option<u32> {
    let name = zbus::names::BusName::try_from(bus_name.to_string()).ok()?;
    let proxy = tokio::time::timeout(timeout, zbus::fdo::DBusProxy::new(connection))
        .await
        .ok()?
        .ok()?;
    tokio::time::timeout(timeout, proxy.get_connection_unix_process_id(name))
        .await
        .ok()?
        .ok()
}

fn model_from_properties(
    registration: &Registration,
    values: &HashMap<String, OwnedValue>,
    icons: &IconCache,
) -> TrayItemModel {
    let id = get::<String>(values, "Id").unwrap_or_default();
    let title = get::<String>(values, "Title").unwrap_or_default();
    let status = TrayStatus::parse(&get::<String>(values, "Status").unwrap_or_default());
    let category = TrayCategory::parse(&get::<String>(values, "Category").unwrap_or_default());

    let tooltip = get::<(String, Vec<(i32, i32, Vec<u8>)>, String, String)>(values, "ToolTip");
    let (tooltip_title, tooltip_description) = match tooltip {
        Some((_, _, tooltip_title, description)) => (
            if tooltip_title.is_empty() {
                title.clone()
            } else {
                tooltip_title
            },
            description,
        ),
        None => (title.clone(), String::new()),
    };

    let icon_theme_path = get::<String>(values, "IconThemePath")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);

    let normal_name = get::<String>(values, "IconName").filter(|name| !name.is_empty());
    let normal_pixmap =
        largest_pixmap(&get::<Vec<(i32, i32, Vec<u8>)>>(values, "IconPixmap").unwrap_or_default());

    let (icon_name, icon_pixmap) = if status == TrayStatus::NeedsAttention {
        let attention_name =
            get::<String>(values, "AttentionIconName").filter(|name| !name.is_empty());
        let attention_pixmap = largest_pixmap(
            &get::<Vec<(i32, i32, Vec<u8>)>>(values, "AttentionIconPixmap").unwrap_or_default(),
        );
        if attention_name.is_some() || attention_pixmap.is_some() {
            (attention_name, attention_pixmap)
        } else {
            (normal_name, normal_pixmap)
        }
    } else {
        (normal_name, normal_pixmap)
    };

    let icon_file = icon_name
        .as_deref()
        .and_then(|name| resolve_icon_file(name, icon_theme_path.as_deref(), icons));

    let menu_path = get::<OwnedObjectPath>(values, "Menu")
        .map(|path| path.as_str().to_string())
        .filter(|path| !path.is_empty() && path != "/");
    let is_menu = get::<bool>(values, "ItemIsMenu").unwrap_or(false);

    TrayItemModel {
        item_id: registration.item_id(),
        bus_name: registration.bus_name.clone(),
        object_path: registration.object_path.clone(),
        id,
        title,
        status,
        category,
        tooltip_title,
        tooltip_description,
        icon_name,
        icon_theme_path,
        icon_pixmap,
        icon_file,
        menu_path,
        is_menu,
    }
}

fn resolve_icon_file(name: &str, theme_path: Option<&Path>, icons: &IconCache) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() && path.is_file() {
        return Some(path.to_path_buf());
    }

    icons.find(theme_path?, name)
}

async fn item_signal_streams(proxy: &Proxy<'static>) -> Vec<SignalStream<'static>> {
    let mut streams = Vec::new();
    for name in [
        "NewTitle",
        "NewIcon",
        "NewAttentionIcon",
        "NewOverlayIcon",
        "NewToolTip",
        "NewStatus",
    ] {
        if let Ok(stream) = proxy.receive_signal(name).await {
            streams.push(stream);
        }
    }
    streams
}

async fn menu_signal_streams(client: &DbusMenuClient) -> Vec<SignalStream<'static>> {
    let mut streams = Vec::new();
    for name in ["LayoutUpdated", "ItemsPropertiesUpdated"] {
        if let Ok(stream) = client.proxy().receive_signal(name).await {
            streams.push(stream);
        }
    }
    streams
}

async fn watch_name_owner_changes(connection: &Connection) -> Option<SignalStream<'static>> {
    let proxy = Proxy::new_owned(
        connection.clone(),
        DBUS_SERVICE.to_string(),
        DBUS_PATH.to_string(),
        DBUS_INTERFACE.to_string(),
    )
    .await
    .ok()?;

    proxy.receive_signal("NameOwnerChanged").await.ok()
}

async fn next_owner(stream: &mut Option<OwnerChangedStream<'static>>) -> Option<Option<String>> {
    match stream {
        Some(stream) => stream
            .next()
            .await
            .map(|owner| owner.map(|name| name.to_string())),
        None => std::future::pending().await,
    }
}

async fn next_name_owner_changed(
    stream: &mut Option<SignalStream<'static>>,
) -> Option<(String, String, String)> {
    let Some(stream) = stream.as_mut() else {
        return std::future::pending().await;
    };

    while let Some(message) = stream.next().await {
        if let Ok(args) = message.body().deserialize::<(String, String, String)>() {
            return Some(args);
        }
    }
    None
}

async fn next_message(
    stream: &mut Option<SelectAll<SignalStream<'static>>>,
) -> Option<zbus::Message> {
    match stream {
        Some(stream) => stream.next().await,
        None => std::future::pending().await,
    }
}

fn unavailable(error: impl std::fmt::Display) -> BackendError {
    BackendError::Unavailable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn registration() -> Registration {
        Registration {
            bus_name: ":1.42".to_string(),
            object_path: "/StatusNotifierItem".to_string(),
        }
    }

    fn value<T: Into<Value<'static>>>(value: T) -> OwnedValue {
        OwnedValue::try_from(value.into()).unwrap()
    }

    fn values(entries: &[(&str, OwnedValue)]) -> HashMap<String, OwnedValue> {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn item_properties_map_to_the_model() {
        let cache = IconCache::default();
        let map = values(&[
            ("Id", value("discord")),
            ("Title", value("Discord")),
            ("Status", value("Active")),
            ("Category", value("Communications")),
            (
                "ToolTip",
                value((
                    String::new(),
                    Vec::<(i32, i32, Vec<u8>)>::new(),
                    "Discord".to_string(),
                    "Chat".to_string(),
                )),
            ),
            ("IconName", value("discord")),
            (
                "Menu",
                value(OwnedObjectPath::try_from("/MenuBar").unwrap()),
            ),
            ("ItemIsMenu", value(false)),
        ]);

        let model = model_from_properties(&registration(), &map, &cache);
        assert_eq!(model.item_id, ":1.42/StatusNotifierItem");
        assert_eq!(model.id, "discord");
        assert_eq!(model.title, "Discord");
        assert_eq!(model.status, TrayStatus::Active);
        assert_eq!(model.category, TrayCategory::Communications);
        assert_eq!(model.tooltip_title, "Discord");
        assert_eq!(model.tooltip_description, "Chat");
        assert_eq!(model.icon_name.as_deref(), Some("discord"));
        assert_eq!(model.menu_path.as_deref(), Some("/MenuBar"));
        assert!(!model.is_menu);
    }

    #[test]
    fn a_root_menu_path_is_not_a_menu() {
        let cache = IconCache::default();
        let map = values(&[("Menu", value(OwnedObjectPath::try_from("/").unwrap()))]);
        let model = model_from_properties(&registration(), &map, &cache);
        assert_eq!(model.menu_path, None);
    }

    #[test]
    fn the_tooltip_falls_back_to_the_item_title() {
        let cache = IconCache::default();
        let map = values(&[
            ("Title", value("Item")),
            (
                "ToolTip",
                value((
                    String::new(),
                    Vec::<(i32, i32, Vec<u8>)>::new(),
                    String::new(),
                    "Description".to_string(),
                )),
            ),
        ]);

        let model = model_from_properties(&registration(), &map, &cache);
        assert_eq!(model.tooltip_title, "Item");
        assert_eq!(model.tooltip_description, "Description");
    }

    #[test]
    fn a_needs_attention_item_prefers_the_attention_icon() {
        let cache = IconCache::default();
        let map = values(&[
            ("Status", value("NeedsAttention")),
            ("IconName", value("normal")),
            ("AttentionIconName", value("attention")),
        ]);

        let model = model_from_properties(&registration(), &map, &cache);
        assert_eq!(model.icon_name.as_deref(), Some("attention"));
    }

    #[test]
    fn a_needs_attention_item_without_an_attention_icon_uses_the_normal_one() {
        let cache = IconCache::default();
        let map = values(&[
            ("Status", value("NeedsAttention")),
            ("IconName", value("normal")),
        ]);

        let model = model_from_properties(&registration(), &map, &cache);
        assert_eq!(model.icon_name.as_deref(), Some("normal"));
    }

    #[test]
    fn a_zero_sized_pixmap_is_ignored() {
        let cache = IconCache::default();
        let map = values(&[
            ("IconName", value("fallback")),
            ("IconPixmap", value(vec![(0i32, 0i32, Vec::<u8>::new())])),
        ]);

        let model = model_from_properties(&registration(), &map, &cache);
        assert_eq!(model.icon_name.as_deref(), Some("fallback"));
        assert!(model.icon_pixmap.is_none());
        assert_eq!(
            model.icon(),
            Some(crate::tray::model::TrayIcon::Named("fallback".to_string()))
        );
    }

    #[test]
    fn a_missing_pixmap_property_does_not_break_the_model() {
        let cache = IconCache::default();
        let map = values(&[("Title", value("Only a title"))]);
        let model = model_from_properties(&registration(), &map, &cache);
        assert!(model.icon_pixmap.is_none());
        assert_eq!(model.icon(), None);
    }

    fn item(id: &str) -> TrayItemModel {
        TrayItemModel {
            item_id: id.to_string(),
            bus_name: ":1.1".to_string(),
            object_path: "/StatusNotifierItem".to_string(),
            id: id.to_string(),
            title: id.to_string(),
            status: TrayStatus::default(),
            category: TrayCategory::default(),
            tooltip_title: id.to_string(),
            tooltip_description: String::new(),
            icon_name: None,
            icon_theme_path: None,
            icon_pixmap: None,
            icon_file: None,
            menu_path: None,
            is_menu: false,
        }
    }

    #[test]
    fn upsert_replaces_an_existing_item_in_place() {
        let mut models = vec![item("a"), item("b")];
        upsert_model(&mut models, item("a"));
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].item_id, "a");
        assert_eq!(models[1].item_id, "b");
    }

    #[test]
    fn equivalent_registrations_share_a_dedupe_key() {
        let well_known = Registration {
            bus_name: "org.example.App".to_string(),
            object_path: "/StatusNotifierItem".to_string(),
        };
        let unique = Registration {
            bus_name: ":1.42".to_string(),
            object_path: "/StatusNotifierItem".to_string(),
        };

        assert_eq!(
            dedupe_key(Some(":1.42"), &well_known),
            dedupe_key(Some(":1.42"), &unique)
        );
        assert_ne!(
            dedupe_key(Some(":1.42"), &well_known),
            dedupe_key(Some(":1.43"), &unique)
        );
        assert_eq!(
            dedupe_key(None, &well_known),
            "org.example.App/StatusNotifierItem"
        );
    }
}
