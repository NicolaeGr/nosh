//! Power profiles from `power-profiles-daemon`.
//!
//! The daemon is a soft dependency. When it is missing the service reports
//! itself as unavailable and the quick settings control hides.

pub mod backend;
pub mod dbus;
pub mod model;
pub mod service;

pub use crate::event::PowerProfilesEvent;
pub use backend::{PowerProfilesBackend, PowerProfilesBackendEvent, PowerProfilesBackendRef};
pub use dbus::{
    DbusPowerProfilesBackend, POWER_PROFILES_INTERFACE, POWER_PROFILES_PATH,
    POWER_PROFILES_SERVICE, PROPERTIES_INTERFACE,
};
pub use model::{default_profiles, next_profile, PowerProfile, PowerProfilesModel};
pub use service::{PowerProfilesService, PowerProfilesServiceConfig};
