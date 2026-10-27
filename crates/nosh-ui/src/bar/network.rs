//! The bar network icon: wired when connected, otherwise Wi-Fi.

use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::NetworkModel;

pub struct Network {
    button: gtk::Button,
    icon: gtk::Image,
}

impl Network {
    pub fn new() -> Self {
        let button = gtk::Button::new();
        button.add_css_class("square-icon");
        button.set_focus_on_click(false);
        button.set_visible(false);

        let icon = gtk::Image::new();
        button.set_child(Some(&icon));

        Self { button, icon }
    }

    pub fn widget(&self) -> &gtk::Button {
        &self.button
    }

    pub fn render(&self, model: &NetworkModel) {
        let visible = model.wifi.available
            || !model.wired.is_empty()
            || model.vpn.available
            || model.airplane.available;
        self.button.set_visible(visible);
        if !visible {
            return;
        }

        let icon = if model.airplane.enabled {
            model.airplane.icon_name()
        } else if let Some(wired) = model.wired.iter().find(|device| device.connected) {
            wired.icon_name()
        } else if model.wifi.available {
            model.wifi.icon_name.as_str()
        } else if let Some(wired) = model.wired.first() {
            wired.icon_name()
        } else if model.vpn.active_name().is_some() {
            "network-vpn-symbolic"
        } else {
            "network-wireless-offline-symbolic"
        };

        self.icon.set_icon_name(Some(icon));
        let tooltip = match model.vpn.active_name() {
            Some(vpn) => format!("{} · VPN: {vpn}", model.summary()),
            None => model.summary().to_string(),
        };
        self.button.set_tooltip_text(Some(&tooltip));
    }
}
