//! One notification card.
//!
//! The card owns its display timer and the dismissal animation. It sends
//! `NotificationDismiss` and `NotificationInvoke` and never touches D-Bus, as
//! required by `docs/features/notifications.md`.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{Command, Core, Notification, Urgency};

const TICK: Duration = Duration::from_millis(100);
const DISMISS: Duration = Duration::from_millis(250);
const FALLBACK_ICONS: [&str; 3] = [
    "preferences-system-notifications-symbolic",
    "notification-symbolic",
    "dialog-information-symbolic",
];
const ICON_PIXELS: i32 = 24;
const INTERNAL_ACTIONS: [&str; 2] = ["dismiss", "expire"];

pub struct NotificationCard {
    root: gtk::Box,
    revealer: gtk::Revealer,
    id: u32,
    core: Rc<Core>,
    on_done: Rc<dyn Fn(u32)>,
    dismissal: RefCell<Option<glib::SourceId>>,
    dismissed: Cell<bool>,
    hovered: Cell<bool>,
    remaining: Cell<i64>,
    last_tick: Cell<i64>,
    /// Set when the card should not expire on its own.
    persistent: bool,
}

impl NotificationCard {
    /// Builds the card and starts its display timer when `timeout_ms` is
    /// positive. Call back into [`Self::stop`] when the card is removed early.
    pub fn new(
        notification: &Notification,
        timeout_ms: i64,
        core: Rc<Core>,
        on_done: Rc<dyn Fn(u32)>,
    ) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.add_css_class("NotificationCard");
        for class in ["p-2", "gap-1", "rounded-2xl"] {
            root.add_css_class(class);
        }
        match notification.urgency() {
            Urgency::Critical => root.add_css_class("critical"),
            Urgency::Low => root.add_css_class("low"),
            Urgency::Normal => {}
        }

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.add_css_class("header");
        header.add_css_class("gap-2");

        let (icon, fallback) = build_icon(notification);
        icon.add_css_class("AppIcon");
        if fallback {
            icon.add_css_class("FallbackIcon");
        }
        header.append(&icon);

        let title = gtk::Label::new(Some(&notification.summary));
        title.add_css_class("heading");
        title.add_css_class("Title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.set_single_line_mode(true);
        title.set_max_width_chars(32);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        header.append(&title);

        let time = gtk::Label::new(Some(&format_time(notification.time)));
        time.add_css_class("heading");
        time.add_css_class("Time");
        time.set_xalign(1.0);
        header.append(&time);

        let expand = gtk::Button::new();
        expand.add_css_class("square-icon");
        expand.add_css_class("min-w-0");
        expand.add_css_class("min-h-0");
        expand.add_css_class("ExpandButton");
        expand.set_focus_on_click(false);
        expand.set_child(Some(&gtk::Image::from_icon_name("pan-up-symbolic")));
        expand.set_tooltip_text(Some("Collapse"));
        expand.set_visible(!notification.body.is_empty());
        header.append(&expand);

        let close = gtk::Button::new();
        close.add_css_class("square-icon");
        close.add_css_class("min-w-0");
        close.add_css_class("min-h-0");
        close.add_css_class("CloseButton");
        close.set_focus_on_click(false);
        close.set_child(Some(&gtk::Image::from_icon_name("window-close-symbolic")));
        close.set_tooltip_text(Some("Dismiss"));
        header.append(&close);

        root.append(&header);

        let body = gtk::Label::new(None);
        body.add_css_class("Body");
        body.set_wrap(true);
        body.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        body.set_xalign(0.0);
        body.set_max_width_chars(40);
        body.set_lines(3);
        body.set_ellipsize(gtk::pango::EllipsizeMode::End);
        set_body_markup(&body, &notification.body);

        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        revealer.set_transition_duration(200);
        revealer.set_child(Some(&body));
        revealer.set_reveal_child(!notification.body.is_empty());
        root.append(&revealer);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        actions.add_css_class("Actions");
        actions.add_css_class("gap-1");
        actions.add_css_class("mt-2");
        actions.set_homogeneous(true);
        for action in notification
            .actions
            .iter()
            .filter(|action| !INTERNAL_ACTIONS.contains(&action.id.as_str()))
        {
            let button = gtk::Button::with_label(&action.label);
            button.add_css_class("TextButton");
            button.set_focus_on_click(false);
            let core = Rc::clone(&core);
            let id = notification.id;
            let action_id = action.id.clone();
            button.connect_clicked(move |_| {
                core.send(Command::NotificationInvoke {
                    id,
                    action: action_id.clone(),
                });
            });
            actions.append(&button);
        }
        actions.set_visible(actions.first_child().is_some());
        root.append(&actions);

        let card = Rc::new_cyclic(|weak: &Weak<NotificationCard>| {
            let motion = gtk::EventControllerMotion::new();
            {
                let weak = weak.clone();
                motion.connect_enter(move |_, _, _| {
                    if let Some(card) = weak.upgrade() {
                        card.set_hovered(true);
                    }
                });
            }
            {
                let weak = weak.clone();
                motion.connect_leave(move |_| {
                    if let Some(card) = weak.upgrade() {
                        card.set_hovered(false);
                    }
                });
            }
            root.add_controller(motion);

            {
                let weak = weak.clone();
                expand.connect_clicked(move |button| {
                    let Some(card) = weak.upgrade() else {
                        return;
                    };
                    let revealed = !card.revealer.reveals_child();
                    card.revealer.set_reveal_child(revealed);
                    button.set_child(Some(&gtk::Image::from_icon_name(if revealed {
                        "pan-up-symbolic"
                    } else {
                        "pan-down-symbolic"
                    })));
                    button.set_tooltip_text(Some(if revealed { "Collapse" } else { "Expand" }));
                });
            }
            {
                let weak = weak.clone();
                close.connect_clicked(move |_| {
                    if let Some(card) = weak.upgrade() {
                        card.close();
                    }
                });
            }

            Self {
                root,
                revealer,
                id: notification.id,
                core: Rc::clone(&core),
                on_done,
                dismissal: RefCell::new(None),
                dismissed: Cell::new(false),
                hovered: Cell::new(false),
                remaining: Cell::new(0),
                last_tick: Cell::new(0),
                persistent: notification.urgency() == Urgency::Critical || timeout_ms <= 0,
            }
        });

        if !card.persistent {
            card.start_timer(timeout_ms, &card);
        }

        card
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    /// Sends the dismiss command, then plays the exit animation.
    pub fn close(&self) {
        self.core.send(Command::NotificationDismiss(self.id));
        self.dismiss();
    }

    /// Sends the expire command, then plays the exit animation. Used when the
    /// display timer runs out instead of a user action.
    pub fn expire(&self) {
        self.core.send(Command::NotificationExpire(self.id));
        self.dismiss();
    }

    /// Plays the exit animation without sending a command. Used when the core
    /// reports that the notification is already gone.
    pub fn dismiss(&self) {
        if self.dismissed.replace(true) {
            return;
        }
        self.root.add_css_class("notification-card-dismiss");

        let id = self.id;
        let on_done = Rc::clone(&self.on_done);
        let source = glib::timeout_add_local_once(DISMISS, move || on_done(id));
        *self.dismissal.borrow_mut() = Some(source);
    }

    /// Stops the timer and cancels a pending removal without animating. Called
    /// when the overlay drops the card for another reason.
    pub fn stop(&self) {
        self.dismissed.set(true);
        if let Some(source) = self.dismissal.borrow_mut().take() {
            source.remove();
        }
    }

    fn start_timer(&self, timeout_ms: i64, rc: &Rc<Self>) {
        self.remaining.set(timeout_ms * 1000);
        self.last_tick.set(glib::monotonic_time());

        let weak = Rc::downgrade(rc);
        glib::timeout_add_local(TICK, move || {
            let Some(card) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if card.dismissed.get() {
                return glib::ControlFlow::Break;
            }

            let now = glib::monotonic_time();
            let elapsed = now.saturating_sub(card.last_tick.get());
            card.last_tick.set(now);

            if !card.hovered.get() {
                let remaining = card.remaining.get() - elapsed;
                card.remaining.set(remaining);
                if remaining <= 0 {
                    card.expire();
                    return glib::ControlFlow::Break;
                }
            }

            glib::ControlFlow::Continue
        });
    }

    fn set_hovered(&self, hovered: bool) {
        let now = glib::monotonic_time();
        if hovered && !self.hovered.get() {
            let elapsed = now.saturating_sub(self.last_tick.get());
            self.last_tick.set(now);
            if !self.persistent {
                self.remaining
                    .set(self.remaining.get().saturating_sub(elapsed));
            }
        } else if !hovered && self.hovered.get() {
            self.last_tick.set(now);
        }
        self.hovered.set(hovered);
    }
}

pub(crate) fn set_body_markup(label: &gtk::Label, body: &str) {
    if body.is_empty() {
        label.set_text("");
        return;
    }
    let repaired = repair_markup(body);
    tracing::debug!(
        body = %truncate(body, 256),
        repaired = repaired != body,
        "notification body"
    );
    if gtk::pango::parse_markup(&repaired, '\0').is_ok() {
        label.set_markup(&repaired);
    } else {
        label.set_markup(&glib::markup_escape_text(body));
    }
}

fn truncate(value: &str, limit: usize) -> &str {
    match value.char_indices().nth(limit) {
        Some((index, _)) => &value[..index],
        None => value,
    }
}

/// Converts the HTML that some applications send into Pango markup and repairs
/// unescaped characters. Telegram, for example, sends `<br>`, bare `&` in
/// links, and tags Pango does not know.
fn repair_markup(body: &str) -> String {
    let mut out = body.to_string();
    for (from, to) in [
        ("<br/>", "\n"),
        ("<br />", "\n"),
        ("<br>", "\n"),
        ("<strong>", "<b>"),
        ("</strong>", "</b>"),
        ("<em>", "<i>"),
        ("</em>", "</i>"),
        ("<code>", "<tt>"),
        ("</code>", "</tt>"),
        ("<pre>", "<tt>"),
        ("</pre>", "</tt>"),
        ("&nbsp;", " "),
    ] {
        out = replace_ignore_case(&out, from, to);
    }
    for tag in ["tg-spoiler", "blockquote", "tg-emoji"] {
        out = strip_tag(&out, tag);
    }
    escape_bare_ampersands(&escape_bare_angles(&out))
}

fn replace_ignore_case(input: &str, from: &str, to: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let needle = from.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(offset) = lower[cursor..].find(&needle) {
        let start = cursor + offset;
        out.push_str(&input[cursor..start]);
        out.push_str(to);
        cursor = start + needle.len();
    }
    out.push_str(&input[cursor..]);
    out
}

/// Drops an HTML tag pair but keeps the text between them.
fn strip_tag(input: &str, name: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(offset) = lower[cursor..].find(&open) {
        let start = cursor + offset;
        out.push_str(&input[cursor..start]);
        let Some(end_offset) = lower[start..].find('>') else {
            out.push_str(&input[start..]);
            return out;
        };
        let end = start + end_offset + 1;
        match lower[end..].find(&close) {
            Some(close_offset) => {
                out.push_str(&input[end..end + close_offset]);
                cursor = end + close_offset + close.len();
            }
            None => cursor = end,
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn escape_bare_ampersands(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let after = &rest[index + 1..];
        let known = ["amp;", "lt;", "gt;", "quot;", "apos;"]
            .iter()
            .any(|entity| after.starts_with(entity))
            || after.starts_with('#');
        out.push_str(if known { "&" } else { "&amp;" });
        rest = after;
    }
    out.push_str(rest);
    out
}

fn escape_bare_angles(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let tag = matches!(chars.peek(), Some(next) if next.is_ascii_alphabetic() || matches!(next, '/' | '!' | '?'));
            out.push_str(if tag { "<" } else { "&lt;" });
        } else {
            out.push(ch);
        }
    }
    out
}

pub(crate) fn format_time(unix_seconds: i64) -> String {
    glib::DateTime::from_unix_local(unix_seconds)
        .ok()
        .and_then(|time| time.format("%H:%M").ok())
        .map(|text| text.to_string())
        .unwrap_or_default()
}

fn build_icon(notification: &Notification) -> (gtk::Image, bool) {
    if let Some(path) = notification.image_path() {
        if Path::new(&path).is_file() {
            let image = gtk::Image::from_file(&path);
            image.set_pixel_size(ICON_PIXELS);
            return (image, false);
        }
    }

    let (name, fallback) = lookup_icon(&notification.app_icon);
    let image = gtk::Image::from_icon_name(&name);
    image.set_pixel_size(ICON_PIXELS);
    (image, fallback)
}

/// Searches the default theme the way `LookupIcon` does, then falls back to a
/// notification icon. The second value is true when the fallback was used.
fn lookup_icon(name: &str) -> (String, bool) {
    let Some(display) = gtk::gdk::Display::default() else {
        return (FALLBACK_ICONS[0].to_string(), true);
    };
    let theme = gtk::IconTheme::for_display(&display);

    if !name.is_empty() {
        for candidate in [name.to_string(), name.to_lowercase(), name.to_uppercase()] {
            if theme.has_icon(&candidate) {
                return (candidate, false);
            }
        }
    }

    for candidate in FALLBACK_ICONS {
        if theme.has_icon(candidate) {
            return (candidate.to_string(), true);
        }
    }

    (FALLBACK_ICONS[2].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_line_breaks_become_newlines() {
        assert_eq!(repair_markup("one<br>two<br/>three"), "one\ntwo\nthree");
    }

    #[test]
    fn html_bold_and_italic_keep_their_text() {
        assert_eq!(
            repair_markup("<strong>bold</strong> <em>italic</em>"),
            "<b>bold</b> <i>italic</i>"
        );
    }

    #[test]
    fn a_bare_ampersand_is_escaped() {
        assert_eq!(repair_markup("Tom & Jerry"), "Tom &amp; Jerry");
        assert_eq!(
            repair_markup("<a href=\"https://x.test/?a=1&b=2\">link</a>"),
            "<a href=\"https://x.test/?a=1&amp;b=2\">link</a>"
        );
    }

    #[test]
    fn existing_entities_survive() {
        assert_eq!(repair_markup("a &amp; b &#38; c"), "a &amp; b &#38; c");
    }

    #[test]
    fn unknown_tags_are_unwrapped_and_stray_angles_escaped() {
        assert_eq!(repair_markup("<tg-spoiler>hidden</tg-spoiler>"), "hidden");
        assert_eq!(repair_markup("5 < 6"), "5 &lt; 6");
    }
}
