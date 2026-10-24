//! System tray state.
//!
//! The core owns the StatusNotifierWatcher role, the item proxies, the pixmap
//! decoding, and the DBusMenu client. The UI only renders the models.

pub mod focus;
pub mod icon;
pub mod menu;
pub mod model;
pub mod registration;
pub mod service;
pub mod watcher;

pub use crate::event::TrayEvent;
pub use focus::{pick_window, HyprlandFocus, WindowFocus, WindowFocusRef, WindowHint};
pub use icon::{decode_argb32, find_icon_file, largest_pixmap, IconCache, MAX_PIXMAP_SIZE};
pub use menu::{DbusMenuClient, MAX_MENU_DEPTH, MAX_MENU_ITEMS, MENU_TIMEOUT, WANTED_PROPERTIES};
pub use model::{
    IconPixmap, TrayCategory, TrayIcon, TrayItemModel, TrayMenu, TrayMenuItem, TrayMenuItemKind,
    TrayStatus, TrayToggleState, TrayToggleType,
};
pub use registration::{Registration, DEFAULT_OBJECT_PATH};
pub use service::{TrayService, TrayServiceConfig};
pub use watcher::{
    default_candidates, NoTrayBackend, OwnWatcherBackend, ProxyWatcherBackend, TrayCandidate,
    WatcherBackend, WatcherBackendRef, WatcherChange, WatcherSession, WATCHER_INTERFACE,
    WATCHER_PATH, WATCHER_SERVICE,
};
