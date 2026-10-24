mod battery;
mod idle_inhibitor;
mod stats;
mod tray;
mod volume;
mod workspaces;

pub use battery::Battery;
pub use idle_inhibitor::IdleInhibitor;
pub use stats::SystemStats;
pub use tray::Tray;
pub use volume::Volume;
pub use workspaces::Workspaces;

use std::collections::HashMap;
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nosh_core::{
    AudioModel, BatteryModel, Core, IdleInhibitState, SystemStats as StatsSnapshot, TrayItemModel,
    TrayMenu, WorkspaceSnapshot,
};

pub struct BarWindow {
    connector: String,
    window: gtk::ApplicationWindow,
    workspaces: Workspaces,
    stats: SystemStats,
    tray: Tray,
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
        bar.add_css_class("min-h-6");
        bar.set_hexpand(true);

        let workspaces = Workspaces::new(Rc::clone(&core));
        let stats = SystemStats::new();
        let tray = Tray::new(Rc::clone(&core));
        let idle_inhibitor = IdleInhibitor::new(Rc::clone(&core));
        let battery = Battery::new();
        let volume = Volume::new(core);

        let left = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        left.set_halign(gtk::Align::Start);
        left.set_hexpand(true);
        left.append(workspaces.widget());
        bar.set_start_widget(Some(&left));

        let center = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        center.set_halign(gtk::Align::Center);
        center.append(&placeholder("center"));
        bar.set_center_widget(Some(&center));

        let right = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        right.set_halign(gtk::Align::End);
        right.set_hexpand(true);
        right.add_css_class("Right");
        right.append(stats.widget());
        right.append(tray.widget());
        right.append(idle_inhibitor.widget());
        right.append(battery.widget());
        right.append(volume.widget());
        right.append(&placeholder("right"));
        bar.set_end_widget(Some(&right));

        window.set_child(Some(&bar));

        Self {
            connector,
            window,
            workspaces,
            stats,
            tray,
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

    pub fn set_audio(&mut self, model: &AudioModel) {
        self.volume.render(model);
    }
}

fn placeholder(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("Placeholder");
    label
}
