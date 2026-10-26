//! Generic menus: configuration, sources, search, actions, and the service.
//!
//! A menu is one view plus one source. Sources are functions of a query. The
//! app launcher, a wallpaper picker, a clipboard menu, and a power menu are all
//! the same engine with different configuration.

pub mod action;
pub mod config;
pub mod model;
pub mod query;
pub mod service;
pub mod source;
pub mod usage;

pub use action::{expand_argv, spawn_detached};
pub use config::{
    default_menus, load_menu_config, load_menus, parse_menus, MenuConfig, MenuConfigError,
};
pub use model::{
    dedupe_ids, ActionDef, After, Anchor, IconKind, IconRef, ItemError, ItemLayout, ItemState,
    ItemViewDef, Keyboard, Layer, MenuDef, MenuIcon, MenuItem, MenuPreview, MenuValue, PreviewKind,
    PreviewRef, SearchMode, SourceDef, SourceKind, Template, ViewDef, ViewKind, WindowDef,
    DEFAULT_DEBOUNCE_MS, DEFAULT_LIMIT, DEFAULT_TIMEOUT_MS, MAX_LINE_BYTES, MAX_OUTPUT_BYTES,
    SCHEMA_VERSION,
};
pub use query::{filter_and_sort, fuzzy_score};
pub use service::{MenuEvent, MenuService, MenuServiceConfig};
pub use source::{builtin_source, menu_source, run_external, MenuSource, SourceEnv, SourceError};
pub use usage::Usage;
