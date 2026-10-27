//! Wired, Wi-Fi, VPN, and airplane mode.
//!
//! NetworkManager over D-Bus is the only backend. The service reads the overview
//! on startup, follows D-Bus signals, and loads access points only while the
//! network dropdown is open.
//!
//! Airplane mode disables NetworkManager's Wi-Fi and WWAN and soft-blocks every
//! rfkill radio. It never touches a wired interface.

pub mod backend;
pub mod model;
pub mod nm;
pub mod rfkill;
pub mod service;

pub use crate::event::NetworkEvent;
pub use backend::{NetworkBackend, NetworkBackendEvent, NetworkBackendRef};
pub use model::{
    group_access_points, security_from_flags, signal_icon, sort_networks, ssid_to_utf8, wifi_icon,
    AccessPointModel, AirplaneModeModel, Connectivity, DeviceState, NetworkModel, SecurityType,
    VpnKind, VpnModel, VpnProfileModel, WifiMode, WifiModel, WiredDeviceModel,
};
pub use nm::{
    NetworkManagerBackend, NM_ACTIVE_INTERFACE, NM_AP_INTERFACE, NM_CONNECTION_INTERFACE,
    NM_DEVICE_INTERFACE, NM_INTERFACE, NM_IP4_INTERFACE, NM_OBJECT_MANAGER_PATH, NM_PATH,
    NM_SERVICE, NM_SETTINGS_INTERFACE, NM_SETTINGS_PATH, NM_WIRED_INTERFACE, NM_WIRELESS_INTERFACE,
    OBJECT_MANAGER_INTERFACE, PROPERTIES_INTERFACE,
};
pub use rfkill::{
    is_available as rfkill_available, read_devices as read_rfkill_devices, RfkillDevice,
    RfkillKind, RFKILL_ROOT,
};
pub use service::{NetworkService, NetworkServiceConfig};
