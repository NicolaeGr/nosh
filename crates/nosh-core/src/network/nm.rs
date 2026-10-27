//! The NetworkManager D-Bus backend.
//!
//! One `GetManagedObjects` call returns devices, access points, and active
//! connections. Connection profiles need one `GetSettings` call each because
//! the settings objects expose no properties. The service debounces reads, so a
//! burst of access point strength updates costs one round of calls.
//!
//! Nothing here scans on a timer. `RequestScan` is only called from `scan_wifi`.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::sync::OnceCell;
use zbus::message::Type;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::MatchRule;

use crate::error::BackendError;
use crate::network::backend::{NetworkBackend, NetworkBackendEvent};
use crate::network::model::{
    group_access_points, security_from_flags, ssid_to_utf8, AccessPointModel, AirplaneModeModel,
    Connectivity, DeviceState, NetworkModel, SecurityType, VpnKind, VpnModel, VpnProfileModel,
    WifiMode, WifiModel, WiredDeviceModel,
};
use crate::network::rfkill::{self, RfkillDevice};

pub const NM_SERVICE: &str = "org.freedesktop.NetworkManager";
pub const NM_PATH: &str = "/org/freedesktop/NetworkManager";
/// NetworkManager exports its object manager at `/org/freedesktop`, not at the
/// NetworkManager root object.
pub const NM_OBJECT_MANAGER_PATH: &str = "/org/freedesktop";
pub const NM_INTERFACE: &str = "org.freedesktop.NetworkManager";
pub const NM_SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
pub const NM_SETTINGS_INTERFACE: &str = "org.freedesktop.NetworkManager.Settings";
pub const NM_CONNECTION_INTERFACE: &str = "org.freedesktop.NetworkManager.Settings.Connection";
pub const NM_DEVICE_INTERFACE: &str = "org.freedesktop.NetworkManager.Device";
pub const NM_WIRELESS_INTERFACE: &str = "org.freedesktop.NetworkManager.Device.Wireless";
pub const NM_WIRED_INTERFACE: &str = "org.freedesktop.NetworkManager.Device.Wired";
pub const NM_AP_INTERFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";
pub const NM_ACTIVE_INTERFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub const NM_IP4_INTERFACE: &str = "org.freedesktop.NetworkManager.IP4Config";
pub const OBJECT_MANAGER_INTERFACE: &str = "org.freedesktop.DBus.ObjectManager";
pub const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
pub const DBUS_SERVICE: &str = "org.freedesktop.DBus";
pub const DBUS_PATH: &str = "/org/freedesktop/DBus";
pub const DBUS_INTERFACE: &str = "org.freedesktop.DBus";

const DEVICE_TYPE_ETHERNET: u32 = 1;
const DEVICE_TYPE_WIFI: u32 = 2;
const DEVICE_TYPE_WWAN: u32 = 8;

/// `NM_ACTIVATION_STATE_FLAG_EXTERNAL` from `nm-dbus-interface.h`.
const ACTIVE_CONNECTION_EXTERNAL: u32 = 0x80;

type Interfaces = HashMap<String, HashMap<String, OwnedValue>>;
type ManagedObjects = HashMap<OwnedObjectPath, Interfaces>;
type SettingsMap = HashMap<String, HashMap<String, OwnedValue>>;

/// Reads and writes network state over the system bus.
pub struct NetworkManagerBackend {
    connection: OnceCell<zbus::Connection>,
    rfkill_root: PathBuf,
    disabled_wired: Mutex<HashSet<String>>,
    airplane: AtomicBool,
    airplane_saved: Mutex<Option<AirplaneSaved>>,
    vpn_failures: Mutex<HashMap<String, String>>,
}

struct AirplaneSaved {
    wireless_enabled: bool,
    wwan_enabled: Option<bool>,
    radios: Vec<(RfkillDevice, bool)>,
}

#[derive(Debug, Clone)]
struct ConnectionProfile {
    path: OwnedObjectPath,
    settings: SettingsMap,
}

impl ConnectionProfile {
    fn connection(&self) -> Option<&HashMap<String, OwnedValue>> {
        self.settings.get("connection")
    }

    fn kind(&self) -> Option<String> {
        self.connection()
            .and_then(|connection| get_string(connection, "type"))
    }

    fn uuid(&self) -> Option<String> {
        self.connection()
            .and_then(|connection| get_string(connection, "uuid"))
    }

    fn id(&self) -> Option<String> {
        self.connection()
            .and_then(|connection| get_string(connection, "id"))
    }

    fn ssid(&self) -> Option<Vec<u8>> {
        self.settings
            .get("802-11-wireless")
            .and_then(|wireless| get_bytes(wireless, "ssid"))
    }
}

struct ActiveConnection {
    uuid: Option<String>,
    id: Option<String>,
    kind: Option<String>,
    vpn: bool,
    external: bool,
}

impl NetworkManagerBackend {
    pub fn new() -> Self {
        Self::with_rfkill_root(Path::new(rfkill::RFKILL_ROOT))
    }

    pub fn with_rfkill_root(root: &Path) -> Self {
        Self {
            connection: OnceCell::new(),
            rfkill_root: root.to_path_buf(),
            disabled_wired: Mutex::new(HashSet::new()),
            airplane: AtomicBool::new(false),
            airplane_saved: Mutex::new(None),
            vpn_failures: Mutex::new(HashMap::new()),
        }
    }

    async fn connection(&self) -> Result<&zbus::Connection, BackendError> {
        self.connection
            .get_or_try_init(|| async {
                zbus::Connection::system()
                    .await
                    .map_err(|error| BackendError::Unavailable(error.to_string()))
            })
            .await
    }

    async fn managed_objects(
        &self,
        connection: &zbus::Connection,
    ) -> Result<ManagedObjects, BackendError> {
        let proxy = zbus::Proxy::new(
            connection,
            NM_SERVICE,
            NM_OBJECT_MANAGER_PATH,
            OBJECT_MANAGER_INTERFACE,
        )
        .await
        .map_err(dbus_error)?;
        proxy
            .call::<_, _, ManagedObjects>("GetManagedObjects", &())
            .await
            .map_err(dbus_error)
    }

    async fn nm_properties(
        &self,
        connection: &zbus::Connection,
    ) -> Result<HashMap<String, OwnedValue>, BackendError> {
        let proxy = zbus::Proxy::new(connection, NM_SERVICE, NM_PATH, PROPERTIES_INTERFACE)
            .await
            .map_err(dbus_error)?;
        proxy
            .call("GetAll", &(NM_INTERFACE,))
            .await
            .map_err(dbus_error)
    }

    async fn profiles(
        &self,
        connection: &zbus::Connection,
    ) -> Result<Vec<ConnectionProfile>, BackendError> {
        let proxy = zbus::Proxy::new(
            connection,
            NM_SERVICE,
            NM_SETTINGS_PATH,
            NM_SETTINGS_INTERFACE,
        )
        .await
        .map_err(dbus_error)?;
        let paths: Vec<OwnedObjectPath> = proxy
            .call("ListConnections", &())
            .await
            .map_err(dbus_error)?;

        let mut profiles = Vec::with_capacity(paths.len());
        for path in paths {
            let proxy = zbus::Proxy::new(
                connection,
                NM_SERVICE,
                path.as_str(),
                NM_CONNECTION_INTERFACE,
            )
            .await
            .map_err(dbus_error)?;
            match proxy.call::<_, _, SettingsMap>("GetSettings", &()).await {
                Ok(settings) => profiles.push(ConnectionProfile { path, settings }),
                Err(error) => {
                    // A profile can disappear between the list and the read.
                    tracing::debug!(%error, "a NetworkManager connection profile could not be read");
                }
            }
        }
        Ok(profiles)
    }

    fn model(
        &self,
        managed: &ManagedObjects,
        values: &HashMap<String, OwnedValue>,
        profiles: &[ConnectionProfile],
        details: bool,
    ) -> NetworkModel {
        let active = active_connections(managed);
        let saved_ssids: HashSet<Vec<u8>> = profiles
            .iter()
            .filter(|profile| profile.kind().as_deref() == Some("802-11-wireless"))
            .filter_map(|profile| profile.ssid())
            .collect();

        let connectivity = get_u32(values, "Connectivity")
            .map(Connectivity::from_u32)
            .unwrap_or_default();
        let wireless_enabled = get_bool(values, "WirelessEnabled").unwrap_or(false);

        let mut wifi_device: Option<WifiDevice> = None;
        let mut wired = Vec::new();
        let mut wwan_present = false;

        for (path, interfaces) in managed {
            let Some(device) = interfaces.get(NM_DEVICE_INTERFACE) else {
                continue;
            };
            let device_type = get_u32(device, "DeviceType").unwrap_or(0);
            let interface = get_string(device, "Interface").unwrap_or_default();
            let state = DeviceState::from_u32(get_u32(device, "State").unwrap_or(0));
            let active_path = get_object(device, "ActiveConnection");

            match device_type {
                DEVICE_TYPE_WIFI => {
                    let wireless = interfaces.get(NM_WIRELESS_INTERFACE);
                    let active_ap =
                        wireless.and_then(|wireless| get_object(wireless, "ActiveAccessPoint"));
                    let access_points = wireless
                        .and_then(|wireless| get_objects(wireless, "AccessPoints"))
                        .unwrap_or_default();
                    let candidate = WifiDevice {
                        state,
                        active_ap,
                        access_points,
                    };
                    // Prefer an activated radio when several exist.
                    if wifi_device
                        .as_ref()
                        .map(|current| !current.state.is_active() && state.is_active())
                        .unwrap_or(true)
                    {
                        wifi_device = Some(candidate);
                    }
                }
                DEVICE_TYPE_ETHERNET => {
                    if !get_bool(device, "Managed").unwrap_or(true) {
                        continue;
                    }
                    let wired_info = interfaces.get(NM_WIRED_INTERFACE);
                    let carrier = wired_info
                        .and_then(|info| get_bool(info, "Carrier"))
                        .unwrap_or(false);
                    let speed = wired_info
                        .and_then(|info| get_u32(info, "Speed"))
                        .unwrap_or(0);
                    let connection_name = active_path
                        .as_ref()
                        .and_then(|path| active.get(path))
                        .and_then(|active| active.id.clone());
                    let ip4_address = ip4_address(managed, device);
                    let enabled = !self
                        .disabled_wired
                        .lock()
                        .map(|disabled| disabled.contains(&interface))
                        .unwrap_or(false);
                    let connected = state.is_active();
                    wired.push(WiredDeviceModel {
                        device_path: path.to_string(),
                        interface,
                        state,
                        connected,
                        enabled,
                        carrier,
                        connection_name,
                        ip4_address,
                        speed,
                    });
                }
                DEVICE_TYPE_WWAN => wwan_present = true,
                _ => {}
            }
        }

        wired.sort_by(|left, right| left.interface.cmp(&right.interface));

        let wlan_radio = rfkill::has_kind(&self.rfkill_root, rfkill::RfkillKind::Wlan);
        let (wifi, _, _) = match &wifi_device {
            Some(device) => {
                let active_info = device
                    .active_ap
                    .as_ref()
                    .and_then(|path| access_point(managed, path));
                let active_ssid = active_info.as_ref().and_then(|info| info.ssid.clone());
                let active_bssid = active_info.as_ref().and_then(|info| info.bssid.clone());
                let active_strength = active_info.as_ref().map(|info| info.strength);

                let access_points = if details {
                    let members = device
                        .access_points
                        .iter()
                        .filter_map(|path| {
                            access_point(managed, path).map(|info| {
                                let security =
                                    security_from_flags(info.flags, info.wpa_flags, info.rsn_flags);
                                let saved = info
                                    .ssid_bytes
                                    .as_ref()
                                    .map(|ssid| saved_ssids.contains(ssid))
                                    .unwrap_or(false);
                                let mut model = info.into_model(security, saved);
                                model.active = device.active_ap.as_ref() == Some(path);
                                model
                            })
                        })
                        .collect();
                    group_access_points(members)
                } else {
                    Vec::new()
                };

                let mut model = WifiModel {
                    available: true,
                    enabled: wireless_enabled,
                    scanning: false,
                    state: device.state,
                    connectivity,
                    active_ssid: active_ssid.clone(),
                    active_bssid: active_bssid.clone(),
                    active_strength,
                    icon_name: String::new(),
                    access_points,
                    details_loaded: details,
                };
                model.compute_icon();
                (model, active_ssid, active_bssid)
            }
            None => {
                let mut model = WifiModel {
                    available: wlan_radio,
                    enabled: wireless_enabled,
                    state: DeviceState::Unavailable,
                    connectivity,
                    ..WifiModel::default()
                };
                model.compute_icon();
                (model, None, None)
            }
        };

        let vpn = vpn_model(profiles, &active, &self.vpn_failures);

        let rfkill_available = rfkill::is_available(&self.rfkill_root);
        let airplane = AirplaneModeModel {
            available: rfkill_available || wifi.available || wwan_present,
            enabled: self.airplane.load(Ordering::Relaxed),
            transitioning: false,
        };

        NetworkModel {
            available: true,
            connectivity,
            airplane,
            wired,
            wifi,
            vpn,
        }
    }

    async fn set_nm_property(
        &self,
        connection: &zbus::Connection,
        iface: &str,
        name: &str,
        value: Value<'static>,
    ) -> Result<(), BackendError> {
        let proxy = zbus::Proxy::new(connection, NM_SERVICE, NM_PATH, PROPERTIES_INTERFACE)
            .await
            .map_err(dbus_error)?;
        proxy
            .call::<_, _, ()>("Set", &(iface, name, value))
            .await
            .map_err(dbus_error)
    }

    async fn device_path(
        &self,
        connection: &zbus::Connection,
        device_type: u32,
        interface: Option<&str>,
    ) -> Result<OwnedObjectPath, BackendError> {
        let managed = self.managed_objects(connection).await?;
        for (path, interfaces) in &managed {
            let Some(device) = interfaces.get(NM_DEVICE_INTERFACE) else {
                continue;
            };
            if get_u32(device, "DeviceType") != Some(device_type) {
                continue;
            }
            if let Some(interface) = interface {
                if get_string(device, "Interface").as_deref() != Some(interface) {
                    continue;
                }
            }
            return Ok(path.clone());
        }
        Err(BackendError::Unavailable(match device_type {
            DEVICE_TYPE_WIFI => "no Wi-Fi device".to_string(),
            DEVICE_TYPE_ETHERNET => format!("no wired device {}", interface.unwrap_or("")),
            _ => "device not found".to_string(),
        }))
    }

    async fn activate_connection(
        &self,
        connection: &zbus::Connection,
        profile: &OwnedObjectPath,
        device: &OwnedObjectPath,
    ) -> Result<(), BackendError> {
        let proxy = zbus::Proxy::new(connection, NM_SERVICE, NM_PATH, NM_INTERFACE)
            .await
            .map_err(dbus_error)?;
        let specific = null_path();
        proxy
            .call::<_, _, OwnedObjectPath>("ActivateConnection", &(profile, device, &specific))
            .await
            .map(|_| ())
            .map_err(dbus_error)
    }

    fn rfkill_radios(&self) -> Vec<(RfkillDevice, bool)> {
        rfkill::read_devices(&self.rfkill_root)
            .unwrap_or_default()
            .into_iter()
            .filter(|device| device.kind.is_wireless())
            .map(|device| {
                let soft = device.soft_blocked;
                (device, soft)
            })
            .collect()
    }
}

impl Default for NetworkManagerBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NetworkBackend for NetworkManagerBackend {
    fn name(&self) -> &'static str {
        "networkmanager"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        self.managed_objects(connection).await.map(|_| ())
    }

    async fn read(&self, details: bool) -> Result<NetworkModel, BackendError> {
        let connection = self.connection().await?;
        let managed = self.managed_objects(connection).await?;
        let values = self.nm_properties(connection).await?;
        let profiles = match self.profiles(connection).await {
            Ok(profiles) => profiles,
            Err(error) => {
                tracing::debug!(%error, "reading NetworkManager connection profiles failed");
                Vec::new()
            }
        };
        Ok(self.model(&managed, &values, &profiles, details))
    }

    async fn set_wifi_enabled(&self, enabled: bool) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        self.set_nm_property(
            connection,
            NM_INTERFACE,
            "WirelessEnabled",
            Value::from(enabled),
        )
        .await
    }

    async fn scan_wifi(&self) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let device = self.device_path(connection, DEVICE_TYPE_WIFI, None).await?;
        let proxy = zbus::Proxy::new(
            connection,
            NM_SERVICE,
            device.as_str(),
            NM_WIRELESS_INTERFACE,
        )
        .await
        .map_err(dbus_error)?;
        let options: HashMap<String, OwnedValue> = HashMap::new();
        proxy
            .call::<_, _, ()>("RequestScan", &(options,))
            .await
            .map_err(dbus_error)
    }

    async fn activate_wifi(
        &self,
        ssid: &str,
        password: Option<&str>,
        security: Option<SecurityType>,
    ) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let device = self.device_path(connection, DEVICE_TYPE_WIFI, None).await?;
        let managed = self.managed_objects(connection).await?;

        let security = security
            .or_else(|| access_point_security(&managed, ssid))
            .unwrap_or(SecurityType::None);

        let profiles = self.profiles(connection).await?;
        let ssid_bytes = ssid.as_bytes();
        let existing = find_wireless_profile(&profiles, ssid_bytes);

        if let Some(profile) = existing {
            if let Some(password) = password {
                let mut settings = profile.settings.clone();
                set_wifi_security(&mut settings, security, password)?;
                let proxy = zbus::Proxy::new(
                    connection,
                    NM_SERVICE,
                    profile.path.as_str(),
                    NM_CONNECTION_INTERFACE,
                )
                .await
                .map_err(dbus_error)?;
                proxy
                    .call::<_, _, ()>("Update", &(settings,))
                    .await
                    .map_err(dbus_error)?;
            }
            return self
                .activate_connection(connection, &profile.path, &device)
                .await;
        }

        let settings = wifi_settings(ssid, security, password)?;
        let proxy = zbus::Proxy::new(connection, NM_SERVICE, NM_PATH, NM_INTERFACE)
            .await
            .map_err(dbus_error)?;
        let specific = null_path();
        proxy
            .call::<_, _, (OwnedObjectPath, OwnedObjectPath)>(
                "AddAndActivateConnection",
                &(settings, &device, &specific),
            )
            .await
            .map(|_| ())
            .map_err(dbus_error)
    }

    async fn deactivate_wifi(&self) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let device = self.device_path(connection, DEVICE_TYPE_WIFI, None).await?;
        let proxy = zbus::Proxy::new(connection, NM_SERVICE, device.as_str(), NM_DEVICE_INTERFACE)
            .await
            .map_err(dbus_error)?;
        proxy
            .call::<_, _, ()>("Disconnect", &())
            .await
            .map_err(dbus_error)
    }

    async fn set_wired_enabled(&self, interface: &str, enabled: bool) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let device = self
            .device_path(connection, DEVICE_TYPE_ETHERNET, Some(interface))
            .await?;

        let proxy = zbus::Proxy::new(connection, NM_SERVICE, device.as_str(), NM_DEVICE_INTERFACE)
            .await
            .map_err(dbus_error)?;

        if !enabled {
            if let Ok(mut disabled) = self.disabled_wired.lock() {
                disabled.insert(interface.to_string());
            }
            return match proxy.call::<_, _, ()>("Disconnect", &()).await {
                Ok(()) => Ok(()),
                // The device already has no active connection.
                Err(zbus::Error::MethodError(name, _, _))
                    if name.as_str().ends_with(".NotActive") =>
                {
                    Ok(())
                }
                Err(error) => Err(dbus_error(error)),
            };
        }

        if let Ok(mut disabled) = self.disabled_wired.lock() {
            disabled.remove(interface);
        }

        let available: Vec<OwnedObjectPath> = proxy
            .get_property("AvailableConnections")
            .await
            .map_err(dbus_error)?;
        let Some(profile) = available.into_iter().next() else {
            // Enabled means no longer administratively disabled. Without a
            // saved profile there is nothing to activate, which is not an error.
            return Ok(());
        };
        self.activate_connection(connection, &profile, &device)
            .await
    }

    async fn activate_vpn(&self, uuid: &str) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let profiles = self.profiles(connection).await?;
        let Some(profile) = profiles
            .iter()
            .find(|profile| profile.uuid().as_deref() == Some(uuid))
        else {
            return Err(BackendError::Unavailable(format!(
                "VPN profile {uuid} was not found"
            )));
        };

        if let Ok(mut failures) = self.vpn_failures.lock() {
            failures.remove(uuid);
        }

        let device = null_path();
        let result = self
            .activate_connection(connection, &profile.path, &device)
            .await;
        if let Err(error) = &result {
            if let Ok(mut failures) = self.vpn_failures.lock() {
                failures.insert(uuid.to_string(), error.to_string());
            }
        }
        result
    }

    async fn deactivate_vpn(&self, uuid: &str) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let managed = self.managed_objects(connection).await?;
        let active = active_connections(&managed);
        let Some((path, _)) = active
            .iter()
            .find(|(_, active)| active.uuid.as_deref() == Some(uuid))
        else {
            return Ok(());
        };

        if let Ok(mut failures) = self.vpn_failures.lock() {
            failures.remove(uuid);
        }

        let proxy = zbus::Proxy::new(connection, NM_SERVICE, NM_PATH, NM_INTERFACE)
            .await
            .map_err(dbus_error)?;
        proxy
            .call::<_, _, ()>("DeactivateConnection", &(path,))
            .await
            .map_err(dbus_error)
    }

    async fn set_airplane_mode(&self, enabled: bool) -> Result<(), BackendError> {
        let connection = self.connection().await?;

        if enabled {
            let values = self.nm_properties(connection).await?;
            if let Ok(mut saved) = self.airplane_saved.lock() {
                if saved.is_none() {
                    *saved = Some(AirplaneSaved {
                        wireless_enabled: get_bool(&values, "WirelessEnabled").unwrap_or(false),
                        wwan_enabled: get_bool(&values, "WwanEnabled"),
                        radios: self.rfkill_radios(),
                    });
                }
            }

            self.set_nm_property(
                connection,
                NM_INTERFACE,
                "WirelessEnabled",
                Value::from(false),
            )
            .await?;
            if get_bool(&values, "WwanEnabled").is_some() {
                if let Err(error) = self
                    .set_nm_property(connection, NM_INTERFACE, "WwanEnabled", Value::from(false))
                    .await
                {
                    tracing::debug!(%error, "disabling the mobile radio failed");
                }
            }

            for (name, error) in rfkill::set_wireless_soft_blocked_via(
                &self.rfkill_root,
                true,
                Path::new(rfkill::RFKILL_DEVICE),
            ) {
                tracing::debug!(radio = %name, %error, "blocking a radio for airplane mode failed");
            }
        } else {
            let saved = self
                .airplane_saved
                .lock()
                .ok()
                .and_then(|mut saved| saved.take());
            if let Some(saved) = saved {
                if let Err(error) = self
                    .set_nm_property(
                        connection,
                        NM_INTERFACE,
                        "WirelessEnabled",
                        Value::from(saved.wireless_enabled),
                    )
                    .await
                {
                    tracing::warn!(%error, "restoring Wi-Fi after airplane mode failed");
                }
                if let Some(wwan) = saved.wwan_enabled {
                    if let Err(error) = self
                        .set_nm_property(connection, NM_INTERFACE, "WwanEnabled", Value::from(wwan))
                        .await
                    {
                        tracing::debug!(%error, "restoring the mobile radio failed");
                    }
                }
                for (name, error) in rfkill::restore_soft_state_via(
                    &self.rfkill_root,
                    &saved.radios,
                    Path::new(rfkill::RFKILL_DEVICE),
                ) {
                    tracing::debug!(radio = %name, %error, "restoring a radio failed");
                }
            } else {
                // Without a snapshot, turning airplane mode off should at
                // least unblock the radios the user can see.
                for (name, error) in rfkill::set_wireless_soft_blocked_via(
                    &self.rfkill_root,
                    false,
                    Path::new(rfkill::RFKILL_DEVICE),
                ) {
                    tracing::debug!(radio = %name, %error, "unblocking a radio failed");
                }
            }
        }

        self.airplane.store(enabled, Ordering::Relaxed);
        Ok(())
    }

    async fn watch(&self, events: Sender<NetworkBackendEvent>) -> Result<(), BackendError> {
        let connection = self.connection().await?;

        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .path_namespace(NM_PATH)
            .map_err(dbus_error)?
            .build();
        let mut signals = zbus::MessageStream::for_match_rule(rule, connection, Some(64))
            .await
            .map_err(dbus_error)?;

        let dbus = zbus::Proxy::new(connection, DBUS_SERVICE, DBUS_PATH, DBUS_INTERFACE)
            .await
            .map_err(dbus_error)?;
        let mut owner_changed = dbus
            .receive_signal("NameOwnerChanged")
            .await
            .map_err(dbus_error)?;

        loop {
            tokio::select! {
                message = signals.next() => {
                    let Some(message) = message else {
                        break;
                    };
                    let Ok(message) = message else {
                        continue;
                    };
                    let Ok(member) = message
                        .header()
                        .member()
                        .map(|member| member.to_string())
                        .ok_or(())
                    else {
                        continue;
                    };
                    if !matches!(
                        member.as_str(),
                        "PropertiesChanged" | "InterfacesAdded" | "InterfacesRemoved"
                    ) {
                        continue;
                    }
                    if events.send(NetworkBackendEvent::Changed).await.is_err() {
                        return Ok(());
                    }
                }
                owner = owner_changed.next() => {
                    let Some(message) = owner else {
                        break;
                    };
                    let Ok((name, _, new_owner)) = message.body().deserialize::<(String, String, String)>() else {
                        continue;
                    };
                    if name != NM_SERVICE {
                        continue;
                    }
                    if !new_owner.is_empty()
                        && events.send(NetworkBackendEvent::Reconnected).await.is_err()
                    {
                        return Ok(());
                    }
                }
            }
        }

        Err(BackendError::Unavailable(
            "NetworkManager left the bus".to_string(),
        ))
    }
}

// `MessageStream` yields `Option<Message>` through `StreamExt`.
use futures_util::StreamExt;

struct WifiDevice {
    state: DeviceState,
    active_ap: Option<OwnedObjectPath>,
    access_points: Vec<OwnedObjectPath>,
}

struct AccessPointInfo {
    ssid: Option<String>,
    ssid_bytes: Option<Vec<u8>>,
    bssid: Option<String>,
    strength: u8,
    frequency: u32,
    bandwidth: u32,
    max_bitrate: u32,
    mode: WifiMode,
    flags: u32,
    wpa_flags: u32,
    rsn_flags: u32,
}

impl AccessPointInfo {
    fn into_model(self, security: SecurityType, saved: bool) -> AccessPointModel {
        AccessPointModel::member(
            self.ssid,
            self.bssid,
            self.mode,
            security,
            self.strength,
            self.frequency,
            self.bandwidth,
            self.max_bitrate,
            saved,
            false,
        )
    }
}

fn access_point(managed: &ManagedObjects, path: &OwnedObjectPath) -> Option<AccessPointInfo> {
    let ap = managed.get(path)?.get(NM_AP_INTERFACE)?;
    let ssid_bytes = get_bytes(ap, "Ssid");
    let ssid = ssid_bytes
        .as_deref()
        .map(ssid_to_utf8)
        .filter(|ssid| !ssid.is_empty());
    Some(AccessPointInfo {
        ssid,
        ssid_bytes,
        bssid: get_string(ap, "Bssid").or_else(|| get_string(ap, "HwAddress")),
        strength: get_u8(ap, "Strength").unwrap_or(0),
        frequency: get_u32(ap, "Frequency").unwrap_or(0),
        bandwidth: get_u32(ap, "Bandwidth").unwrap_or(0),
        max_bitrate: get_u32(ap, "MaxBitrate").unwrap_or(0),
        mode: WifiMode::from_u32(get_u32(ap, "Mode").unwrap_or(2)),
        flags: get_u32(ap, "Flags").unwrap_or(0),
        wpa_flags: get_u32(ap, "WpaFlags").unwrap_or(0),
        rsn_flags: get_u32(ap, "RsnFlags").unwrap_or(0),
    })
}

fn access_point_security(managed: &ManagedObjects, ssid: &str) -> Option<SecurityType> {
    let mut best: Option<AccessPointInfo> = None;
    for interfaces in managed.values() {
        let Some(ap) = interfaces.get(NM_AP_INTERFACE) else {
            continue;
        };
        let Some(ssid_bytes) = get_bytes(ap, "Ssid") else {
            continue;
        };
        if ssid_to_utf8(&ssid_bytes) != ssid {
            continue;
        }
        let strength = get_u8(ap, "Strength").unwrap_or(0);
        if best
            .as_ref()
            .map(|best| strength > best.strength)
            .unwrap_or(true)
        {
            best = Some(AccessPointInfo {
                ssid: None,
                ssid_bytes: None,
                bssid: None,
                strength,
                frequency: 0,
                bandwidth: 0,
                max_bitrate: 0,
                mode: WifiMode::Infrastructure,
                flags: get_u32(ap, "Flags").unwrap_or(0),
                wpa_flags: get_u32(ap, "WpaFlags").unwrap_or(0),
                rsn_flags: get_u32(ap, "RsnFlags").unwrap_or(0),
            });
        }
    }
    best.map(|info| security_from_flags(info.flags, info.wpa_flags, info.rsn_flags))
}

fn active_connections(managed: &ManagedObjects) -> HashMap<OwnedObjectPath, ActiveConnection> {
    managed
        .iter()
        .filter_map(|(path, interfaces)| {
            let active = interfaces.get(NM_ACTIVE_INTERFACE)?;
            Some((
                path.clone(),
                ActiveConnection {
                    uuid: get_string(active, "Uuid"),
                    id: get_string(active, "Id"),
                    kind: get_string(active, "Type"),
                    vpn: get_bool(active, "Vpn").unwrap_or(false),
                    external: get_u32(active, "StateFlags")
                        .map(|flags| flags & ACTIVE_CONNECTION_EXTERNAL != 0)
                        .unwrap_or(false),
                },
            ))
        })
        .collect()
}

fn vpn_model(
    profiles: &[ConnectionProfile],
    active: &HashMap<OwnedObjectPath, ActiveConnection>,
    failures: &Mutex<HashMap<String, String>>,
) -> VpnModel {
    let active_vpns: Vec<&ActiveConnection> = active
        .values()
        .filter(|active| {
            active.vpn || matches!(active.kind.as_deref(), Some("vpn") | Some("wireguard"))
        })
        .collect();
    let failures = failures.lock().ok();

    let profiles: Vec<VpnProfileModel> = profiles
        .iter()
        .filter_map(|profile| {
            let kind = VpnKind::parse(&profile.kind()?)?;
            let uuid = profile.uuid()?;
            let name = profile.id().unwrap_or_else(|| uuid.clone());
            let is_active = active_vpns
                .iter()
                .find(|active| active.uuid.as_deref() == Some(uuid.as_str()));
            let external = is_active.map(|active| active.external).unwrap_or(false);
            let activation_failed = failures
                .as_ref()
                .and_then(|failures| failures.get(&uuid).cloned());
            Some(VpnProfileModel {
                uuid,
                name,
                kind,
                active: is_active.is_some(),
                external,
                icon_name: "network-vpn-symbolic".to_string(),
                activation_failed,
            })
        })
        .collect();

    VpnModel {
        available: !profiles.is_empty(),
        profiles,
    }
}

fn ip4_address(managed: &ManagedObjects, device: &HashMap<String, OwnedValue>) -> Option<String> {
    let config = get_object(device, "Ip4Config")?;
    let config = managed.get(&config)?.get(NM_IP4_INTERFACE)?;

    if let Some(data) = config.get("AddressData") {
        if let Ok(entries) = Vec::<HashMap<String, OwnedValue>>::try_from(data.clone()) {
            if let Some(address) = entries
                .first()
                .and_then(|entry| get_string(entry, "address"))
            {
                return Some(address);
            }
        }
    }

    let addresses = config
        .get("Addresses")
        .and_then(|value| Vec::<Vec<u32>>::try_from(value.clone()).ok())?;
    let address = addresses.first()?.first()?;
    // The legacy `Addresses` property stores the address in network byte order.
    Some(Ipv4Addr::from(u32::from_be(*address)).to_string())
}

fn wifi_settings(
    ssid: &str,
    security: SecurityType,
    password: Option<&str>,
) -> Result<SettingsMap, BackendError> {
    let mut settings: SettingsMap = HashMap::new();
    settings.insert(
        "connection".to_string(),
        HashMap::from([
            ("id".to_string(), value(ssid.to_string())),
            ("type".to_string(), value("802-11-wireless")),
            ("uuid".to_string(), value(generate_uuid())),
        ]),
    );
    settings.insert(
        "802-11-wireless".to_string(),
        HashMap::from([
            ("ssid".to_string(), value(ssid.as_bytes().to_vec())),
            ("mode".to_string(), value("infrastructure")),
        ]),
    );
    settings.insert(
        "ipv4".to_string(),
        HashMap::from([("method".to_string(), value("auto"))]),
    );
    settings.insert(
        "ipv6".to_string(),
        HashMap::from([("method".to_string(), value("auto"))]),
    );

    if security.is_secured() {
        let mut wireless_security = HashMap::new();
        match security {
            SecurityType::Owe => {
                wireless_security.insert("key-mgmt".to_string(), value("owe"));
            }
            SecurityType::StaticWep | SecurityType::DynamicWep => {
                wireless_security.insert("key-mgmt".to_string(), value("none"));
                wireless_security.insert(
                    "wep-key-type".to_string(),
                    value(1u32), // NM_WEP_KEY_TYPE_PASSPHRASE
                );
                wireless_security.insert(
                    "wep-key0".to_string(),
                    value(
                        password
                            .ok_or_else(|| missing_password(security))?
                            .to_string(),
                    ),
                );
            }
            SecurityType::WpaPsk | SecurityType::Wpa2Psk => {
                wireless_security.insert("key-mgmt".to_string(), value("wpa-psk"));
                wireless_security.insert(
                    "psk".to_string(),
                    value(
                        password
                            .ok_or_else(|| missing_password(security))?
                            .to_string(),
                    ),
                );
            }
            SecurityType::Wpa3Sae => {
                wireless_security.insert("key-mgmt".to_string(), value("sae"));
                wireless_security.insert(
                    "psk".to_string(),
                    value(
                        password
                            .ok_or_else(|| missing_password(security))?
                            .to_string(),
                    ),
                );
            }
            SecurityType::Wpa2Enterprise
            | SecurityType::WpaEnterprise
            | SecurityType::Wpa3SuiteB192 => {
                return Err(BackendError::Unsupported {
                    backend: "networkmanager",
                    capability: "creating an 802.1X profile from a password",
                });
            }
            _ => {}
        }
        settings.insert("802-11-wireless-security".to_string(), wireless_security);
    }

    Ok(settings)
}

fn set_wifi_security(
    settings: &mut SettingsMap,
    security: SecurityType,
    password: &str,
) -> Result<(), BackendError> {
    let mut wireless_security = HashMap::new();
    match security {
        SecurityType::Owe => {
            wireless_security.insert("key-mgmt".to_string(), value("owe"));
        }
        SecurityType::StaticWep | SecurityType::DynamicWep => {
            wireless_security.insert("key-mgmt".to_string(), value("none"));
            wireless_security.insert("wep-key-type".to_string(), value(1u32));
            wireless_security.insert("wep-key0".to_string(), value(password.to_string()));
        }
        SecurityType::WpaPsk | SecurityType::Wpa2Psk => {
            wireless_security.insert("key-mgmt".to_string(), value("wpa-psk"));
            wireless_security.insert("psk".to_string(), value(password.to_string()));
        }
        SecurityType::Wpa3Sae => {
            wireless_security.insert("key-mgmt".to_string(), value("sae"));
            wireless_security.insert("psk".to_string(), value(password.to_string()));
        }
        SecurityType::Wpa2Enterprise
        | SecurityType::WpaEnterprise
        | SecurityType::Wpa3SuiteB192 => {
            return Err(BackendError::Unsupported {
                backend: "networkmanager",
                capability: "creating an 802.1X profile from a password",
            });
        }
        _ => {}
    }
    settings.insert("802-11-wireless-security".to_string(), wireless_security);
    Ok(())
}

fn missing_password(security: SecurityType) -> BackendError {
    BackendError::Message(format!("{} network needs a password", security.label()))
}

/// Find a wireless profile for this SSID. Checking the kind and the SSID in one
/// predicate matters: the first wireless profile may belong to another network.
fn find_wireless_profile<'a>(
    profiles: &'a [ConnectionProfile],
    ssid: &[u8],
) -> Option<&'a ConnectionProfile> {
    profiles.iter().find(|profile| {
        profile.kind().as_deref() == Some("802-11-wireless")
            && profile.ssid().as_deref() == Some(ssid)
    })
}

fn null_path() -> OwnedObjectPath {
    OwnedObjectPath::try_from("/").expect("the root object path is valid")
}

fn dbus_error(error: zbus::Error) -> BackendError {
    BackendError::Unavailable(error.to_string())
}

fn value<T: Into<Value<'static>>>(value: T) -> OwnedValue {
    OwnedValue::try_from(value.into()).expect("a plain value converts to OwnedValue")
}

fn get_value<'a>(map: &'a HashMap<String, OwnedValue>, key: &str) -> Option<&'a OwnedValue> {
    map.get(key)
}

fn get_string(map: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    get_value(map, key).and_then(|value| String::try_from(value.clone()).ok())
}

fn get_u32(map: &HashMap<String, OwnedValue>, key: &str) -> Option<u32> {
    get_value(map, key).and_then(|value| u32::try_from(value.clone()).ok())
}

fn get_u8(map: &HashMap<String, OwnedValue>, key: &str) -> Option<u8> {
    get_value(map, key).and_then(|value| u8::try_from(value.clone()).ok())
}

fn get_bool(map: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    get_value(map, key).and_then(|value| bool::try_from(value.clone()).ok())
}

fn get_object(map: &HashMap<String, OwnedValue>, key: &str) -> Option<OwnedObjectPath> {
    get_value(map, key).and_then(|value| OwnedObjectPath::try_from(value.clone()).ok())
}

fn get_objects(map: &HashMap<String, OwnedValue>, key: &str) -> Option<Vec<OwnedObjectPath>> {
    get_value(map, key).and_then(|value| Vec::<OwnedObjectPath>::try_from(value.clone()).ok())
}

fn get_bytes(map: &HashMap<String, OwnedValue>, key: &str) -> Option<Vec<u8>> {
    get_value(map, key).and_then(|value| Vec::<u8>::try_from(value.clone()).ok())
}

/// A UUID v4 shaped string. NetworkManager only requires uniqueness.
fn generate_uuid() -> String {
    let mut bytes = [0u8; 16];
    let filled = File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_ok();
    if !filled {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id() as u128;
        bytes = (nanos ^ (pid << 64)).to_le_bytes();
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, ssid: &[u8]) -> ConnectionProfile {
        let mut settings = SettingsMap::new();
        settings.insert(
            "connection".to_string(),
            HashMap::from([
                ("id".to_string(), value(id.to_string())),
                ("type".to_string(), value("802-11-wireless")),
            ]),
        );
        settings.insert(
            "802-11-wireless".to_string(),
            HashMap::from([("ssid".to_string(), value(ssid.to_vec()))]),
        );
        ConnectionProfile {
            path: OwnedObjectPath::try_from(format!(
                "/org/freedesktop/NetworkManager/Settings/{id}"
            ))
            .unwrap(),
            settings,
        }
    }

    #[test]
    fn a_profile_is_found_past_another_wireless_profile() {
        let profiles = [profile("1", b"home"), profile("2", b"cafe")];
        assert_eq!(
            find_wireless_profile(&profiles, b"cafe").unwrap().id(),
            Some("2".to_string())
        );
        assert!(find_wireless_profile(&profiles, b"office").is_none());
    }
}
