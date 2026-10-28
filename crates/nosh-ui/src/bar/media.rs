use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::MediaModel;

/// The media button in the centre island and the divider before it.
///
/// The section hides when no player has a track, matching the idle centre
/// island in the prototype. The centre panel measures the button when the
/// island grows into the media tab.
pub struct Media {
    section: gtk::Box,
    button: gtk::Button,
    label: gtk::Label,
}

impl Media {
    pub fn new() -> Self {
        let button = gtk::Button::new();
        button.add_css_class("MediaButton");
        button.set_focus_on_click(false);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        row.add_css_class("MediaRow");
        let icon = gtk::Image::from_icon_name("audio-x-generic-symbolic");
        icon.add_css_class("MediaIcon");
        let label = gtk::Label::new(None);
        label.add_css_class("MediaLabel");
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        label.set_lines(1);
        // Room for three or four words before the middle ellipsis kicks in.
        label.set_max_width_chars(18);
        row.append(&icon);
        row.append(&label);
        button.set_child(Some(&row));

        let divider = gtk::Box::new(gtk::Orientation::Vertical, 0);
        divider.add_css_class("MediaDivider");
        divider.set_size_request(1, 1);
        divider.set_valign(gtk::Align::Center);

        let section = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        section.add_css_class("MediaSection");
        section.append(&divider);
        section.append(&button);
        section.set_visible(false);

        Self {
            section,
            button,
            label,
        }
    }

    /// The divider and the button. The bar places this as one centre island
    /// item so the divider disappears with the player.
    pub fn section(&self) -> &gtk::Box {
        &self.section
    }

    pub fn button(&self) -> &gtk::Button {
        &self.button
    }

    /// Shows the track or hides the section when nothing is playing.
    pub fn render(&self, model: &MediaModel) {
        if !model.is_active() {
            self.section.set_visible(false);
            self.label.set_text("");
            return;
        }

        self.section.set_visible(true);
        self.label.set_text(&model.title);
        self.button
            .set_tooltip_text(Some(&format!("{} · {}", model.title, model.subtitle())));
    }
}
