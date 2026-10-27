//! A passive overlay for volume, mute, and brightness changes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nosh_core::{volume_icon, EndpointKind};

const HIDE_DELAY: Duration = Duration::from_millis(800);

pub struct ChangeIndicator {
    window: gtk::ApplicationWindow,
    icon: gtk::Image,
    value: gtk::Label,
    bar: gtk::ProgressBar,
    marker: Rc<Cell<Option<f64>>>,
    marker_area: gtk::DrawingArea,
    timer: Rc<RefCell<Option<glib::SourceId>>>,
    generation: Rc<Cell<u64>>,
}

impl ChangeIndicator {
    pub fn new(app: &gtk::Application) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.set_decorated(false);
        window.init_layer_shell();
        window.set_namespace(Some("nosh-change-indicator"));
        window.set_layer(Layer::Top);
        window.set_anchor(Edge::Bottom, true);
        window.set_keyboard_mode(KeyboardMode::None);
        window.add_css_class("ChangeIndicator");
        window.set_visible(false);

        let wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        wrap.add_css_class("IndicatorWrap");
        wrap.set_halign(gtk::Align::Center);
        wrap.set_valign(gtk::Align::End);

        let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        card.add_css_class("IndicatorCard");

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.add_css_class("IndicatorRow");

        let icon = gtk::Image::new();
        icon.add_css_class("IndicatorIcon");

        let value = gtk::Label::new(None);
        value.add_css_class("IndicatorValue");
        value.set_xalign(0.0);
        value.set_hexpand(true);

        row.append(&icon);
        row.append(&value);
        card.append(&row);

        let bar = gtk::ProgressBar::new();
        bar.add_css_class("IndicatorBar");
        bar.set_show_text(false);
        bar.set_hexpand(true);

        let marker: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
        let marker_area = gtk::DrawingArea::new();
        marker_area.add_css_class("IndicatorMarker");
        marker_area.set_can_target(false);
        marker_area.set_hexpand(true);
        marker_area.set_vexpand(true);
        {
            let marker = Rc::clone(&marker);
            marker_area.set_draw_func(
                move |area: &gtk::DrawingArea,
                      cr: &gtk::cairo::Context,
                      width: i32,
                      height: i32| {
                    let Some(fraction) = marker.get() else {
                        return;
                    };
                    let color = area.color();
                    let x = (width as f64 * fraction).round() + 0.5;
                    cr.set_source_rgba(
                        f64::from(color.red()),
                        f64::from(color.green()),
                        f64::from(color.blue()),
                        0.7,
                    );
                    cr.set_line_width(1.0);
                    cr.move_to(x, 0.0);
                    cr.line_to(x, height as f64);
                    let _ = cr.stroke();
                },
            );
        }

        let bar_layer = gtk::Overlay::new();
        bar_layer.set_child(Some(&bar));
        bar_layer.add_overlay(&marker_area);
        card.append(&bar_layer);

        wrap.append(&card);
        window.set_child(Some(&wrap));

        Self {
            window,
            icon,
            value,
            bar,
            marker,
            marker_area,
            timer: Rc::new(RefCell::new(None)),
            generation: Rc::new(Cell::new(0)),
        }
    }

    pub fn show_audio(
        &self,
        monitor: Option<&gdk::Monitor>,
        kind: EndpointKind,
        volume: f64,
        muted: bool,
    ) {
        let (icon, label) = match (kind, muted) {
            (EndpointKind::Sink, true) => ("audio-volume-muted-symbolic", "Muted".to_string()),
            (EndpointKind::Sink, false) => (
                volume_icon(kind, volume, false),
                format_percent(volume * 100.0),
            ),
            (EndpointKind::Source, true) => (
                "microphone-sensitivity-muted-symbolic",
                "Mic muted".to_string(),
            ),
            (EndpointKind::Source, false) => (
                "audio-input-microphone-symbolic",
                format_percent(volume * 100.0),
            ),
        };
        let fraction = if muted {
            0.0
        } else {
            (volume / kind.max_volume()).clamp(0.0, 1.0)
        };
        let overamp = kind == EndpointKind::Sink && !muted && (volume * 100.0).round() >= 101.0;
        let marker = (kind == EndpointKind::Sink).then(|| 1.0 / kind.max_volume());

        self.show(monitor, icon, &label, fraction, overamp, marker);
    }

    pub fn show_brightness(&self, monitor: Option<&gdk::Monitor>, percent: f64) {
        let percent = percent.clamp(0.0, 100.0);
        self.show(
            monitor,
            "display-brightness-symbolic",
            &format_percent(percent),
            percent / 100.0,
            false,
            None,
        );
    }

    fn show(
        &self,
        monitor: Option<&gdk::Monitor>,
        icon: &str,
        label: &str,
        fraction: f64,
        overamp: bool,
        marker: Option<f64>,
    ) {
        self.icon.set_icon_name(Some(icon));
        self.value.set_text(label);
        self.bar.set_fraction(fraction);
        self.marker.set(marker);
        self.marker_area.queue_draw();
        if overamp {
            self.bar.add_css_class("overamp");
        } else {
            self.bar.remove_css_class("overamp");
        }

        if let Some(monitor) = monitor {
            self.window.set_monitor(Some(monitor));
        }
        self.window.present();

        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.remove();
        }

        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);

        let window = self.window.clone();
        let slot = Rc::clone(&self.timer);
        let current = Rc::clone(&self.generation);
        let timer = glib::timeout_add_local_once(HIDE_DELAY, move || {
            if current.get() == generation {
                window.set_visible(false);
                slot.borrow_mut().take();
            }
        });
        *self.timer.borrow_mut() = Some(timer);
    }
}

fn format_percent(value: f64) -> String {
    format!("{}%", value.round() as i64)
}

#[cfg(test)]
mod tests {
    use super::format_percent;

    #[test]
    fn percentages_are_whole_numbers() {
        assert_eq!(format_percent(42.4), "42%");
        assert_eq!(format_percent(42.6), "43%");
        assert_eq!(format_percent(125.0), "125%");
    }
}
