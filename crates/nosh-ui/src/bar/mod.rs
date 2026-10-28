mod battery;
mod idle_inhibitor;
mod media;
mod network;
mod stats;
pub(crate) mod time;
mod tray;
mod volume;
mod workspaces;

pub use battery::Battery;
pub use idle_inhibitor::IdleInhibitor;
pub use network::Network;
pub use stats::SystemStats;
pub use tray::Tray;
pub use volume::Volume;
pub use workspaces::Workspaces;

use std::collections::HashMap;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nosh_core::{
    AudioModel, BatteryModel, Core, IdleInhibitState, NetworkModel, SystemStats as StatsSnapshot,
    TrayItemModel, TrayMenu, WorkspaceSnapshot,
};

use crate::center_panel::{CenterPanelWindow, Geometry, Rect, Tab};
use crate::quick_settings::QuickSettingsWindow;

pub struct BarWindow {
    connector: String,
    window: gtk::ApplicationWindow,
    workspaces: Workspaces,
    stats: SystemStats,
    tray: Tray,
    network: Network,
    idle_inhibitor: IdleInhibitor,
    battery: Battery,
    volume: Volume,
}

impl BarWindow {
    pub fn new(
        app: &gtk::Application,
        monitor: &gdk::Monitor,
        connector: String,
        core: Rc<Core>,
        quick_settings: Rc<QuickSettingsWindow>,
        center_panel: Rc<CenterPanelWindow>,
    ) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.set_decorated(false);
        // Do not make the window non-resizable. GTK pins it to its natural
        // width, which breaks the left and right anchors.
        window.set_width_request(1);
        window.set_height_request(1);

        window.init_layer_shell();
        window.set_namespace(Some("nosh-topbar"));
        window.set_layer(Layer::Top);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Right, true);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_monitor(Some(monitor));
        window.auto_exclusive_zone_enable();

        window.add_css_class("Bar");
        for class in ["min-h-6", "px-1", "py-0", "mx-1", "text-sm", "font-bold"] {
            window.add_css_class(class);
        }
        if cfg!(feature = "dev") {
            window.add_css_class("secondary-instance");
        }

        let bar = gtk::CenterBox::new();
        bar.add_css_class("BarRoot");
        bar.add_css_class("min-h-6");
        bar.set_hexpand(true);

        let workspaces = Workspaces::new(Rc::clone(&core));
        let stats = SystemStats::new();
        let tray = Tray::new(Rc::clone(&core));
        let network = Network::new();
        let idle_inhibitor = IdleInhibitor::new(Rc::clone(&core));
        let battery = Battery::new();
        let volume = Volume::new(core);

        let quick_settings_button = gtk::Button::new();
        quick_settings_button.add_css_class("square-icon");
        quick_settings_button.set_focus_on_click(false);
        quick_settings_button.set_child(Some(&gtk::Image::from_icon_name(
            "preferences-other-symbolic",
        )));
        quick_settings_button.set_tooltip_text(Some("Quick settings"));
        {
            let quick_settings = Rc::clone(&quick_settings);
            let monitor = monitor.clone();
            quick_settings_button.connect_clicked(move |_| {
                quick_settings.toggle(Some(&monitor));
            });
        }
        {
            let button = quick_settings_button.clone();
            quick_settings
                .window()
                .connect_visible_notify(move |window| {
                    if window.is_visible() {
                        button.add_css_class("is-open");
                    } else {
                        button.remove_css_class("is-open");
                    }
                });
        }

        let left = section(gtk::Align::Start);
        let left_island = island("IslandStart");
        left_island.append(workspaces.widget());
        left.append(&left_island);
        bar.set_start_widget(Some(&left));

        let center = section(gtk::Align::Center);
        let center_island = island("IslandCenter");
        let clock = time::Clock::new();
        let media = media::Media::new();
        center_island.append(clock.button());
        let media_divider = gtk::Box::new(gtk::Orientation::Vertical, 0);
        media_divider.add_css_class("MediaDivider");
        media_divider.set_size_request(1, 1);
        media_divider.set_valign(gtk::Align::Center);
        center_island.append(&media_divider);
        center_island.append(media.button());
        center.append(&center_island);
        bar.set_center_widget(Some(&center));

        {
            let panel = Rc::clone(&center_panel);
            let monitor = monitor.clone();
            let island = center_island.clone();
            let root = bar.clone();
            let clock_label = clock.label().clone();
            let media_button = media.button().clone();
            let open: Rc<dyn Fn(Tab, bool)> = Rc::new(move |tab, hover| {
                if panel.is_open() {
                    return;
                }
                let Some(island_rect) = bounds(&island, &root) else {
                    return;
                };
                let Some(clock_rect) = bounds(&clock_label, &root) else {
                    return;
                };
                let Some(media_rect) = bounds(&media_button, &root) else {
                    return;
                };
                let restore = island.clone();
                let fallback = restore.clone();
                island.set_opacity(0.0);
                let opened = panel.open(
                    Some(&monitor),
                    Geometry {
                        island: island_rect,
                        clock: clock_rect,
                        media: media_rect,
                    },
                    &time::now_text(),
                    tab,
                    hover,
                    move || restore.set_opacity(1.0),
                );
                if !opened {
                    fallback.set_opacity(1.0);
                }
            });

            let pending: Rc<std::cell::RefCell<Option<glib::SourceId>>> =
                Rc::new(std::cell::RefCell::new(None));

            let open_click = Rc::clone(&open);
            clock
                .button()
                .connect_clicked(move |_| open_click(Tab::Calendar, false));
            let open_click = Rc::clone(&open);
            media
                .button()
                .connect_clicked(move |_| open_click(Tab::Media, false));

            attach_hover(&center_island, media.button().clone(), open, pending);
        }

        let right = section(gtk::Align::End);
        right.add_css_class("Right");
        let right_island = island("IslandEnd");
        right_island.append(stats.widget());
        right_island.append(tray.widget());
        right_island.append(network.widget());
        right_island.append(idle_inhibitor.widget());
        right_island.append(battery.widget());
        right_island.append(volume.widget());
        right_island.append(&quick_settings_button);
        right.append(&right_island);
        bar.set_end_widget(Some(&right));

        window.set_child(Some(&bar));

        Self {
            connector,
            window,
            workspaces,
            stats,
            tray,
            network,
            idle_inhibitor,
            battery,
            volume,
        }
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn destroy(&self) {
        self.window.destroy();
    }

    pub fn set_workspaces(&mut self, snapshot: &WorkspaceSnapshot) {
        self.workspaces.render(snapshot, &self.connector);
    }

    pub fn set_stats(&mut self, stats: &StatsSnapshot) {
        self.stats.render(stats);
    }

    pub fn set_tray(&mut self, items: &[TrayItemModel], menus: &HashMap<String, TrayMenu>) {
        self.tray.render(items, menus);
    }

    pub fn set_idle_inhibit(&mut self, state: &IdleInhibitState) {
        self.idle_inhibitor.render(state);
    }

    pub fn set_battery(&mut self, model: &BatteryModel) {
        self.battery.render(model);
    }

    pub fn set_audio(&self, model: &AudioModel) {
        self.volume.render(model);
    }

    pub fn set_network(&self, model: &NetworkModel) {
        self.network.render(model);
    }
}

/// Hovering the centre island for a moment opens the panel. The tab follows
/// the part of the island under the pointer when the delay fires, so sliding
/// from the clock onto the player opens the media tab. Leaving before the delay
/// fires cancels the open.
fn attach_hover(
    island: &gtk::Box,
    media: gtk::Button,
    open: Rc<dyn Fn(Tab, bool)>,
    pending: Rc<std::cell::RefCell<Option<glib::SourceId>>>,
) {
    const HOVER_OPEN_MS: u64 = 500;

    let pointer: Rc<std::cell::Cell<(f64, f64)>> = Rc::new(std::cell::Cell::new((0.0, 0.0)));
    let motion = gtk::EventControllerMotion::new();
    {
        let open = Rc::clone(&open);
        let pending = Rc::clone(&pending);
        let island = island.clone();
        let media = media.clone();
        let pointer = Rc::clone(&pointer);
        motion.connect_enter(move |_, x, y| {
            cancel_hover(&pending);
            pointer.set((x, y));
            let open = Rc::clone(&open);
            let fired = Rc::clone(&pending);
            let island = island.clone();
            let media = media.clone();
            let pointer = Rc::clone(&pointer);
            let source = glib::timeout_add_local(
                std::time::Duration::from_millis(HOVER_OPEN_MS),
                move || {
                    *fired.borrow_mut() = None;
                    let (x, y) = pointer.get();
                    open(tab_at(&island, &media, x, y), true);
                    glib::ControlFlow::Break
                },
            );
            *pending.borrow_mut() = Some(source);
        });
    }
    {
        let pointer = Rc::clone(&pointer);
        motion.connect_motion(move |_, x, y| pointer.set((x, y)));
    }
    {
        let pending = Rc::clone(&pending);
        motion.connect_leave(move |_| cancel_hover(&pending));
    }
    island.add_controller(motion);
}

/// Which tab the pointer is over, using hit testing so the check still works
/// when the pointer sits on the media label or icon inside the button.
fn tab_at(island: &gtk::Box, media: &gtk::Button, x: f64, y: f64) -> Tab {
    let media: gtk::Widget = media.clone().upcast();
    let mut hit = island.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(widget) = hit {
        if widget == media {
            return Tab::Media;
        }
        hit = widget.parent();
    }
    Tab::Calendar
}

fn cancel_hover(pending: &std::cell::RefCell<Option<glib::SourceId>>) {
    if let Some(source) = pending.take() {
        source.remove();
    }
}

/// A widget rectangle relative to the bar root, in monitor coordinates.
fn bounds(widget: &impl IsA<gtk::Widget>, root: &impl IsA<gtk::Widget>) -> Option<Rect> {
    let rect = widget.compute_bounds(root)?;
    Some(Rect {
        x: rect.x().round() as i32,
        y: rect.y().round() as i32,
        width: rect.width().round().max(1.0) as i32,
        height: rect.height().round().max(1.0) as i32,
    })
}

/// One of the three bar sections. Only the outer sections expand.
fn section(align: gtk::Align) -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    outer.set_halign(align);
    outer.set_valign(gtk::Align::Start);
    outer.set_hexpand(align != gtk::Align::Center);
    outer
}

/// A floating island surface.
fn island(class: &str) -> gtk::Box {
    let island = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    island.add_css_class("Island");
    island.add_css_class(class);
    island.set_valign(gtk::Align::Start);
    island
}
