//! One implementation of network reading and writing.
//!
//! NetworkManager over D-Bus is the only backend. The trait keeps the service
//! testable without a system bus and without a radio.

use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;

use crate::error::BackendError;
use crate::network::model::{NetworkModel, SecurityType};

/// A change reported by a running backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkBackendEvent {
    /// A property, device, access point, connection, or rfkill switch changed.
    Changed,
    /// NetworkManager came back. The next read is a fresh snapshot.
    Reconnected,
}

/// Reads and writes the network state.
#[async_trait]
pub trait NetworkBackend: Send + Sync {
    fn name(&self) -> &'static str;

    async fn probe(&self) -> Result<(), BackendError>;

    /// Read the complete state.
    ///
    /// `details` asks for the Wi-Fi access point list. The service sets it only
    /// while the network dropdown is open, so the bus and the radio stay quiet
    /// the rest of the time.
    async fn read(&self, details: bool) -> Result<NetworkModel, BackendError>;

    async fn set_wifi_enabled(&self, enabled: bool) -> Result<(), BackendError>;

    /// Ask the Wi-Fi device for a scan. Only sent when the dropdown opens or
    /// the user presses refresh.
    async fn scan_wifi(&self) -> Result<(), BackendError>;

    /// Connect to a network. The backend finds the access point and its
    /// security type itself so the command stays small.
    async fn activate_wifi(
        &self,
        ssid: &str,
        password: Option<&str>,
        security: Option<SecurityType>,
    ) -> Result<(), BackendError>;

    async fn deactivate_wifi(&self) -> Result<(), BackendError>;

    /// Turn a wired interface off with `Device.Disconnect` and back on with the
    /// first available connection.
    async fn set_wired_enabled(&self, interface: &str, enabled: bool) -> Result<(), BackendError>;

    async fn activate_vpn(&self, uuid: &str) -> Result<(), BackendError>;

    async fn deactivate_vpn(&self, uuid: &str) -> Result<(), BackendError>;

    /// Airplane mode disables NetworkManager Wi-Fi and WWAN and soft-blocks
    /// every rfkill radio. It never touches a wired interface.
    async fn set_airplane_mode(&self, enabled: bool) -> Result<(), BackendError>;

    /// Runs until the properties stream ends or the receiver closes.
    async fn watch(&self, events: Sender<NetworkBackendEvent>) -> Result<(), BackendError>;
}

pub type NetworkBackendRef = Arc<dyn NetworkBackend>;
