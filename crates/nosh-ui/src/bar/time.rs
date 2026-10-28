use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

const TIME_FORMAT: &str = "%H:%M";
const DATE_FORMAT: &str = "%A, %B %e";

/// The clock button and its label. The centre panel measures the label when
/// the island grows into the clock tab.
pub struct Clock {
    button: gtk::Button,
    label: gtk::Label,
}

impl Clock {
    pub fn new() -> Self {
        let button = gtk::Button::new();
        button.add_css_class("Time");
        button.add_css_class("TimeButton");
        button.set_focus_on_click(false);

        let label = gtk::Label::new(None);
        label.add_css_class("TimeLabel");
        label.set_hexpand(true);
        button.set_child(Some(&label));
        update(&label);

        let weak_label = label.downgrade();
        glib::timeout_add_seconds_local(1, move || match weak_label.upgrade() {
            Some(label) => {
                update(&label);
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });

        Self { button, label }
    }

    pub fn button(&self) -> &gtk::Button {
        &self.button
    }

    pub fn label(&self) -> &gtk::Label {
        &self.label
    }
}

/// The current time in the bar format, used as the collapsed clock text.
pub fn now_text() -> String {
    glib::DateTime::now_local()
        .and_then(|now| now.format(TIME_FORMAT))
        .map(|text| text.to_string())
        .unwrap_or_default()
}

fn update(label: &gtk::Label) {
    let Ok(now) = glib::DateTime::now_local() else {
        return;
    };

    if let Ok(time) = now.format(TIME_FORMAT) {
        label.set_label(&time);
    }
    if let Ok(date) = now.format(DATE_FORMAT) {
        label.set_tooltip_text(Some(&date));
    }
}
