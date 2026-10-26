use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{
    volume_icon, AudioEndpoint, AudioEvent, AudioModel, BatteryEvent, BatteryModel, Command, Core,
    EndpointKind, Event, IdleInhibitEvent, IdleInhibitState, MenuEvent, SystemStats,
    SystemStatsEvent, TrayEvent, TrayItemModel, TrayMenu, WorkspaceEvent, WorkspaceSnapshot,
};

use crate::bar::BarWindow;
use crate::menu::MenuManager;

#[cfg(all(feature = "dev", feature = "stable"))]
compile_error!("features `dev` and `stable` are mutually exclusive");

pub const APP_ID: &str = if cfg!(feature = "dev") {
    "com.nicolaegr.nosh.dev"
} else {
    "com.nicolaegr.nosh"
};

const STYLE: &str = include_str!("../resources/style.css");
const MENU_STYLE: &str = include_str!("../resources/menu.css");

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
    tray: RefCell<Vec<TrayItemModel>>,
    tray_menus: RefCell<HashMap<String, TrayMenu>>,
    bars: RefCell<BTreeMap<String, BarWindow>>,
    menus: RefCell<MenuManager>,
}

impl Ui {
    fn new(app: gtk::Application, core: Rc<Core>) -> Self {
        Self {
            app,
            core,
            workspaces: RefCell::new(WorkspaceSnapshot::default()),
            stats: RefCell::new(SystemStats::default()),
            audio: RefCell::new(AudioModel::default()),
            battery: RefCell::new(BatteryModel::default()),
            idle_inhibit: RefCell::new(IdleInhibitState::default()),
            tray: RefCell::new(Vec::new()),
            tray_menus: RefCell::new(HashMap::new()),
            bars: RefCell::new(BTreeMap::new()),
            menus: RefCell::new(MenuManager::new()),
        }
    }

    fn sync_bars(&self) {
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
            );
            bar.set_workspaces(&self.workspaces.borrow());
            bar.set_stats(&self.stats.borrow());
            bar.set_audio(&self.audio.borrow());
            bar.set_battery(&self.battery.borrow());
            bar.set_idle_inhibit(&self.idle_inhibit.borrow());
            bar.set_tray(&self.tray.borrow(), &self.tray_menus.borrow());
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
    }

    fn render_audio(&self) {
        let model = self.audio.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_audio(&model);
        }
    }

    fn render_battery(&self) {
        let model = self.battery.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_battery(&model);
        }
    }

    fn render_tray(&self) {
        let items = self.tray.borrow();
        let menus = self.tray_menus.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_tray(&items, &menus);
        }
    }

    fn apply(&self, event: Event) {
        match event {
            Event::Snapshot(snapshot) => {
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
                if let Some(tray) = snapshot.tray {
                    self.replace_tray(tray);
                }
            }
            Event::Workspaces(event) => self.apply_workspace_event(event),
            Event::SystemStats(SystemStatsEvent::Changed(stats)) => {
                *self.stats.borrow_mut() = stats;
                self.render_stats();
            }
            Event::Audio(event) => self.apply_audio_event(event),
            Event::Battery(event) => self.apply_battery_event(event),
            Event::IdleInhibit(event) => self.apply_idle_inhibit_event(event),
            Event::Tray(event) => self.apply_tray_event(event),
            Event::Menu(event) => self.apply_menu_event(event),
            _ => {}
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

    fn apply_audio_event(&self, event: AudioEvent) {
        {
            let mut model = self.audio.borrow_mut();
            match event {
                AudioEvent::Snapshot(snapshot) => *model = snapshot,
                AudioEvent::VolumeChanged {
                    kind, id, volume, ..
                } => update_endpoint(&mut model, kind, id, |endpoint| {
                    endpoint.volume = volume;
                    endpoint.icon_name =
                        volume_icon(kind, endpoint.volume, endpoint.muted).to_string();
                }),
                AudioEvent::MuteChanged {
                    kind, id, muted, ..
                } => update_endpoint(&mut model, kind, id, |endpoint| {
                    endpoint.muted = muted;
                    endpoint.icon_name =
                        volume_icon(kind, endpoint.volume, endpoint.muted).to_string();
                }),
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
        self.core.send(Command::MenuOpen(id.to_string()));
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

pub fn run(arguments: Vec<std::ffi::OsString>) -> Result<u8, Box<dyn Error>> {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let pending: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let ui_slot: Rc<RefCell<Option<Rc<Ui>>>> = Rc::new(RefCell::new(None));

    app.connect_startup(|_| install_css());

    // A secondary `nosh menu <id>` forwards its command line to the running
    // instance, which owns the windows.
    {
        let ui_slot = Rc::clone(&ui_slot);
        let pending = Rc::clone(&pending);
        app.connect_command_line(move |app, command_line| {
            if let Some(id) = menu_from_arguments(&command_line.arguments()) {
                let handled = ui_slot
                    .borrow()
                    .as_ref()
                    .map(|ui| ui.open_menu(&id))
                    .is_some();
                if !handled {
                    *pending.borrow_mut() = Some(id);
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
            // `nosh menu <id>` process forwards its command line and exits
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
            ui.sync_bars();

            if let Some(display) = gdk::Display::default() {
                let ui = Rc::clone(&ui);
                display
                    .monitors()
                    .connect_items_changed(move |_, _, _, _| ui.sync_bars());
            }

            let pending = pending.borrow_mut().take();
            ui_slot.borrow_mut().replace(Rc::clone(&ui));
            if let Some(id) = pending {
                ui.open_menu(&id);
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

fn menu_from_arguments(arguments: &[std::ffi::OsString]) -> Option<String> {
    let mut words = arguments.iter().filter_map(|argument| argument.to_str());
    while let Some(word) = words.next() {
        if word == "menu" {
            return words.next().map(ToString::to_string);
        }
    }
    None
}

fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };

    let builtin = format!("{STYLE}\n{MENU_STYLE}");
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
    fn the_command_line_maps_menu_ids() {
        let arguments = vec![
            std::ffi::OsString::from("nosh"),
            std::ffi::OsString::from("menu"),
            std::ffi::OsString::from("wallpapers"),
        ];
        assert_eq!(
            menu_from_arguments(&arguments).as_deref(),
            Some("wallpapers")
        );
    }
}
