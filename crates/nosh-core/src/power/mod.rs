//! Session actions through logind for the power menu.

pub mod backend;
pub mod logind;
pub mod model;
pub mod service;

pub use backend::{PowerBackend, PowerBackendRef};
pub use logind::{
    display_session, LogindBackend, LOGIND_MANAGER, LOGIND_PATH, LOGIND_SERVICE,
    LOGIND_SESSION_INTERFACE, LOGIND_SESSION_PATH, LOGIND_USER_INTERFACE,
};
pub use model::{PowerAction, PowerModel};
pub use service::{PowerService, PowerServiceConfig};
