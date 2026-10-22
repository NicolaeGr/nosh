use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use nosh_core::{Core, Event, WorkspaceEvent, WorkspaceSnapshot};

use crate::bar::BarWindow;

#[cfg(all(feature = "dev", feature = "stable"))]
compile_error!("features `dev` and `stable` are mutually exclusive");

pub const APP_ID: &str = if cfg!(feature = "dev") {
    "com.nicolaegr.nosh.dev"
} else {
    "com.nicolaegr.nosh"
};

const STYLE: &str = include_str!("../resources/style.css");

struct Ui {
    app: gtk::Application,
    core: Rc<Core>,
    workspaces: RefCell<WorkspaceSnapshot>,
    bars: RefCell<BTreeMap<String, BarWindow>>,
}

impl Ui {
    fn new(app: gtk::Application, core: Rc<Core>) -> Self {
        Self {
            app,
            core,
            workspaces: RefCell::new(WorkspaceSnapshot::default()),
            bars: RefCell::new(BTreeMap::new()),
        }
    }

    fn sync_bars(&self) {
        let Some(display) = gdk::Display::default() else {
            return;
        };

        let monitors = display.monitors();
        let mut live = BTreeSet::new();

        for index in 0..monitors.n_items() {
            let Some(monitor) = monitors.item(index).and_downcast::<gdk::Monitor>() else {
                continue;
            };

            let connector = monitor
                .connector()
                .map(|value| value.to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| format!("monitor-{index}"));
            live.insert(connector.clone());

            if self.bars.borrow().contains_key(&connector) {
                continue;
            }

            let mut bar = BarWindow::new(
                &self.app,
                &monitor,
                connector.clone(),
                Rc::clone(&self.core),
            );
            bar.set_workspaces(&self.workspaces.borrow());
            bar.present();
            self.bars.borrow_mut().insert(connector, bar);
        }

        self.bars.borrow_mut().retain(|connector, bar| {
            let keep = live.contains(connector);
            if !keep {
                bar.destroy();
            }
            keep
        });
    }

    fn render_workspaces(&self) {
        let snapshot = self.workspaces.borrow();
        for bar in self.bars.borrow_mut().values_mut() {
            bar.set_workspaces(&snapshot);
        }
    }

    fn apply(&self, event: Event) {
        match event {
            Event::Snapshot(snapshot) => {
                if let Some(workspaces) = snapshot.workspaces {
                    *self.workspaces.borrow_mut() = workspaces;
                    self.render_workspaces();
                }
            }
            Event::Workspaces(event) => self.apply_workspace_event(event),
            _ => {}
        }
    }

    fn apply_workspace_event(&self, event: WorkspaceEvent) {
        match event {
            WorkspaceEvent::Snapshot { state, .. } => {
                *self.workspaces.borrow_mut() = state;
            }
            WorkspaceEvent::FocusedChanged { workspace } => {
                let mut snapshot = self.workspaces.borrow_mut();
                snapshot.focused_workspace = workspace.clone();
                for item in &mut snapshot.workspaces {
                    item.focused = workspace.as_deref() == Some(item.id.as_str());
                }
            }
            WorkspaceEvent::UrgentChanged { workspace, urgent } => {
                let mut snapshot = self.workspaces.borrow_mut();
                if let Some(item) = snapshot
                    .workspaces
                    .iter_mut()
                    .find(|item| item.id == workspace)
                {
                    item.urgent = urgent;
                }
            }
            WorkspaceEvent::Unavailable => {
                *self.workspaces.borrow_mut() = WorkspaceSnapshot::default();
            }
            WorkspaceEvent::BackendChanged { .. } => return,
        }

        self.render_workspaces();
    }
}

pub fn run() -> Result<u8, Box<dyn Error>> {
    let core = Rc::new(Core::start()?);
    let events = core.subscribe();

    let app = gtk::Application::builder().application_id(APP_ID).build();

    app.connect_startup(|_| install_css());

    app.connect_activate(move |app| {
        if !gtk4_layer_shell::is_supported() {
            eprintln!("nosh: gtk4-layer-shell is not available on this display server");
            app.quit();
            return;
        }

        let ui = Rc::new(Ui::new(app.clone(), Rc::clone(&core)));
        ui.sync_bars();

        if let Some(display) = gdk::Display::default() {
            let ui = Rc::clone(&ui);
            display
                .monitors()
                .connect_items_changed(move |_, _, _, _| ui.sync_bars());
        }

        let events = events.clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = events.recv().await {
                ui.apply(event);
            }
        });
    });

    Ok(app.run().into())
}

fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };

    let provider = gtk::CssProvider::new();
    provider.load_from_data(STYLE);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );
}
