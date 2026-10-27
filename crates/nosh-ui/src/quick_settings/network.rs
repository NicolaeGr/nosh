//! The network and VPN dropdowns.
//!
//! Both open inline under the row that owns the toggle. The network panel is
//! the only place that asks the core for the access point list: opening it sends
//! `WifiScan`; closing it sends `WifiSetDetails(false)`, so a closed dropdown
//! costs no bus traffic and no radio scans.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{AccessPointModel, Command, Core, NetworkModel, VpnProfileModel, WiredDeviceModel};

use super::dropdown::Dropdown;

pub struct NetworkPanel {
    dropdown: Dropdown,
    list: gtk::Box,
    message: gtk::Label,
    wifi_switch: gtk::Switch,
    password: gtk::Box,
    password_label: gtk::Label,
    password_entry: gtk::PasswordEntry,
    password_target: RefCell<Option<(String, String)>>,
    core: Rc<Core>,
    updating: Cell<bool>,
    model: RefCell<NetworkModel>,
}

impl NetworkPanel {
    pub fn new(
        core: Rc<Core>,
        container: &gtk::Box,
        after: &impl IsA<gtk::Widget>,
        indicator: Option<&gtk::Button>,
    ) -> Rc<Self> {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("NetworkPanel");
        content.add_css_class("SliderDropdown");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.add_css_class("NetworkHeader");

        let title = gtk::Label::new(Some("Network"));
        title.add_css_class("NetworkTitle");
        title.set_xalign(0.0);
        title.set_hexpand(true);

        let wifi_label = gtk::Label::new(Some("Wi-Fi"));
        wifi_label.add_css_class("NetworkSwitchLabel");
        let wifi_switch = gtk::Switch::new();
        wifi_switch.add_css_class("NetworkSwitch");
        wifi_switch.set_valign(gtk::Align::Center);

        let refresh = icon_button("view-refresh-symbolic", "Scan for networks");

        header.append(&title);
        header.append(&wifi_label);
        header.append(&wifi_switch);
        header.append(&refresh);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 1);
        list.add_css_class("NetworkList");

        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scrolled.set_min_content_height(110);
        scrolled.set_max_content_height(340);
        scrolled.set_propagate_natural_height(true);
        scrolled.set_child(Some(&list));

        let message = gtk::Label::new(None);
        message.add_css_class("NetworkMessage");
        message.set_xalign(0.0);
        message.set_wrap(true);
        message.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        message.set_max_width_chars(34);
        message.set_visible(false);

        let password = gtk::Box::new(gtk::Orientation::Vertical, 4);
        password.add_css_class("NetworkPassword");
        password.set_visible(false);

        let password_label = gtk::Label::new(None);
        password_label.add_css_class("NetworkName");
        password_label.set_xalign(0.0);
        password_label.set_wrap(true);
        password_label.set_max_width_chars(34);

        let password_entry = gtk::PasswordEntry::new();
        password_entry.set_placeholder_text(Some("Password"));

        let password_actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        password_actions.set_halign(gtk::Align::End);
        let password_cancel = gtk::Button::with_label("Cancel");
        password_cancel.add_css_class("NetworkPasswordButton");
        let password_connect = gtk::Button::with_label("Connect");
        password_connect.add_css_class("NetworkPasswordButton");
        password_connect.add_css_class("suggested-action");
        password_actions.append(&password_cancel);
        password_actions.append(&password_connect);

        password.append(&password_label);
        password.append(&password_entry);
        password.append(&password_actions);

        content.append(&header);
        content.append(&scrolled);
        content.append(&message);

        let dropdown = Dropdown::new(container, after, indicator);
        dropdown.set_child(&content);

        let panel = Rc::new(Self {
            dropdown,
            list,
            message,
            wifi_switch: wifi_switch.clone(),
            password,
            password_label,
            password_entry: password_entry.clone(),
            password_target: RefCell::new(None),
            core,
            updating: Cell::new(false),
            model: RefCell::new(NetworkModel::default()),
        });

        {
            let panel = Rc::clone(&panel);
            password_cancel.connect_clicked(move |_| panel.cancel_password());
        }
        {
            let panel = Rc::clone(&panel);
            password_connect.connect_clicked(move |_| panel.submit_password());
        }
        {
            let panel = Rc::clone(&panel);
            password_entry.connect_activate(move |_| panel.submit_password());
        }

        {
            let panel = Rc::clone(&panel);
            refresh.connect_clicked(move |_| panel.scan());
        }
        {
            let panel = Rc::clone(&panel);
            wifi_switch.connect_active_notify(move |switch| {
                if panel.updating.get() {
                    return;
                }
                panel.core.send(Command::WifiSetEnabled(switch.is_active()));
            });
        }

        panel
    }

    pub fn is_open(&self) -> bool {
        self.dropdown.is_open()
    }

    /// Opening asks for access points. Closing releases them.
    pub fn set_open(self: &Rc<Self>, open: bool) {
        if self.dropdown.is_open() == open {
            return;
        }
        if !open {
            self.cancel_password();
            self.dropdown.set_open(false);
            self.core.send(Command::WifiSetDetails(false));
            return;
        }

        self.message.set_visible(false);
        let model = self.model.borrow().clone();
        if model.wifi.available && model.wifi.enabled && !model.airplane.enabled {
            self.core.send(Command::WifiScan);
        } else {
            self.core.send(Command::WifiSetDetails(true));
        }
        self.rebuild(&model);
        self.dropdown.set_open(true);
    }

    pub fn render(self: &Rc<Self>, model: &NetworkModel) {
        *self.model.borrow_mut() = model.clone();

        self.updating.set(true);
        self.wifi_switch.set_active(model.wifi.enabled);
        self.wifi_switch
            .set_sensitive(model.wifi.available && !model.airplane.enabled);
        self.updating.set(false);

        if !self.dropdown.is_open() {
            return;
        }
        if self.password_target.borrow().is_some() {
            // Keep the entry's text and focus while a scan lands. The form is
            // removed by cancel or submit, which rebuilds the list.
            return;
        }
        self.rebuild(model);
    }

    pub fn show_message(&self, text: &str) {
        self.message.set_text(text);
        self.message.set_visible(!text.is_empty());
    }

    fn scan(&self) {
        let model = self.model.borrow();
        if model.wifi.available && model.wifi.enabled && !model.airplane.enabled {
            self.core.send(Command::WifiScan);
        }
    }

    fn rebuild(self: &Rc<Self>, model: &NetworkModel) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        if model.airplane.enabled {
            self.list.append(&empty_state(
                "Airplane mode is on. Wired connections stay available.",
            ));
        }

        section(&self.list, "Wired");
        if model.wired.is_empty() {
            self.list.append(&empty_state("No wired devices"));
        } else {
            for device in &model.wired {
                self.list.append(&self.wired_row(device));
            }
        }

        section(&self.list, "Wi-Fi");
        if !model.wifi.available {
            self.list.append(&empty_state("No Wi-Fi hardware"));
        } else if !model.wifi.enabled {
            self.list.append(&empty_state("Wi-Fi is off"));
        } else if model.wifi.access_points.is_empty() {
            self.list.append(&empty_state(if model.wifi.scanning {
                "Scanning for networks"
            } else {
                "No Wi-Fi networks available"
            }));
        } else {
            let target = self.password_target.borrow().clone();
            let mut inserted = false;
            for network in &model.wifi.access_points {
                self.list.append(&self.wifi_row(network));
                if !inserted {
                    if let Some((id, _)) = &target {
                        if network.id == *id && !network.active {
                            self.password.set_visible(true);
                            self.list.append(&self.password);
                            inserted = true;
                        }
                    }
                }
            }
        }
    }

    fn wired_row(self: &Rc<Self>, device: &WiredDeviceModel) -> gtk::Widget {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("NetworkItem");

        let name = gtk::Label::new(Some(device.title()));
        name.add_css_class("NetworkName");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let status = gtk::Label::new(Some(&wired_status(device)));
        status.add_css_class("NetworkStatus");
        status.set_xalign(1.0);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        if device.connected {
            status.add_css_class("Connected");
        }

        let toggle = gtk::Switch::new();
        toggle.add_css_class("NetworkSwitch");
        toggle.set_active(device.enabled);
        toggle.set_valign(gtk::Align::Center);
        toggle.set_tooltip_text(Some(if device.enabled {
            "Turn off this wired interface"
        } else {
            "Turn on this wired interface"
        }));

        {
            let core = Rc::clone(&self.core);
            let interface = device.interface.clone();
            toggle.connect_active_notify(move |switch| {
                core.send(Command::WiredSetEnabled {
                    interface: interface.clone(),
                    enabled: switch.is_active(),
                });
            });
        }

        row.append(&name);
        row.append(&status);
        row.append(&toggle);
        row.upcast()
    }

    fn wifi_row(self: &Rc<Self>, network: &AccessPointModel) -> gtk::Widget {
        let button = gtk::Button::new();
        button.add_css_class("NetworkItem");
        if network.active {
            button.add_css_class("is-active");
        }

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let name = gtk::Label::new(Some(network.display_name()));
        name.add_css_class("NetworkName");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let status = gtk::Label::new(Some(&wifi_status(network)));
        status.add_css_class("NetworkStatus");
        status.set_xalign(1.0);
        if network.active {
            status.add_css_class("Connected");
        }

        content.append(&name);
        content.append(&status);

        if network.active {
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.add_css_class("NetworkCheck");
            content.append(&check);
        } else if network.requires_password {
            let lock = gtk::Image::from_icon_name("changes-prevent-symbolic");
            lock.add_css_class("NetworkLock");
            lock.set_tooltip_text(Some(network.security.label()));
            content.append(&lock);
        }

        button.set_child(Some(&content));

        let panel = Rc::clone(self);
        let network = network.clone();
        button.connect_clicked(move |_| {
            if network.active {
                panel.core.send(Command::WifiDeactivate);
                return;
            }

            let Some(ssid) = network.ssid.clone() else {
                panel.show_message("Hidden networks need an existing connection profile.");
                return;
            };

            if network.has_saved_profile || !network.requires_password {
                panel.core.send(Command::WifiActivate {
                    ssid,
                    password: None,
                });
            } else if network.security.is_enterprise() {
                panel.show_message(
                    "Enterprise networks need an existing 802.1X profile. Create one in NetworkManager first.",
                );
            } else {
                panel.prompt_password(network.id.clone(), ssid);
            }
        });

        button.upcast()
    }

    fn prompt_password(self: &Rc<Self>, id: String, ssid: String) {
        *self.password_target.borrow_mut() = Some((id, ssid.clone()));
        self.password_label
            .set_text(&format!("Password for {ssid}"));
        self.password_entry.set_text("");
        self.password.set_visible(true);
        self.message.set_visible(false);
        let model = self.model.borrow().clone();
        self.rebuild(&model);
        self.password_entry.grab_focus();
    }

    fn clear_password(&self) {
        self.password_target.borrow_mut().take();
        self.password_entry.set_text("");
        self.password.set_visible(false);
    }

    fn cancel_password(self: &Rc<Self>) {
        if self.password_target.borrow().is_none() {
            return;
        }
        self.clear_password();
        let model = self.model.borrow().clone();
        self.rebuild(&model);
    }

    fn submit_password(self: &Rc<Self>) {
        let password = self.password_entry.text().to_string();
        if password.is_empty() {
            self.password_entry.grab_focus();
            return;
        }
        let Some((_, ssid)) = self.password_target.borrow_mut().take() else {
            return;
        };
        self.clear_password();
        self.core.send(Command::WifiActivate {
            ssid,
            password: Some(password),
        });
        let model = self.model.borrow().clone();
        self.rebuild(&model);
    }
}

/// The VPN page. It has no scan and no side effects on open.
pub struct VpnPanel {
    dropdown: Dropdown,
    list: gtk::Box,
    message: gtk::Label,
    confirm: gtk::Box,
    confirm_label: gtk::Label,
    confirm_uuid: RefCell<Option<String>>,
    core: Rc<Core>,
    model: RefCell<NetworkModel>,
}

impl VpnPanel {
    pub fn new(
        core: Rc<Core>,
        container: &gtk::Box,
        after: &impl IsA<gtk::Widget>,
        indicator: Option<&gtk::Button>,
    ) -> Rc<Self> {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("NetworkPanel");
        content.add_css_class("SliderDropdown");

        let title = gtk::Label::new(Some("VPN"));
        title.add_css_class("NetworkTitle");
        title.set_xalign(0.0);
        title.add_css_class("NetworkHeader");

        let list = gtk::Box::new(gtk::Orientation::Vertical, 1);
        list.add_css_class("NetworkList");

        let message = gtk::Label::new(None);
        message.add_css_class("NetworkMessage");
        message.set_xalign(0.0);
        message.set_wrap(true);
        message.set_visible(false);

        let confirm = gtk::Box::new(gtk::Orientation::Vertical, 6);
        confirm.add_css_class("NetworkPassword");
        confirm.add_css_class("NetworkConfirm");
        confirm.set_visible(false);

        let confirm_label = gtk::Label::new(None);
        confirm_label.add_css_class("NetworkName");
        confirm_label.set_xalign(0.0);
        confirm_label.set_wrap(true);
        confirm_label.set_max_width_chars(34);

        let confirm_detail = gtk::Label::new(Some(
            "NetworkManager did not start this tunnel, so it cannot start it again. You may need to start it from its own system service.",
        ));
        confirm_detail.add_css_class("NetworkConfirmDetail");
        confirm_detail.set_xalign(0.0);
        confirm_detail.set_wrap(true);
        confirm_detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        confirm_detail.set_max_width_chars(34);

        let confirm_actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        confirm_actions.set_halign(gtk::Align::End);
        let confirm_cancel = gtk::Button::with_label("Cancel");
        confirm_cancel.add_css_class("NetworkPasswordButton");
        let confirm_stop = gtk::Button::with_label("Stop Anyway");
        confirm_stop.add_css_class("NetworkPasswordButton");
        confirm_stop.add_css_class("destructive-action");
        confirm_actions.append(&confirm_cancel);
        confirm_actions.append(&confirm_stop);

        confirm.append(&confirm_label);
        confirm.append(&confirm_detail);
        confirm.append(&confirm_actions);

        content.append(&title);
        content.append(&list);
        content.append(&message);

        let dropdown = Dropdown::new(container, after, indicator);
        dropdown.set_child(&content);

        let panel = Rc::new(Self {
            dropdown,
            list,
            message,
            confirm,
            confirm_label,
            confirm_uuid: RefCell::new(None),
            core,
            model: RefCell::new(NetworkModel::default()),
        });

        {
            let panel = Rc::clone(&panel);
            confirm_cancel.connect_clicked(move |_| panel.cancel_confirm());
        }
        {
            let panel = Rc::clone(&panel);
            confirm_stop.connect_clicked(move |_| panel.submit_confirm());
        }

        panel
    }

    pub fn is_open(&self) -> bool {
        self.dropdown.is_open()
    }

    pub fn set_open(self: &Rc<Self>, open: bool) {
        if self.dropdown.is_open() == open {
            return;
        }
        if !open {
            self.clear_confirm();
            self.dropdown.set_open(false);
            return;
        }
        self.message.set_visible(false);
        let model = self.model.borrow().clone();
        self.rebuild(&model);
        self.dropdown.set_open(true);
    }

    pub fn render(self: &Rc<Self>, model: &NetworkModel) {
        *self.model.borrow_mut() = model.clone();
        if !self.dropdown.is_open() {
            return;
        }
        if self.confirm_uuid.borrow().is_some() {
            // Keep the confirmation open while state changes arrive.
            return;
        }
        self.rebuild(model);
    }

    pub fn show_message(&self, text: &str) {
        self.message.set_text(text);
        self.message.set_visible(!text.is_empty());
    }

    fn rebuild(self: &Rc<Self>, model: &NetworkModel) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        section(&self.list, "Connections");
        if model.vpn.profiles.is_empty() {
            self.list.append(&empty_state("No VPN profiles configured"));
        } else {
            let confirm_uuid = self.confirm_uuid.borrow().clone();
            for profile in &model.vpn.profiles {
                self.list.append(&self.vpn_row(profile));
                if confirm_uuid.as_deref() == Some(profile.uuid.as_str())
                    && profile.active
                    && profile.external
                {
                    self.confirm.set_visible(true);
                    self.list.append(&self.confirm);
                }
            }
        }
    }

    fn vpn_row(self: &Rc<Self>, profile: &VpnProfileModel) -> gtk::Widget {
        let button = gtk::Button::new();
        button.add_css_class("NetworkItem");
        if profile.active {
            button.add_css_class("is-active");
        }

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let name = gtk::Label::new(Some(&profile.name));
        name.add_css_class("NetworkName");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let status = gtk::Label::new(Some(profile.status()));
        status.add_css_class("NetworkStatus");
        status.set_xalign(1.0);
        if profile.active {
            status.add_css_class("Connected");
        } else if profile.activation_failed.is_some() {
            status.add_css_class("Failed");
        }

        content.append(&name);
        content.append(&status);
        if profile.active {
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.add_css_class("NetworkCheck");
            content.append(&check);
        }
        button.set_child(Some(&content));

        let core = Rc::clone(&self.core);
        let panel = Rc::clone(self);
        let dialog_name = profile.name.clone();
        let uuid = profile.uuid.clone();
        let active = profile.active;
        let external = profile.external;
        if external && active {
            button.set_tooltip_text(Some(
                "This tunnel was started outside NetworkManager. Stopping it may require its system service.",
            ));
        }
        button.connect_clicked(move |_| {
            if active && external {
                panel.prompt_confirm(uuid.clone(), &dialog_name);
                return;
            }
            core.send(if active {
                Command::VpnDeactivate(uuid.clone())
            } else {
                Command::VpnActivate(uuid.clone())
            });
        });

        button.upcast()
    }

    fn prompt_confirm(self: &Rc<Self>, uuid: String, name: &str) {
        *self.confirm_uuid.borrow_mut() = Some(uuid);
        self.confirm_label.set_text(&format!("Stop {name}?"));
        self.confirm.set_visible(true);
        self.message.set_visible(false);
        let model = self.model.borrow().clone();
        self.rebuild(&model);
    }

    fn clear_confirm(&self) {
        self.confirm_uuid.borrow_mut().take();
        self.confirm.set_visible(false);
    }

    fn cancel_confirm(self: &Rc<Self>) {
        if self.confirm_uuid.borrow().is_none() {
            return;
        }
        self.clear_confirm();
        let model = self.model.borrow().clone();
        self.rebuild(&model);
    }

    fn submit_confirm(self: &Rc<Self>) {
        let Some(uuid) = self.confirm_uuid.borrow_mut().take() else {
            return;
        };
        self.clear_confirm();
        self.core.send(Command::VpnDeactivate(uuid));
        let model = self.model.borrow().clone();
        self.rebuild(&model);
    }
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("NetworkIconButton");
    button.set_child(Some(&gtk::Image::from_icon_name(icon)));
    button.set_tooltip_text(Some(tooltip));
    button
}

fn section(list: &gtk::Box, title: &str) {
    let label = gtk::Label::new(Some(title));
    label.add_css_class("NetworkSection");
    label.set_xalign(0.0);
    list.append(&label);
}

fn empty_state(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("NetworkEmpty");
    label.set_xalign(0.0);
    label.set_wrap(true);
    label
}

fn wifi_status(network: &AccessPointModel) -> String {
    if network.active {
        "Connected".to_string()
    } else if network.access_point_count > 1 {
        format!("{}% · {} APs", network.strength, network.access_point_count)
    } else {
        format!("{}%", network.strength)
    }
}

fn wired_status(device: &WiredDeviceModel) -> String {
    let mut status = device.status().to_string();
    if device.connected {
        if let Some(address) = &device.ip4_address {
            status.push_str(" · ");
            status.push_str(address);
        }
        if device.speed > 0 {
            status.push_str(&format!(" · {} Mb/s", device.speed));
        }
    } else if device.enabled && !device.carrier {
        status = "No cable".to_string();
    }
    status
}
