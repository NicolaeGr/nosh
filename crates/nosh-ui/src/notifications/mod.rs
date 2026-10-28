//! The notification overlay.
//!
//! Placement, the queue, and the card lifecycle follow
//! `docs/features/notifications.md` and the corner rule in
//! `docs/features/monitors.md`.
//!
//! The window is sized to the cards instead of the output. That keeps every
//! pixel that is not a card out of the window's input region, so the bar and
//! the applications underneath stay clickable. For top corners the window is
//! also anchored below the bar, which leaves the bar itself uncovered.

mod card;
mod history;

pub(crate) use card::{format_time, set_body_markup};
pub use history::NotificationHistory;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::{Rc, Weak};

use gtk::gdk;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nosh_core::{
    select_monitor, Core, Corner, MonitorLayout, Notification, NotificationModel,
    NotificationSettings,
};

use card::NotificationCard;

const NAMESPACE: &str = "nosh-notifications-outer";
const MIN_WIDTH: i32 = 360;
const SIDE_MARGIN: i32 = 28;
/// Inside the top edge margin, below the bar. The bar is about 40 logical
/// pixels tall, so this clears it and leaves a small gap.
const TOP_INSET: i32 = 44;
const BOTTOM_MARGIN: i32 = 8;

pub struct NotificationsWindow {
    inner: Rc<Inner>,
}

struct Inner {
    core: Rc<Core>,
    window: gtk::ApplicationWindow,
    stack: gtk::Box,
    settings: RefCell<NotificationSettings>,
    layout: RefCell<MonitorLayout>,
    visible: RefCell<Vec<u32>>,
    cards: RefCell<HashMap<u32, Rc<NotificationCard>>>,
    queued: RefCell<VecDeque<Notification>>,
    monitor: RefCell<Option<String>>,
    dnd: Cell<bool>,
    self_weak: RefCell<Weak<Inner>>,
}

impl NotificationsWindow {
    pub fn new(app: &gtk::Application, core: Rc<Core>) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.set_decorated(false);
        window.init_layer_shell();
        window.set_namespace(Some(NAMESPACE));
        window.set_layer(Layer::Overlay);
        window.set_exclusive_zone(-1);
        window.set_keyboard_mode(KeyboardMode::None);
        window.add_css_class("Notifications-outer");
        window.set_visible(false);

        let stack = gtk::Box::new(gtk::Orientation::Vertical, 4);
        stack.add_css_class("NotificationsContainer");
        stack.add_css_class("notifications-stack");
        stack.set_size_request(MIN_WIDTH, -1);
        window.set_child(Some(&stack));
        // Width is fixed by the contract. The height follows the cards so the
        // surface never covers the empty space below them.
        window.set_default_size(MIN_WIDTH, -1);

        let inner = Rc::new(Inner {
            core,
            window,
            stack,
            settings: RefCell::new(NotificationSettings::default()),
            layout: RefCell::new(MonitorLayout::default()),
            visible: RefCell::new(Vec::new()),
            cards: RefCell::new(HashMap::new()),
            queued: RefCell::new(VecDeque::new()),
            monitor: RefCell::new(None),
            dnd: Cell::new(false),
            self_weak: RefCell::new(Weak::new()),
        });
        *inner.self_weak.borrow_mut() = Rc::downgrade(&inner);
        inner.sync_placement();

        Self { inner }
    }

    /// Applies fresh state after startup or a backend reconnect.
    ///
    /// Every full snapshot carries the notification list, and other services
    /// emit one whenever their own state changes. Reconciling by id keeps
    /// those snapshots from replaying cards that are already on screen.
    pub fn apply_snapshot(&self, model: &NotificationModel) {
        *self.inner.settings.borrow_mut() = model.settings.clone();
        self.inner.dnd.set(model.settings.dont_disturb);

        let ids: HashSet<u32> = model
            .notifications
            .iter()
            .map(|notification| notification.id)
            .collect();
        self.inner.retain(&ids);

        if !model.settings.dont_disturb {
            for notification in &model.notifications {
                if !self.inner.is_known(notification.id) {
                    self.inner.enqueue(notification.clone());
                }
            }
        }
        self.inner.sync_placement();
    }

    pub fn set_layout(&self, layout: &MonitorLayout) {
        *self.inner.layout.borrow_mut() = layout.clone();
        self.inner.sync_placement();
    }

    pub fn received(&self, notification: &Notification, suppress_popup: bool) {
        if suppress_popup || self.inner.dnd.get() {
            return;
        }
        self.inner.enqueue(notification.clone());
    }

    pub fn closed(&self, id: u32) {
        let card = self.inner.cards.borrow().get(&id).cloned();
        match card {
            Some(card) => card.dismiss(),
            None => self.inner.queued.borrow_mut().retain(|item| item.id != id),
        }
    }

    pub fn set_dnd(&self, enabled: bool) {
        self.inner.dnd.set(enabled);
        if !enabled {
            self.inner.promote();
            self.inner.sync_visibility();
        }
    }
}

impl Inner {
    fn max_visible(&self) -> usize {
        self.settings.borrow().max_visible.max(1)
    }

    /// Drops cards and queued items that the snapshot no longer lists.
    fn retain(&self, ids: &HashSet<u32>) {
        let stale: Vec<u32> = self
            .cards
            .borrow()
            .keys()
            .copied()
            .filter(|id| !ids.contains(id))
            .collect();
        for id in stale {
            let card = self.cards.borrow().get(&id).cloned();
            if let Some(card) = card {
                card.dismiss();
            }
        }
        self.queued
            .borrow_mut()
            .retain(|item| ids.contains(&item.id));
    }

    fn is_known(&self, id: u32) -> bool {
        self.cards.borrow().contains_key(&id)
            || self.queued.borrow().iter().any(|item| item.id == id)
    }

    fn enqueue(&self, notification: Notification) {
        let id = notification.id;

        if self.cards.borrow().contains_key(&id) {
            self.replace_card(id, notification);
            return;
        }

        if let Some(slot) = self
            .queued
            .borrow_mut()
            .iter_mut()
            .find(|item| item.id == id)
        {
            *slot = notification;
            return;
        }

        if self.visible.borrow().len() < self.max_visible() {
            self.show_card(notification);
        } else {
            self.queued.borrow_mut().push_back(notification);
        }
    }

    fn show_card(&self, notification: Notification) {
        tracing::info!(
            id = notification.id,
            app = %notification.app_name,
            summary = %notification.summary,
            "notification card shown"
        );
        let card = self.build_card(notification.clone());
        card.widget().add_css_class("notification-card-appear");
        self.stack.append(card.widget());
        self.visible.borrow_mut().push(notification.id);
        self.cards.borrow_mut().insert(notification.id, card);
        self.sync_visibility();
    }

    /// A replacement keeps the card's position and does not replay the appear
    /// animation, per the UI contract.
    fn replace_card(&self, id: u32, notification: Notification) {
        let Some(old) = self.cards.borrow().get(&id).cloned() else {
            return;
        };
        let index = self
            .visible
            .borrow()
            .iter()
            .position(|item| *item == id)
            .unwrap_or(0);
        let previous = if index == 0 {
            None
        } else {
            let previous_id = self.visible.borrow()[index - 1];
            self.cards
                .borrow()
                .get(&previous_id)
                .map(|card| card.widget().clone())
        };

        let card = self.build_card(notification);
        self.stack.remove(old.widget());
        old.stop();
        match previous {
            Some(widget) => self.stack.insert_child_after(card.widget(), Some(&widget)),
            None => self.stack.prepend(card.widget()),
        }
        self.cards.borrow_mut().insert(id, card);
        self.sync_visibility();
    }

    fn build_card(&self, notification: Notification) -> Rc<NotificationCard> {
        let timeout = i64::from(self.settings.borrow().display_timeout);
        let weak = self.self_weak.borrow().clone();
        let on_done: Rc<dyn Fn(u32)> = Rc::new(move |id| {
            if let Some(inner) = weak.upgrade() {
                inner.finished(id);
            }
        });
        NotificationCard::new(&notification, timeout, Rc::clone(&self.core), on_done)
    }

    /// Removes a card after its exit animation and promotes the next queued
    /// notification.
    fn finished(&self, id: u32) {
        let Some(card) = self.cards.borrow_mut().remove(&id) else {
            return;
        };
        self.stack.remove(card.widget());
        self.visible.borrow_mut().retain(|item| *item != id);

        self.promote();
        self.sync_visibility();
    }

    /// Shows queued notifications while there is room. DND keeps the queue
    /// parked until it is turned off.
    fn promote(&self) {
        if self.dnd.get() {
            return;
        }
        loop {
            if self.visible.borrow().len() >= self.max_visible() {
                break;
            }
            let Some(next) = self.queued.borrow_mut().pop_front() else {
                break;
            };
            self.show_card(next);
        }
    }

    fn sync_visibility(&self) {
        if self.visible.borrow().is_empty() {
            self.stack.add_css_class("pointer-events-none");
            self.window.set_visible(false);
            return;
        }
        self.stack.remove_css_class("pointer-events-none");
        self.sync_placement();
        if !self.window.is_visible() {
            self.window.present();
        }
    }

    /// Moves the window to the monitor selected by the corner rule. The window
    /// is hidden before `set_monitor` and presented again, so the layer surface
    /// is recreated on the new output.
    fn sync_placement(&self) {
        apply_anchors(&self.window, self.settings.borrow().position);
        let Some((monitor, connector)) = self.target_monitor() else {
            return;
        };
        if self.monitor.borrow().as_deref() == Some(connector.as_str()) {
            return;
        }

        let was_visible = self.window.is_visible();
        self.window.set_visible(false);
        self.window.set_monitor(Some(&monitor));
        *self.monitor.borrow_mut() = Some(connector);
        if was_visible {
            self.window.present();
        }
    }

    fn target_monitor(&self) -> Option<(gdk::Monitor, String)> {
        let display = gdk::Display::default()?;
        let monitors = display.monitors();

        let (connector, geometry) = {
            let settings = self.settings.borrow();
            let layout = self.layout.borrow();
            match select_monitor(&layout, settings.position, &settings.monitor_policy) {
                Some(model) => (
                    Some(model.connector().to_string()),
                    Some((
                        model.x,
                        model.y,
                        model.logical_width(),
                        model.logical_height(),
                    )),
                ),
                None => (None, None),
            }
        };

        for index in 0..monitors.n_items() {
            let Some(monitor) = monitors.item(index).and_downcast::<gdk::Monitor>() else {
                continue;
            };
            let name = monitor.connector().map(|value| value.to_string());
            if name.as_deref() == connector.as_deref() {
                return Some((monitor, connector.clone().unwrap_or_default()));
            }
        }

        // Connector names are not always available. Fall back to the geometry
        // from the core layout, in logical pixels.
        if let Some((x, y, width, height)) = geometry {
            for index in 0..monitors.n_items() {
                let Some(monitor) = monitors.item(index).and_downcast::<gdk::Monitor>() else {
                    continue;
                };
                let rect = monitor.geometry();
                if (rect.x() - x).abs() <= 2
                    && (rect.y() - y).abs() <= 2
                    && (rect.width() as f64 - width).abs() <= 2.0
                    && (rect.height() as f64 - height).abs() <= 2.0
                {
                    let name = monitor
                        .connector()
                        .map(|value| value.to_string())
                        .unwrap_or_default();
                    return Some((monitor, name));
                }
            }
        }

        let monitor = monitors.item(0).and_downcast::<gdk::Monitor>()?;
        let name = monitor
            .connector()
            .map(|value| value.to_string())
            .unwrap_or_default();
        Some((monitor, name))
    }
}

fn apply_anchors(window: &gtk::ApplicationWindow, corner: Corner) {
    let (top, bottom, left, right) = match corner {
        Corner::TopLeft => (true, false, true, false),
        Corner::TopRight => (true, false, false, true),
        Corner::BottomLeft => (false, true, true, false),
        Corner::BottomRight => (false, true, false, true),
    };

    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, false);
    }
    if top {
        window.set_anchor(Edge::Top, true);
    }
    if bottom {
        window.set_anchor(Edge::Bottom, true);
    }
    if left {
        window.set_anchor(Edge::Left, true);
    }
    if right {
        window.set_anchor(Edge::Right, true);
    }

    window.set_margin(Edge::Top, if top { TOP_INSET } else { 0 });
    window.set_margin(Edge::Bottom, if bottom { BOTTOM_MARGIN } else { 0 });
    window.set_margin(Edge::Left, if left { SIDE_MARGIN } else { 0 });
    window.set_margin(Edge::Right, if right { SIDE_MARGIN } else { 0 });
}
