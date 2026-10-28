//! The two-column toggle grid.
//!
//! Controls whose core service does not exist yet stay visible and insensitive,
//! following the VPN rule in `docs/features/quick-settings.md`. Power profiles
//! are a soft dependency, so that control hides when the daemon is missing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{Command, Core, IdleInhibitState, NetworkModel, PowerProfile, PowerProfilesModel};

use super::dropdown::Dropdown;
use super::network::{NetworkPanel, VpnPanel};

pub struct Toggles {
    root: gtk::Box,
    core: Rc<Core>,
    airplane: Toggle,
    wifi: Toggle,
    vpn: Toggle,
    network: Rc<NetworkPanel>,
    vpn_page: Rc<VpnPanel>,
    network_model: Rc<RefCell<NetworkModel>>,
    caffeine: Toggle,
    caffeine_active: Rc<Cell<bool>>,
    dnd: Toggle,
    dnd_active: Rc<Cell<bool>>,
    power: Toggle,
    power_list: gtk::Box,
    power_dropdown: Dropdown,
    power_rows: RefCell<Vec<PowerRow>>,
    power_model: RefCell<Option<PowerProfilesModel>>,
}

impl Toggles {
    pub fn new(core: Rc<Core>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("QuickSettingsToggles");
        root.set_margin_bottom(10);

        let wifi = Toggle::new("Network", "network-wireless-symbolic", "Off", true);
        let bluetooth = Toggle::new("Bluetooth", "bluetooth-symbolic", "Off", true);
        let vpn = Toggle::new("VPN", "network-vpn-symbolic", "Off", true);
        let kde = Toggle::new("KDE Connect", "phone-symbolic", "Not available", true);
        let power = Toggle::new(
            "Power Profile",
            "power-profile-balanced-symbolic",
            "Balanced",
            true,
        );
        let airplane = Toggle::new("Airplane", "airplane-mode-disabled-symbolic", "Off", false);
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

        wifi.set_available(false, "No Wi-Fi hardware");
        vpn.set_available(false, "No VPN profiles configured");
        bluetooth.set_available(false, "Bluetooth support is not available yet");
        kde.set_available(false, "KDE Connect support is not available yet");
        dnd.set_available(false, "Notifications are not available yet");
        airplane.set_available(false, "Airplane mode is not available");
        airplane.set_tooltip(Some(
            "Disables Wi-Fi, mobile, and Bluetooth radios. Wired connections stay on.",
        ));

        // Hidden until the daemon reports a state.
        power.widget().set_visible(false);

        let row0 = toggle_row(&wifi, &bluetooth);
        let row1 = toggle_row(&vpn, &kde);
        let row2 = toggle_row(&power, &airplane);
        let row3 = toggle_row(&dnd, &caffeine);
        for row in [&row0, &row1, &row2, &row3] {
            root.append(row);
        }

        let power_list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        power_list.add_css_class("PowerProfileList");
        power_list.add_css_class("SliderDropdown");
        let power_dropdown = Dropdown::new(&root, &row2, power.disclosure());
        power_dropdown.set_child(&power_list);

        let network = NetworkPanel::new(Rc::clone(&core), &root, &row0, wifi.disclosure());
        let vpn_page = VpnPanel::new(Rc::clone(&core), &root, &row1, vpn.disclosure());
        let network_model = Rc::new(RefCell::new(NetworkModel::default()));

        {
            let core = Rc::clone(&core);
            let airplane_model = Rc::clone(&network_model);
            airplane.button().connect_clicked(move |_| {
                let enabled = airplane_model.borrow().airplane.enabled;
                core.send(Command::AirplaneModeSet(!enabled));
            });
        }

        let open_network: Rc<dyn Fn()> = {
            let network = Rc::clone(&network);
            let vpn_page = Rc::clone(&vpn_page);
            let power = power_dropdown.clone();
            Rc::new(move || {
                let open = !network.is_open();
                vpn_page.set_open(false);
                power.set_open(false);
                network.set_open(open);
            })
        };
        {
            let open = Rc::clone(&open_network);
            wifi.button().connect_clicked(move |_| open());
        }
        if let Some(disclosure) = wifi.disclosure() {
            let open = Rc::clone(&open_network);
            disclosure.connect_clicked(move |_| open());
        }

        let open_vpn: Rc<dyn Fn()> = {
            let network = Rc::clone(&network);
            let vpn_page = Rc::clone(&vpn_page);
            let power = power_dropdown.clone();
            Rc::new(move || {
                let open = !vpn_page.is_open();
                network.set_open(false);
                power.set_open(false);
                vpn_page.set_open(open);
            })
        };
        {
            let open = Rc::clone(&open_vpn);
            vpn.button().connect_clicked(move |_| open());
        }
        if let Some(disclosure) = vpn.disclosure() {
            let open = Rc::clone(&open_vpn);
            disclosure.connect_clicked(move |_| open());
        }

        let caffeine_active = Rc::new(Cell::new(false));
        let dnd_active = Rc::new(Cell::new(false));
        {
            let core = Rc::clone(&core);
            let active = Rc::clone(&dnd_active);
            dnd.button().connect_clicked(move |_| {
                core.send(Command::NotificationSetDnd(!active.get()));
            });
        }
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
        if let Some(disclosure) = power.disclosure() {
            let network = Rc::clone(&network);
            let vpn_page = Rc::clone(&vpn_page);
            let dropdown = power_dropdown.clone();
            disclosure.connect_clicked(move |_| {
                let open = !dropdown.is_open();
                network.set_open(false);
                vpn_page.set_open(false);
                dropdown.set_open(open);
            });
        }

        let toggles = Self {
            root,
            core,
            airplane,
            wifi,
            vpn,
            network,
            vpn_page,
            network_model,
            caffeine,
            caffeine_active,
            dnd,
            dnd_active,
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

    /// DND follows the daemon state, so the toggle flips only after the core
    /// reports the new value.
    pub fn render_dnd(&self, enabled: bool) {
        self.dnd_active.set(enabled);
        self.dnd.set_available(true, "");
        self.dnd.set_on(enabled);
        self.dnd.set_icon(if enabled {
            "notifications-disabled-symbolic"
        } else {
            "notifications-symbolic"
        });
        self.dnd.set_status(if enabled { "On" } else { "Off" });
        self.dnd.set_tooltip(Some(if enabled {
            "Notifications are stored without showing a card"
        } else {
            "Show notification cards"
        }));
    }

    pub fn render_dnd_unavailable(&self) {
        self.dnd_active.set(false);
        self.dnd
            .set_available(false, "The notification daemon is not available");
        self.dnd.set_on(false);
        self.dnd.set_status("Not available");
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

    /// Renders the network toggles from the core state.
    pub fn render_network(&self, model: &NetworkModel) {
        *self.network_model.borrow_mut() = model.clone();

        let airplane = &model.airplane;
        self.airplane.set_available(
            airplane.available,
            "Airplane mode needs NetworkManager or an rfkill radio",
        );
        self.airplane.set_on(airplane.enabled);
        self.airplane.set_icon(airplane.icon_name());
        self.airplane.set_status(if airplane.transitioning {
            "Switching"
        } else if airplane.enabled {
            "On"
        } else {
            "Off"
        });
        self.airplane.set_tooltip(Some(
            "Disables Wi-Fi, mobile, and Bluetooth radios. Wired connections stay on.",
        ));

        let wifi = &model.wifi;
        // The toggle opens the network page, which also controls wired devices.
        // Keep it usable when NetworkManager is up but no Wi-Fi radio exists.
        self.wifi
            .set_available(model.available, "NetworkManager is not available");
        self.wifi
            .set_on(wifi.available && wifi.enabled && !airplane.enabled);
        let wired = model.wired.iter().find(|device| device.connected);
        let icon: &str = if airplane.enabled {
            airplane.icon_name()
        } else if let Some(device) = wired {
            device.icon_name()
        } else if wifi.available {
            wifi.icon_name.as_str()
        } else if let Some(device) = model.wired.first() {
            device.icon_name()
        } else {
            "network-wireless-offline-symbolic"
        };
        self.wifi.set_icon(icon);
        self.wifi.set_on(
            !airplane.enabled
                && (wired.is_some()
                    || (wifi.available && wifi.enabled && wifi.active_ssid.is_some())),
        );
        self.wifi.set_status(if airplane.enabled {
            "Airplane"
        } else if let Some(device) = wired {
            device.title()
        } else if !wifi.available {
            model
                .wired
                .first()
                .map(|device| device.status())
                .unwrap_or("No network")
        } else if !wifi.enabled {
            "Wi-Fi off"
        } else {
            wifi.active_ssid.as_deref().unwrap_or("Disconnected")
        });

        let vpn = &model.vpn;
        let tooltip = match vpn.active_name() {
            Some(vpn) => format!("{} · VPN: {vpn}", model.summary()),
            None => model.summary().to_string(),
        };
        self.wifi.set_tooltip(Some(&tooltip));

        let vpn = &model.vpn;
        self.vpn.set_available(
            !vpn.profiles.is_empty(),
            "No VPN profiles configured in NetworkManager",
        );
        self.vpn.set_on(vpn.active_name().is_some());
        self.vpn.set_icon("network-vpn-symbolic");
        self.vpn.set_status(if let Some(name) = vpn.active_name() {
            name
        } else if vpn.profiles.is_empty() {
            "Not configured"
        } else {
            "Off"
        });
        self.vpn.set_tooltip(Some(if vpn.profiles.is_empty() {
            "No VPN profiles configured in NetworkManager"
        } else {
            "Open the VPN list to choose a connection"
        }));

        self.network.render(model);
        self.vpn_page.render(model);
    }

    pub fn show_network_error(&self, message: &str) {
        self.network.show_message(message);
        self.vpn_page.show_message(message);
    }

    pub fn close_dropdowns(&self) {
        self.power_dropdown.set_open(false);
        self.network.set_open(false);
        self.vpn_page.set_open(false);
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

struct PowerRow {
    profile: PowerProfile,
    button: gtk::Button,
    check: gtk::Image,
}

fn toggle_row(left: &Toggle, right: &Toggle) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.set_homogeneous(true);
    row.append(left.widget());
    row.append(right.widget());
    row
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
