//! The quick settings overlay.
//!
//! The window rules follow `docs/features/quick-settings.md`. The card layout,
//! spacing, and class names follow `docs/reference/design_proto.html`.
//!
//! The UI owns the open state. Services own the toggle states, so a control
//! only changes after the core reports the new value.

mod dropdown;
mod header;
mod network;
mod sliders;
mod toggles;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nosh_core::{
    AudioModel, BatteryModel, BrightnessDevice, Core, IdleInhibitState, NetworkModel,
    PowerProfilesModel,
};

use header::Header;
use sliders::Sliders;
use toggles::Toggles;

pub struct QuickSettingsWindow {
    window: gtk::ApplicationWindow,
    inner: Rc<Inner>,
}

struct Inner {
    open: Cell<bool>,
    card: gtk::Box,
    header: Header,
    toggles: Toggles,
    sliders: Rc<Sliders>,
    monitor: RefCell<Option<gdk::Monitor>>,
    session_handler: RefCell<Option<Rc<dyn Fn()>>>,
}

impl QuickSettingsWindow {
    pub fn new(app: &gtk::Application, core: Rc<Core>) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.set_decorated(false);
        window.init_layer_shell();
        window.set_namespace(Some("nosh-quick-settings"));
        window.set_layer(Layer::Overlay);
        window.set_exclusive_zone(-1);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.add_css_class("QuickSettings");
        window.set_visible(false);

        let background = gtk::Box::new(gtk::Orientation::Vertical, 0);
        background.add_css_class("QuickSettingsBackground");
        background.set_hexpand(true);
        background.set_vexpand(true);

        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("QuickSettingsCard");
        card.set_halign(gtk::Align::End);
        card.set_valign(gtk::Align::Start);
        // Scaled from the prototype's 58/16 by the bar's 0.75 factor.
        card.set_margin_top(44);
        card.set_margin_end(12);
        card.set_margin_start(12);
        card.set_margin_bottom(12);
        card.set_can_focus(true);

        let header = Header::new();
        let toggles = Toggles::new(Rc::clone(&core));
        let sliders = Sliders::new(core);

        card.append(header.widget());
        card.append(toggles.widget());
        card.append(sliders.widget());
        background.append(&card);
        window.set_child(Some(&background));

        let inner = Rc::new(Inner {
            open: Cell::new(false),
            card,
            header,
            toggles,
            sliders,
            monitor: RefCell::new(None),
            session_handler: RefCell::new(None),
        });

        {
            let inner = Rc::clone(&inner);
            let session = inner.header.session_button().clone();
            session.connect_clicked(move |_| {
                let handler = inner.session_handler.borrow().clone();
                if let Some(handler) = handler {
                    handler();
                }
            });
        }

        let click = gtk::GestureClick::new();
        {
            let background = background.clone();
            let window = window.clone();
            let inner = Rc::clone(&inner);
            click.connect_pressed(move |_, _, x, y| {
                let target: gtk::Widget = background.clone().upcast();
                if target.pick(x, y, gtk::PickFlags::DEFAULT).as_ref() == Some(&target) {
                    close_window(&window, &inner);
                }
            });
        }
        background.add_controller(click);

        let keys = gtk::EventControllerKey::new();
        {
            let window = window.clone();
            let inner = Rc::clone(&inner);
            keys.connect_key_pressed(move |_, key, _, _| {
                if key == gdk::Key::Escape {
                    close_window(&window, &inner);
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
        }
        window.add_controller(keys);

        Self { window, inner }
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.window
    }

    pub fn is_open(&self) -> bool {
        self.inner.open.get()
    }

    pub fn toggle(&self, monitor: Option<&gdk::Monitor>) {
        if self.is_open() {
            self.close();
        } else {
            self.open(monitor);
        }
    }

    pub fn open(&self, monitor: Option<&gdk::Monitor>) {
        if let Some(monitor) = monitor {
            self.window.set_monitor(Some(monitor));
            *self.inner.monitor.borrow_mut() = Some(monitor.clone());
        }
        self.inner.open.set(true);
        self.window.present();
        self.inner.card.grab_focus();
    }

    /// The monitor the panel was last opened on. The power menu reuses it.
    pub fn current_monitor(&self) -> Option<gdk::Monitor> {
        self.inner.monitor.borrow().clone()
    }

    /// Runs when the header's session button is pressed.
    pub fn set_session_handler(&self, handler: Rc<dyn Fn()>) {
        *self.inner.session_handler.borrow_mut() = Some(handler);
    }

    pub fn close(&self) {
        close_window(&self.window, &self.inner);
    }

    pub fn set_audio(&self, model: &AudioModel) {
        self.inner.sliders.set_audio(model);
    }

    pub fn set_brightness(&self, devices: &[BrightnessDevice]) {
        self.inner.sliders.set_brightness(devices);
    }

    pub fn set_battery(&self, model: &BatteryModel) {
        self.inner.header.render(model);
    }

    pub fn set_idle_inhibit(&self, state: &IdleInhibitState) {
        self.inner.toggles.render_idle_inhibit(state);
    }

    pub fn set_dnd(&self, enabled: bool) {
        self.inner.toggles.render_dnd(enabled);
    }

    pub fn set_dnd_unavailable(&self) {
        self.inner.toggles.render_dnd_unavailable();
    }

    pub fn set_power_profiles(&self, model: Option<&PowerProfilesModel>) {
        self.inner.toggles.render_power_profiles(model);
    }

    pub fn set_network(&self, model: &NetworkModel) {
        self.inner.toggles.render_network(model);
    }

    pub fn set_network_error(&self, message: &str) {
        self.inner.toggles.show_network_error(message);
    }
}

fn close_window(window: &gtk::ApplicationWindow, inner: &Inner) {
    inner.sliders.close_dropdowns();
    inner.toggles.close_dropdowns();
    inner.open.set(false);
    gtk::prelude::GtkWindowExt::set_focus(window, None::<&gtk::Widget>);
    window.set_visible(false);
}
