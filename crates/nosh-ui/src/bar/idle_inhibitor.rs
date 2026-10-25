use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{Command, Core, IdleInhibitState};

const ACTIVE_ICON: &str = "my-caffeine-on-symbolic";
const INACTIVE_ICON: &str = "my-caffeine-off-symbolic";
const ERROR_ICON: &str = "dialog-error";

pub struct IdleInhibitor {
    button: gtk::Button,
    icon: gtk::Image,
}

impl IdleInhibitor {
    pub fn new(core: Rc<Core>) -> Self {
        let button = gtk::Button::new();
        button.add_css_class("square-icon");
        button.set_focus_on_click(false);

        let icon = gtk::Image::new();
        button.set_child(Some(&icon));

        let widget = Self { button, icon };
        widget.render(&IdleInhibitState::default());

        let core = Rc::clone(&core);
        widget.button.connect_clicked(move |_| {
            core.send(Command::IdleInhibitToggle);
        });

        widget
    }

    pub fn widget(&self) -> &gtk::Button {
        &self.button
    }

    pub fn render(&self, state: &IdleInhibitState) {
        self.button.set_sensitive(state.available);

        let (icon, tooltip) = if !state.available {
            (ERROR_ICON, "Error: Idle inhibitor not available")
        } else if state.active {
            (ACTIVE_ICON, "Active")
        } else {
            (INACTIVE_ICON, "Inactive")
        };

        self.icon.set_icon_name(Some(icon));
        self.button.set_tooltip_text(Some(tooltip));
    }
}
