use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{bar_step_volume, AudioModel, Command, Core, VOLUME_STEP};

struct State {
    volume: Cell<f64>,
    available: Cell<bool>,
}

pub struct Volume {
    button: gtk::Button,
    icon: gtk::Image,
    state: Rc<State>,
}

impl Volume {
    pub fn new(core: Rc<Core>) -> Self {
        let button = gtk::Button::new();
        button.add_css_class("square-icon");
        button.set_focus_on_click(false);

        let icon = gtk::Image::new();
        icon.set_pixel_size(16);
        button.set_child(Some(&icon));

        let state = Rc::new(State {
            volume: Cell::new(0.0),
            available: Cell::new(false),
        });

        let controller = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let scroll_state = Rc::clone(&state);
        controller.connect_scroll(move |_, _, dy| {
            if !scroll_state.available.get() {
                return gtk::glib::Propagation::Proceed;
            }

            let delta = if dy < 0.0 {
                VOLUME_STEP
            } else if dy > 0.0 {
                -VOLUME_STEP
            } else {
                return gtk::glib::Propagation::Proceed;
            };

            if bar_step_volume(scroll_state.volume.get(), delta).is_some() {
                core.send(Command::AudioStepVolume(delta));
            }

            gtk::glib::Propagation::Stop
        });
        button.add_controller(controller);

        let widget = Self {
            button,
            icon,
            state,
        };
        widget.render(&AudioModel::default());
        widget
    }

    pub fn widget(&self) -> &gtk::Button {
        &self.button
    }

    pub fn render(&self, model: &AudioModel) {
        let sink = model.default_sink.as_ref();
        self.state.available.set(sink.is_some());
        self.button.set_visible(sink.is_some());

        let Some(sink) = sink else {
            return;
        };

        self.state.volume.set(sink.volume);
        self.icon.set_icon_name(Some(&sink.icon_name));
        self.button
            .set_tooltip_text(Some(&format!("Volume: {}%", percent(sink.volume))));
    }
}

fn percent(volume: f64) -> i64 {
    (volume * 100.0).floor() as i64
}

#[cfg(test)]
mod tests {
    use super::percent;

    #[test]
    fn the_tooltip_floors_the_volume() {
        assert_eq!(percent(0.0), 0);
        assert_eq!(percent(0.999), 99);
        assert_eq!(percent(1.0), 100);
        assert_eq!(percent(1.25), 125);
    }
}
