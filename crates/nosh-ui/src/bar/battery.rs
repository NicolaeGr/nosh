use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::BatteryModel;

use crate::scale;

pub struct Battery {
    button: gtk::Button,
    icon: gtk::Image,
}

impl Battery {
    pub fn new() -> Self {
        let button = gtk::Button::new();
        button.add_css_class("square-icon");
        button.set_focus_on_click(false);

        let icon = gtk::Image::new();
        icon.set_pixel_size(scale::icon_px());
        button.set_child(Some(&icon));

        let widget = Self { button, icon };
        widget.render(&BatteryModel::default());
        widget
    }

    pub fn widget(&self) -> &gtk::Button {
        &self.button
    }

    pub fn render(&self, model: &BatteryModel) {
        let visible = model.present && model.is_battery;
        self.button.set_visible(visible);
        if !visible {
            return;
        }

        self.icon.set_icon_name(Some(&model.icon_name));
        self.button.set_tooltip_text(Some(&model.tooltip()));
    }
}

impl Default for Battery {
    fn default() -> Self {
        Self::new()
    }
}
