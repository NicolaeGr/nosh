//! An inline dropdown that slides open directly below a row of toggles.
//!
//! The revealer is inserted into the toggle column after the row that owns the
//! control, so later rows move down while it is open. Only one is open at a
//! time; the owner closes the others before opening a new one.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

#[derive(Clone)]
pub(crate) struct Dropdown {
    revealer: gtk::Revealer,
    container: gtk::Box,
    after: gtk::Widget,
    indicator: Option<gtk::Button>,
    open: Rc<Cell<bool>>,
}

impl Dropdown {
    pub fn new(
        container: &gtk::Box,
        after: &impl IsA<gtk::Widget>,
        indicator: Option<&gtk::Button>,
    ) -> Self {
        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        revealer.set_transition_duration(300);
        revealer.set_reveal_child(false);
        revealer.set_hexpand(true);
        revealer.set_visible(false);

        Self {
            revealer,
            container: container.clone(),
            after: after.clone().upcast(),
            indicator: indicator.cloned(),
            open: Rc::new(Cell::new(false)),
        }
    }

    pub fn set_child(&self, child: &impl IsA<gtk::Widget>) {
        self.revealer.set_child(Some(child));
    }

    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    pub fn set_open(&self, open: bool) {
        if self.open.get() == open {
            return;
        }
        self.open.set(open);

        if let Some(indicator) = &self.indicator {
            if open {
                indicator.add_css_class("is-open");
            } else {
                indicator.remove_css_class("is-open");
            }
        }

        if open {
            if self.revealer.parent().is_none() {
                self.container
                    .insert_child_after(&self.revealer, Some(&self.after));
            }
            self.revealer.set_visible(true);
            self.revealer.set_reveal_child(true);
            return;
        }

        self.revealer.set_reveal_child(false);
        // Pull the revealer out once it has collapsed, so the rows below move
        // back up. Reopening during the animation cancels the removal.
        let revealer = self.revealer.clone();
        let still_open = Rc::clone(&self.open);
        glib::timeout_add_local_once(Duration::from_millis(320), move || {
            if still_open.get() {
                return;
            }
            revealer.set_visible(false);
            revealer.unparent();
        });
    }
}
