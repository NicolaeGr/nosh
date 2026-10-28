//! The session lock screen.
//!
//! The screen uses the `ext-session-lock-v1` protocol through
//! `gtk4-session-lock`, so the compositor holds every input device and the
//! shell's other windows cannot take focus. One window is assigned to each
//! monitor while the session is locked.
//!
//! The password is checked through `AuthService`. The screen never unlocks on
//! a logind `Unlock` signal alone.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_session_lock::Instance;
use nosh_core::{AuthResult, Command, Core, LockMarker};

use crate::auth_request::{next_request_id, secret_from_text};

/// A PAM stack can block on NSS or `pam_faillock`. The screen gives up after
/// this, so the entry does not stay disabled forever.
const UNLOCK_TIMEOUT_SECONDS: u32 = 30;

pub struct LockScreen {
    state: Rc<State>,
}

struct State {
    app: gtk::Application,
    core: Rc<Core>,
    instance: RefCell<Option<Instance>>,
    surfaces: RefCell<Vec<Surface>>,
    locked: Cell<bool>,
    busy: Cell<bool>,
    request: Cell<u64>,
    clock_source: RefCell<Option<glib::SourceId>>,
    auth_timeout: RefCell<Option<glib::SourceId>>,
    on_unlocked: RefCell<Option<Rc<dyn Fn()>>>,
    marker: Option<LockMarker>,
}

/// The widgets of one monitor. The library destroys the window when the lock
/// ends or the monitor disappears.
struct Surface {
    monitor: gdk::Monitor,
    window: gtk::ApplicationWindow,
    entry: gtk::PasswordEntry,
    error: gtk::Label,
    unlock: gtk::Button,
    clock: gtk::Label,
    date: gtk::Label,
}

impl LockScreen {
    pub fn new(app: &gtk::Application, core: Rc<Core>) -> Self {
        Self {
            state: Rc::new(State {
                app: app.clone(),
                core,
                instance: RefCell::new(None),
                surfaces: RefCell::new(Vec::new()),
                locked: Cell::new(false),
                busy: Cell::new(false),
                request: Cell::new(0),
                clock_source: RefCell::new(None),
                auth_timeout: RefCell::new(None),
                on_unlocked: RefCell::new(None),
                marker: LockMarker::from_env(),
            }),
        }
    }

    pub fn lock(&self) {
        self.state.start();
    }

    /// Locks when the previous run left a marker behind.
    ///
    /// A crash while locked leaves the compositor holding the session, so the
    /// restarted shell has to ask for the lock again. `misc:allow_session_lock_restore`
    /// in Hyprland lets the new client take the existing lock over.
    pub fn lock_if_pending(&self) -> bool {
        let pending = self
            .state
            .marker
            .as_ref()
            .is_some_and(|marker| marker.exists());
        if pending {
            self.state.start();
        }
        pending
    }

    /// True from the moment a lock is requested until the session unlocks.
    pub fn is_locked(&self) -> bool {
        self.state.instance.borrow().is_some()
    }

    pub fn set_unlocked_handler(&self, handler: Rc<dyn Fn()>) {
        *self.state.on_unlocked.borrow_mut() = Some(handler);
    }

    pub fn handle_auth(&self, result: &AuthResult) {
        let state = &self.state;
        if !state.busy.get() || state.request.get() != result.request_id {
            return;
        }
        state.stop_auth_timeout();

        match &result.error {
            Some(message) => state.show_failure(message),
            None => {
                state.set_error("");
                for surface in state.surfaces.borrow().iter() {
                    surface.unlock.set_label("Unlocking…");
                }
                state.unlock();
            }
        }
    }
}

impl State {
    fn start(self: &Rc<Self>) {
        if self.instance.borrow().is_some() {
            return;
        }
        if !gtk4_session_lock::is_supported() {
            tracing::error!(
                "the compositor does not support ext-session-lock, the session cannot be locked"
            );
            self.clear_marker();
            return;
        }

        let instance = Instance::new();

        {
            let weak = Rc::downgrade(self);
            instance.connect_locked(move |_| {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                state.locked.set(true);
                state.set_error("");
                state.start_clock();
                for surface in state.surfaces.borrow().iter() {
                    surface.entry.grab_focus();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            instance.connect_monitor(move |instance, monitor| {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                state.add_surface(instance, monitor);
            });
        }
        {
            let weak = Rc::downgrade(self);
            instance.connect_unlocked(move |_| {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                state.teardown();
                let handler = state.on_unlocked.borrow().clone();
                if let Some(handler) = handler {
                    handler();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            instance.connect_failed(move |_| {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                tracing::error!(
                    "the compositor refused the session lock; the session is not locked"
                );
                state.teardown();
            });
        }

        *self.instance.borrow_mut() = Some(instance.clone());
        // Write the marker before the request, so a crash between the two
        // still re-locks on the next start. A panic after `locked` keeps it.
        self.write_marker();
        if !instance.lock() {
            tracing::error!("the compositor rejected the session lock, the session is not locked");
            self.teardown();
        }
    }

    fn unlock(&self) {
        let instance = self.instance.borrow().clone();
        if let Some(instance) = instance {
            instance.unlock();
        }
    }

    /// Clears all state after a failed lock or a successful unlock.
    fn teardown(&self) {
        self.locked.set(false);
        self.busy.set(false);
        self.request.set(0);
        if let Some(source) = self.clock_source.borrow_mut().take() {
            source.remove();
        }
        self.stop_auth_timeout();
        self.surfaces.borrow_mut().clear();
        *self.instance.borrow_mut() = None;
        // The lock is no longer held, so a restart must not reclaim it.
        self.clear_marker();
    }

    fn write_marker(&self) {
        let Some(marker) = &self.marker else {
            tracing::warn!(
                "XDG_RUNTIME_DIR is not set, so a lock cannot be recovered after a crash"
            );
            return;
        };
        if let Err(error) = marker.set() {
            tracing::warn!(
                %error,
                path = %marker.path().display(),
                "the lock marker could not be written"
            );
        }
    }

    fn clear_marker(&self) {
        if let Some(marker) = &self.marker {
            marker.clear();
        }
    }

    fn add_surface(self: &Rc<Self>, instance: &Instance, monitor: &gdk::Monitor) {
        let surface = build_surface(&self.app, monitor);

        {
            let state = Rc::clone(self);
            surface
                .entry
                .connect_activate(move |entry| state.submit(entry));
        }
        {
            let state = Rc::clone(self);
            let entry = surface.entry.clone();
            surface
                .unlock
                .connect_clicked(move |_| state.submit(&entry));
        }

        instance.assign_window_to_monitor(&surface.window, monitor);
        surface.entry.grab_focus();
        self.surfaces.borrow_mut().push(surface);
        self.update_clock();
        self.set_error("");
    }

    fn submit(self: &Rc<Self>, entry: &gtk::PasswordEntry) {
        if self.busy.get() {
            return;
        }

        let password = entry.text();
        if password.is_empty() {
            self.set_error("Enter your password");
            entry.grab_focus();
            return;
        }

        let request_id = next_request_id();
        self.request.set(request_id);
        self.busy.set(true);
        self.set_error("");
        for surface in self.surfaces.borrow().iter() {
            surface.entry.set_text("");
            surface.entry.set_sensitive(false);
            surface.unlock.set_sensitive(false);
            surface.unlock.set_label("Unlocking…");
        }

        self.core.send(Command::AuthAuthenticate {
            request_id,
            username: glib::user_name().to_string_lossy().into_owned(),
            password: secret_from_text(&password),
        });
        self.start_auth_timeout();
    }

    fn start_auth_timeout(self: &Rc<Self>) {
        self.stop_auth_timeout();
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_seconds_local_once(UNLOCK_TIMEOUT_SECONDS, move || {
            let Some(state) = weak.upgrade() else {
                return;
            };
            if !state.busy.get() {
                return;
            }
            // The late result no longer matches, so it is ignored.
            state.request.set(0);
            state.show_failure("Authentication timed out. Try again.");
        });
        *self.auth_timeout.borrow_mut() = Some(source);
    }

    fn stop_auth_timeout(&self) {
        if let Some(source) = self.auth_timeout.borrow_mut().take() {
            source.remove();
        }
    }

    fn show_failure(&self, message: &str) {
        self.busy.set(false);
        self.set_error(message);
        for surface in self.surfaces.borrow().iter() {
            surface.entry.set_sensitive(true);
            surface.entry.grab_focus();
            surface.unlock.set_sensitive(true);
            surface.unlock.set_label("Unlock");
        }
    }

    fn set_error(&self, message: &str) {
        for surface in self.surfaces.borrow().iter() {
            surface.error.set_text(message);
        }
    }

    fn start_clock(self: &Rc<Self>) {
        self.update_clock();
        if self.clock_source.borrow().is_some() {
            return;
        }

        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_seconds_local(5, move || {
            let Some(state) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !state.locked.get() {
                return glib::ControlFlow::Break;
            }
            state.update_clock();
            glib::ControlFlow::Continue
        });
        *self.clock_source.borrow_mut() = Some(source);
    }

    fn update_clock(&self) {
        self.prune_surfaces();
        let now = glib::DateTime::now_local().ok();
        let time = now
            .as_ref()
            .and_then(|date| date.format("%H:%M").ok())
            .map(|value| value.to_string())
            .unwrap_or_default();
        let day = now
            .as_ref()
            .and_then(|date| date.format("%A, %-d %B").ok())
            .map(|value| value.to_string())
            .unwrap_or_default();

        for surface in self.surfaces.borrow().iter() {
            surface.clock.set_text(&time);
            surface.date.set_text(&day);
        }
    }

    /// Drops surfaces whose monitor is gone. The library destroys the window
    /// on its own, so the stale entry would only keep the widgets alive.
    fn prune_surfaces(&self) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let monitors = display.monitors();
        self.surfaces.borrow_mut().retain(|surface| {
            (0..monitors.n_items()).any(|index| {
                monitors
                    .item(index)
                    .and_then(|object| object.downcast::<gdk::Monitor>().ok())
                    .is_some_and(|monitor| monitor == surface.monitor)
            })
        });
    }
}

fn build_surface(app: &gtk::Application, monitor: &gdk::Monitor) -> Surface {
    let window = gtk::ApplicationWindow::new(app);
    window.set_decorated(false);
    window.add_css_class("LockScreen");

    let background = gtk::Box::new(gtk::Orientation::Vertical, 0);
    background.add_css_class("LockBackground");
    background.set_hexpand(true);
    background.set_vexpand(true);

    let clock = gtk::Label::new(None);
    clock.add_css_class("LockClock");
    clock.set_xalign(0.5);

    let date = gtk::Label::new(None);
    date.add_css_class("LockDate");
    date.set_xalign(0.5);

    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("LockCard");
    card.set_halign(gtk::Align::Center);
    card.set_valign(gtk::Align::Center);

    let identity = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    identity.add_css_class("LockIdentity");
    identity.set_halign(gtk::Align::Center);

    let avatar_frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    avatar_frame.add_css_class("LockAvatarFrame");
    let avatar = gtk::Image::from_icon_name("avatar-default-symbolic");
    avatar.add_css_class("LockAvatar");
    avatar.set_pixel_size(26);
    avatar_frame.append(&avatar);
    identity.append(&avatar_frame);

    let names = gtk::Box::new(gtk::Orientation::Vertical, 1);
    let name = gtk::Label::new(Some(&user_name()));
    name.add_css_class("LockName");
    name.set_xalign(0.0);
    names.append(&name);
    let host = gtk::Label::new(Some(&format!("on {}", host_name())));
    host.add_css_class("LockHost");
    host.set_xalign(0.0);
    names.append(&host);
    identity.append(&names);

    card.append(&identity);

    let entry = gtk::PasswordEntry::new();
    entry.add_css_class("LockPassword");
    entry.set_show_peek_icon(true);
    entry.set_placeholder_text(Some("Password"));
    entry.set_width_chars(20);
    card.append(&entry);

    let error = gtk::Label::new(None);
    error.add_css_class("LockError");
    error.set_xalign(0.5);
    error.set_wrap(true);
    card.append(&error);

    let unlock = gtk::Button::with_label("Unlock");
    unlock.add_css_class("LockPrimary");
    card.append(&unlock);

    let top = gtk::Box::new(gtk::Orientation::Vertical, 0);
    top.set_vexpand(true);
    let bottom = gtk::Box::new(gtk::Orientation::Vertical, 0);
    bottom.set_vexpand(true);

    background.append(&top);
    background.append(&clock);
    background.append(&date);
    background.append(&card);
    background.append(&bottom);

    // Clicking empty space puts the caret back in the password field. Clicking
    // the button still runs its own handler.
    {
        let entry = entry.clone();
        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_, _, _, _| {
            entry.grab_focus();
        });
        background.add_controller(click);
    }

    window.set_child(Some(&background));

    Surface {
        monitor: monitor.clone(),
        window,
        entry,
        error,
        unlock,
        clock,
        date,
    }
}

fn user_name() -> String {
    let name = glib::user_name().to_string_lossy().into_owned();
    if name.is_empty() {
        "Session".to_string()
    } else {
        name
    }
}

fn host_name() -> String {
    let host = glib::host_name().to_string();
    if host.is_empty() {
        "this computer".to_string()
    } else {
        host
    }
}
