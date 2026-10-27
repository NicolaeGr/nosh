//! The two-column toggle grid.
//!
//! Controls whose core service does not exist yet stay visible and insensitive,
//! following the VPN rule in `docs/features/quick-settings.md`. Power profiles
//! are a soft dependency, so that control hides when the daemon is missing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{Command, Core, IdleInhibitState, PowerProfile, PowerProfilesModel};

pub struct Toggles {
    root: gtk::Box,
    core: Rc<Core>,
    caffeine: Toggle,
    caffeine_active: Rc<Cell<bool>>,
    power: Toggle,
    power_list: gtk::Box,
    power_dropdown: PowerDropdown,
    power_rows: RefCell<Vec<PowerRow>>,
    power_model: RefCell<Option<PowerProfilesModel>>,
}

impl Toggles {
    pub fn new(core: Rc<Core>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_margin_bottom(10);

        let grid = gtk::Grid::new();
        grid.add_css_class("QuickSettingsToggles");
        grid.set_column_spacing(6);
        grid.set_row_spacing(6);
        grid.set_column_homogeneous(true);

        let wifi = Toggle::new("Wi-Fi", "network-wireless-symbolic", "Off", true);
        let bluetooth = Toggle::new("Bluetooth", "bluetooth-symbolic", "Off", true);
        let vpn = Toggle::new("VPN", "security-high-symbolic", "Off", true);
        let kde = Toggle::new("KDE Connect", "phone-symbolic", "Not available", true);
        let power = Toggle::new(
            "Power Profile",
            "power-profile-balanced-symbolic",
            "Balanced",
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
        night.set_available(false, "Night Light support is not available yet");
        dnd.set_available(false, "Notifications are not available yet");

        let power_list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        power_list.add_css_class("SliderDropdown");
        let power_revealer = gtk::Revealer::new();
        power_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        power_revealer.set_transition_duration(300);
        power_revealer.set_reveal_child(false);
        power_revealer.set_child(Some(&power_list));
        // Hidden until the daemon reports a state.
        power_revealer.set_visible(false);
        power.widget().set_visible(false);

        let power_dropdown = PowerDropdown {
            grid: grid.clone(),
            revealer: power_revealer.clone(),
            disclosure: power.disclosure().cloned(),
            later: vec![dnd.widget().clone(), caffeine.widget().clone()],
            open: Rc::new(Cell::new(false)),
        };

        let caffeine_active = Rc::new(Cell::new(false));
        {
            let core = Rc::clone(&core);
            let active = Rc::clone(&caffeine_active);
            caffeine.button().connect_clicked(move |_| {
                core.send(Command::IdleInhibitSet(!active.get()));
            });
        }
        {
            let core = Rc::clone(&core);
            power.button().connect_clicked(move |_| {
                core.send(Command::PowerProfileCycle);
            });
        }
        if let Some(disclosure) = &power_dropdown.disclosure {
            let dropdown = power_dropdown.clone();
            disclosure.connect_clicked(move |_| dropdown.toggle());
        }

        // The drop-down lives on the row under the power toggle. The toggles
        // below it move down while it is open, so the list opens where the
        // control sits instead of under the last row.
        for (toggle, column, row) in [
            (&wifi, 0, 0),
            (&bluetooth, 1, 0),
            (&vpn, 0, 1),
            (&kde, 1, 1),
            (&power, 0, 2),
            (&night, 1, 2),
            (&dnd, 0, 3),
            (&caffeine, 1, 3),
        ] {
            grid.attach(toggle.widget(), column, row, 1, 1);
        }
        grid.attach(&power_revealer, 0, 3, 2, 1);

        root.append(&grid);

        let toggles = Self {
            root,
            core,
            caffeine,
            caffeine_active,
            power,
            power_list,
            power_dropdown,
            power_rows: RefCell::new(Vec::new()),
            power_model: RefCell::new(None),
        };
        toggles.render_idle_inhibit(&IdleInhibitState::default());
        toggles
    }

    pub fn widget(&self) -> &gtk::Box {
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

    /// Hides the control when the daemon is missing, following the soft
    /// dependency rule for power profiles.
    pub fn render_power_profiles(&self, model: Option<&PowerProfilesModel>) {
        let Some(model) = model else {
            self.power.widget().set_visible(false);
            self.power_dropdown.set_open(false);
            self.power_dropdown.revealer.set_visible(false);
            self.power.set_on(false);
            *self.power_model.borrow_mut() = None;
            return;
        };

        self.power.widget().set_visible(true);
        self.power.set_available(true, "");
        self.power.set_icon(&model.active.icon_name());
        self.power.set_status(model.active.label());
        // Balanced is the neutral state, so the toggle reads as off.
        self.power.set_on(!model.active.is_balanced());
        self.power.set_tooltip(Some(&match &model.degraded {
            Some(reason) => format!("{} profile. {}", model.active.label(), reason),
            None => format!("{} profile", model.active.label()),
        }));

        let profiles_changed = self
            .power_model
            .borrow()
            .as_ref()
            .map(|previous| previous.profiles != model.profiles)
            .unwrap_or(true);
        *self.power_model.borrow_mut() = Some(model.clone());
        if profiles_changed {
            self.rebuild_power_rows(&model.profiles);
        }

        for row in self.power_rows.borrow().iter() {
            if row.profile == model.active {
                row.button.add_css_class("is-active");
                row.check.set_visible(true);
            } else {
                row.button.remove_css_class("is-active");
                row.check.set_visible(false);
            }
        }
    }

    pub fn close_dropdowns(&self) {
        self.power_dropdown.set_open(false);
    }

    fn rebuild_power_rows(&self, profiles: &[PowerProfile]) {
        while let Some(child) = self.power_list.first_child() {
            self.power_list.remove(&child);
        }

        let mut rows = Vec::new();
        for profile in profiles {
            let button = gtk::Button::new();
            button.add_css_class("PowerProfileRow");

            let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let label = gtk::Label::new(Some(profile.label()));
            label.add_css_class("PowerProfileName");
            label.set_xalign(0.0);
            label.set_hexpand(true);
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.add_css_class("PowerProfileCheck");
            check.set_visible(false);
            content.append(&label);
            content.append(&check);
            button.set_child(Some(&content));

            {
                let core = Rc::clone(&self.core);
                let dropdown = self.power_dropdown.clone();
                let profile = profile.clone();
                button.connect_clicked(move |_| {
                    core.send(Command::PowerProfileSet(profile.clone()));
                    dropdown.set_open(false);
                });
            }

            self.power_list.append(&button);
            rows.push(PowerRow {
                profile: profile.clone(),
                button,
                check,
            });
        }

        *self.power_rows.borrow_mut() = rows;
    }
}

/// The power profile list and the grid positions it needs while open.
#[derive(Clone)]
struct PowerDropdown {
    grid: gtk::Grid,
    revealer: gtk::Revealer,
    disclosure: Option<gtk::Button>,
    later: Vec<gtk::Box>,
    open: Rc<Cell<bool>>,
}

impl PowerDropdown {
    fn toggle(&self) {
        self.set_open(!self.open.get());
    }

    fn set_open(&self, open: bool) {
        if self.open.get() == open {
            return;
        }
        self.open.set(open);

        if let Some(disclosure) = &self.disclosure {
            if open {
                disclosure.add_css_class("is-open");
            } else {
                disclosure.remove_css_class("is-open");
            }
        }

        if open {
            self.layout_later(true);
            self.revealer.set_visible(true);
            self.revealer.set_reveal_child(true);
            return;
        }

        self.revealer.set_reveal_child(false);
        // Move the rows back up once the list has finished collapsing, so the
        // shrinking list does not overlap them.
        let grid = self.grid.clone();
        let later = self.later.clone();
        let revealer = self.revealer.clone();
        let still_open = Rc::clone(&self.open);
        glib::timeout_add_local_once(Duration::from_millis(320), move || {
            if still_open.get() {
                return;
            }
            revealer.set_visible(false);
            layout_later(&grid, &later, false);
        });
    }

    fn layout_later(&self, open: bool) {
        layout_later(&self.grid, &self.later, open);
    }
}

fn layout_later(grid: &gtk::Grid, later: &[gtk::Box], open: bool) {
    for widget in later {
        grid.remove(widget);
    }

    let row = if open { 4 } else { 3 };
    for (column, widget) in later.iter().enumerate() {
        grid.attach(widget, column as i32, row, 1, 1);
    }
}

struct PowerRow {
    profile: PowerProfile,
    button: gtk::Button,
    check: gtk::Image,
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

    fn disclosure(&self) -> Option<&gtk::Button> {
        self.disclosure.as_ref()
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

    fn set_tooltip(&self, text: Option<&str>) {
        self.root.set_tooltip_text(text);
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
