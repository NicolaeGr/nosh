use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{sort_workspaces, Command, Core, WorkspaceModel, WorkspaceSnapshot};

const BUTTON_CLASSES: [&str; 10] = [
    "min-w-8",
    "min-h-2",
    "p-0",
    "mx-0",
    "text-xs",
    "font-semibold",
    "opacity-60",
    "transition-all",
    "duration-200",
    "ease-in-out",
];

pub struct Workspaces {
    root: gtk::Box,
    core: Rc<Core>,
    buttons: RefCell<BTreeMap<String, gtk::Button>>,
}

impl Workspaces {
    pub fn new(core: Rc<Core>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        for class in ["Workspaces", "p-1", "mr-2", "gap-1"] {
            root.add_css_class(class);
        }

        Self {
            root,
            core,
            buttons: RefCell::new(BTreeMap::new()),
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn render(&self, snapshot: &WorkspaceSnapshot, connector: &str) {
        let mut items: Vec<WorkspaceModel> = snapshot
            .workspaces
            .iter()
            .filter(|workspace| !workspace.special)
            .filter(|workspace| workspace.monitor.as_deref() == Some(connector))
            .cloned()
            .collect();
        sort_workspaces(&mut items);

        let mut buttons = self.buttons.borrow_mut();
        let live: Vec<String> = items.iter().map(|workspace| workspace.id.clone()).collect();

        buttons.retain(|id, button| {
            let keep = live.contains(id);
            if !keep {
                self.root.remove(button);
            }
            keep
        });

        let mut previous: Option<gtk::Button> = None;

        for workspace in &items {
            let button = buttons
                .entry(workspace.id.clone())
                .or_insert_with(|| {
                    let button = gtk::Button::new();
                    let label = gtk::Label::new(Some(&label_for(workspace)));
                    button.set_child(Some(&label));
                    for class in BUTTON_CLASSES {
                        button.add_css_class(class);
                    }
                    button.set_focus_on_click(false);

                    let core = Rc::clone(&self.core);
                    let id = workspace.id.clone();
                    button.connect_clicked(move |_| {
                        core.send(Command::WorkspaceActivate(id.clone()));
                    });

                    self.root.append(&button);
                    button
                })
                .clone();

            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_text(&label_for(workspace));
            }

            set_class(&button, "focused", workspace.focused);
            set_class(&button, "urgent", workspace.urgent);

            self.root.reorder_child_after(&button, previous.as_ref());
            previous = Some(button);
        }
    }
}

fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, enabled: bool) {
    if enabled {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

fn label_for(workspace: &WorkspaceModel) -> String {
    if workspace.id.parse::<i64>().is_ok() || workspace.name.is_empty() {
        workspace.id.clone()
    } else {
        workspace.name.clone()
    }
}
