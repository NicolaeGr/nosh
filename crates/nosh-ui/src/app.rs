use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{
    volume_icon, AudioEndpoint, AudioEvent, AudioModel, AuthEvent, BatteryEvent, BatteryModel,
    BrightnessDevice, BrightnessEvent, ChangeOrigin, Command, Core, EndpointKind, Event,
    IdleInhibitEvent, IdleInhibitState, LayoutPreset, MediaEvent, MediaModel, MenuEvent,
    MonitorEvent, NetworkEvent, NetworkModel, NotificationEvent, PolkitEvent, PowerEvent,
    PowerModel, PowerProfilesEvent, PowerProfilesModel, SessionLockEvent, SystemStats,
    SystemStatsEvent, TrayEvent, TrayItemModel, TrayMenu, WorkspaceEvent, WorkspaceSnapshot,
    BRIGHTNESS_STEP, DIM_PERCENT, VOLUME_STEP,
};

use crate::bar::BarWindow;
use crate::center_panel::CenterPanelWindow;
use crate::indicator::ChangeIndicator;
use crate::lock_screen::LockScreen;
use crate::menu::MenuManager;
use crate::notifications::{NotificationHistory, NotificationsWindow};
use crate::polkit_prompt::PolkitPromptWindow;
use crate::power_menu::PowerMenuWindow;
use crate::quick_settings::QuickSettingsWindow;

#[cfg(all(feature = "dev", feature = "stable"))]
compile_error!("features `dev` and `stable` are mutually exclusive");

pub const APP_ID: &str = if cfg!(feature = "dev") {
    "com.nicolaegr.nosh.dev"
} else {
    "com.nicolaegr.nosh"
};

const STYLE: &str = include_str!("../resources/style.css");
const MENU_STYLE: &str = include_str!("../resources/menu.css");
const QUICK_SETTINGS_STYLE: &str = include_str!("../resources/quick_settings.css");
const NOTIFICATIONS_STYLE: &str = include_str!("../resources/notifications.css");
const CENTER_PANEL_STYLE: &str = include_str!("../resources/center_panel.css");
const POWER_MENU_STYLE: &str = include_str!("../resources/power_menu.css");
const LOCK_SCREEN_STYLE: &str = include_str!("../resources/lock_screen.css");

/// When this marker is present in the user stylesheet it is replaced with the
/// complete built-in stylesheet at load time.
const BUILTIN_MARKER: &str = "/* nosh:builtin-styles */";

const USER_STYLE_TEMPLATE: &str = "\
/*
 * User stylesheet for nosh.
 *
 * The marker below is replaced with the built-in stylesheet at startup, so
 * everything here can extend or override the shell without copying it. Add
 * your own rules after the marker. Remove the marker to stop including the
 * built-in rules in this file.
 */
/* nosh:builtin-styles */
";

struct Ui {
    app: gtk::Application,
    core: Rc<Core>,
    workspaces: RefCell<WorkspaceSnapshot>,
    stats: RefCell<SystemStats>,
    audio: RefCell<AudioModel>,
    battery: RefCell<BatteryModel>,
    idle_inhibit: RefCell<IdleInhibitState>,
    power_profiles: RefCell<Option<PowerProfilesModel>>,
    network: RefCell<NetworkModel>,
    brightness: RefCell<Vec<BrightnessDevice>>,
    tray: RefCell<Vec<TrayItemModel>>,
    tray_menus: RefCell<HashMap<String, TrayMenu>>,
    media: RefCell<MediaModel>,
    bars: RefCell<BTreeMap<String, BarWindow>>,
    menus: RefCell<MenuManager>,
    quick_settings: Rc<QuickSettingsWindow>,
    power_menu: Rc<PowerMenuWindow>,
    lock_screen: Rc<LockScreen>,
    polkit_prompt: Rc<PolkitPromptWindow>,
    center_panel: Rc<CenterPanelWindow>,
    notification_history: Rc<NotificationHistory>,
    notifications: Rc<NotificationsWindow>,
    indicator: Rc<ChangeIndicator>,
}

/// A session lock forbids mapping a new surface, so the passive overlays stay
/// hidden from the lock request until the compositor reports the unlock.
fn set_passive_overlays_locked(
    notifications: &NotificationsWindow,
    indicator: &ChangeIndicator,
    locked: bool,
) {
    notifications.set_locked(locked);
    indicator.set_locked(locked);
}

impl Ui {
    fn new(app: gtk::Application, core: Rc<Core>) -> Self {
        let indicator = Rc::new(ChangeIndicator::new(&app));
        let lock_screen = Rc::new(LockScreen::new(&app, Rc::clone(&core)));
        let notifications = Rc::new(NotificationsWindow::new(&app, Rc::clone(&core)));
        let power_menu = Rc::new(PowerMenuWindow::new(&app, Rc::clone(&core)));
        {
            let screen = Rc::clone(&lock_screen);
            let weak_menu = Rc::downgrade(&power_menu);
            let notifications = Rc::clone(&notifications);
            let indicator = Rc::clone(&indicator);
            power_menu.set_lock_handler(Rc::new(move || {
                if let Some(menu) = weak_menu.upgrade() {
                    menu.close();
                }
                screen.lock();
                set_passive_overlays_locked(&notifications, &indicator, screen.is_locked());
            }));
        }
        let polkit_prompt = Rc::new(PolkitPromptWindow::new(&app, Rc::clone(&core)));
        let quick_settings = Rc::new(QuickSettingsWindow::new(&app, Rc::clone(&core)));
        {
            let weak_settings = Rc::downgrade(&quick_settings);
            let power_menu = Rc::clone(&power_menu);
            quick_settings.set_session_handler(Rc::new(move || {
                let monitor = weak_settings.upgrade().map(|settings| {
                    let monitor = settings.current_monitor();
                    settings.close();
                    monitor
                });
                power_menu.open(monitor.flatten().as_ref());
            }));
        }
        let notification_history = Rc::new(NotificationHistory::new());
        let center_panel = Rc::new(CenterPanelWindow::new(
            &app,
            Rc::clone(&core),
            Rc::clone(&notification_history),
        ));
        Self {
            app,
            core,
            workspaces: RefCell::new(WorkspaceSnapshot::default()),
            stats: RefCell::new(SystemStats::default()),
            audio: RefCell::new(AudioModel::default()),
            battery: RefCell::new(BatteryModel::default()),
            idle_inhibit: RefCell::new(IdleInhibitState::default()),
            power_profiles: RefCell::new(None),
            network: RefCell::new(NetworkModel::default()),
            brightness: RefCell::new(Vec::new()),
            tray: RefCell::new(Vec::new()),
            tray_menus: RefCell::new(HashMap::new()),
            media: RefCell::new(MediaModel::default()),
            bars: RefCell::new(BTreeMap::new()),
            menus: RefCell::new(MenuManager::new()),
            quick_settings,
            power_menu,
            lock_screen,
            polkit_prompt,
            center_panel,
            notification_history,
            notifications,
            indicator,
        }
    }

    fn sync_bars(&self) {
        // A session lock does not allow new toplevels. Bars that appear on a
        // monitor plugged in while locked are created after the unlock.
        if self.lock_screen.is_locked() {
            return;
        }

        let Some(display) = gdk::Display::default() else {
            return;
        };

        let monitors = display.monitors();
        let mut live = BTreeSet::new();

        for index in 0..monitors.n_items() {
            let Some(monitor) = monitors.item(index).and_downcast::<gdk::Monitor>() else {
                continue;
            };

            let connector = monitor
                .connector()
                .map(|value| value.to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| format!("monitor-{index}"));
            live.insert(connector.clone());

            if self.bars.borrow().contains_key(&connector) {
                continue;
            }

            let mut bar = BarWindow::new(
                &self.app,
                &monitor,
                connector.clone(),
                Rc::clone(&self.core),
                Rc::clone(&self.quick_settings),
                Rc::clone(&self.center_panel),
            );
            bar.set_workspaces(&self.workspaces.borrow());
            bar.set_stats(&self.stats.borrow());
            bar.set_audio(&self.audio.borrow());
            bar.set_battery(&self.battery.borrow());
            bar.set_idle_inhibit(&self.idle_inhibit.borrow());
            bar.set_network(&self.network.borrow());
            bar.set_tray(&self.tray.borrow(), &self.tray_menus.borrow());
            bar.set_media(&self.media.borrow());
            bar.present();
            self.bars.borrow_mut().insert(connector, bar);
        }

        self.bars.borrow_mut().retain(|connector, bar| {
            let keep = live.contains(connector);
            if !keep {
                bar.destroy();
            }
            keep
        });
    }

    fn render_workspaces(&self) {
        let snapshot = self.workspaces.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_workspaces(&snapshot);
        }
    }

    fn render_stats(&self) {
        let stats = *self.stats.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_stats(&stats);
        }
    }

    fn render_idle_inhibit(&self) {
        let state = *self.idle_inhibit.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_idle_inhibit(&state);
        }
        self.quick_settings.set_idle_inhibit(&state);
    }

    fn render_power_profiles(&self) {
        let model = self.power_profiles.borrow();
        self.quick_settings.set_power_profiles(model.as_ref());
    }

    fn render_network(&self) {
        let model = self.network.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_network(&model);
        }
        self.quick_settings.set_network(&model);
    }

    fn render_audio(&self) {
        let model = self.audio.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_audio(&model);
        }
        self.quick_settings.set_audio(&model);
    }

    fn render_battery(&self) {
        let model = self.battery.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_battery(&model);
        }
        self.quick_settings.set_battery(&model);
    }

    fn render_brightness(&self) {
        let devices = self.brightness.borrow();
        self.quick_settings.set_brightness(devices.as_slice());
    }

    fn render_tray(&self) {
        let items = self.tray.borrow();
        let menus = self.tray_menus.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_tray(&items, &menus);
        }
    }

    fn render_media(&self) {
        let model = self.media.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_media(&model);
        }
        self.center_panel.set_media(&model);
    }

    fn show_indicator(&self, change: IndicatorChange) {
        // The quick settings panel already shows the value it changed, so the
        // floating indicator would be redundant over the open overlay.
        if self.quick_settings.is_open() {
            return;
        }
        let monitor = self.active_monitor();
        match change {
            IndicatorChange::Audio {
                kind,
                volume,
                muted,
            } => self
                .indicator
                .show_audio(monitor.as_ref(), kind, volume, muted),
            IndicatorChange::Brightness(percent) => {
                self.indicator.show_brightness(monitor.as_ref(), percent)
            }
        }
    }

    /// The monitor of the focused workspace, falling back to the first output.
    fn active_monitor(&self) -> Option<gdk::Monitor> {
        let display = gdk::Display::default()?;
        let connector = {
            let snapshot = self.workspaces.borrow();
            snapshot
                .focused_workspace
                .as_ref()
                .and_then(|id| snapshot.workspace(id))
                .and_then(|workspace| workspace.monitor.clone())
                .or_else(|| {
                    snapshot
                        .monitors
                        .iter()
                        .find(|monitor| monitor.focused)
                        .map(|monitor| monitor.connector.clone())
                })
        };

        let monitors = display.monitors();
        if let Some(connector) = connector {
            for index in 0..monitors.n_items() {
                let Some(monitor) = monitors.item(index).and_downcast::<gdk::Monitor>() else {
                    continue;
                };
                let name = monitor.connector().map(|value| value.to_string());
                if name.as_deref() == Some(connector.as_str()) {
                    return Some(monitor);
                }
            }
        }

        monitors.item(0).and_downcast::<gdk::Monitor>()
    }

    fn apply(&self, event: Event) {
        match event {
            Event::Snapshot(snapshot) => {
                if let Some(layout) = snapshot.monitors {
                    self.notifications.set_layout(&layout);
                }
                if let Some(notifications) = snapshot.notifications {
                    self.notifications.apply_snapshot(&notifications);
                    self.quick_settings
                        .set_dnd(notifications.settings.dont_disturb);
                }
                if let Some(workspaces) = snapshot.workspaces {
                    *self.workspaces.borrow_mut() = workspaces;
                    self.render_workspaces();
                }
                if let Some(stats) = snapshot.system_stats {
                    *self.stats.borrow_mut() = stats;
                    self.render_stats();
                }
                if let Some(audio) = snapshot.audio {
                    *self.audio.borrow_mut() = audio;
                    self.render_audio();
                }
                if let Some(battery) = snapshot.battery {
                    *self.battery.borrow_mut() = battery;
                    self.render_battery();
                }
                if let Some(state) = snapshot.idle_inhibit {
                    *self.idle_inhibit.borrow_mut() = state;
                    self.render_idle_inhibit();
                }
                if let Some(power_profiles) = snapshot.power_profiles {
                    *self.power_profiles.borrow_mut() = Some(power_profiles);
                    self.render_power_profiles();
                }
                if let Some(network) = snapshot.network {
                    *self.network.borrow_mut() = network;
                    self.render_network();
                }
                if let Some(brightness) = snapshot.brightness {
                    *self.brightness.borrow_mut() = brightness;
                    self.render_brightness();
                }
                if let Some(tray) = snapshot.tray {
                    self.replace_tray(tray);
                }
                if let Some(media) = snapshot.media {
                    *self.media.borrow_mut() = media;
                    self.render_media();
                }
                if let Some(power) = snapshot.power {
                    self.power_menu.set_power(&power);
                }
            }
            Event::Workspaces(event) => self.apply_workspace_event(event),
            Event::SystemStats(SystemStatsEvent::Changed(stats)) => {
                *self.stats.borrow_mut() = stats;
                self.render_stats();
            }
            Event::Audio(event) => self.apply_audio_event(event),
            Event::Brightness(event) => self.apply_brightness_event(event),
            Event::Battery(event) => self.apply_battery_event(event),
            Event::IdleInhibit(event) => self.apply_idle_inhibit_event(event),
            Event::PowerProfiles(event) => self.apply_power_profiles_event(event),
            Event::Network(event) => self.apply_network_event(event),
            Event::Tray(event) => self.apply_tray_event(event),
            Event::Media(event) => self.apply_media_event(event),
            Event::Auth(AuthEvent::Finished(result)) => {
                self.power_menu.handle_auth(&result);
                self.lock_screen.handle_auth(&result);
            }
            Event::Polkit(event) => self.apply_polkit_event(event),
            Event::Power(event) => self.apply_power_event(event),
            Event::SessionLock(event) => self.apply_session_lock(event),
            Event::Menu(event) => self.apply_menu_event(event),
            Event::Monitors(MonitorEvent::Snapshot { layout, .. })
            | Event::Monitors(MonitorEvent::LayoutChanged(layout)) => {
                self.notifications.set_layout(&layout);
            }
            Event::Monitors(_) => {}
            Event::Notifications(event) => self.apply_notification_event(event),
            _ => {}
        }
    }

    fn apply_notification_event(&self, event: NotificationEvent) {
        match event {
            NotificationEvent::Snapshot(model) => {
                for notification in &model.notifications {
                    self.notification_history.remember(notification);
                }
                self.notifications.apply_snapshot(&model);
                self.quick_settings.set_dnd(model.settings.dont_disturb);
                self.center_panel.refresh_notifications();
            }
            NotificationEvent::Received {
                notification,
                suppress_popup,
                ..
            } => {
                self.notification_history.remember(&notification);
                self.notifications.received(&notification, suppress_popup);
            }
            NotificationEvent::Closed { id, reason } => {
                self.notifications.closed(id);
                self.notification_history.close(id, reason);
                self.center_panel.refresh_notifications();
            }
            NotificationEvent::DndChanged { enabled } => {
                self.notifications.set_dnd(enabled);
                self.quick_settings.set_dnd(enabled);
            }
            NotificationEvent::Invoked { .. } | NotificationEvent::BackendChanged { .. } => {}
            NotificationEvent::Unavailable => {
                self.notifications.set_dnd(false);
                self.quick_settings.set_dnd_unavailable();
            }
        }
    }

    fn replace_tray(&self, items: Vec<TrayItemModel>) {
        self.tray_menus
            .borrow_mut()
            .retain(|item_id, _| items.iter().any(|item| &item.item_id == item_id));
        *self.tray.borrow_mut() = items;
        self.render_tray();
    }

    fn upsert_tray_item(&self, item: TrayItemModel) {
        let mut items = self.tray.borrow_mut();
        if let Some(existing) = items
            .iter_mut()
            .find(|existing| existing.item_id == item.item_id)
        {
            *existing = item;
        } else {
            items.push(item);
        }
    }

    fn apply_tray_event(&self, event: TrayEvent) {
        match event {
            TrayEvent::Snapshot(items) => {
                self.tray_menus
                    .borrow_mut()
                    .retain(|item_id, _| items.iter().any(|item| &item.item_id == item_id));
                *self.tray.borrow_mut() = items;
            }
            TrayEvent::ItemAdded { item, menu } => {
                let item_id = item.item_id.clone();
                self.upsert_tray_item(item);
                match menu {
                    Some(menu) => {
                        self.tray_menus.borrow_mut().insert(item_id, menu);
                    }
                    None => {
                        self.tray_menus.borrow_mut().remove(&item_id);
                    }
                }
            }
            TrayEvent::ItemUpdated { item } => self.upsert_tray_item(item),
            TrayEvent::ItemRemoved { item_id } => {
                self.tray
                    .borrow_mut()
                    .retain(|item| item.item_id != item_id);
                self.tray_menus.borrow_mut().remove(&item_id);
            }
            TrayEvent::MenuUpdated { item_id, menu } => match menu {
                Some(menu) => {
                    self.tray_menus.borrow_mut().insert(item_id, menu);
                }
                None => {
                    self.tray_menus.borrow_mut().remove(&item_id);
                }
            },
            TrayEvent::BackendChanged { .. } => return,
            TrayEvent::Unavailable => {
                self.tray.borrow_mut().clear();
                self.tray_menus.borrow_mut().clear();
            }
        }

        self.render_tray();
    }

    fn apply_media_event(&self, event: MediaEvent) {
        match event {
            MediaEvent::Snapshot(model) | MediaEvent::Changed(model) => {
                *self.media.borrow_mut() = model;
            }
            MediaEvent::WriteFailed(message) => {
                tracing::warn!(%message, "a media command failed");
                return;
            }
            MediaEvent::Unavailable => {
                *self.media.borrow_mut() = MediaModel::default();
            }
        }

        self.render_media();
    }

    fn apply_audio_event(&self, event: AudioEvent) {
        let mut change = None;
        {
            let mut model = self.audio.borrow_mut();
            match event {
                AudioEvent::Snapshot(snapshot) => *model = snapshot,
                AudioEvent::VolumeChanged {
                    kind,
                    id,
                    volume,
                    origin,
                    ..
                } => {
                    update_endpoint(&mut model, kind, id, |endpoint| {
                        endpoint.volume = volume;
                        endpoint.icon_name =
                            volume_icon(kind, endpoint.volume, endpoint.muted).to_string();
                    });
                    if origin == ChangeOrigin::User {
                        let muted = model
                            .endpoint(kind, id)
                            .map(|item| item.muted)
                            .unwrap_or(false);
                        change = Some(IndicatorChange::Audio {
                            kind,
                            volume,
                            muted,
                        });
                    }
                }
                AudioEvent::MuteChanged {
                    kind,
                    id,
                    muted,
                    origin,
                } => {
                    update_endpoint(&mut model, kind, id, |endpoint| {
                        endpoint.muted = muted;
                        endpoint.icon_name =
                            volume_icon(kind, endpoint.volume, endpoint.muted).to_string();
                    });
                    if origin == ChangeOrigin::User {
                        let volume = model
                            .endpoint(kind, id)
                            .map(|item| item.volume)
                            .unwrap_or(0.0);
                        change = Some(IndicatorChange::Audio {
                            kind,
                            volume,
                            muted,
                        });
                    }
                }
                AudioEvent::DefaultChanged { kind, endpoint } => match kind {
                    EndpointKind::Sink => {
                        for sink in &mut model.sinks {
                            sink.is_default = sink.id == endpoint.id;
                        }
                        model.default_sink = Some(endpoint);
                    }
                    EndpointKind::Source => {
                        for source in &mut model.sources {
                            source.is_default = source.id == endpoint.id;
                        }
                        model.default_source = Some(endpoint);
                    }
                },
                AudioEvent::Unavailable => *model = AudioModel::default(),
                AudioEvent::WriteFailed(_)
                | AudioEvent::ReadFailed(_)
                | AudioEvent::BackendChanged { .. } => {}
            }
        }

        self.render_audio();
        if let Some(change) = change {
            self.show_indicator(change);
        }
    }

    fn apply_brightness_event(&self, event: BrightnessEvent) {
        match event {
            BrightnessEvent::Snapshot(devices) => {
                *self.brightness.borrow_mut() = devices;
            }
            BrightnessEvent::DeviceChanged {
                device,
                percent,
                old: _,
                origin,
            } => {
                if let Some(entry) = self
                    .brightness
                    .borrow_mut()
                    .iter_mut()
                    .find(|entry| entry.id == device)
                {
                    entry.percent = percent;
                    entry.pending_percent = None;
                }
                if origin == ChangeOrigin::User {
                    self.show_indicator(IndicatorChange::Brightness(percent));
                }
            }
            BrightnessEvent::Pending { device, target } => {
                if let Some(entry) = self
                    .brightness
                    .borrow_mut()
                    .iter_mut()
                    .find(|entry| entry.id == device)
                {
                    entry.pending_percent = Some(target);
                }
            }
            BrightnessEvent::Restored => {}
            BrightnessEvent::Unavailable => {
                self.brightness.borrow_mut().clear();
            }
        }

        self.render_brightness();
    }

    fn apply_battery_event(&self, event: BatteryEvent) {
        match event {
            BatteryEvent::Snapshot(model) | BatteryEvent::Changed(model) => {
                *self.battery.borrow_mut() = model;
            }
            BatteryEvent::Unavailable => {
                *self.battery.borrow_mut() = BatteryModel::default();
            }
        }

        self.render_battery();
    }

    fn apply_idle_inhibit_event(&self, event: IdleInhibitEvent) {
        match event {
            IdleInhibitEvent::Snapshot { available, active } => {
                *self.idle_inhibit.borrow_mut() = IdleInhibitState::new(available, active);
            }
            IdleInhibitEvent::Changed { active } => {
                self.idle_inhibit.borrow_mut().active = active;
            }
            IdleInhibitEvent::Unavailable => {
                *self.idle_inhibit.borrow_mut() = IdleInhibitState::default();
            }
        }

        self.render_idle_inhibit();
    }

    fn apply_power_profiles_event(&self, event: PowerProfilesEvent) {
        match event {
            PowerProfilesEvent::Snapshot(model) | PowerProfilesEvent::Changed(model) => {
                *self.power_profiles.borrow_mut() = Some(model);
            }
            PowerProfilesEvent::Unavailable => {
                *self.power_profiles.borrow_mut() = None;
            }
        }

        self.render_power_profiles();
    }

    fn apply_network_event(&self, event: NetworkEvent) {
        match event {
            NetworkEvent::Snapshot(model) | NetworkEvent::Changed(model) => {
                *self.network.borrow_mut() = model;
            }
            NetworkEvent::Unavailable => {
                *self.network.borrow_mut() = NetworkModel::default();
            }
            NetworkEvent::OperationFailed(message) => {
                self.quick_settings.set_network_error(&message);
                return;
            }
            NetworkEvent::ScanStarted | NetworkEvent::ScanFinished => return,
        }

        self.render_network();
    }

    fn apply_menu_event(&self, event: MenuEvent) {
        match event {
            MenuEvent::Opened { id, menu } => {
                self.menus
                    .borrow_mut()
                    .open(id, *menu, &self.app, Rc::clone(&self.core));
            }
            MenuEvent::Results { id, items, .. } => {
                self.menus.borrow_mut().set_results(&id, items);
            }
            MenuEvent::Failed { id, message } | MenuEvent::ActionFailed { id, message } => {
                self.menus.borrow_mut().show_error(&id, &message);
            }
            MenuEvent::Closed { id } => {
                self.menus.borrow_mut().close(&id);
            }
            MenuEvent::ActionDone { id, after } => match after {
                nosh_core::After::Close => self.core.send(Command::MenuClose(id)),
                nosh_core::After::Reload => self.core.send(Command::MenuReload(id)),
                nosh_core::After::None => {}
            },
        }
    }

    /// Sends a request to the core. The window appears when `Opened` arrives.
    fn open_menu(&self, id: &str) {
        if self.lock_screen.is_locked() {
            return;
        }
        self.core.send(Command::MenuOpen(id.to_string()));
    }

    fn open_power_menu(&self) {
        if self.lock_screen.is_locked() {
            return;
        }
        let monitor = self.active_monitor();
        self.power_menu.open(monitor.as_ref());
    }

    /// Closes the transient panels and locks the session.
    fn lock(&self) {
        self.quick_settings.close();
        self.power_menu.close();
        // A prompt opened before the lock must not wait behind the lock screen.
        self.polkit_prompt.cancel();
        self.lock_screen.lock();
        self.apply_lock_to_popups();
    }

    /// Tells the passive overlays whether a session lock is held. A lock
    /// forbids mapping a new surface, so both must stay hidden while it lasts.
    fn apply_lock_to_popups(&self) {
        set_passive_overlays_locked(
            &self.notifications,
            &self.indicator,
            self.lock_screen.is_locked(),
        );
    }

    fn apply_session_lock(&self, event: SessionLockEvent) {
        match event {
            SessionLockEvent::Lock => self.lock(),
            SessionLockEvent::Unlock => {
                tracing::debug!("logind asked to unlock, the lock screen unlocks through PAM");
            }
            SessionLockEvent::Unavailable => {}
        }
    }

    fn apply_polkit_event(&self, event: PolkitEvent) {
        if self.lock_screen.is_locked() {
            // A lock surface cannot host the prompt, and showing it after the
            // unlock would surprise the user. The cookie is dropped so polkitd
            // sees the request end.
            if let PolkitEvent::Prompt(prompt) = &event {
                tracing::debug!(
                    cookie = %prompt.cookie,
                    "cancelling a polkit prompt while the session is locked"
                );
                self.core.send(Command::PolkitCancel {
                    cookie: prompt.cookie.clone(),
                });
            }
            return;
        }
        let monitor = self.active_monitor();
        self.polkit_prompt.handle_event(&event, monitor.as_ref());
    }

    fn apply_power_event(&self, event: PowerEvent) {
        match event {
            PowerEvent::Snapshot(model) => {
                self.power_menu.set_power(&model);
            }
            PowerEvent::ActionStarted(action) => {
                tracing::info!(action = action.as_str(), "session action sent");
            }
            PowerEvent::ActionCompleted(_) => self.power_menu.close(),
            PowerEvent::ActionFailed { action, message } => {
                tracing::warn!(action = action.as_str(), %message, "session action failed");
                self.power_menu.show_failure(&message);
            }
            PowerEvent::Unavailable => {
                self.power_menu.set_power(&PowerModel::default());
            }
        }
    }

    fn apply_workspace_event(&self, event: WorkspaceEvent) {
        match event {
            WorkspaceEvent::Snapshot { state, .. } => {
                *self.workspaces.borrow_mut() = state;
            }
            WorkspaceEvent::FocusedChanged { workspace } => {
                let mut snapshot = self.workspaces.borrow_mut();
                snapshot.focused_workspace = workspace.clone();
                for item in &mut snapshot.workspaces {
                    item.focused = workspace.as_deref() == Some(item.id.as_str());
                }
            }
            WorkspaceEvent::UrgentChanged { workspace, urgent } => {
                let mut snapshot = self.workspaces.borrow_mut();
                if let Some(item) = snapshot
                    .workspaces
                    .iter_mut()
                    .find(|item| item.id == workspace)
                {
                    item.urgent = urgent;
                }
            }
            WorkspaceEvent::Unavailable => {
                *self.workspaces.borrow_mut() = WorkspaceSnapshot::default();
            }
            WorkspaceEvent::BackendChanged { .. } => return,
        }

        self.render_workspaces();
    }
}

enum IndicatorChange {
    Audio {
        kind: EndpointKind,
        volume: f64,
        muted: bool,
    },
    Brightness(f64),
}

fn update_endpoint(
    model: &mut AudioModel,
    kind: EndpointKind,
    id: u32,
    update: impl Fn(&mut AudioEndpoint),
) {
    let endpoints = match kind {
        EndpointKind::Sink => &mut model.sinks,
        EndpointKind::Source => &mut model.sources,
    };

    if let Some(endpoint) = endpoints.iter_mut().find(|endpoint| endpoint.id == id) {
        update(endpoint);
    }

    if let Some(endpoint) = model
        .default_for_mut(kind)
        .filter(|endpoint| endpoint.id == id)
    {
        update(endpoint);
    }
}

#[derive(Debug, Clone, PartialEq)]
enum StartupAction {
    Menu(String),
    Session,
    Lock,
    Command(Command),
}

pub fn run(arguments: Vec<std::ffi::OsString>) -> Result<u8, Box<dyn Error>> {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let pending: Rc<RefCell<Vec<StartupAction>>> = Rc::new(RefCell::new(Vec::new()));
    let ui_slot: Rc<RefCell<Option<Rc<Ui>>>> = Rc::new(RefCell::new(None));

    app.connect_startup(|_| install_css());

    // A secondary `nosh <command>` forwards its command line to the running
    // instance, which owns the windows.
    {
        let ui_slot = Rc::clone(&ui_slot);
        let pending = Rc::clone(&pending);
        app.connect_command_line(move |app, command_line| {
            if let Some(action) = startup_action(&command_line.arguments()) {
                let ui = ui_slot.borrow().clone();
                match action {
                    StartupAction::Menu(id) => match &ui {
                        Some(ui) => ui.open_menu(&id),
                        None => pending.borrow_mut().push(StartupAction::Menu(id)),
                    },
                    StartupAction::Session => match &ui {
                        Some(ui) => ui.open_power_menu(),
                        None => pending.borrow_mut().push(StartupAction::Session),
                    },
                    StartupAction::Lock => match &ui {
                        Some(ui) => ui.lock(),
                        None => pending.borrow_mut().push(StartupAction::Lock),
                    },
                    StartupAction::Command(command) => match &ui {
                        Some(ui) => ui.core.send(command),
                        None => pending.borrow_mut().push(StartupAction::Command(command)),
                    },
                }
            }
            app.activate();
            glib::ExitCode::SUCCESS
        });
    }

    {
        let ui_slot = Rc::clone(&ui_slot);
        let pending = Rc::clone(&pending);
        app.connect_activate(move |app| {
            if ui_slot.borrow().is_some() {
                return;
            }

            if !gtk4_layer_shell::is_supported() {
                eprintln!("nosh: gtk4-layer-shell is not available on this display server");
                app.quit();
                return;
            }

            // Start the core only in the primary instance. A secondary
            // `nosh <command>` process forwards its command line and exits
            // without opening audio, battery, or tray connections.
            let core = match Core::start() {
                Ok(core) => Rc::new(core),
                Err(error) => {
                    eprintln!("nosh: {error}");
                    app.quit();
                    return;
                }
            };
            let events = core.subscribe();

            let ui = Rc::new(Ui::new(app.clone(), Rc::clone(&core)));
            {
                let weak = Rc::downgrade(&ui);
                ui.lock_screen.set_unlocked_handler(Rc::new(move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.apply_lock_to_popups();
                        ui.sync_bars();
                    }
                }));
            }
            if ui.lock_screen.lock_if_pending() {
                tracing::info!("re-acquiring the session lock from the previous run");
                ui.apply_lock_to_popups();
            }
            ui.sync_bars();

            if let Some(display) = gdk::Display::default() {
                let ui = Rc::clone(&ui);
                display
                    .monitors()
                    .connect_items_changed(move |_, _, _, _| ui.sync_bars());
            }

            let pending: Vec<StartupAction> = pending.borrow_mut().drain(..).collect();
            ui_slot.borrow_mut().replace(Rc::clone(&ui));
            for action in pending {
                match action {
                    StartupAction::Menu(id) => ui.open_menu(&id),
                    StartupAction::Session => ui.open_power_menu(),
                    StartupAction::Lock => ui.lock(),
                    StartupAction::Command(command) => ui.core.send(command),
                }
            }

            let events = events.clone();
            glib::spawn_future_local(async move {
                while let Ok(event) = events.recv().await {
                    ui.apply(event);
                }
            });
        });
    }

    Ok(app.run_with_args_os(&arguments).into())
}

fn startup_action(arguments: &[std::ffi::OsString]) -> Option<StartupAction> {
    let words: Vec<&str> = arguments
        .iter()
        .filter_map(|argument| argument.to_str())
        .collect();
    let mut index = 1;
    while index < words.len() {
        let word = words[index];
        index += 1;
        if word.starts_with('-') {
            continue;
        }

        match word {
            "menu" => {
                return words
                    .get(index)
                    .map(|id| StartupAction::Menu((*id).to_string()));
            }
            "session" => return Some(StartupAction::Session),
            "lock" => return Some(StartupAction::Lock),
            "volume-up" => {
                return Some(StartupAction::Command(Command::AudioStepVolume(
                    VOLUME_STEP,
                )))
            }
            "volume-down" => {
                return Some(StartupAction::Command(Command::AudioStepVolume(
                    -VOLUME_STEP,
                )))
            }
            "volume-mute" => {
                return Some(StartupAction::Command(Command::AudioToggleMute(
                    EndpointKind::Sink,
                )))
            }
            "mic-mute" => {
                return Some(StartupAction::Command(Command::AudioToggleMute(
                    EndpointKind::Source,
                )))
            }
            "brightness-up" => {
                return Some(StartupAction::Command(Command::BrightnessStep(
                    BRIGHTNESS_STEP,
                )))
            }
            "brightness-down" => {
                return Some(StartupAction::Command(Command::BrightnessStep(
                    -BRIGHTNESS_STEP,
                )))
            }
            "brightness-dim" => {
                return Some(StartupAction::Command(Command::BrightnessDim {
                    value: DIM_PERCENT,
                }))
            }
            "brightness-restore" => {
                return Some(StartupAction::Command(Command::BrightnessRestore))
            }
            "monitor-cycle" => {
                let target = words
                    .get(index)
                    .map(|word| (*word).to_string())
                    .unwrap_or_default();
                let mode = words
                    .get(index + 1)
                    .and_then(|word| LayoutPreset::parse(word));
                return Some(StartupAction::Command(Command::MonitorCycle {
                    target,
                    mode,
                }));
            }
            _ => {
                eprintln!(
                    "Unknown command or failed to handle command. \nIf you intended to start the main instance, it is already running.\n"
                );
                return None;
            }
        }
    }
    None
}

fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };

    let builtin = format!(
        "{STYLE}\n{MENU_STYLE}\n{QUICK_SETTINGS_STYLE}\n{NOTIFICATIONS_STYLE}\n{CENTER_PANEL_STYLE}\n{POWER_MENU_STYLE}\n{LOCK_SCREEN_STYLE}"
    );
    add_css(&display, &builtin, gtk::STYLE_PROVIDER_PRIORITY_USER);

    let Ok(paths) = nosh_core::NoshPaths::from_env() else {
        return;
    };

    if let Err(error) = std::fs::create_dir_all(paths.menus_dir()) {
        tracing::warn!(
            %error,
            path = %paths.menus_dir().display(),
            "cannot create the menus directory"
        );
    }

    let user_path = paths.user_styles();
    ensure_user_style(&user_path);

    match std::fs::read_to_string(&user_path) {
        Ok(text) => {
            let css = expand_user_style(&text, &builtin);
            add_css(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_USER + 1);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(
            %error,
            path = %user_path.display(),
            "cannot read the user stylesheet"
        ),
    }
}

fn ensure_user_style(path: &std::path::Path) {
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::warn!(%error, path = %parent.display(), "cannot create the config directory");
            return;
        }
    }
    match std::fs::write(path, USER_STYLE_TEMPLATE) {
        Ok(()) => tracing::info!(path = %path.display(), "wrote the user stylesheet template"),
        Err(error) => tracing::warn!(
            %error,
            path = %path.display(),
            "cannot write the user stylesheet template"
        ),
    }
}

fn add_css(display: &gdk::Display, css: &str, priority: u32) {
    let provider = gtk::CssProvider::new();
    provider.connect_parsing_error(|_, section, error| {
        tracing::warn!(
            line = section.start_location().lines() + 1,
            %error,
            "CSS parse error"
        );
    });
    provider.load_from_data(css);
    gtk::style_context_add_provider_for_display(display, &provider, priority);
}

fn expand_user_style(text: &str, builtin: &str) -> String {
    text.replace(BUILTIN_MARKER, builtin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_marker_is_replaced() {
        let text = "a\n/* nosh:builtin-styles */\nb";
        assert_eq!(expand_user_style(text, "BUILTIN"), "a\nBUILTIN\nb");
    }

    #[test]
    fn a_stylesheet_without_the_marker_is_unchanged() {
        let text = ".MenuItem { opacity: 0.5; }";
        assert_eq!(expand_user_style(text, "BUILTIN"), text);
    }

    #[test]
    fn the_command_line_maps_a_menu_id() {
        assert_eq!(
            startup_action(&arguments(&["menu", "wallpapers"])),
            Some(StartupAction::Menu("wallpapers".to_string()))
        );
        assert_eq!(startup_action(&arguments(&["menu"])), None);
    }

    #[test]
    fn the_command_line_opens_the_session_menu() {
        assert_eq!(
            startup_action(&arguments(&["session"])),
            Some(StartupAction::Session)
        );
    }

    #[test]
    fn the_command_line_locks_the_session() {
        assert_eq!(
            startup_action(&arguments(&["lock"])),
            Some(StartupAction::Lock)
        );
    }

    #[test]
    fn the_command_line_maps_each_command() {
        assert_eq!(
            startup_action(&arguments(&["volume-up"])),
            Some(StartupAction::Command(Command::AudioStepVolume(
                VOLUME_STEP
            )))
        );
        assert_eq!(
            startup_action(&arguments(&["volume-down"])),
            Some(StartupAction::Command(Command::AudioStepVolume(
                -VOLUME_STEP
            )))
        );
        assert_eq!(
            startup_action(&arguments(&["volume-mute"])),
            Some(StartupAction::Command(Command::AudioToggleMute(
                EndpointKind::Sink
            )))
        );
        assert_eq!(
            startup_action(&arguments(&["mic-mute"])),
            Some(StartupAction::Command(Command::AudioToggleMute(
                EndpointKind::Source
            )))
        );
        assert_eq!(
            startup_action(&arguments(&["brightness-up"])),
            Some(StartupAction::Command(Command::BrightnessStep(
                BRIGHTNESS_STEP
            )))
        );
        assert_eq!(
            startup_action(&arguments(&["brightness-down"])),
            Some(StartupAction::Command(Command::BrightnessStep(
                -BRIGHTNESS_STEP
            )))
        );
        assert_eq!(
            startup_action(&arguments(&["brightness-dim"])),
            Some(StartupAction::Command(Command::BrightnessDim {
                value: DIM_PERCENT
            }))
        );
        assert_eq!(
            startup_action(&arguments(&["brightness-restore"])),
            Some(StartupAction::Command(Command::BrightnessRestore))
        );
    }

    #[test]
    fn the_command_line_maps_monitor_cycle_targets_and_modes() {
        assert_eq!(
            startup_action(&arguments(&["monitor-cycle", "DP-2", "mirror"])),
            Some(StartupAction::Command(Command::MonitorCycle {
                target: "DP-2".to_string(),
                mode: Some(LayoutPreset::Mirror),
            }))
        );
    }

    #[test]
    fn an_unknown_command_returns_none() {
        assert_eq!(startup_action(&arguments(&["power-profile"])), None);
    }

    fn arguments(words: &[&str]) -> Vec<std::ffi::OsString> {
        std::iter::once(std::ffi::OsString::from("nosh"))
            .chain(words.iter().map(std::ffi::OsString::from))
            .collect()
    }
}
