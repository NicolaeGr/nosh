//! The two-column toggle grid.
//!
//! Controls whose core service does not exist yet stay visible and insensitive,
//! following the VPN rule in `docs/features/quick-settings.md`.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{Command, Core, IdleInhibitState};

pub struct Toggles {
    root: gtk::Grid,
    caffeine: Toggle,
    caffeine_active: Rc<Cell<bool>>,
}

impl Toggles {
    pub fn new(core: Rc<Core>) -> Self {
        let root = gtk::Grid::new();
        root.add_css_class("QuickSettingsToggles");
        root.set_column_spacing(6);
        root.set_row_spacing(6);
        root.set_column_homogeneous(true);
        root.set_margin_bottom(10);

        let wifi = Toggle::new("Wi-Fi", "network-wireless-symbolic", "Off", true);
        let bluetooth = Toggle::new("Bluetooth", "bluetooth-symbolic", "Off", true);
        let vpn = Toggle::new("VPN", "security-high-symbolic", "Off", true);
        let kde = Toggle::new("KDE Connect", "phone-symbolic", "Not available", true);
        let power = Toggle::new(
            "Power Profile",
            "power-profile-balanced-symbolic",
            "Not available",
            true,
        );
        let night = Toggle::new("Night Light", "night-light-symbolic", "Off", false);
        let dnd = Toggle::new(
            "Do Not Disturb",
            "notifications-disabled-symbolic",
            "Off",
            false,
        );
        let caffeine = Toggle::new(
            "Caffeine",
            "my-caffeine-off-symbolic",
            "Sleep allowed",
            false,
        );

        wifi.set_available(false, "Wi-Fi support is not available yet");
        bluetooth.set_available(false, "Bluetooth support is not available yet");
        vpn.set_available(false, "VPN support is not available");
        kde.set_available(false, "KDE Connect support is not available yet");
        power.set_available(false, "Power profiles are not available");
        night.set_available(false, "Night Light support is not available yet");
        dnd.set_available(false, "Notifications are not available yet");

        let caffeine_active = Rc::new(Cell::new(false));
        {
            let core = Rc::clone(&core);
            let active = Rc::clone(&caffeine_active);
            caffeine.button().connect_clicked(move |_| {
                core.send(Command::IdleInhibitSet(!active.get()));
            });
        }

        for (index, toggle) in [
            &wifi, &bluetooth, &vpn, &kde, &power, &night, &dnd, &caffeine,
        ]
        .into_iter()
        .enumerate()
        {
            let column = (index % 2) as i32;
            let row = (index / 2) as i32;
            root.attach(toggle.widget(), column, row, 1, 1);
        }

        let toggles = Self {
            root,
            caffeine,
            caffeine_active,
        };
        toggles.render_idle_inhibit(&IdleInhibitState::default());
        toggles
    }

    pub fn widget(&self) -> &gtk::Grid {
        &self.root
    }

    pub fn render_idle_inhibit(&self, state: &IdleInhibitState) {
        self.caffeine_active.set(state.active);
        self.caffeine
            .set_available(state.available, "Idle inhibitor is not available");
        self.caffeine.set_on(state.active);
        self.caffeine.set_icon(if state.active {
            "my-caffeine-on-symbolic"
        } else {
            "my-caffeine-off-symbolic"
        });
        self.caffeine.set_status(if !state.available {
            "Not available"
        } else if state.active {
            "Sleep blocked"
        } else {
            "Sleep allowed"
        });
    }
}

struct Toggle {
    root: gtk::Box,
    button: gtk::Button,
    icon: gtk::Image,
    status: gtk::Label,
    disclosure: Option<gtk::Button>,
}

impl Toggle {
    fn new(name: &str, icon: &str, status: &str, with_disclosure: bool) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("QuickToggle");
        root.set_hexpand(true);

        let button = gtk::Button::new();
        button.add_css_class("QuickToggleButton");
        button.set_hexpand(true);

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        content.set_valign(gtk::Align::Center);

        let icon_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        icon_box.add_css_class("QuickToggleIcon");
        // Keep the rounded box square instead of letting the two-line label
        // stretch it vertically, and center the icon inside it.
        icon_box.set_halign(gtk::Align::Start);
        icon_box.set_valign(gtk::Align::Center);
        let icon = gtk::Image::from_icon_name(icon);
        icon.set_halign(gtk::Align::Center);
        icon.set_valign(gtk::Align::Center);
        // Fill the fixed-size box so the glyph centers instead of sitting left.
        icon.set_hexpand(true);
        icon.set_vexpand(true);
        icon_box.append(&icon);

        let meta = gtk::Box::new(gtk::Orientation::Vertical, 1);
        meta.set_hexpand(true);
        meta.set_valign(gtk::Align::Center);
        let name = gtk::Label::new(Some(name));
        name.add_css_class("QuickToggleName");
        name.set_xalign(0.0);
        let status = gtk::Label::new(Some(status));
        status.add_css_class("QuickToggleStatus");
        status.set_xalign(0.0);
        status.set_hexpand(true);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        meta.append(&name);
        meta.append(&status);

        content.append(&icon_box);
        content.append(&meta);
        button.set_child(Some(&content));
        root.append(&button);

        let disclosure = if with_disclosure {
            let divider = gtk::Box::new(gtk::Orientation::Vertical, 0);
            divider.add_css_class("QuickToggleDivider");
            divider.set_size_request(1, -1);
            root.append(&divider);

            let disclosure = gtk::Button::new();
            disclosure.add_css_class("QuickToggleDisclosure");
            disclosure.set_child(Some(&gtk::Image::from_icon_name("pan-down-symbolic")));
            root.append(&disclosure);
            Some(disclosure)
        } else {
            None
        };

        Self {
            root,
            button,
            icon,
            status,
            disclosure,
        }
    }

    fn widget(&self) -> &gtk::Box {
        &self.root
    }

    fn button(&self) -> &gtk::Button {
        &self.button
    }

    fn set_on(&self, on: bool) {
        if on {
            self.root.add_css_class("on");
        } else {
            self.root.remove_css_class("on");
        }
    }

    fn set_status(&self, text: &str) {
        self.status.set_text(text);
    }

    fn set_available(&self, available: bool, tooltip: &str) {
        self.button.set_sensitive(available);
        if let Some(disclosure) = &self.disclosure {
            disclosure.set_sensitive(available);
        }
        if available {
            self.root.remove_css_class("unavailable");
            self.root.set_tooltip_text(None);
        } else {
            self.root.add_css_class("unavailable");
            self.root.set_tooltip_text(Some(tooltip));
        }
    }

    fn set_icon(&self, icon_name: &str) {
        self.icon.set_icon_name(Some(icon_name));
    }
}
