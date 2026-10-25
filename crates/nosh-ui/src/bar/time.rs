use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

const TIME_FORMAT: &str = "%H:%M";
const DATE_FORMAT: &str = "%A, %B %e";

/// Builds the clock. The box fills its parent so the text stays centered when
/// the center island is wider than the clock.
pub fn build() -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class("Time");
    root.set_hexpand(true);

    let label = gtk::Label::new(None);
    label.add_css_class("TimeLabel");
    label.set_hexpand(true);
    root.append(&label);
    update(&label);

    let weak_label = label.downgrade();
    glib::timeout_add_seconds_local(1, move || match weak_label.upgrade() {
        Some(label) => {
            update(&label);
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });

    root
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
