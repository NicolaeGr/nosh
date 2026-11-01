//! The system tray section of the bar.
//!
//! The widget owns buttons and popovers only. Rendering is incremental: each
//! entry keeps the last icon, tooltip, and menu it displayed, so a snapshot
//! update does not rebuild widgets whose data did not change.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use gtk::glib::variant::ToVariant;
use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{
    Command, Core, TrayIcon, TrayItemModel, TrayMenu, TrayMenuItem, TrayMenuItemKind,
    TrayToggleState,
};

pub struct Tray {
    container: gtk::Box,
    core: Rc<Core>,
    entries: RefCell<Vec<TrayEntry>>,
}

struct TrayEntry {
    item_id: String,
    button: gtk::Button,
    image: gtk::Image,
    menu: Rc<MenuState>,
    popover: gtk::PopoverMenu,
    menu_available: Rc<Cell<bool>>,
    rendered_icon: Option<TrayIcon>,
    rendered_tooltip: String,
}

/// The popover's menu model. Items republish their menu whenever their state
/// changes, which some panel applets do several times a second, so the model
/// is rebuilt only when the popover is about to open. Rebuilding a menu that
/// nobody is looking at costs CPU and retains the objects GTK creates for it.
struct MenuState {
    model: gio::Menu,
    group: gio::SimpleActionGroup,
    core: Rc<Core>,
    item_id: String,
    pending: RefCell<Option<TrayMenu>>,
    rendered: RefCell<Option<TrayMenu>>,
}

impl MenuState {
    fn new(core: Rc<Core>, item_id: &str) -> Self {
        Self {
            model: gio::Menu::new(),
            group: gio::SimpleActionGroup::new(),
            core,
            item_id: item_id.to_string(),
            pending: RefCell::new(None),
            rendered: RefCell::new(None),
        }
    }

    fn set(&self, menu: Option<&TrayMenu>) {
        *self.pending.borrow_mut() = menu.cloned();
    }

    fn sync(&self) {
        let pending = self.pending.borrow();
        if *self.rendered.borrow() == *pending {
            return;
        }

        self.model.remove_all();
        for name in self.group.list_actions() {
            self.group.remove_action(&name);
        }
        if let Some(menu) = pending.as_ref() {
            build_menu(
                &self.model,
                &menu.items,
                &self.group,
                &self.core,
                &self.item_id,
            );
        }
        *self.rendered.borrow_mut() = pending.clone();
    }
}

impl Drop for TrayEntry {
    fn drop(&mut self) {
        self.popover.popdown();
        self.popover.unparent();
    }
}

impl TrayEntry {
    fn update(&mut self, item: &TrayItemModel, menu: Option<&TrayMenu>) {
        let icon = item.icon();
        if self.rendered_icon != icon {
            set_icon(&self.image, icon.as_ref());
            self.rendered_icon = icon;
        }

        let tooltip = item.tooltip_markup();
        if self.rendered_tooltip != tooltip {
            self.button
                .set_tooltip_markup((!tooltip.is_empty()).then_some(tooltip.as_str()));
            self.rendered_tooltip = tooltip;
        }

        self.menu_available
            .set(item.menu_path.is_some() || menu.is_some());

        self.menu.set(menu);
        if self.popover.is_visible() {
            self.menu.sync();
        }
    }
}

impl Tray {
    pub fn new(core: Rc<Core>) -> Self {
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        container.add_css_class("SysTray");

        Self {
            container,
            core,
            entries: RefCell::new(Vec::new()),
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.container
    }

    pub fn render(&self, items: &[TrayItemModel], menus: &HashMap<String, TrayMenu>) {
        let mut entries = self.entries.borrow_mut();
        let live: BTreeSet<&str> = items.iter().map(|item| item.item_id.as_str()).collect();

        entries.retain(|entry| {
            let keep = live.contains(entry.item_id.as_str());
            if !keep {
                self.container.remove(&entry.button);
            }
            keep
        });

        for item in items {
            let menu = menus.get(&item.item_id);
            if let Some(position) = entries
                .iter()
                .position(|entry| entry.item_id == item.item_id)
            {
                entries[position].update(item, menu);
            } else {
                let entry = self.create(item, menu);
                self.container.append(&entry.button);
                entries.push(entry);
            }
        }
    }

    fn create(&self, item: &TrayItemModel, menu: Option<&TrayMenu>) -> TrayEntry {
        let image = gtk::Image::new();

        let button = gtk::Button::new();
        for class in ["menu-icon", "p-0"] {
            button.add_css_class(class);
        }
        button.set_focus_on_click(false);
        button.set_child(Some(&image));

        let menu_state = Rc::new(MenuState::new(Rc::clone(&self.core), &item.item_id));
        menu_state.set(menu);
        button.insert_action_group("dbusmenu", Some(&menu_state.group));

        let popover = gtk::PopoverMenu::from_model(Some(&menu_state.model));
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        popover.set_parent(&button);
        popover.insert_action_group("dbusmenu", Some(&menu_state.group));

        let icon = item.icon();
        set_icon(&image, icon.as_ref());

        let tooltip = item.tooltip_markup();
        button.set_tooltip_markup((!tooltip.is_empty()).then_some(tooltip.as_str()));

        let menu_available = Rc::new(Cell::new(item.menu_path.is_some() || menu.is_some()));
        install_input(
            &button,
            &popover,
            &self.core,
            &item.item_id,
            Rc::clone(&menu_available),
            Rc::clone(&menu_state),
        );
        install_scroll(&button, &self.core, &item.item_id);

        TrayEntry {
            item_id: item.item_id.clone(),
            button,
            image,
            menu: menu_state,
            popover,
            menu_available,
            rendered_icon: icon,
            rendered_tooltip: tooltip,
        }
    }
}

/// One gesture handles every button. A left click activates the item, a right
/// click opens the context menu or falls back to `SecondaryActivate`, and a
/// middle click sends `SecondaryActivate`. The popover's visibility decides
/// whether a click only dismisses the menu, so no click counter is needed.
fn install_input(
    button: &gtk::Button,
    popover: &gtk::PopoverMenu,
    core: &Rc<Core>,
    item_id: &str,
    menu_available: Rc<Cell<bool>>,
    menu: Rc<MenuState>,
) {
    let click = gtk::GestureClick::new();
    // The default listens for button 1 only, so widen it to any button.
    click.set_button(0);
    click.set_propagation_phase(gtk::PropagationPhase::Capture);

    let popover = popover.clone();
    let popover_button = button.clone();
    let core = Rc::clone(core);
    let item_id = item_id.to_string();

    click.connect_pressed(move |gesture, _, _, _| {
        match gesture.current_button() {
            gdk::BUTTON_PRIMARY => {
                if popover.is_visible() {
                    popover.popdown();
                } else {
                    core.send(Command::TrayActivate {
                        item_id: item_id.clone(),
                        x: 0,
                        y: 0,
                    });
                }
            }
            gdk::BUTTON_SECONDARY => {
                if menu_available.get() {
                    // The item may have published a new menu since the last
                    // time this popover was shown.
                    menu.sync();
                    let rect = gdk::Rectangle::new(
                        0,
                        0,
                        popover_button.width().max(1),
                        popover_button.height().max(1),
                    );
                    popover.set_pointing_to(Some(&rect));
                    popover.popup();
                    core.send(Command::TrayMenuAboutToShow {
                        item_id: item_id.clone(),
                    });
                } else {
                    core.send(Command::TraySecondaryActivate {
                        item_id: item_id.clone(),
                        x: 0,
                        y: 0,
                    });
                }
            }
            gdk::BUTTON_MIDDLE => {
                core.send(Command::TraySecondaryActivate {
                    item_id: item_id.clone(),
                    x: 0,
                    y: 0,
                });
            }
            _ => return,
        }

        gesture.set_state(gtk::EventSequenceState::Claimed);
    });

    button.add_controller(click);
}

fn install_scroll(button: &gtk::Button, core: &Rc<Core>, item_id: &str) {
    let scroll = gtk::EventControllerScroll::new(
        gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::HORIZONTAL,
    );
    let core = Rc::clone(core);
    let item_id = item_id.to_string();

    scroll.connect_scroll(move |_, dx, dy| {
        let (delta, horizontal) = if dy != 0.0 {
            (if dy > 0.0 { -1 } else { 1 }, false)
        } else if dx != 0.0 {
            (if dx > 0.0 { -1 } else { 1 }, true)
        } else {
            return glib::Propagation::Proceed;
        };

        core.send(Command::TrayScroll {
            item_id: item_id.clone(),
            delta,
            horizontal,
        });
        glib::Propagation::Stop
    });

    button.add_controller(scroll);
}

/// Builds the menu from the D-Bus menu tree.
///
/// `GtkPopoverMenu` draws a separator between adjacent sections. It renders an
/// item with no label and no action as an empty row, so the tree's separators
/// become section boundaries.
fn build_menu(
    menu: &gio::Menu,
    items: &[TrayMenuItem],
    group: &gio::SimpleActionGroup,
    core: &Rc<Core>,
    item_id: &str,
) {
    let mut section = gio::Menu::new();

    for item in items.iter().filter(|item| item.visible) {
        if item.kind == TrayMenuItemKind::Separator {
            append_section(menu, &section);
            section = gio::Menu::new();
            continue;
        }

        append_menu_item(&section, item, group, core, item_id);
    }

    append_section(menu, &section);
}

/// Appends a section unless it is empty, so a leading or trailing separator
/// does not produce a stray line.
fn append_section(menu: &gio::Menu, section: &gio::Menu) {
    if section.n_items() > 0 {
        menu.append_section(None, section);
    }
}

fn append_menu_item(
    menu: &gio::Menu,
    item: &TrayMenuItem,
    group: &gio::SimpleActionGroup,
    core: &Rc<Core>,
    item_id: &str,
) {
    let action_name = format!("item{}", item.id);

    if !item.children.is_empty() {
        let submenu = gio::Menu::new();
        build_menu(&submenu, &item.children, group, core, item_id);
        menu.append_submenu(Some(&item.label), &submenu);
        return;
    }

    if item.is_toggle() {
        let state = (item.toggle_state == TrayToggleState::On).to_variant();
        let action =
            gio::SimpleAction::new_stateful(&action_name, Some(glib::VariantTy::BOOLEAN), &state);
        action.set_enabled(item.enabled);
        let action_core = Rc::clone(core);
        let action_item = item_id.to_string();
        let menu_id = item.id;
        action.connect_change_state(move |action, value| {
            if let Some(value) = value {
                action.set_state(value);
            }
            action_core.send(Command::TrayMenuEvent {
                item_id: action_item.clone(),
                menu_id,
                event: "clicked".to_string(),
            });
        });
        group.add_action(&action);
    } else {
        let action = gio::SimpleAction::new(&action_name, None);
        action.set_enabled(item.enabled);
        let action_core = Rc::clone(core);
        let action_item = item_id.to_string();
        let menu_id = item.id;
        action.connect_activate(move |_, _| {
            action_core.send(Command::TrayMenuEvent {
                item_id: action_item.clone(),
                menu_id,
                event: "clicked".to_string(),
            });
        });
        group.add_action(&action);
    }

    let detailed_action = format!("dbusmenu.{action_name}");
    let menu_item = gio::MenuItem::new(Some(&item.label), Some(&detailed_action));
    if let Some(icon_name) = item.icon_name.as_deref().filter(|name| !name.is_empty()) {
        let icon = gio::ThemedIcon::new(icon_name);
        if let Some(variant) = icon.serialize() {
            menu_item.set_attribute_value("icon", Some(&variant));
        }
    }
    menu.append_item(&menu_item);
}

fn set_icon(image: &gtk::Image, icon: Option<&TrayIcon>) {
    match icon {
        Some(TrayIcon::File(path)) => image.set_from_file(Some(path)),
        Some(TrayIcon::Named(name)) => image.set_icon_name(Some(name)),
        Some(TrayIcon::Pixmap(pixmap)) => {
            let bytes = glib::Bytes::from(&pixmap.rgba);
            let stride = pixmap.width as usize * 4;
            let texture = gdk::MemoryTexture::new(
                pixmap.width as i32,
                pixmap.height as i32,
                gdk::MemoryFormat::R8g8b8a8,
                &bytes,
                stride,
            );
            image.set_paintable(Some(&texture));
        }
        None => image.set_icon_name(Some("image-missing")),
    }
}
