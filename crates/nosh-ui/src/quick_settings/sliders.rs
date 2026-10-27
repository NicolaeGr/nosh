//! Brightness, output, and input sliders with one open device list.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{
    volume_icon, AudioEndpoint, AudioModel, BrightnessDevice, Command, Core, EndpointKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dropdown {
    None,
    Audio,
    Microphone,
}

pub struct Sliders {
    root: gtk::Box,
    brightness: BrightnessRow,
    audio: SliderBlock,
    microphone: SliderBlock,
    open: Cell<Dropdown>,
}

impl Sliders {
    pub fn new(core: Rc<Core>) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.add_css_class("QuickSettingsSliders");
        root.set_margin_bottom(10);

        let brightness = BrightnessRow::new(Rc::clone(&core));
        let audio = SliderBlock::new(
            EndpointKind::Sink,
            "Audio",
            "audio-volume-high-symbolic",
            Rc::clone(&core),
        );
        let microphone = SliderBlock::new(
            EndpointKind::Source,
            "Microphone",
            "audio-input-microphone-symbolic",
            core,
        );

        root.append(brightness.widget());
        root.append(audio.widget());
        root.append(microphone.widget());

        let sliders = Rc::new(Self {
            root,
            brightness,
            audio,
            microphone,
            open: Cell::new(Dropdown::None),
        });

        let weak = Rc::downgrade(&sliders);
        sliders.audio.connect_expand(move || {
            if let Some(sliders) = weak.upgrade() {
                sliders.toggle(Dropdown::Audio);
            }
        });
        let weak = Rc::downgrade(&sliders);
        sliders.microphone.connect_expand(move || {
            if let Some(sliders) = weak.upgrade() {
                sliders.toggle(Dropdown::Microphone);
            }
        });

        sliders
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_audio(&self, model: &AudioModel) {
        self.audio.render(model);
        self.microphone.render(model);
        self.close_empty_dropdown(EndpointKind::Sink, Dropdown::Audio, model);
        self.close_empty_dropdown(EndpointKind::Source, Dropdown::Microphone, model);
    }

    fn close_empty_dropdown(&self, kind: EndpointKind, target: Dropdown, model: &AudioModel) {
        if self.open.get() == target && model.endpoints(kind).is_empty() {
            self.set_open(Dropdown::None);
        }
    }

    pub fn set_brightness(&self, devices: &[BrightnessDevice]) {
        self.brightness.render(devices);
    }

    pub fn close_dropdowns(&self) {
        self.set_open(Dropdown::None);
    }

    fn toggle(&self, target: Dropdown) {
        let next = if self.open.get() == target {
            Dropdown::None
        } else {
            target
        };
        self.set_open(next);
    }

    fn set_open(&self, target: Dropdown) {
        self.open.set(target);
        self.audio.set_open(target == Dropdown::Audio);
        self.microphone.set_open(target == Dropdown::Microphone);
    }
}

struct BrightnessRow {
    root: gtk::Box,
    slider: gtk::Scale,
    value: gtk::Label,
    updating: Rc<Cell<bool>>,
}

impl BrightnessRow {
    fn new(core: Rc<Core>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 9);
        root.add_css_class("SliderSimple");

        let icon = gtk::Image::from_icon_name("display-brightness-symbolic");
        icon.add_css_class("SliderIcon");

        let slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        slider.add_css_class("QuickSlider");
        slider.set_draw_value(false);
        slider.set_hexpand(true);

        let value = gtk::Label::new(Some("0%"));
        value.add_css_class("SliderValue");
        value.set_width_chars(4);
        value.set_xalign(1.0);

        root.append(&icon);
        root.append(&slider);
        root.append(&value);

        let updating = Rc::new(Cell::new(false));
        {
            let updating = Rc::clone(&updating);
            slider.connect_value_changed(move |scale| {
                if updating.get() {
                    return;
                }
                core.send(Command::BrightnessSet(scale.value()));
            });
        }

        Self {
            root,
            slider,
            value,
            updating,
        }
    }

    fn widget(&self) -> &gtk::Box {
        &self.root
    }

    fn render(&self, devices: &[BrightnessDevice]) {
        let Some(device) = master_device(devices) else {
            self.root.set_visible(false);
            return;
        };

        self.root.set_visible(true);
        self.updating.set(true);
        let percent = device.pending_percent.unwrap_or(device.percent);
        self.slider.set_value(percent.round());
        self.value.set_text(&percent_text(percent));
        self.slider.set_sensitive(device.available);
        self.updating.set(false);
    }
}

struct SliderBlock {
    kind: EndpointKind,
    core: Rc<Core>,
    root: gtk::Box,
    icon: gtk::Image,
    mute: gtk::Button,
    slider: gtk::Scale,
    value: gtk::Label,
    expand: gtk::Button,
    revealer: gtk::Revealer,
    list: gtk::Box,
    rows: RefCell<Vec<EndpointRow>>,
    row_ids: RefCell<Vec<u32>>,
    updating: Rc<Cell<bool>>,
}

impl SliderBlock {
    fn new(kind: EndpointKind, name: &str, icon: &str, core: Rc<Core>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("SliderBlock");

        let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        head.add_css_class("SliderBlockHead");
        head.set_margin_bottom(4);
        let label = gtk::Label::new(Some(name));
        label.add_css_class("SliderBlockName");
        label.set_xalign(0.0);
        label.set_hexpand(true);
        let expand = gtk::Button::new();
        expand.add_css_class("SliderBlockExpand");
        expand.set_child(Some(&gtk::Image::from_icon_name("pan-down-symbolic")));
        expand.set_tooltip_text(Some(&format!("Show other {name} devices")));
        head.append(&label);
        head.append(&expand);

        let body = gtk::Box::new(gtk::Orientation::Horizontal, 9);
        body.add_css_class("SliderBlockBody");
        let icon = gtk::Image::from_icon_name(icon);
        icon.set_halign(gtk::Align::Center);
        icon.set_valign(gtk::Align::Center);
        let mute = gtk::Button::new();
        mute.add_css_class("SliderMute");
        mute.set_valign(gtk::Align::Center);
        mute.set_child(Some(&icon));
        mute.set_tooltip_text(Some("Mute"));
        let slider = gtk::Scale::with_range(
            gtk::Orientation::Horizontal,
            0.0,
            (kind.max_volume() * 100.0).round(),
            1.0,
        );
        slider.add_css_class("QuickSlider");
        slider.set_draw_value(false);
        slider.set_hexpand(true);
        let value = gtk::Label::new(Some("0%"));
        value.add_css_class("SliderValue");
        value.set_width_chars(4);
        value.set_xalign(1.0);
        body.append(&mute);
        body.append(&slider);
        body.append(&value);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 9);
        list.add_css_class("SliderDropdown");
        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        revealer.set_transition_duration(300);
        revealer.set_reveal_child(false);
        revealer.set_margin_top(6);
        revealer.set_child(Some(&list));

        root.append(&head);
        root.append(&body);
        root.append(&revealer);

        let updating = Rc::new(Cell::new(false));
        {
            let core = Rc::clone(&core);
            mute.connect_clicked(move |_| core.send(Command::AudioToggleMute(kind)));
        }
        {
            let updating = Rc::clone(&updating);
            let core = Rc::clone(&core);
            slider.connect_value_changed(move |scale| {
                if updating.get() {
                    return;
                }
                let volume = scale.value() / 100.0;
                let command = match kind {
                    EndpointKind::Sink => Command::AudioSetVolume(volume),
                    EndpointKind::Source => Command::AudioSetSourceVolume(volume),
                };
                core.send(command);
            });
        }

        Self {
            kind,
            core,
            root,
            icon,
            mute,
            slider,
            value,
            expand,
            revealer,
            list,
            rows: RefCell::new(Vec::new()),
            row_ids: RefCell::new(Vec::new()),
            updating,
        }
    }

    fn widget(&self) -> &gtk::Box {
        &self.root
    }

    fn connect_expand(&self, callback: impl Fn() + 'static) {
        self.expand.connect_clicked(move |_| callback());
    }

    fn set_open(&self, open: bool) {
        self.revealer.set_reveal_child(open);
        if open {
            self.expand.add_css_class("is-open");
        } else {
            self.expand.remove_css_class("is-open");
        }
    }

    fn render(&self, model: &AudioModel) {
        self.updating.set(true);
        match model.default_for(self.kind) {
            Some(endpoint) => {
                self.slider.set_sensitive(true);
                self.mute.set_sensitive(true);
                self.mute
                    .set_tooltip_text(Some(if endpoint.muted { "Unmute" } else { "Mute" }));
                self.slider.set_value((endpoint.volume * 100.0).round());
                self.value.set_text(&percent_text(endpoint.volume * 100.0));
                self.icon.set_icon_name(Some(volume_icon(
                    self.kind,
                    endpoint.volume,
                    endpoint.muted,
                )));
                set_overamp(&self.slider, self.kind, endpoint.volume);
            }
            None => {
                self.slider.set_sensitive(false);
                self.mute.set_sensitive(false);
                self.mute.set_tooltip_text(Some("Mute"));
                self.slider.set_value(0.0);
                self.value.set_text("—");
                self.icon.set_icon_name(Some(match self.kind {
                    EndpointKind::Sink => "audio-volume-muted-symbolic",
                    EndpointKind::Source => "microphone-sensitivity-muted-symbolic",
                }));
                self.slider.remove_css_class("overamp");
            }
        }
        self.updating.set(false);

        let endpoints = model.endpoints(self.kind);
        let ids: Vec<u32> = endpoints.iter().map(|endpoint| endpoint.id).collect();
        if *self.row_ids.borrow() != ids {
            while let Some(child) = self.list.first_child() {
                self.list.remove(&child);
            }
            let rows: Vec<EndpointRow> = endpoints
                .iter()
                .map(|endpoint| {
                    EndpointRow::new(
                        self.kind,
                        endpoint,
                        Rc::clone(&self.core),
                        self.list.clone(),
                    )
                })
                .collect();
            *self.rows.borrow_mut() = rows;
            *self.row_ids.borrow_mut() = ids;
        }

        for (row, endpoint) in self.rows.borrow().iter().zip(endpoints) {
            row.render(endpoint);
        }
        self.expand.set_sensitive(!endpoints.is_empty());
    }
}

struct EndpointRow {
    kind: EndpointKind,
    root: gtk::Box,
    icon: gtk::Image,
    mute: gtk::Button,
    muted: Rc<Cell<bool>>,
    slider: gtk::Scale,
    value: gtk::Label,
    default: gtk::Button,
    updating: Rc<Cell<bool>>,
}

impl EndpointRow {
    fn new(kind: EndpointKind, endpoint: &AudioEndpoint, core: Rc<Core>, parent: gtk::Box) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 3);
        root.add_css_class("EndpointRow");

        let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        head.add_css_class("EndpointHead");
        let name = gtk::Label::new(Some(&endpoint_name(endpoint)));
        name.add_css_class("EndpointName");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let default = gtk::Button::new();
        default.add_css_class("EndpointDefault");
        default.set_child(Some(&gtk::Image::from_icon_name("object-select-symbolic")));
        default.set_tooltip_text(Some("Set as default"));
        head.append(&name);
        head.append(&default);

        let body = gtk::Box::new(gtk::Orientation::Horizontal, 9);
        let icon = gtk::Image::from_icon_name(volume_icon(kind, endpoint.volume, endpoint.muted));
        icon.set_halign(gtk::Align::Center);
        icon.set_valign(gtk::Align::Center);
        let mute = gtk::Button::new();
        mute.add_css_class("SliderMute");
        mute.set_valign(gtk::Align::Center);
        mute.set_child(Some(&icon));
        mute.set_tooltip_text(Some("Mute"));
        let slider = gtk::Scale::with_range(
            gtk::Orientation::Horizontal,
            0.0,
            (kind.max_volume() * 100.0).round(),
            1.0,
        );
        slider.add_css_class("QuickSlider");
        slider.set_draw_value(false);
        slider.set_hexpand(true);
        let value = gtk::Label::new(Some("0%"));
        value.add_css_class("SliderValue");
        value.set_width_chars(4);
        value.set_xalign(1.0);
        body.append(&mute);
        body.append(&slider);
        body.append(&value);

        root.append(&head);
        root.append(&body);
        parent.append(&root);

        let updating = Rc::new(Cell::new(false));
        {
            let updating = Rc::clone(&updating);
            let core = Rc::clone(&core);
            let id = endpoint.id;
            slider.connect_value_changed(move |scale| {
                if updating.get() {
                    return;
                }
                core.send(Command::AudioSetEndpointVolume {
                    kind,
                    id,
                    volume: scale.value() / 100.0,
                });
            });
        }
        {
            let id = endpoint.id;
            let core = Rc::clone(&core);
            default.connect_clicked(move |_| {
                core.send(Command::AudioSetDefault(id));
            });
        }

        let muted = Rc::new(Cell::new(endpoint.muted));
        {
            let muted = Rc::clone(&muted);
            let core = Rc::clone(&core);
            let id = endpoint.id;
            mute.connect_clicked(move |_| {
                core.send(Command::AudioSetEndpointMuted {
                    kind,
                    id,
                    muted: !muted.get(),
                });
            });
        }

        let row = Self {
            kind,
            root,
            icon,
            mute,
            muted,
            slider,
            value,
            default,
            updating,
        };
        row.render(endpoint);
        row
    }

    fn render(&self, endpoint: &AudioEndpoint) {
        self.updating.set(true);
        self.muted.set(endpoint.muted);
        self.icon.set_icon_name(Some(volume_icon(
            self.kind,
            endpoint.volume,
            endpoint.muted,
        )));
        self.mute
            .set_tooltip_text(Some(if endpoint.muted { "Unmute" } else { "Mute" }));
        self.slider.set_value((endpoint.volume * 100.0).round());
        self.value.set_text(&percent_text(endpoint.volume * 100.0));
        set_overamp(&self.slider, self.kind, endpoint.volume);
        if endpoint.is_default {
            self.root.add_css_class("is-default");
            self.default.add_css_class("is-default");
        } else {
            self.root.remove_css_class("is-default");
            self.default.remove_css_class("is-default");
        }
        self.updating.set(false);
    }
}

fn endpoint_name(endpoint: &AudioEndpoint) -> String {
    if endpoint.description.is_empty() {
        endpoint.name.clone()
    } else {
        endpoint.description.clone()
    }
}

fn percent_text(percent: f64) -> String {
    format!("{:.0}%", percent.max(0.0))
}

fn set_overamp(scale: &gtk::Scale, kind: EndpointKind, volume: f64) {
    if overamp(kind, volume) {
        scale.add_css_class("overamp");
    } else {
        scale.remove_css_class("overamp");
    }
}

fn overamp(kind: EndpointKind, volume: f64) -> bool {
    kind == EndpointKind::Sink && (volume * 100.0).round() >= 101.0
}

fn master_device(devices: &[BrightnessDevice]) -> Option<&BrightnessDevice> {
    devices
        .iter()
        .find(|device| device.available && device.kind.is_internal())
        .or_else(|| devices.iter().find(|device| device.available))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nosh_core::{MAX_SOURCE_VOLUME, MAX_VOLUME};

    #[test]
    fn percentages_are_whole_numbers() {
        assert_eq!(percent_text(0.0), "0%");
        assert_eq!(percent_text(42.4), "42%");
        assert_eq!(percent_text(42.6), "43%");
        assert_eq!(percent_text(-4.0), "0%");
        assert_eq!(percent_text(100.0), "100%");
        assert_eq!(percent_text(101.0), "101%");
        assert_eq!(percent_text(140.0), "140%");
    }

    #[test]
    fn endpoint_names_prefer_the_description() {
        let mut endpoint = AudioEndpoint {
            id: 1,
            name: "alsa_output.pci".to_string(),
            description: "Laptop Speakers".to_string(),
            volume: 0.5,
            muted: false,
            icon_name: String::new(),
            is_default: false,
        };
        assert_eq!(endpoint_name(&endpoint), "Laptop Speakers");

        endpoint.description.clear();
        assert_eq!(endpoint_name(&endpoint), "alsa_output.pci");
    }

    #[test]
    fn device_limits_match_the_audio_rules() {
        assert_eq!((MAX_VOLUME * 100.0).round(), 150.0);
        assert_eq!((MAX_SOURCE_VOLUME * 100.0).round(), 100.0);
    }

    #[test]
    fn overamp_starts_above_one_hundred_percent() {
        assert!(!overamp(EndpointKind::Sink, 1.0));
        assert!(!overamp(EndpointKind::Sink, 1.004));
        assert!(!overamp(EndpointKind::Sink, 1.005));
        assert!(overamp(EndpointKind::Sink, 1.01));
        assert!(!overamp(EndpointKind::Source, 1.5));
    }
}
