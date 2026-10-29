//! One layer-shell window per open menu, with a virtualized item list.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nosh_core::menu::model::{Anchor, Keyboard, Layer as CoreLayer};
use nosh_core::{
    Command, Core, ItemLayout, MenuDef, MenuIcon, MenuItem, MenuKey, MenuPreview, ViewKind,
};

pub struct MenuManager {
    windows: HashMap<String, MenuWindow>,
    styles: HashMap<String, gtk::CssProvider>,
}

impl MenuManager {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            styles: HashMap::new(),
        }
    }

    pub fn open(&mut self, id: String, def: MenuDef, app: &gtk::Application, core: Rc<Core>) {
        self.close(&id);
        self.refresh_style(&id);
        let window = MenuWindow::new(app, &id, def, core);
        window.present();
        self.windows.insert(id, window);
    }

    /// Re-reads `menus.d/<id>.css` so a change reaches the next invocation.
    fn refresh_style(&mut self, id: &str) {
        let Ok(paths) = nosh_core::NoshPaths::from_env() else {
            return;
        };
        let path = paths.menus_dir().join(format!("{id}.css"));
        let css = match std::fs::read_to_string(&path) {
            Ok(css) => css,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Some(provider) = self.styles.remove(id) {
                    if let Some(display) = gdk::Display::default() {
                        gtk::style_context_remove_provider_for_display(&display, &provider);
                    }
                }
                return;
            }
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "cannot read the per-menu stylesheet");
                return;
            }
        };

        if let Some(provider) = self.styles.get(id) {
            provider.load_from_data(&css);
            return;
        }

        let Some(display) = gdk::Display::default() else {
            return;
        };
        let provider = style_provider(&css);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER + 2,
        );
        self.styles.insert(id.to_string(), provider);
    }

    pub fn set_results(&mut self, id: &str, items: Vec<MenuItem>) {
        if let Some(window) = self.windows.get_mut(id) {
            window.set_results(items);
        }
    }

    pub fn show_error(&mut self, id: &str, message: &str) {
        if let Some(window) = self.windows.get_mut(id) {
            window.show_error(message);
        }
    }

    pub fn close(&mut self, id: &str) {
        if let Some(window) = self.windows.remove(id) {
            window.destroy();
        }
    }
}

struct MenuWindow {
    window: gtk::ApplicationWindow,
    store: gio::ListStore,
    selection: gtk::SingleSelection,
    entry: gtk::Entry,
    error: gtk::Label,
}

impl MenuWindow {
    fn new(app: &gtk::Application, id: &str, def: MenuDef, core: Rc<Core>) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.set_decorated(false);
        window.init_layer_shell();
        window.set_namespace(Some(&def.namespace(id)));
        window.set_layer(map_layer(def.window.layer));
        for anchor in &def.window.anchors {
            match anchor {
                Anchor::Top => window.set_anchor(Edge::Top, true),
                Anchor::Bottom => window.set_anchor(Edge::Bottom, true),
                Anchor::Left => window.set_anchor(Edge::Left, true),
                Anchor::Right => window.set_anchor(Edge::Right, true),
                // No anchors centers the surface on the output.
                Anchor::Center => {}
            }
        }
        window.set_keyboard_mode(map_keyboard(def.window.keyboard));
        window.set_size_request(def.width() as i32, def.window.height.unwrap_or(600) as i32);
        window.add_css_class("Menu");
        window.add_css_class(&format!("menu-{id}"));
        window.add_css_class(match def.view.kind {
            ViewKind::List => "MenuListWindow",
            ViewKind::Grid => "MenuGridWindow",
        });

        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("MenuRoot");
        window.set_child(Some(&root));

        let entry = gtk::Entry::new();
        entry.add_css_class("MenuSearch");
        entry.set_hexpand(true);
        if let Some(placeholder) = &def.placeholder {
            entry.set_placeholder_text(Some(placeholder));
        }
        root.append(&entry);

        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::new(Some(store.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);

        let layout = def.view.item.layout;
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_bind(move |_, object| {
            let Some(list_item) = object.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(boxed) = list_item.item().and_downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            let item = boxed.borrow::<MenuItem>();
            let child = build_item(&item, layout);
            list_item.set_child(Some(&child));
        });

        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_hexpand(true);

        let bindings = menu_bindings(&def);

        let send = {
            let core = Rc::clone(&core);
            let id = id.to_string();
            let selection = selection.clone();
            move |action: Option<String>| {
                let index = selection.selected();
                let index = if index == u32::MAX { 0 } else { index };
                core.send(Command::MenuActivate {
                    id: id.clone(),
                    index: index as usize,
                    action,
                });
            }
        };
        let activate = {
            let send = send.clone();
            move || send(None)
        };

        let view: gtk::Widget = match def.view.kind {
            ViewKind::List => {
                let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
                list.set_single_click_activate(true);
                list.add_css_class("MenuList");
                let activate = activate.clone();
                list.connect_activate(move |_, _| activate());
                scrolled.set_child(Some(&list));
                list.upcast()
            }
            ViewKind::Grid => {
                let grid = gtk::GridView::new(Some(selection.clone()), Some(factory));
                grid.set_max_columns(def.view.resolved_columns());
                grid.set_single_click_activate(true);
                grid.add_css_class("MenuGrid");
                let activate = activate.clone();
                grid.connect_activate(move |_, _| activate());
                scrolled.set_child(Some(&grid));
                grid.upcast()
            }
        };
        root.append(&scrolled);

        let error = gtk::Label::new(None);
        error.add_css_class("MenuError");
        error.set_visible(false);
        error.set_xalign(0.0);
        root.append(&error);

        entry.connect_activate({
            let activate = activate.clone();
            move |_| activate()
        });
        entry.connect_changed({
            let core = Rc::clone(&core);
            let id = id.to_string();
            move |entry| {
                core.send(Command::MenuQuery {
                    id: id.clone(),
                    query: entry.text().to_string(),
                });
            }
        });

        let controller = gtk::EventControllerKey::new();
        controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        controller.connect_key_pressed({
            let entry = entry.clone();
            let view = view.clone();
            let selection = selection.clone();
            let core = Rc::clone(&core);
            let id = id.to_string();
            let columns = def.view.resolved_columns();
            move |_, key, _, state| {
                handle_key(
                    key, state, &entry, &view, &selection, columns, &id, &core, &send, &bindings,
                )
            }
        });
        window.add_controller(controller);

        window.connect_close_request({
            let core = Rc::clone(&core);
            let id = id.to_string();
            move |_| {
                core.send(Command::MenuClose(id.clone()));
                glib::Propagation::Proceed
            }
        });

        Self {
            window,
            store,
            selection,
            entry,
            error,
        }
    }

    fn present(&self) {
        self.window.present();
        let entry = self.entry.clone();
        glib::idle_add_local_once(move || {
            entry.grab_focus();
        });
    }

    fn destroy(&self) {
        self.window.destroy();
    }

    fn set_results(&mut self, items: Vec<MenuItem>) {
        self.store.remove_all();
        for item in items {
            self.store.append(&glib::BoxedAnyObject::new(item));
        }
        if self.store.n_items() > 0 {
            self.selection.set_selected(0);
        }
        self.error.set_visible(false);
    }

    fn show_error(&mut self, message: &str) {
        self.error.set_text(message);
        self.error.set_visible(true);
    }
}

struct KeyBinding {
    menu: MenuKey,
    keys: Vec<gdk::Key>,
    action: String,
}

fn menu_bindings(def: &MenuDef) -> Vec<KeyBinding> {
    def.keys
        .iter()
        .filter_map(|(chord, action)| {
            let menu = match MenuKey::parse(chord) {
                Ok(menu) => menu,
                Err(error) => {
                    tracing::warn!(%error, chord, "ignoring an invalid menu key binding");
                    return None;
                }
            };
            let keys = resolve_keys(&menu.key);
            if keys.is_empty() {
                tracing::warn!(
                    chord,
                    key = %menu.key,
                    "ignoring a menu key with an unknown key name"
                );
                return None;
            }
            Some(KeyBinding {
                menu,
                keys,
                action: action.clone(),
            })
        })
        .collect()
}

/// Resolves a config key name against GDK key names. `enter` covers both the
/// main and keypad Return keys, and `return` covers the main one.
fn resolve_keys(name: &str) -> Vec<gdk::Key> {
    match name {
        "enter" => vec![gdk::Key::Return, gdk::Key::KP_Enter],
        "return" => vec![gdk::Key::Return],
        "kp_enter" => vec![gdk::Key::KP_Enter],
        _ => gdk::Key::from_name(name)
            .or_else(|| gdk::Key::from_name(capitalize(name)))
            .or_else(|| gdk::Key::from_name(name.to_ascii_uppercase()))
            .into_iter()
            .collect(),
    }
}

/// Turns `page_up` into `Page_Up` for GDK names that use capitals.
fn capitalize(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("_")
}

fn matches_binding(binding: &KeyBinding, key: gdk::Key, state: gdk::ModifierType) -> bool {
    let relevant = gdk::ModifierType::CONTROL_MASK
        | gdk::ModifierType::SHIFT_MASK
        | gdk::ModifierType::ALT_MASK
        | gdk::ModifierType::SUPER_MASK;
    let held = state & relevant;

    binding.keys.contains(&key)
        && binding.menu.ctrl == held.contains(gdk::ModifierType::CONTROL_MASK)
        && binding.menu.shift == held.contains(gdk::ModifierType::SHIFT_MASK)
        && binding.menu.alt == held.contains(gdk::ModifierType::ALT_MASK)
        && binding.menu.logo == held.contains(gdk::ModifierType::SUPER_MASK)
}

#[allow(clippy::too_many_arguments)]
fn handle_key(
    key: gdk::Key,
    state: gdk::ModifierType,
    entry: &gtk::Entry,
    view: &gtk::Widget,
    selection: &gtk::SingleSelection,
    columns: u32,
    id: &str,
    core: &Rc<Core>,
    activate: &impl Fn(Option<String>),
    bindings: &[KeyBinding],
) -> glib::Propagation {
    if key == gdk::Key::Escape {
        core.send(Command::MenuClose(id.to_string()));
        return glib::Propagation::Stop;
    }

    for binding in bindings {
        if matches_binding(binding, key, state) {
            activate(Some(binding.action.clone()));
            return glib::Propagation::Stop;
        }
    }

    let entry_focused = entry.has_focus();

    if matches!(key, gdk::Key::Down | gdk::Key::Tab) && entry_focused {
        view.grab_focus();
        selection.set_selected(0);
        return glib::Propagation::Stop;
    }

    if key == gdk::Key::Up && !entry_focused && selection.selected() < columns {
        entry.grab_focus();
        return glib::Propagation::Stop;
    }

    if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter)
        && !entry_focused
        && !state.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        )
    {
        activate(None);
        return glib::Propagation::Stop;
    }

    // Any focus outside the search bar means the user is navigating items.
    // Focus may sit on the list or grid itself or on a row inside it, so do
    // not test the view widget. Typing always belongs to the search bar.
    if !entry_focused
        && !state.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        )
    {
        if key == gdk::Key::BackSpace {
            entry.grab_focus();
            delete_before_cursor(entry);
            return glib::Propagation::Stop;
        }

        if let Some(character) = key.to_unicode() {
            if !character.is_control() {
                entry.grab_focus();
                insert_at_cursor(entry, character);
                return glib::Propagation::Stop;
            }
        }
    }

    glib::Propagation::Proceed
}

fn insert_at_cursor(entry: &gtk::Entry, character: char) {
    let mut text = entry.text().to_string();
    let position = entry.position().max(0) as usize;
    let byte = text
        .char_indices()
        .nth(position)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    text.insert(byte, character);
    entry.set_text(&text);
    entry.set_position((position + 1) as i32);
}

fn delete_before_cursor(entry: &gtk::Entry) {
    let mut text = entry.text().to_string();
    let position = entry.position().max(0) as usize;
    if position == 0 {
        return;
    }
    if let Some((byte, _)) = text.char_indices().nth(position - 1) {
        text.remove(byte);
        entry.set_text(&text);
        entry.set_position((position - 1) as i32);
    }
}

fn build_item(item: &MenuItem, layout: ItemLayout) -> gtk::Box {
    let row = layout == ItemLayout::Row;
    let root = gtk::Box::new(
        if row {
            gtk::Orientation::Horizontal
        } else {
            gtk::Orientation::Vertical
        },
        if row { 10 } else { 4 },
    );
    root.add_css_class("MenuItem");
    root.add_css_class(match layout {
        ItemLayout::Row => "MenuItemRow",
        ItemLayout::Tile => "MenuItemTile",
        ItemLayout::Card => "MenuItemCard",
    });
    if let nosh_core::ItemState::Disabled { reason } = &item.state {
        root.add_css_class("MenuItemDisabled");
        root.set_tooltip_text(Some(reason));
    }
    if !row {
        root.set_size_request(160, -1);
    }

    if let Some(media) = media_widget(item, layout) {
        root.append(&media);
    }

    let title = gtk::Label::new(Some(&item.name));
    title.add_css_class("MenuItemTitle");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);

    if row {
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        text.append(&title);
        if let Some(description) = &item.description {
            text.append(&description_label(description));
        }
        root.append(&text);
        if let Some(badge) = &item.badge {
            root.append(&badge_label(badge));
        }
    } else {
        root.append(&title);
        if layout == ItemLayout::Card {
            if let Some(description) = &item.description {
                root.append(&description_label(description));
            }
        }
        if let Some(badge) = &item.badge {
            root.append(&badge_label(badge));
        }
    }

    root
}

fn media_widget(item: &MenuItem, layout: ItemLayout) -> Option<gtk::Widget> {
    let (width, height) = if layout == ItemLayout::Row {
        (36, 36)
    } else {
        (180, 110)
    };

    match &item.preview {
        Some(MenuPreview::Image { image }) => Some(picture(image, width, height)),
        Some(MenuPreview::Text { .. }) => None,
        None => match &item.icon {
            Some(MenuIcon::Image { image }) => Some(picture(image, width, height)),
            Some(MenuIcon::Named(name)) => {
                Some(icon(name, if layout == ItemLayout::Row { 24 } else { 40 }))
            }
            None => None,
        },
    }
}

fn picture(path: &Path, width: i32, height: i32) -> gtk::Widget {
    let picture = gtk::Picture::for_filename(path);
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture.set_can_shrink(true);
    picture.set_size_request(width, height);
    picture.add_css_class("MenuItemMedia");
    picture.upcast()
}

fn icon(name: &str, size: i32) -> gtk::Widget {
    let image = gtk::Image::from_icon_name(name);
    image.set_pixel_size(size);
    image.add_css_class("MenuItemMedia");
    image.upcast()
}

fn description_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("MenuItemDescription");
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label
}

fn badge_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("MenuItemBadge");
    label.set_valign(gtk::Align::Center);
    label
}

fn map_layer(layer: CoreLayer) -> Layer {
    match layer {
        CoreLayer::Background => Layer::Background,
        CoreLayer::Bottom => Layer::Bottom,
        CoreLayer::Top => Layer::Top,
        CoreLayer::Overlay => Layer::Overlay,
    }
}

fn map_keyboard(keyboard: Keyboard) -> KeyboardMode {
    match keyboard {
        Keyboard::None => KeyboardMode::None,
        Keyboard::OnDemand => KeyboardMode::OnDemand,
        Keyboard::Exclusive => KeyboardMode::Exclusive,
    }
}

/// Builds a provider for the per-menu stylesheet. Rules should be scoped with
/// `.menu-<id>` to avoid changing other menus.
fn style_provider(css: &str) -> gtk::CssProvider {
    let provider = gtk::CssProvider::new();
    provider.connect_parsing_error(|_, section, error| {
        tracing::warn!(
            line = section.start_location().lines() + 1,
            %error,
            "per-menu CSS parse error"
        );
    });
    provider.load_from_data(css);
    provider
}
