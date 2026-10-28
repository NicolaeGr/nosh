//! The quick settings header: battery state and the session entry point.

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::BatteryModel;

pub struct Header {
    root: gtk::Box,
    battery: gtk::Box,
    icon: gtk::Image,
    percent: gtk::Label,
    time: gtk::Label,
}

impl Header {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 9);
        root.add_css_class("QuickSettingsHeader");
        root.set_margin_bottom(10);

        let battery = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        battery.add_css_class("QuickSettingsBattery");
        let icon = gtk::Image::new();
        let percent = gtk::Label::new(None);
        percent.add_css_class("Percent");
        let time = gtk::Label::new(None);
        time.add_css_class("Remaining");
        battery.append(&icon);
        battery.append(&percent);
        battery.append(&time);

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);

        let session = gtk::Button::new();
        session.add_css_class("QuickSettingsSession");
        session.set_sensitive(false);
        session.set_tooltip_text(Some("Session actions are not available yet"));
        let session_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        session_box.append(&gtk::Image::from_icon_name("avatar-default-symbolic"));
        let name = user_name();
        let label = gtk::Label::new(Some(&name));
        label.add_css_class("Name");
        session_box.append(&label);
        session.set_child(Some(&session_box));

        root.append(&battery);
        root.append(&spacer);
        root.append(&session);

        let header = Self {
            root,
            battery,
            icon,
            percent,
            time,
        };
        header.render(&BatteryModel::default());
        header
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn render(&self, battery: &BatteryModel) {
        let visible = battery.present && battery.is_battery;
        self.battery.set_visible(visible);
        if !visible {
            return;
        }

        self.icon.set_icon_name(Some(&battery.icon_name));
        self.percent.set_text(&percent_text(battery.percentage));
        self.time.set_text(&battery.status_text());
        self.battery.set_tooltip_text(Some(&battery.tooltip()));
    }
}

fn user_name() -> String {
    let name = glib::user_name().to_string_lossy().into_owned();
    if name.is_empty() {
        "Session".to_string()
    } else {
        name
    }
}

fn percent_text(percentage: f64) -> String {
    format!("{}%", (percentage.clamp(0.0, 1.0) * 100.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_are_whole_numbers() {
        assert_eq!(percent_text(0.0), "0%");
        assert_eq!(percent_text(0.864), "86%");
        assert_eq!(percent_text(1.0), "100%");
        assert_eq!(percent_text(2.0), "100%");
    }
}
