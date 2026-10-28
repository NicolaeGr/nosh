use gtk::prelude::*;
use gtk4 as gtk;

/// Static now-playing text. The player is hardcoded until the core exposes an
/// MPRIS model.
pub const TITLE: &str = "Midnight City";
pub const ARTIST: &str = "M83";

/// The media button in the centre island. The centre panel measures the button
/// when the island grows into the media tab.
pub struct Media {
    button: gtk::Button,
}

impl Media {
    pub fn new() -> Self {
        let button = gtk::Button::new();
        button.add_css_class("MediaButton");
        button.set_focus_on_click(false);
        button.set_tooltip_text(Some(&format!("{TITLE} · {ARTIST}")));

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        row.add_css_class("MediaRow");
        let icon = gtk::Image::from_icon_name("audio-x-generic-symbolic");
        icon.add_css_class("MediaIcon");
        let label = gtk::Label::new(Some(TITLE));
        label.add_css_class("MediaLabel");
        row.append(&icon);
        row.append(&label);
        button.set_child(Some(&row));

        Self { button }
    }

    pub fn button(&self) -> &gtk::Button {
        &self.button
    }
}
