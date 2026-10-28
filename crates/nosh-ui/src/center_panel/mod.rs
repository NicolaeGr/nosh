//! The centre island panel: notifications, calendar, and media.
//!
//! The panel is a transparent overlay window with a single card. The card
//! starts at the measured centre island, grows to the panel size with an
//! overshoot, and shrinks back before the window is hidden. The clock label
//! stays pinned to the card's left edge and the media label to its right edge,
//! so they travel to opposite corners while the card grows.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use nosh_core::{Notification, Urgency};

use crate::notifications::{format_time, set_body_markup, NotificationHistory};

/// Panel size in logical pixels. The stylesheet uses a 12px root font. The
/// calendar tab is wider than the media tab.
const CALENDAR_WIDTH: i32 = 48 * 12;
const MEDIA_WIDTH: i32 = 32 * 12;
const PANEL_HEIGHT: i32 = 27 * 12;
const OPEN_SECONDS: f64 = 0.36;
const CLOSE_SECONDS: f64 = 0.24;
const TAB_SECONDS: f64 = 0.28;
/// Grace period before a hover-opened panel closes when the pointer leaves.
const HOVER_CLOSE_MS: u64 = 450;
/// Distance from the card's top corners to the clock and media labels.
const HEADER_INSET: i32 = 12;
/// Padding inside the header buttons, matching the stylesheet.
const HEADER_PAD_X: i32 = 3;
const HEADER_PAD_Y: i32 = 2;
/// Side of a calendar day cell, matching 1.75em at the 12px root font.
const DAY_CELL: i32 = 21;

const NOW_PLAYING_TITLE: &str = "Midnight City";
const NOW_PLAYING_ARTIST: &str = "M83";

/// The calendar column keeps one third of the panel content width. The panel
/// padding and divider take the rest.
const CALENDAR_COLUMN_WIDTH: i32 = 176;

/// Which tab the panel shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Calendar,
    Media,
}

/// One widget rectangle in monitor coordinates.
#[derive(Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The pieces of the centre island the panel animation starts from.
#[derive(Clone, Copy)]
pub struct Geometry {
    pub island: Rect,
    pub clock: Rect,
    pub media: Rect,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Closed,
    Opening,
    Open,
    Closing,
}

/// The collapsed margins, restored at the end of the close animation.
#[derive(Clone, Copy)]
struct Collapsed {
    width: i32,
    height: i32,
    clock_start: i32,
    clock_top: i32,
    media_end: i32,
    media_top: i32,
}

pub struct CenterPanelWindow {
    inner: Rc<Inner>,
}

struct Inner {
    window: gtk::ApplicationWindow,
    card: gtk::Overlay,
    viewport: gtk::ScrolledWindow,
    content: gtk::Box,
    clock: gtk::Button,
    clock_label: gtk::Label,
    media: gtk::Button,
    media_label: gtk::Label,
    stack: gtk::Stack,
    calendar: Rc<CalendarView>,
    notifications_view: Rc<NotificationsView>,
    state: Cell<State>,
    generation: Cell<u64>,
    collapsed: Cell<Collapsed>,
    hover_close: Cell<bool>,
    leave_timer: RefCell<Option<glib::SourceId>>,
    restore: RefCell<Option<Box<dyn Fn()>>>,
}

impl CenterPanelWindow {
    pub fn new(app: &gtk::Application, history: Rc<NotificationHistory>) -> Self {
        let window = gtk::ApplicationWindow::new(app);
        window.set_decorated(false);
        window.init_layer_shell();
        window.set_namespace(Some("nosh-center-panel"));
        window.set_layer(Layer::Overlay);
        window.set_exclusive_zone(-1);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.add_css_class("CenterPanel");
        window.set_visible(false);

        let background = gtk::Box::new(gtk::Orientation::Vertical, 0);
        background.add_css_class("CenterPanelBackground");
        background.set_hexpand(true);
        background.set_vexpand(true);

        let (clock_label, clock) = header_button(None);
        clock_label.set_text("");
        clock_label.add_css_class("CenterPanelClockLabel");
        clock.add_css_class("CenterPanelClock");
        let (media_label, media) = header_button(Some("audio-x-generic-symbolic"));
        media_label.set_text(NOW_PLAYING_TITLE);
        media_label.add_css_class("CenterPanelMediaLabel");
        media.add_css_class("CenterPanelMedia");

        clock.set_halign(gtk::Align::Start);
        clock.set_valign(gtk::Align::Start);
        clock.set_can_target(false);
        media.set_halign(gtk::Align::End);
        media.set_valign(gtk::Align::Start);
        media.set_can_target(false);

        let (content, stack, calendar, notifications_view) = build_content(history);
        content.set_can_target(false);

        let viewport = gtk::ScrolledWindow::new();
        viewport.add_css_class("CenterPanelViewport");
        // External policies keep the scrolled window's minimum size at zero,
        // so the card can start at the island size. The viewport still clips.
        viewport.set_policy(gtk::PolicyType::External, gtk::PolicyType::External);
        viewport.set_propagate_natural_width(false);
        viewport.set_propagate_natural_height(false);
        viewport.set_hexpand(true);
        viewport.set_vexpand(true);
        viewport.set_child(Some(&content));

        let card = gtk::Overlay::new();
        card.add_css_class("CenterPanelCard");
        card.set_halign(gtk::Align::Center);
        card.set_valign(gtk::Align::Start);
        card.set_hexpand(false);
        card.set_vexpand(false);
        card.set_size_request(1, 1);
        card.set_child(Some(&viewport));
        card.add_overlay(&clock);
        card.add_overlay(&media);

        background.append(&card);
        window.set_child(Some(&background));

        let inner = Rc::new(Inner {
            window,
            card,
            viewport,
            content,
            clock,
            clock_label,
            media,
            media_label,
            stack,
            calendar,
            notifications_view,
            state: Cell::new(State::Closed),
            generation: Cell::new(0),
            collapsed: Cell::new(Collapsed {
                width: 1,
                height: 1,
                clock_start: 0,
                clock_top: 0,
                media_end: 0,
                media_top: 0,
            }),
            hover_close: Cell::new(false),
            leave_timer: RefCell::new(None),
            restore: RefCell::new(None),
        });

        {
            let inner = Rc::clone(&inner);
            let clock = inner.clock.clone();
            clock.connect_clicked(move |_| show_tab(&inner, Tab::Calendar));
        }
        {
            let inner = Rc::clone(&inner);
            let media = inner.media.clone();
            media.connect_clicked(move |_| show_tab(&inner, Tab::Media));
        }

        let click = gtk::GestureClick::new();
        {
            let background = background.clone();
            let inner = Rc::clone(&inner);
            click.connect_pressed(move |_, _, x, y| {
                let target: gtk::Widget = background.clone().upcast();
                if target.pick(x, y, gtk::PickFlags::DEFAULT).as_ref() == Some(&target) {
                    close_window(&inner);
                }
            });
        }
        background.add_controller(click);

        let keys = gtk::EventControllerKey::new();
        {
            let inner = Rc::clone(&inner);
            keys.connect_key_pressed(move |_, key, _, _| {
                if key == gdk::Key::Escape {
                    close_window(&inner);
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
        }
        inner.window.add_controller(keys);

        let motion = gtk::EventControllerMotion::new();
        {
            let inner = Rc::clone(&inner);
            motion.connect_enter(move |_, _, _| cancel_leave(&inner));
        }
        {
            let inner = Rc::clone(&inner);
            motion.connect_leave(move |_| schedule_hover_close(&inner));
        }
        inner.card.add_controller(motion);

        let weak_clock = inner.clock_label.downgrade();
        glib::timeout_add_seconds_local(1, move || match weak_clock.upgrade() {
            Some(label) => {
                if label.parent().is_some() {
                    label.set_text(&crate::bar::time::now_text());
                    glib::ControlFlow::Continue
                } else {
                    glib::ControlFlow::Break
                }
            }
            None => glib::ControlFlow::Break,
        });

        Self { inner }
    }

    pub fn is_open(&self) -> bool {
        self.inner.state.get() != State::Closed
    }

    /// Rebuilds the notification list after the history changed.
    pub fn refresh_notifications(&self) {
        self.inner.notifications_view.refresh();
    }

    /// Opens the panel from `geometry`, the centre island in monitor coordinates.
    ///
    /// `restore` runs after the close animation has finished. The caller uses
    /// it to bring the real island back.
    pub fn open(
        &self,
        monitor: Option<&gdk::Monitor>,
        geometry: Geometry,
        clock_text: &str,
        tab: Tab,
        hover: bool,
        restore: impl Fn() + 'static,
    ) -> bool {
        if self.inner.state.get() != State::Closed {
            return false;
        }

        let island = geometry.island;
        let width = island.width.max(1);
        let height = island.height.max(1);
        let collapsed = Collapsed {
            width,
            height,
            clock_start: (geometry.clock.x - island.x - HEADER_PAD_X).max(0),
            clock_top: (geometry.clock.y - island.y - HEADER_PAD_Y).max(0),
            media_end: (island.x + island.width - geometry.media.x - geometry.media.width).max(0),
            media_top: (geometry.media.y - island.y - HEADER_PAD_Y).max(0),
        };
        self.inner.collapsed.set(collapsed);

        if let Some(monitor) = monitor {
            self.inner.window.set_monitor(Some(monitor));
        }

        self.inner.card.set_margin_top(island.y.max(0));
        self.inner.card.set_size_request(width, height);
        self.inner.clock.set_margin_start(collapsed.clock_start);
        self.inner.clock.set_margin_top(collapsed.clock_top);
        self.inner.media.set_margin_end(collapsed.media_end);
        self.inner.media.set_margin_top(collapsed.media_top);
        self.inner.clock_label.set_text(clock_text);
        self.inner.media_label.set_text(NOW_PLAYING_TITLE);
        self.inner.content.set_opacity(0.0);
        self.inner.content.set_can_target(false);
        self.inner.clock.set_can_target(false);
        self.inner.media.set_can_target(false);
        self.inner.clock.remove_css_class("is-active");
        self.inner.media.remove_css_class("is-active");
        self.inner.clock.add_css_class("is-open");
        self.inner.media.add_css_class("is-open");
        self.inner.stack.set_visible_child_name(tab_name(tab));
        self.inner.calendar.render();
        self.inner.notifications_view.refresh();
        *self.inner.restore.borrow_mut() = Some(Box::new(restore));

        self.inner.state.set(State::Opening);
        self.inner.hover_close.set(hover);
        let generation = self.inner.generation.get().wrapping_add(1);
        self.inner.generation.set(generation);

        self.inner.window.present();
        self.inner.viewport.grab_focus();
        animate_open(Rc::clone(&self.inner), width, height, tab, generation);
        true
    }
}

fn cancel_leave(inner: &Rc<Inner>) {
    if let Some(source) = inner.leave_timer.borrow_mut().take() {
        source.remove();
    }
}

/// Waits out a short grace period before closing, so a shrinking card does not
/// close the panel while the pointer travels to the other tab.
fn schedule_hover_close(inner: &Rc<Inner>) {
    if !inner.hover_close.get() || matches!(inner.state.get(), State::Closed | State::Closing) {
        return;
    }
    cancel_leave(inner);
    let weak = Rc::downgrade(inner);
    let source = glib::timeout_add_local(
        std::time::Duration::from_millis(HOVER_CLOSE_MS),
        move || {
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            *inner.leave_timer.borrow_mut() = None;
            if inner.hover_close.get()
                && !matches!(inner.state.get(), State::Closed | State::Closing)
            {
                close_window(&inner);
            }
            glib::ControlFlow::Break
        },
    );
    *inner.leave_timer.borrow_mut() = Some(source);
}

fn close_window(inner: &Rc<Inner>) {
    if matches!(inner.state.get(), State::Closed | State::Closing) {
        return;
    }

    let from_width = inner.card.width().max(1);
    let from_height = inner.card.height().max(1);
    let from_clock_start = inner.clock.margin_start();
    let from_clock_top = inner.clock.margin_top();
    let from_media_end = inner.media.margin_end();
    let from_media_top = inner.media.margin_top();
    let collapsed = inner.collapsed.get();

    inner.state.set(State::Closing);
    inner.content.set_can_target(false);
    inner.clock.set_can_target(false);
    inner.media.set_can_target(false);
    inner.clock.remove_css_class("is-active");
    inner.media.remove_css_class("is-active");
    inner.clock.remove_css_class("is-open");
    inner.media.remove_css_class("is-open");
    cancel_leave(inner);
    let generation = inner.generation.get().wrapping_add(1);
    inner.generation.set(generation);

    let start = Instant::now();
    let window = inner.window.clone();
    let inner = Rc::clone(inner);
    window.add_tick_callback(move |_, _| {
        if inner.generation.get() != generation {
            return glib::ControlFlow::Break;
        }

        let t = (start.elapsed().as_secs_f64() / CLOSE_SECONDS).clamp(0.0, 1.0);
        let eased = ease_in_cubic(t);
        inner.card.set_size_request(
            lerp(from_width, collapsed.width, eased),
            lerp(from_height, collapsed.height, eased),
        );
        inner
            .clock
            .set_margin_start(lerp(from_clock_start, collapsed.clock_start, eased));
        inner
            .clock
            .set_margin_top(lerp(from_clock_top, collapsed.clock_top, eased));
        inner
            .media
            .set_margin_end(lerp(from_media_end, collapsed.media_end, eased));
        inner
            .media
            .set_margin_top(lerp(from_media_top, collapsed.media_top, eased));

        let hidden = (t / 0.6).clamp(0.0, 1.0);
        inner.content.set_opacity(1.0 - hidden);

        if t >= 1.0 {
            inner.window.set_visible(false);
            inner.state.set(State::Closed);
            inner.hover_close.set(false);
            let restore = inner.restore.borrow_mut().take();
            if let Some(restore) = restore {
                restore();
            }
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

fn animate_open(inner: Rc<Inner>, from_width: i32, from_height: i32, tab: Tab, generation: u64) {
    let collapsed = inner.collapsed.get();
    let start = Instant::now();
    let window = inner.window.clone();
    window.add_tick_callback(move |_, _| {
        if inner.generation.get() != generation {
            return glib::ControlFlow::Break;
        }

        let t = (start.elapsed().as_secs_f64() / OPEN_SECONDS).clamp(0.0, 1.0);
        let eased = ease_out_back(t);
        let (to_width, to_height) = panel_size(tab);
        inner.card.set_size_request(
            lerp(from_width, to_width, eased),
            lerp(from_height, to_height, eased),
        );
        inner
            .clock
            .set_margin_start(lerp(collapsed.clock_start, HEADER_INSET, eased));
        inner
            .clock
            .set_margin_top(lerp(collapsed.clock_top, HEADER_INSET, eased));
        inner
            .media
            .set_margin_end(lerp(collapsed.media_end, HEADER_INSET, eased));
        inner
            .media
            .set_margin_top(lerp(collapsed.media_top, HEADER_INSET, eased));

        let revealed = ((t - 0.42) / 0.58).clamp(0.0, 1.0);
        inner.content.set_opacity(revealed);

        if t >= 1.0 {
            let (to_width, to_height) = panel_size(tab);
            inner.card.set_size_request(to_width, to_height);
            inner.clock.set_margin_start(HEADER_INSET);
            inner.clock.set_margin_top(HEADER_INSET);
            inner.media.set_margin_end(HEADER_INSET);
            inner.media.set_margin_top(HEADER_INSET);
            inner.content.set_opacity(1.0);
            inner.content.set_can_target(true);
            inner.clock.set_can_target(true);
            inner.media.set_can_target(true);
            inner.state.set(State::Open);
            show_tab(&inner, tab);
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

fn show_tab(inner: &Rc<Inner>, tab: Tab) {
    if inner.state.get() != State::Open {
        return;
    }
    let name = tab_name(tab);
    let changed = inner.stack.visible_child_name().as_deref() != Some(name);
    inner.stack.set_visible_child_name(name);
    for (button, active) in [
        (&inner.clock, tab == Tab::Calendar),
        (&inner.media, tab == Tab::Media),
    ] {
        if active {
            button.add_css_class("is-active");
        } else {
            button.remove_css_class("is-active");
        }
    }

    if !changed {
        return;
    }

    let (to_width, to_height) = panel_size(tab);
    let from_width = inner.card.width().max(1);
    let from_height = inner.card.height().max(1);
    if from_width == to_width && from_height == to_height {
        return;
    }

    let generation = inner.generation.get().wrapping_add(1);
    inner.generation.set(generation);
    animate_resize(
        Rc::clone(inner),
        from_width,
        from_height,
        to_width,
        to_height,
        generation,
    );
}

fn panel_size(tab: Tab) -> (i32, i32) {
    match tab {
        Tab::Calendar => (CALENDAR_WIDTH, PANEL_HEIGHT),
        Tab::Media => (MEDIA_WIDTH, PANEL_HEIGHT),
    }
}

fn animate_resize(
    inner: Rc<Inner>,
    from_width: i32,
    from_height: i32,
    to_width: i32,
    to_height: i32,
    generation: u64,
) {
    let start = Instant::now();
    let window = inner.window.clone();
    window.add_tick_callback(move |_, _| {
        if inner.generation.get() != generation {
            return glib::ControlFlow::Break;
        }

        let t = (start.elapsed().as_secs_f64() / TAB_SECONDS).clamp(0.0, 1.0);
        let eased = ease_out_back(t);
        inner.card.set_size_request(
            lerp(from_width, to_width, eased),
            lerp(from_height, to_height, eased),
        );

        if t >= 1.0 {
            inner.card.set_size_request(to_width, to_height);
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

fn tab_name(tab: Tab) -> &'static str {
    match tab {
        Tab::Calendar => "calendar",
        Tab::Media => "media",
    }
}

/// Builds one of the two clickable labels that sit above the panel content.
fn header_button(icon_name: Option<&str>) -> (gtk::Label, gtk::Button) {
    let button = gtk::Button::new();
    button.add_css_class("CenterPanelHeaderButton");
    button.set_focus_on_click(false);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    if let Some(icon_name) = icon_name {
        let icon = gtk::Image::from_icon_name(icon_name);
        icon.add_css_class("CenterPanelMediaIcon");
        row.append(&icon);
    }
    let label = gtk::Label::new(None);
    row.append(&label);
    button.set_child(Some(&row));
    (label, button)
}

fn lerp(from: i32, to: i32, progress: f64) -> i32 {
    (from as f64 + (to - from) as f64 * progress)
        .round()
        .max(0.0) as i32
}

fn ease_out_back(t: f64) -> f64 {
    const C1: f64 = 1.70158;
    const C3: f64 = C1 + 1.0;
    1.0 + C3 * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2)
}

fn ease_in_cubic(t: f64) -> f64 {
    t * t * t
}

fn build_content(
    history: Rc<NotificationHistory>,
) -> (
    gtk::Box,
    gtk::Stack,
    Rc<CalendarView>,
    Rc<NotificationsView>,
) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("CenterPanelContent");
    content.set_hexpand(true);
    content.set_vexpand(true);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.add_css_class("CenterPanelDateRow");
    let date = gtk::Label::new(Some(&today_text()));
    date.add_css_class("CenterPanelDate");
    date.set_xalign(0.0);
    date.set_hexpand(true);
    header.append(&date);
    content.append(&header);

    let calendar = CalendarView::new();
    let notifications_view = NotificationsView::new(history);

    let stack = gtk::Stack::new();
    stack.add_css_class("CenterPanelStack");
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(180);
    stack.set_vexpand(true);
    stack.add_named(
        &build_calendar_page(&notifications_view, &calendar),
        Some("calendar"),
    );
    stack.add_named(&build_media_page(), Some("media"));
    content.append(&stack);

    (content, stack, calendar, notifications_view)
}

fn build_calendar_page(
    notifications: &Rc<NotificationsView>,
    calendar: &Rc<CalendarView>,
) -> gtk::Box {
    let columns = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    columns.add_css_class("CenterPanelColumns");
    columns.set_vexpand(true);
    notifications.widget().set_hexpand(true);
    columns.append(notifications.widget());

    let divider = gtk::Box::new(gtk::Orientation::Vertical, 0);
    divider.add_css_class("CenterPanelDivider");
    divider.set_size_request(1, -1);
    divider.set_vexpand(true);
    columns.append(&divider);

    calendar.widget().set_hexpand(false);
    calendar
        .widget()
        .set_size_request(CALENDAR_COLUMN_WIDTH, -1);
    columns.append(calendar.widget());
    columns
}

/// The notifications column: a count, a scrolling list grouped by urgency,
/// and a clear-all action.
struct NotificationsView {
    root: gtk::Box,
    count: gtk::Label,
    list: gtk::Box,
    clear: gtk::Button,
    history: Rc<NotificationHistory>,
}

impl NotificationsView {
    fn new(history: Rc<NotificationHistory>) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("CenterPanelNotifications");
        root.set_hexpand(true);
        root.set_vexpand(true);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = section_title("Notifications");
        title.set_hexpand(true);
        let count = gtk::Label::new(Some("0"));
        count.add_css_class("CenterPanelSectionCount");
        count.set_valign(gtk::Align::Center);
        header.append(&title);
        header.append(&count);
        root.append(&header);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
        list.add_css_class("CenterPanelNotificationList");
        list.set_vexpand(true);

        let scroller = gtk::ScrolledWindow::new();
        scroller.add_css_class("CenterPanelNotificationScroll");
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_propagate_natural_height(false);
        scroller.set_vexpand(true);
        scroller.set_child(Some(&list));
        root.append(&scroller);

        let clear = gtk::Button::with_label("Clear all");
        clear.add_css_class("CenterPanelClearAll");
        clear.set_focus_on_click(false);
        clear.set_halign(gtk::Align::End);
        root.append(&clear);

        let view = Rc::new(Self {
            root,
            count,
            list,
            clear,
            history,
        });
        let weak = Rc::downgrade(&view);
        view.clear.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.history.clear();
                view.refresh();
            }
        });
        view.refresh();
        view
    }

    fn widget(&self) -> &gtk::Box {
        &self.root
    }

    fn refresh(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let entries = self.history.entries();
        let count = self.history.len();
        self.count.set_text(&count.to_string());
        self.clear.set_sensitive(count > 0);

        if entries.is_empty() {
            let empty = gtk::Label::new(Some("No notifications"));
            empty.add_css_class("CenterPanelEmpty");
            empty.set_xalign(0.0);
            self.list.append(&empty);
            return;
        }

        for urgency in [Urgency::Critical, Urgency::Normal, Urgency::Low] {
            let group: Vec<&Notification> = entries
                .iter()
                .filter(|notification| notification.urgency() == urgency)
                .collect();
            if group.is_empty() {
                continue;
            }
            let heading = gtk::Label::new(Some(urgency_title(urgency)));
            heading.add_css_class("CenterPanelGroupTitle");
            heading.set_xalign(0.0);
            self.list.append(&heading);
            for notification in group {
                self.list
                    .append(&notification_row(notification, Rc::downgrade(self)));
            }
        }
    }
}

fn urgency_title(urgency: Urgency) -> &'static str {
    match urgency {
        Urgency::Critical => "Critical",
        Urgency::Normal => "Normal",
        Urgency::Low => "Low",
    }
}

fn notification_icon(notification: &Notification) -> gtk::Image {
    if let Some(path) = notification.image_path() {
        if std::path::Path::new(&path).is_file() {
            let image = gtk::Image::from_file(&path);
            image.set_pixel_size(12);
            return image;
        }
    }

    let app_icon = notification.app_icon.as_str();
    if !app_icon.is_empty() {
        let path = std::path::Path::new(app_icon);
        if path.is_absolute() && path.is_file() {
            let image = gtk::Image::from_file(path);
            image.set_pixel_size(12);
            return image;
        }
        return gtk::Image::from_icon_name(app_icon);
    }

    gtk::Image::from_icon_name("dialog-information-symbolic")
}

fn notification_row(
    notification: &Notification,
    view: std::rc::Weak<NotificationsView>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 3);
    row.add_css_class("CenterPanelNotification");
    match notification.urgency() {
        Urgency::Critical => row.add_css_class("critical"),
        Urgency::Low => row.add_css_class("low"),
        Urgency::Normal => {}
    }

    // The icon shares the first line with the sender, so the title and body
    // keep the full width of the card.
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let icon = notification_icon(notification);
    icon.add_css_class("CenterPanelNotificationIcon");
    icon.set_valign(gtk::Align::Center);

    let app = if notification.app_name.is_empty() {
        "Notification"
    } else {
        notification.app_name.as_str()
    };
    let app_label = gtk::Label::new(Some(app));
    app_label.add_css_class("CenterPanelNotificationApp");
    app_label.set_xalign(0.0);
    app_label.set_hexpand(true);
    app_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    app_label.set_lines(1);

    let time_label = gtk::Label::new(Some(&format_time(notification.time)));
    time_label.add_css_class("CenterPanelNotificationTime");

    let close = gtk::Button::new();
    close.add_css_class("CenterPanelNotificationClose");
    close.set_focus_on_click(false);
    close.set_tooltip_text(Some("Dismiss"));
    close.set_valign(gtk::Align::Center);
    let close_icon = gtk::Image::from_icon_name("window-close-symbolic");
    close.set_child(Some(&close_icon));
    let id = notification.id;
    close.connect_clicked(move |_| {
        if let Some(view) = view.upgrade() {
            view.history.remove(id);
            view.refresh();
        }
    });

    header.append(&icon);
    header.append(&app_label);
    header.append(&time_label);
    header.append(&close);
    row.append(&header);

    let title_label = gtk::Label::new(Some(&notification.summary));
    title_label.add_css_class("CenterPanelNotificationTitle");
    title_label.set_xalign(0.0);
    title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title_label.set_lines(1);
    row.append(&title_label);

    if !notification.body.is_empty() {
        let body_label = gtk::Label::new(None);
        body_label.add_css_class("CenterPanelNotificationBody");
        body_label.set_xalign(0.0);
        body_label.set_wrap(true);
        body_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        body_label.set_lines(2);
        body_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        set_body_markup(&body_label, &notification.body);
        row.append(&body_label);
    }

    row
}

/// The calendar side: a working month switcher, the day grid, and the events
/// placeholder.
struct CalendarView {
    root: gtk::Box,
    month_label: gtk::Label,
    grid_slot: gtk::Box,
    offset: Cell<i32>,
}

impl CalendarView {
    fn new() -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 5);
        root.add_css_class("CalendarSide");
        root.set_vexpand(true);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let previous = nav_button("go-previous-symbolic", "Previous month");
        let next = nav_button("go-next-symbolic", "Next month");
        let month_label = gtk::Label::new(None);
        month_label.add_css_class("CalendarMonth");
        month_label.set_hexpand(true);
        month_label.set_halign(gtk::Align::Center);
        month_label.set_can_target(true);
        header.append(&previous);
        header.append(&month_label);
        header.append(&next);
        root.append(&header);

        root.append(&weekday_header());

        let grid_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&grid_slot);

        let events_title = section_title("Events");
        events_title.set_margin_top(4);
        root.append(&events_title);

        let empty = gtk::Label::new(Some("Not Available"));
        empty.add_css_class("CalendarEventEmpty");
        empty.set_xalign(0.0);
        root.append(&empty);

        let month_click = month_label.clone();
        let view = Rc::new(Self {
            root,
            month_label,
            grid_slot,
            offset: Cell::new(0),
        });
        {
            let weak = Rc::downgrade(&view);
            let double = gtk::GestureClick::new();
            double.connect_pressed(move |_, presses, _, _| {
                if presses != 2 {
                    return;
                }
                if let Some(view) = weak.upgrade() {
                    view.offset.set(0);
                    view.render();
                }
            });
            month_click.add_controller(double);
        }
        {
            let weak = Rc::downgrade(&view);
            previous.connect_clicked(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.shift(-1);
                }
            });
        }
        {
            let weak = Rc::downgrade(&view);
            next.connect_clicked(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.shift(1);
                }
            });
        }
        view.render();
        view
    }

    fn widget(&self) -> &gtk::Box {
        &self.root
    }

    fn shift(&self, delta: i32) {
        self.offset.set(self.offset.get() + delta);
        self.render();
    }

    fn render(&self) {
        let month = Month::for_offset(self.offset.get());
        self.month_label.set_text(&month.label);
        while let Some(child) = self.grid_slot.first_child() {
            self.grid_slot.remove(&child);
        }
        self.grid_slot.append(&day_grid(&month));
    }
}

fn weekday_header() -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 1);
    row.add_css_class("CalendarWeekdays");
    row.set_halign(gtk::Align::Center);
    row.set_homogeneous(true);
    // Monday first, matching the day grid.
    for day in ["M", "T", "W", "T", "F", "S", "S"] {
        let label = gtk::Label::new(Some(day));
        label.add_css_class("CalendarWeekday");
        label.set_size_request(DAY_CELL, -1);
        row.append(&label);
    }
    row
}

fn day_grid(month: &Month) -> gtk::Grid {
    let grid = gtk::Grid::new();
    grid.add_css_class("CalendarGrid");
    grid.set_halign(gtk::Align::Center);
    grid.set_column_homogeneous(true);
    grid.set_row_homogeneous(true);
    grid.set_column_spacing(1);
    grid.set_row_spacing(1);

    let mut position = 0;
    for _ in 0..month.leading {
        grid.attach(&day_cell("", None), position % 7, position / 7, 1, 1);
        position += 1;
    }
    for day in 1..=month.days {
        let today = (month.today == Some(day)).then_some("today");
        grid.attach(
            &day_cell(&day.to_string(), today),
            position % 7,
            position / 7,
            1,
            1,
        );
        position += 1;
    }
    while position % 7 != 0 {
        grid.attach(&day_cell("", None), position % 7, position / 7, 1, 1);
        position += 1;
    }

    grid
}

fn day_cell(text: &str, extra: Option<&str>) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("CalendarDay");
    label.set_size_request(DAY_CELL, DAY_CELL);
    label.set_halign(gtk::Align::Center);
    label.set_valign(gtk::Align::Center);
    if let Some(extra) = extra {
        label.add_css_class(extra);
    }
    label
}

fn nav_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("CalendarNav");
    button.set_focus_on_click(false);
    button.set_tooltip_text(Some(tooltip));
    button.set_child(Some(&gtk::Image::from_icon_name(icon_name)));
    button
}

fn section_title(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("CenterPanelSectionTitle");
    label.set_xalign(0.0);
    label
}

fn today_text() -> String {
    glib::DateTime::now_local()
        .and_then(|now| now.format("%A, %B %e"))
        .map(|text| text.to_string())
        .unwrap_or_else(|_| "Today".to_string())
}

fn build_media_page() -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("MediaPage");

    let sheet = gtk::Box::new(gtk::Orientation::Vertical, 12);
    sheet.add_css_class("MediaSheet");
    sheet.set_halign(gtk::Align::Center);
    sheet.set_valign(gtk::Align::Center);

    let art = gtk::Box::new(gtk::Orientation::Vertical, 0);
    art.add_css_class("MediaArt");
    art.set_halign(gtk::Align::Center);
    art.set_valign(gtk::Align::Center);
    let art_icon = gtk::Image::from_icon_name("audio-x-generic-symbolic");
    art_icon.add_css_class("MediaArtIcon");
    art.append(&art_icon);
    sheet.append(&art);

    let info = gtk::Box::new(gtk::Orientation::Vertical, 2);
    info.set_halign(gtk::Align::Center);
    let title = gtk::Label::new(Some(NOW_PLAYING_TITLE));
    title.add_css_class("MediaTitle");
    let artist = gtk::Label::new(Some(NOW_PLAYING_ARTIST));
    artist.add_css_class("MediaArtist");
    info.append(&title);
    info.append(&artist);
    sheet.append(&info);

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    controls.add_css_class("MediaControls");
    controls.set_halign(gtk::Align::Center);
    controls.append(&media_button("media-skip-backward-symbolic", false));
    controls.append(&media_button("media-playback-start-symbolic", true));
    controls.append(&media_button("media-skip-forward-symbolic", false));
    sheet.append(&controls);

    let seek = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    seek.add_css_class("MediaSeek");
    seek.append(&media_time("1:24"));
    let progress = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    progress.add_css_class("MediaSlider");
    progress.set_draw_value(false);
    progress.set_value(36.0);
    progress.set_hexpand(true);
    seek.append(&progress);
    seek.append(&media_time("3:58"));
    sheet.append(&seek);

    let volume = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    volume.add_css_class("MediaVolume");
    let volume_icon = gtk::Image::from_icon_name("audio-volume-high-symbolic");
    volume_icon.add_css_class("MediaVolumeIcon");
    volume.append(&volume_icon);
    let level = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    level.add_css_class("MediaSlider");
    level.set_draw_value(false);
    level.set_value(42.0);
    level.set_hexpand(true);
    volume.append(&level);
    let value = gtk::Label::new(Some("42%"));
    value.add_css_class("MediaVolumeValue");
    value.set_width_chars(3);
    volume.append(&value);
    sheet.append(&volume);

    page.append(&sheet);
    page
}

fn media_button(icon_name: &str, primary: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("MediaControl");
    if primary {
        button.add_css_class("MediaPlay");
    }
    button.set_focus_on_click(false);
    button.set_child(Some(&gtk::Image::from_icon_name(icon_name)));
    button
}

fn media_time(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("MediaTime");
    label
}

struct Month {
    label: String,
    leading: i32,
    days: i32,
    today: Option<i32>,
}

impl Month {
    fn for_offset(offset: i32) -> Self {
        let Ok(now) = glib::DateTime::now_local() else {
            return Self::fallback(offset);
        };
        let today = now.day_of_month();
        let Some(first) = now.add_days(1 - today).ok() else {
            return Self::fallback(offset);
        };
        let Some(month) = first.add_months(offset).ok() else {
            return Self::fallback(offset);
        };

        let leading = month.day_of_week() - 1;
        let days = month
            .add_months(1)
            .ok()
            .and_then(|date| date.add_days(-1).ok())
            .map(|date| date.day_of_month())
            .unwrap_or(30);
        let label = month
            .format("%B %Y")
            .map(|text| text.to_string())
            .unwrap_or_else(|_| "This month".to_string());

        Self {
            label,
            leading,
            days,
            today: (offset == 0).then_some(today),
        }
    }

    fn fallback(offset: i32) -> Self {
        let (label, leading, days, today) = match offset {
            0 => ("October 2026", 3, 31, Some(24)),
            -1 => ("September 2026", 1, 30, None),
            1 => ("November 2026", 6, 30, None),
            _ => ("Month", 0, 30, None),
        };
        Self {
            label: label.to_string(),
            leading,
            days,
            today,
        }
    }
}
