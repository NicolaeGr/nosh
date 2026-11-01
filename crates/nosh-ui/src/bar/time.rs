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
        arm(label.downgrade());

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

/// Waits until the next minute boundary instead of ticking every second. The
/// label only shows minutes, and a timer that fires while the shell is idle is
/// the main reason an otherwise quiet desktop logs wakeups.
fn arm(weak: glib::WeakRef<gtk::Label>) {
    let seconds = glib::DateTime::now_local()
        .map(|now| now.second().clamp(0, 59))
        .unwrap_or(0);
    glib::timeout_add_seconds_local_once((60 - seconds) as u32, move || {
        let Some(label) = weak.upgrade() else {
            return;
        };
        update(&label);
        arm(label.downgrade());
    });
}

fn update(label: &gtk::Label) {
    let Ok(now) = glib::DateTime::now_local() else {
        return;
    };

    if let Ok(time) = now.format(TIME_FORMAT) {
        if label.text().as_str() != time.as_str() {
            label.set_label(&time);
        }
    }
    if let Ok(date) = now.format(DATE_FORMAT) {
        if label.tooltip_text().as_deref() != Some(date.as_str()) {
            label.set_tooltip_text(Some(&date));
        }
    }
}
