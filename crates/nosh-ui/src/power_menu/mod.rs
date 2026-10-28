//! The power menu overlay.
//!
//! The window rules follow `docs/features/power-menu.md`. The flow is one
//! action, one confirmation, then the logind command. logind decides whether
//! the action also needs a PAM check: a `challenge` answer shows the password
//! field, a `yes` answer sends the action after the confirmation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nosh_core::{Command, Core, PowerAction, PowerModel, Secret};

const ACTIONS_PAGE: &str = "actions";
const CONFIRM_PAGE: &str = "confirm";

pub struct PowerMenuWindow {
    window: gtk::ApplicationWindow,
    inner: Rc<Inner>,
}

/// The action waiting for confirmation.
#[derive(Clone, Copy)]
struct Pending {
    action: PowerAction,
    requires_auth: bool,
}

struct Inner {
    core: Rc<Core>,
    window: gtk::ApplicationWindow,
    background: gtk::Box,
    stack: gtk::Stack,
    actions: Vec<(PowerAction, gtk::Button)>,
    action_label: gtk::Label,
    hint: gtk::Label,
    password_row: gtk::Box,
    password: gtk::PasswordEntry,
    error: gtk::Label,
    confirm: gtk::Button,
    pending: RefCell<Option<Pending>>,
    request: Cell<u64>,
    model: RefCell<PowerModel>,
    open: Cell<bool>,
}

impl PowerMenuWindow {
    pub fn new(app: &gtk::Application, core: Rc<Core>) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.set_decorated(false);
        window.init_layer_shell();
        window.set_namespace(Some("nosh-power-menu"));
        window.set_layer(Layer::Overlay);
        window.set_exclusive_zone(-1);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.add_css_class("PowerMenu");
        window.set_visible(false);

        let background = gtk::Box::new(gtk::Orientation::Vertical, 0);
        background.add_css_class("PowerMenuBackground");
        background.set_hexpand(true);
        background.set_vexpand(true);

        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("PowerCard");
        card.set_halign(gtk::Align::Center);
        card.set_valign(gtk::Align::Center);
        card.set_can_focus(true);

        let title = gtk::Label::new(Some("Session"));
        title.add_css_class("PowerTitle");
        title.set_xalign(0.0);
        card.append(&title);

        let subtitle = gtk::Label::new(Some(&session_line()));
        subtitle.add_css_class("PowerSubtitle");
        subtitle.set_xalign(0.0);
        card.append(&subtitle);

        let (actions_page, actions) = build_actions();
        let confirm = build_confirm();
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(180);
        // Each page sets its own height, so the confirmation does not inherit
        // the height of the action list and leave empty space below it.
        stack.set_vhomogeneous(false);
        stack.set_hhomogeneous(false);
        stack.add_named(&actions_page, Some(ACTIONS_PAGE));
        stack.add_named(&confirm.root, Some(CONFIRM_PAGE));
        stack.set_visible_child_name(ACTIONS_PAGE);
        card.append(&stack);

        background.append(&card);

        // A vertical box lays its children out from the top. Two expanding
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
            stack,
            actions,
            action_label: confirm.action_label,
            hint: confirm.hint,
            password_row: confirm.password_row,
            password: confirm.password,
            error: confirm.error,
            confirm: confirm.confirm,
            pending: RefCell::new(None),
            request: Cell::new(0),
            model: RefCell::new(PowerModel::default()),
            open: Cell::new(false),
        });

        for (action, button) in &inner.actions {
            let action = *action;
            let inner = Rc::clone(&inner);
            button.connect_clicked(move |_| begin_confirm(&inner, action));
        }

        {
            let inner = Rc::clone(&inner);
            let confirm = inner.confirm.clone();
            confirm.connect_clicked(move |_| submit(&inner));
        }
        {
            let inner = Rc::clone(&inner);
            confirm
                .cancel
                .connect_clicked(move |_| show_actions(&inner));
        }
        {
            let inner = Rc::clone(&inner);
            let password = inner.password.clone();
            password.connect_activate(move |_| submit(&inner));
        }

        let click = gtk::GestureClick::new();
        {
            let background = background.clone();
            let inner = Rc::clone(&inner);
            click.connect_pressed(move |_, _, x, y| {
                let target: gtk::Widget = background.clone().upcast();
                if target.pick(x, y, gtk::PickFlags::DEFAULT).as_ref() == Some(&target) {
                    close(&inner);
                }
            });
        }
        background.add_controller(click);

        let keys = gtk::EventControllerKey::new();
        {
            let inner = Rc::clone(&inner);
            keys.connect_key_pressed(move |_, key, _, _| {
                if key != gdk::Key::Escape {
                    return glib::Propagation::Proceed;
                }
                if inner.stack.visible_child_name().as_deref() == Some(CONFIRM_PAGE) {
                    show_actions(&inner);
                } else {
                    close(&inner);
                }
                glib::Propagation::Stop
            });
        }
        window.add_controller(keys);

        let menu = Self { window, inner };
        menu.render_actions();
        menu
    }

    pub fn open(&self, monitor: Option<&gdk::Monitor>) {
        let monitor = monitor
            .cloned()
            .or_else(|| gdk::Display::default()?.monitors().item(0).and_downcast());

        if let Some(monitor) = &monitor {
            let geometry = monitor.geometry();
            // A layer surface with four anchors normally fills the output, but
            // GTK still sizes the child from its own request. The explicit
            // request keeps the dim backdrop and the vertical centering.
            self.inner
                .background
                .set_size_request(geometry.width(), geometry.height());
            self.window.set_monitor(Some(monitor));
        }

        self.inner.open.set(true);
        show_actions(&self.inner);
        self.window.present();
        focus_first(&self.inner);
    }

    pub fn close(&self) {
        close(&self.inner);
    }

    /// Applies the actions that logind accepts on this machine.
    pub fn set_power(&self, model: &PowerModel) {
        *self.inner.model.borrow_mut() = model.clone();
        self.render_actions();
    }

    pub fn handle_auth(&self, result: &nosh_core::AuthResult) {
        let inner = &self.inner;
        if !inner.open.get() || result.request_id != inner.request.get() {
            return;
        }

        inner.confirm.set_sensitive(true);
        match &result.error {
            Some(message) => {
                inner.error.set_text(message);
                inner.password.set_text("");
                inner.password.grab_focus();
            }
            None => {
                inner.error.set_text("");
                if let Some(pending) = inner.pending.borrow().as_ref().copied() {
                    send_action(inner, pending.action);
                }
            }
        }
    }

    /// A logind failure keeps the dialog open so the user can try again.
    pub fn show_failure(&self, message: &str) {
        let inner = &self.inner;
        if !inner.open.get() {
            return;
        }
        let requires_auth = inner
            .pending
            .borrow()
            .as_ref()
            .is_some_and(|pending| pending.requires_auth);
        inner.error.set_text(message);
        inner.password_row.set_visible(requires_auth);
        inner.stack.set_visible_child_name(CONFIRM_PAGE);
        inner.confirm.set_sensitive(true);
        inner.password.set_text("");
        if requires_auth {
            inner.password.grab_focus();
        } else {
            inner.confirm.grab_focus();
        }
    }

    fn render_actions(&self) {
        let model = self.inner.model.borrow();
        let inner = &self.inner;

        for (action, button) in &inner.actions {
            let supported = model.supports(*action);
            button.set_sensitive(supported);
            button.set_tooltip_text((!supported).then_some("Not available on this system"));
        }

        // An empty model means logind has not answered yet. Keeping the
        // buttons sensitive lets the menu open before the core replies.
        if !model.any() {
            for (_, button) in &inner.actions {
                button.set_sensitive(true);
                button.set_tooltip_text(None);
            }
        }
    }
}

fn build_actions() -> (gtk::Box, Vec<(PowerAction, gtk::Button)>) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
    root.add_css_class("PowerActions");

    let mut buttons = Vec::new();
    for action in PowerAction::ALL {
        let button = gtk::Button::new();
        button.add_css_class("PowerAction");
        if action.is_destructive() {
            button.add_css_class("danger");
        }

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let icon = gtk::Image::from_icon_name(action.icon());
        icon.add_css_class("PowerActionIcon");
        row.append(&icon);

        let label = gtk::Label::new(Some(action.label()));
        label.add_css_class("PowerActionLabel");
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row.append(&label);

        let hint = gtk::Label::new(Some(action.hint()));
        hint.add_css_class("PowerActionHint");
        hint.set_xalign(1.0);
        row.append(&hint);

        button.set_child(Some(&row));
        root.append(&button);
        buttons.push((action, button));
    }

    (root, buttons)
}

struct ConfirmWidgets {
    root: gtk::Box,
    action_label: gtk::Label,
    hint: gtk::Label,
    password_row: gtk::Box,
    password: gtk::PasswordEntry,
    error: gtk::Label,
    confirm: gtk::Button,
    cancel: gtk::Button,
}

fn build_confirm() -> ConfirmWidgets {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.add_css_class("PowerConfirm");

    let action_label = gtk::Label::new(None);
    action_label.add_css_class("PowerConfirmAction");
    action_label.set_xalign(0.0);
    root.append(&action_label);

    let hint = gtk::Label::new(None);
    hint.add_css_class("PowerConfirmHint");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    root.append(&hint);

    let password_row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    password_row.add_css_class("PowerPasswordRow");
    let password = gtk::PasswordEntry::new();
    password.add_css_class("PowerPassword");
    password.set_show_peek_icon(true);
    password.set_placeholder_text(Some("Password"));
    password_row.append(&password);
    password_row.set_visible(false);
    root.append(&password_row);

    let error = gtk::Label::new(None);
    error.add_css_class("PowerError");
    error.set_xalign(0.0);
    error.set_wrap(true);
    root.append(&error);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_halign(gtk::Align::End);
    buttons.add_css_class("PowerConfirmButtons");

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("PowerSecondary");
    let confirm = gtk::Button::with_label("Confirm");
    confirm.add_css_class("PowerPrimary");
    buttons.append(&cancel);
    buttons.append(&confirm);
    root.append(&buttons);

    ConfirmWidgets {
        root,
        action_label,
        hint,
        password_row,
        password,
        error,
        confirm,
        cancel,
    }
}

fn begin_confirm(inner: &Rc<Inner>, action: PowerAction) {
    let requires_auth = inner.model.borrow().requires_auth(action);
    if !inner.model.borrow().supports(action) {
        return;
    }

    *inner.pending.borrow_mut() = Some(Pending {
        action,
        requires_auth,
    });
    inner.action_label.set_text(action.label());
    inner.hint.set_text(if requires_auth {
        "This action needs your password."
    } else {
        action.hint()
    });
    inner.password_row.set_visible(requires_auth);
    inner.error.set_text("");
    inner.password.set_text("");
    inner.confirm.set_sensitive(true);
    inner.stack.set_visible_child_name(CONFIRM_PAGE);
    if requires_auth {
        inner.password.grab_focus();
    } else {
        inner.confirm.grab_focus();
    }
}

fn submit(inner: &Rc<Inner>) {
    let Some(pending) = inner.pending.borrow().as_ref().copied() else {
        return;
    };

    inner.error.set_text("");
    inner.confirm.set_sensitive(false);

    if !pending.requires_auth {
        send_action(inner, pending.action);
        return;
    }

    let password = inner.password.text();
    if password.is_empty() {
        inner.error.set_text("Enter your password");
        inner.confirm.set_sensitive(true);
        return;
    }

    let request_id = inner.request.get() + 1;
    inner.request.set(request_id);
    inner.password.set_text("");
    inner
        .action_label
        .set_text(&format!("{} · waiting", pending.action.label()));

    inner.core.send(Command::AuthAuthenticate {
        request_id,
        username: glib::user_name().to_string_lossy().into_owned(),
        password: Secret::new(password),
    });
}

fn send_action(inner: &Rc<Inner>, action: PowerAction) {
    inner
        .action_label
        .set_text(&format!("{} · working", action.label()));
    inner.confirm.set_sensitive(false);
    inner.core.send(Command::PowerAction(action));
}

fn show_actions(inner: &Rc<Inner>) {
    *inner.pending.borrow_mut() = None;
    inner.error.set_text("");
    inner.password.set_text("");
    inner.password_row.set_visible(false);
    inner.confirm.set_sensitive(true);
    inner.stack.set_visible_child_name(ACTIONS_PAGE);
    focus_first(inner);
}

fn focus_first(inner: &Rc<Inner>) {
    if let Some((_, button)) = inner.actions.first() {
        button.grab_focus();
    }
}

fn close(inner: &Rc<Inner>) {
    inner.open.set(false);
    *inner.pending.borrow_mut() = None;
    inner.password.set_text("");
    inner.password_row.set_visible(false);
    inner.error.set_text("");
    gtk::prelude::GtkWindowExt::set_focus(&inner.window, None::<&gtk::Widget>);
    inner.window.set_visible(false);
}

fn session_line() -> String {
    let user = glib::user_name().to_string_lossy().into_owned();
    match glib::host_name() {
        host if host.is_empty() => user,
        host => format!("{user} · {host}"),
    }
}
