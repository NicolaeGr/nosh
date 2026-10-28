//! The polkit password prompt.
//!
//! polkitd asks for a password through the core. The card shows the request,
//! sends the password back without storing it, and closes when the helper
//! session ends. A failed attempt disables the entry, because the helper
//! session is gone and another submit cannot succeed.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nosh_core::{Command, Core, PolkitEvent, PolkitPrompt};

use crate::auth_request::secret_from_text;

pub struct PolkitPromptWindow {
    window: gtk::ApplicationWindow,
    inner: Rc<Inner>,
}

struct Inner {
    core: Rc<Core>,
    window: gtk::ApplicationWindow,
    background: gtk::Box,
    icon: gtk::Image,
    message: gtk::Label,
    action: gtk::Label,
    password: gtk::PasswordEntry,
    error: gtk::Label,
    confirm: gtk::Button,
    cancel: gtk::Button,
    cookie: RefCell<Option<String>>,
    busy: Cell<bool>,
    open: Cell<bool>,
}

impl PolkitPromptWindow {
    pub fn new(app: &gtk::Application, core: Rc<Core>) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.set_decorated(false);
        window.init_layer_shell();
        window.set_namespace(Some("nosh-polkit"));
        window.set_layer(Layer::Overlay);
        window.set_exclusive_zone(-1);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.add_css_class("PolkitPrompt");
        window.set_visible(false);

        let background = gtk::Box::new(gtk::Orientation::Vertical, 0);
        background.add_css_class("PolkitBackground");
        background.set_hexpand(true);
        background.set_vexpand(true);

        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("PowerCard");
        card.add_css_class("PolkitCard");
        card.set_halign(gtk::Align::Center);
        card.set_valign(gtk::Align::Center);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.add_css_class("PolkitHeader");
        let icon = gtk::Image::from_icon_name("dialog-password-symbolic");
        icon.add_css_class("PolkitIcon");
        header.append(&icon);
        let title = gtk::Label::new(Some("Authentication required"));
        title.add_css_class("PowerTitle");
        title.set_xalign(0.0);
        header.append(&title);
        card.append(&header);

        let message = gtk::Label::new(None);
        message.add_css_class("PolkitMessage");
        message.set_xalign(0.0);
        message.set_wrap(true);
        message.set_max_width_chars(63);
        card.append(&message);

        let password = gtk::PasswordEntry::new();
        password.add_css_class("PowerPassword");
        password.set_show_peek_icon(true);
        password.set_placeholder_text(Some("Password"));
        password.set_margin_top(10);
        card.append(&password);

        let error = gtk::Label::new(None);
        error.add_css_class("PowerError");
        error.set_xalign(0.0);
        error.set_wrap(true);
        card.append(&error);

        let action = gtk::Label::new(None);
        action.add_css_class("PolkitAction");
        action.set_xalign(0.0);
        card.append(&action);

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        buttons.set_halign(gtk::Align::End);
        buttons.set_margin_top(8);
        let cancel = gtk::Button::with_label("Cancel");
        cancel.add_css_class("PowerSecondary");
        let confirm = gtk::Button::with_label("Authenticate");
        confirm.add_css_class("PowerPrimary");
        buttons.append(&cancel);
        buttons.append(&confirm);
        card.append(&buttons);

        background.append(&card);

        // A vertical box lays its children out from the top. The expanding
        // spacers center the card on the output.
        let top = gtk::Box::new(gtk::Orientation::Vertical, 0);
        top.set_vexpand(true);
        let bottom = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bottom.set_vexpand(true);
        background.prepend(&top);
        background.append(&bottom);

        window.set_child(Some(&background));

        let inner = Rc::new(Inner {
            core,
            window: window.clone(),
            background: background.clone(),
            icon,
            message,
            action,
            password,
            error,
            confirm,
            cancel,
            cookie: RefCell::new(None),
            busy: Cell::new(false),
            open: Cell::new(false),
        });

        {
            let inner = Rc::clone(&inner);
            let confirm = inner.confirm.clone();
            confirm.connect_clicked(move |_| send_password(&inner));
        }
        {
            let inner = Rc::clone(&inner);
            let password = inner.password.clone();
            password.connect_activate(move |_| send_password(&inner));
        }
        {
            let inner = Rc::clone(&inner);
            let cancel = inner.cancel.clone();
            cancel.connect_clicked(move |_| cancel_prompt(&inner));
        }

        let keys = gtk::EventControllerKey::new();
        {
            let inner = Rc::clone(&inner);
            keys.connect_key_pressed(move |_, key, _, _| {
                if key != gdk::Key::Escape {
                    return glib::Propagation::Proceed;
                }
                cancel_prompt(&inner);
                glib::Propagation::Stop
            });
        }
        window.add_controller(keys);

        Self { window, inner }
    }

    pub fn show(&self, prompt: &PolkitPrompt, monitor: Option<&gdk::Monitor>) {
        let inner = &self.inner;
        if let Some(monitor) = monitor {
            let geometry = monitor.geometry();
            inner
                .background
                .set_size_request(geometry.width(), geometry.height());
            self.window.set_monitor(Some(monitor));
        }

        inner.message.set_text(&prompt.message);
        inner
            .icon
            .set_icon_name(Some(if prompt.icon_name.is_empty() {
                "dialog-password-symbolic"
            } else {
                &prompt.icon_name
            }));
        inner
            .action
            .set_text(&format!("Action: {}", prompt.action_id));
        inner.action.set_visible(!prompt.action_id.is_empty());
        inner.error.set_text("");
        inner.password.set_text("");
        inner.password.set_sensitive(true);
        inner.confirm.set_sensitive(true);
        inner.cancel.set_label("Cancel");
        inner.busy.set(false);
        *inner.cookie.borrow_mut() = Some(prompt.cookie.clone());
        inner.open.set(true);

        self.window.present();
        inner.password.grab_focus();
    }

    /// Cancels the open prompt, if any. The session lock calls this so a
    /// request from before the lock does not survive it.
    pub fn cancel(&self) {
        if self.inner.open.get() {
            cancel_prompt(&self.inner);
        }
    }

    pub fn handle_event(&self, event: &PolkitEvent, monitor: Option<&gdk::Monitor>) {
        let inner = &self.inner;
        match event {
            PolkitEvent::Prompt(prompt) => self.show(prompt, monitor),
            PolkitEvent::Failed { cookie, message } => {
                if !is_current(inner, cookie) {
                    return;
                }
                // The helper session is gone, so another attempt cannot work.
                inner.busy.set(false);
                inner.confirm.set_sensitive(false);
                inner.password.set_sensitive(false);
                inner.error.set_text(message);
                inner.password.set_text("");
                inner.cancel.set_label("Close");
                inner.cancel.grab_focus();
            }
            PolkitEvent::Finished { cookie } | PolkitEvent::Cancelled { cookie } => {
                if !is_current(inner, cookie) {
                    return;
                }
                dismiss(inner);
            }
            PolkitEvent::Unavailable => {}
        }
    }
}

fn is_current(inner: &Inner, cookie: &str) -> bool {
    inner.open.get() && inner.cookie.borrow().as_deref() == Some(cookie)
}

fn send_password(inner: &Rc<Inner>) {
    if inner.busy.get() {
        return;
    }
    let Some(cookie) = inner.cookie.borrow().clone() else {
        return;
    };

    let password = inner.password.text();
    if password.is_empty() {
        inner.error.set_text("Enter your password");
        inner.password.grab_focus();
        return;
    }

    inner.error.set_text("");
    inner.busy.set(true);
    inner.confirm.set_sensitive(false);
    inner.password.set_text("");
    inner.core.send(Command::PolkitRespond {
        cookie,
        password: secret_from_text(&password),
    });
}

fn cancel_prompt(inner: &Rc<Inner>) {
    if let Some(cookie) = inner.cookie.borrow_mut().take() {
        inner.core.send(Command::PolkitCancel { cookie });
    }
    dismiss(inner);
}

fn dismiss(inner: &Rc<Inner>) {
    inner.open.set(false);
    inner.busy.set(false);
    *inner.cookie.borrow_mut() = None;
    inner.password.set_text("");
    inner.password.set_sensitive(true);
    inner.error.set_text("");
    inner.confirm.set_sensitive(true);
    inner.cancel.set_label("Cancel");
    gtk::prelude::GtkWindowExt::set_focus(&inner.window, None::<&gtk::Widget>);
    inner.window.set_visible(false);
}
