//! Network state as the UI renders it.
//!
//! Every value the bar and quick settings show is computed here. The D-Bus
//! backend only parses properties into these types.

use std::collections::BTreeMap;

/// The NetworkManager connectivity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Connectivity {
    #[default]
    Unknown,
    None,
    Portal,
    Limited,
    Full,
}

impl Connectivity {
    pub fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::None,
            2 => Self::Portal,
            3 => Self::Limited,
            4 => Self::Full,
            _ => Self::Unknown,
        }
    }

    pub fn is_full(self) -> bool {
        self == Self::Full
    }
}

/// The NetworkManager device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceState {
    #[default]
    Unknown,
    Unmanaged,
    Unavailable,
    Disconnected,
    Prepare,
    Config,
    NeedAuth,
    IpConfig,
    IpCheck,
    Secondaries,
    Activated,
    Deactivating,
    Failed,
}

impl DeviceState {
    pub fn from_u32(value: u32) -> Self {
        match value {
            10 => Self::Unmanaged,
            20 => Self::Unavailable,
            30 => Self::Disconnected,
            40 => Self::Prepare,
            50 => Self::Config,
            60 => Self::NeedAuth,
            70 => Self::IpConfig,
            80 => Self::IpCheck,
            90 => Self::Secondaries,
            100 => Self::Activated,
            110 => Self::Deactivating,
            120 => Self::Failed,
            _ => Self::Unknown,
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Activated | Self::Secondaries)
    }

    pub fn is_busy(self) -> bool {
        matches!(
            self,
            Self::Prepare
                | Self::Config
                | Self::NeedAuth
                | Self::IpConfig
                | Self::IpCheck
                | Self::Secondaries
        )
    }

    /// The short status text used by the wired and Wi-Fi rows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Unmanaged => "Unmanaged",
            Self::Unavailable => "Unavailable",
            Self::Disconnected => "Disconnected",
            Self::Prepare => "Connecting",
            Self::Config => "Connecting",
            Self::NeedAuth => "Authenticating",
            Self::IpConfig => "Configuring",
            Self::IpCheck => "Checking",
            Self::Secondaries => "Connected",
            Self::Activated => "Connected",
            Self::Deactivating => "Disconnecting",
            Self::Failed => "Failed",
            Self::Unknown => "Unknown",
        }
    }
}

/// The Wi-Fi operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum WifiMode {
    Unknown,
    Adhoc,
    #[default]
    Infrastructure,
    Ap,
    Mesh,
}

impl WifiMode {
    pub fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::Adhoc,
            2 => Self::Infrastructure,
            3 => Self::Ap,
            4 => Self::Mesh,
            _ => Self::Unknown,
        }
    }

    pub fn value(self) -> u32 {
        match self {
            Self::Unknown => 0,
            Self::Adhoc => 1,
            Self::Infrastructure => 2,
            Self::Ap => 3,
            Self::Mesh => 4,
        }
    }
}

/// The security type detected from an access point's flags.
///
/// The order of the variants follows the NetworkManager helper order, strongest
/// first. `security_from_flags` returns the first match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum SecurityType {
    Wpa3SuiteB192,
    Wpa3Sae,
    Owe,
    Wpa2Enterprise,
    Wpa2Psk,
    WpaEnterprise,
    WpaPsk,
    DynamicWep,
    Leap,
    StaticWep,
    #[default]
    None,
    Unknown,
}

pub const AP_FLAG_PRIVACY: u32 = 0x1;
const WPA_PAIR_WEP40: u32 = 0x1;
const WPA_PAIR_WEP104: u32 = 0x2;
const WPA_GROUP_WEP40: u32 = 0x10;
const WPA_GROUP_WEP104: u32 = 0x20;
pub const WPA_KEY_MGMT_PSK: u32 = 0x100;
pub const WPA_KEY_MGMT_802_1X: u32 = 0x200;
pub const WPA_KEY_MGMT_SAE: u32 = 0x400;
pub const WPA_KEY_MGMT_OWE: u32 = 0x800;
pub const WPA_KEY_MGMT_OWE_TM: u32 = 0x1000;
pub const WPA_KEY_MGMT_EAP_SUITE_B_192: u32 = 0x2000;

/// Detect the security type using the NetworkManager helper order.
pub fn security_from_flags(flags: u32, wpa_flags: u32, rsn_flags: u32) -> SecurityType {
    if rsn_flags & WPA_KEY_MGMT_EAP_SUITE_B_192 != 0 {
        return SecurityType::Wpa3SuiteB192;
    }
    if rsn_flags & WPA_KEY_MGMT_SAE != 0 {
        return SecurityType::Wpa3Sae;
    }
    if rsn_flags & (WPA_KEY_MGMT_OWE | WPA_KEY_MGMT_OWE_TM) != 0 {
        return SecurityType::Owe;
    }
    if rsn_flags & WPA_KEY_MGMT_802_1X != 0 {
        return SecurityType::Wpa2Enterprise;
    }
    if rsn_flags & WPA_KEY_MGMT_PSK != 0 {
        return SecurityType::Wpa2Psk;
    }
    if wpa_flags & WPA_KEY_MGMT_802_1X != 0 {
        return SecurityType::WpaEnterprise;
    }
    if wpa_flags & WPA_KEY_MGMT_PSK != 0 {
        return SecurityType::WpaPsk;
    }

    let privacy = flags & AP_FLAG_PRIVACY != 0;
    if !privacy {
        return SecurityType::None;
    }

    // WEP advertised through the WPA/RSN information elements is dynamic WEP.
    // Static WEP sets only the privacy flag.
    let wep_bits = WPA_PAIR_WEP40 | WPA_PAIR_WEP104 | WPA_GROUP_WEP40 | WPA_GROUP_WEP104;
    if (wpa_flags | rsn_flags) & wep_bits != 0 {
        SecurityType::DynamicWep
    } else {
        SecurityType::StaticWep
    }
}

impl SecurityType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wpa3SuiteB192 => "WPA3 Enterprise 192-bit",
            Self::Wpa3Sae => "WPA3",
            Self::Owe => "OWE",
            Self::Wpa2Enterprise => "WPA2 Enterprise",
            Self::Wpa2Psk => "WPA2",
            Self::WpaEnterprise => "WPA Enterprise",
            Self::WpaPsk => "WPA",
            Self::DynamicWep => "Dynamic WEP",
            Self::Leap => "LEAP",
            Self::StaticWep => "WEP",
            Self::None => "Open",
            Self::Unknown => "Unknown",
        }
    }

    /// Whether a password alone can describe this network.
    pub fn requires_password(self) -> bool {
        matches!(
            self,
            Self::StaticWep | Self::WpaPsk | Self::Wpa2Psk | Self::Wpa3Sae
        )
    }

    pub fn is_enterprise(self) -> bool {
        matches!(
            self,
            Self::Wpa2Enterprise | Self::WpaEnterprise | Self::Wpa3SuiteB192
        )
    }

    pub fn is_secured(self) -> bool {
        !matches!(self, Self::None | Self::Unknown)
    }
}

/// One row in the Wi-Fi list. A dual-band router produces one row even when it
/// advertises several BSSIDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessPointModel {
    /// Stable internal id, built from the SSID, mode, and security.
    pub id: String,
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub mode: WifiMode,
    pub security: SecurityType,
    pub strength: u8,
    pub frequency: u32,
    pub bandwidth: u32,
    pub max_bitrate: u32,
    pub requires_password: bool,
    pub has_saved_profile: bool,
    pub access_point_count: usize,
    pub active: bool,
}

impl AccessPointModel {
    /// A single BSSID. `group_access_points` folds members into one row.
    pub fn member(
        ssid: Option<String>,
        bssid: Option<String>,
        mode: WifiMode,
        security: SecurityType,
        strength: u8,
        frequency: u32,
        bandwidth: u32,
        max_bitrate: u32,
        has_saved_profile: bool,
        active: bool,
    ) -> Self {
        Self {
            id: group_key(ssid.as_deref(), mode, security),
            ssid,
            bssid,
            mode,
            security,
            strength,
            frequency,
            bandwidth,
            max_bitrate,
            requires_password: security.requires_password(),
            has_saved_profile,
            access_point_count: 1,
            active,
        }
    }

    pub fn display_name(&self) -> &str {
        self.ssid.as_deref().unwrap_or("Hidden network")
    }
}

/// The key used to group access points. One router with two radios has the same
/// SSID, mode, and security on both.
pub fn group_key(ssid: Option<&str>, mode: WifiMode, security: SecurityType) -> String {
    format!(
        "{}|{}|{}",
        ssid.unwrap_or(""),
        mode.value(),
        security.label()
    )
}

/// Fold per-BSSID rows into one row per network.
///
/// The strongest member supplies the signal, BSSID, frequency, and bandwidth. An
/// active member makes the group active.
pub fn group_access_points(members: Vec<AccessPointModel>) -> Vec<AccessPointModel> {
    let mut groups: BTreeMap<String, Vec<AccessPointModel>> = BTreeMap::new();
    for member in members {
        groups.entry(member.id.clone()).or_default().push(member);
    }

    let mut networks: Vec<AccessPointModel> = groups
        .into_iter()
        .filter_map(|(id, members)| {
            let strongest = members
                .iter()
                .max_by(|left, right| {
                    left.strength
                        .cmp(&right.strength)
                        .then_with(|| right.id.cmp(&left.id))
                })?
                .clone();
            let active = members.iter().any(|member| member.active);
            let has_saved_profile = members.iter().any(|member| member.has_saved_profile);
            Some(AccessPointModel {
                id,
                access_point_count: members.len(),
                active,
                has_saved_profile,
                ..strongest
            })
        })
        .collect();

    sort_networks(&mut networks);
    networks
}

/// Signal strength first, then SSID, as the dropdown renders them.
pub fn sort_networks(networks: &mut [AccessPointModel]) {
    networks.sort_by(|left, right| {
        right
            .strength
            .cmp(&left.strength)
            .then_with(|| left.ssid.cmp(&right.ssid))
            .then_with(|| left.bssid.cmp(&right.bssid))
    });
}

/// The complete Wi-Fi state. It is kept even when no Wi-Fi device exists so the
/// bar widget can hide and return after a hot-plug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiModel {
    pub available: bool,
    pub enabled: bool,
    pub scanning: bool,
    pub state: DeviceState,
    pub connectivity: Connectivity,
    pub active_ssid: Option<String>,
    pub active_bssid: Option<String>,
    /// Strength of the active access point, available even when the full
    /// access point list is not loaded.
    pub active_strength: Option<u8>,
    pub icon_name: String,
    pub access_points: Vec<AccessPointModel>,
    /// Whether the access point list is part of the model. Around zero we only
    /// read the overview, which keeps the radio and the bus quiet.
    pub details_loaded: bool,
}

impl Default for WifiModel {
    fn default() -> Self {
        Self {
            available: false,
            enabled: false,
            scanning: false,
            state: DeviceState::Unavailable,
            connectivity: Connectivity::Unknown,
            active_ssid: None,
            active_bssid: None,
            active_strength: None,
            icon_name: "network-wireless-offline-symbolic".to_string(),
            access_points: Vec::new(),
            details_loaded: false,
        }
    }
}

impl WifiModel {
    /// The icon rules from AstalNetwork.
    pub fn compute_icon(&mut self) {
        self.icon_name = wifi_icon(self).to_string();
    }
}

/// Compute the Wi-Fi icon from the connection and signal state.
pub fn wifi_icon(model: &WifiModel) -> &'static str {
    if !model.available {
        return "network-wireless-offline-symbolic";
    }
    if !model.enabled {
        return "network-wireless-disabled-symbolic";
    }
    if model.state.is_busy() {
        return "network-wireless-acquiring-symbolic";
    }
    if !model.state.is_active() {
        return "network-wireless-offline-symbolic";
    }
    if model.connectivity == Connectivity::Limited {
        return "network-wireless-no-route-symbolic";
    }

    let strength = model
        .access_points
        .iter()
        .find(|network| network.active)
        .map(|network| network.strength)
        .or(model.active_strength)
        .or_else(|| {
            model
                .active_bssid
                .as_ref()
                .and_then(|bssid| {
                    model
                        .access_points
                        .iter()
                        .find(|ap| ap.bssid.as_deref() == Some(bssid))
                })
                .map(|network| network.strength)
        });

    match strength {
        Some(strength) if strength >= 80 => "network-wireless-signal-excellent-symbolic",
        Some(strength) if strength >= 60 => "network-wireless-signal-good-symbolic",
        Some(strength) if strength >= 40 => "network-wireless-signal-ok-symbolic",
        Some(strength) if strength >= 20 => "network-wireless-signal-weak-symbolic",
        Some(_) => "network-wireless-signal-none-symbolic",
        None => "network-wireless-connected-symbolic",
    }
}

/// The icon for an access point's signal strength.
pub fn signal_icon(strength: u8) -> &'static str {
    match strength {
        80..=255 => "network-wireless-signal-excellent-symbolic",
        60..=79 => "network-wireless-signal-good-symbolic",
        40..=59 => "network-wireless-signal-ok-symbolic",
        20..=39 => "network-wireless-signal-weak-symbolic",
        _ => "network-wireless-signal-none-symbolic",
    }
}

/// A wired device, including an interface the user turned off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WiredDeviceModel {
    pub device_path: String,
    pub interface: String,
    pub state: DeviceState,
    pub connected: bool,
    pub enabled: bool,
    pub carrier: bool,
    pub connection_name: Option<String>,
    pub ip4_address: Option<String>,
    pub speed: u32,
}

impl WiredDeviceModel {
    pub fn icon_name(&self) -> &'static str {
        if !self.enabled {
            "network-wired-offline-symbolic"
        } else if self.state.is_busy() {
            "network-wired-acquiring-symbolic"
        } else if self.connected {
            "network-wired-symbolic"
        } else {
            "network-wired-disconnected-symbolic"
        }
    }

    pub fn title(&self) -> &str {
        self.connection_name.as_deref().unwrap_or(&self.interface)
    }

    pub fn status(&self) -> &str {
        if !self.enabled {
            "Off"
        } else if self.connected {
            "Connected"
        } else {
            self.state.label()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnKind {
    Vpn,
    Wireguard,
}

impl VpnKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "vpn" => Some(Self::Vpn),
            "wireguard" => Some(Self::Wireguard),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Vpn => "VPN",
            Self::Wireguard => "WireGuard",
        }
    }
}

/// One configured VPN or WireGuard profile, keyed by UUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnProfileModel {
    pub uuid: String,
    pub name: String,
    pub kind: VpnKind,
    pub active: bool,
    /// The tunnel exists but NetworkManager did not start it. Deactivating it
    /// through NetworkManager would leave the owner out of sync.
    pub external: bool,
    pub icon_name: String,
    pub activation_failed: Option<String>,
}

impl VpnProfileModel {
    pub fn status(&self) -> &str {
        if let Some(reason) = &self.activation_failed {
            reason
        } else if self.active && self.external {
            "External"
        } else if self.active {
            "Connected"
        } else {
            self.kind.label()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VpnModel {
    pub available: bool,
    pub profiles: Vec<VpnProfileModel>,
}

impl VpnModel {
    pub fn active_name(&self) -> Option<&str> {
        self.profiles
            .iter()
            .find(|profile| profile.active)
            .map(|profile| profile.name.as_str())
    }

    pub fn active(&self) -> Option<&VpnProfileModel> {
        self.profiles.iter().find(|profile| profile.active)
    }
}

/// Airplane mode state. Wired devices are never touched by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AirplaneModeModel {
    pub available: bool,
    pub enabled: bool,
    pub transitioning: bool,
}

impl AirplaneModeModel {
    pub fn icon_name(self) -> &'static str {
        if self.enabled {
            "airplane-mode-symbolic"
        } else {
            "airplane-mode-disabled-symbolic"
        }
    }
}

/// The complete network state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkModel {
    pub available: bool,
    pub connectivity: Connectivity,
    pub airplane: AirplaneModeModel,
    pub wired: Vec<WiredDeviceModel>,
    pub wifi: WifiModel,
    pub vpn: VpnModel,
}

impl NetworkModel {
    /// The status shown by the quick settings network toggle.
    pub fn summary(&self) -> &str {
        if self.airplane.enabled {
            return "Airplane";
        }
        if let Some(wired) = self.wired.iter().find(|device| device.connected) {
            return wired.title();
        }
        if let Some(wifi) = &self.wifi.active_ssid {
            return wifi;
        }
        if let Some(name) = self.vpn.active_name() {
            return name;
        }
        if self.wifi.available {
            "Off"
        } else if self.wired.is_empty() {
            "No network"
        } else {
            "Wired"
        }
    }

    /// The icon shown by the quick settings network toggle.
    pub fn icon_name(&self) -> &str {
        if self.airplane.enabled {
            return self.airplane.icon_name();
        }
        if self.wifi.available {
            return &self.wifi.icon_name;
        }
        if let Some(wired) = self.wired.first() {
            return wired.icon_name();
        }
        "network-wireless-offline-symbolic"
    }

    pub fn has_wireless(&self) -> bool {
        self.wifi.available || self.airplane.available
    }
}

/// The NetworkManager SSID-to-UTF-8 rule.
///
/// Valid UTF-8 passes through. Backslashes and control characters are escaped,
/// and every byte that is not part of a valid sequence is written as `\xNN`.
/// This matches `nm_utils_ssid_to_utf8` closely enough for the dropdown.
pub fn ssid_to_utf8(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];

        if byte == b'\\' {
            out.push_str("\\\\");
            index += 1;
            continue;
        }

        if byte < 0x20 || byte == 0x7f {
            out.push_str(&format!("\\x{byte:02x}"));
            index += 1;
            continue;
        }

        // ASCII prints directly.
        if byte < 0x80 {
            out.push(byte as char);
            index += 1;
            continue;
        }

        let remaining = &bytes[index..];
        match std::str::from_utf8(remaining) {
            Ok(text) => {
                out.push_str(&text.chars().next().unwrap_or_default().to_string());
                index += text.chars().next().map(char::len_utf8).unwrap_or(1);
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if valid > 0 {
                    let text = std::str::from_utf8(&remaining[..valid]).unwrap_or_default();
                    out.push_str(text);
                    index += valid;
                } else {
                    out.push_str(&format!("\\x{byte:02x}"));
                    index += 1;
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(
        ssid: Option<&str>,
        bssid: &str,
        strength: u8,
        mode: WifiMode,
        security: SecurityType,
        active: bool,
    ) -> AccessPointModel {
        AccessPointModel::member(
            ssid.map(str::to_string),
            Some(bssid.to_string()),
            mode,
            security,
            strength,
            2412,
            20_000,
            54_000,
            false,
            active,
        )
    }

    #[test]
    fn strong_security_flags_win_in_helper_order() {
        // SAE also carries PSK for transition mode. SAE wins.
        assert_eq!(
            security_from_flags(0, 0, WPA_KEY_MGMT_SAE | WPA_KEY_MGMT_PSK),
            SecurityType::Wpa3Sae
        );
        assert_eq!(
            security_from_flags(0, 0, WPA_KEY_MGMT_EAP_SUITE_B_192),
            SecurityType::Wpa3SuiteB192
        );
        assert_eq!(
            security_from_flags(0, 0, WPA_KEY_MGMT_802_1X),
            SecurityType::Wpa2Enterprise
        );
        assert_eq!(
            security_from_flags(AP_FLAG_PRIVACY, WPA_KEY_MGMT_PSK, 0),
            SecurityType::WpaPsk
        );
        assert_eq!(
            security_from_flags(AP_FLAG_PRIVACY, 0, 0),
            SecurityType::StaticWep
        );
        assert_eq!(security_from_flags(0, 0, 0), SecurityType::None);
        assert_eq!(
            security_from_flags(AP_FLAG_PRIVACY, WPA_PAIR_WEP40, 0),
            SecurityType::DynamicWep
        );
    }

    #[test]
    fn password_requirement_follows_the_security_type() {
        assert!(SecurityType::Wpa2Psk.requires_password());
        assert!(SecurityType::Wpa3Sae.requires_password());
        assert!(SecurityType::StaticWep.requires_password());
        assert!(!SecurityType::None.requires_password());
        assert!(!SecurityType::Owe.requires_password());
        assert!(!SecurityType::Wpa2Enterprise.requires_password());
    }

    #[test]
    fn access_points_with_the_same_identity_group_into_one_row() {
        let grouped = group_access_points(vec![
            member(
                Some("home"),
                "aa",
                40,
                WifiMode::Infrastructure,
                SecurityType::Wpa2Psk,
                false,
            ),
            member(
                Some("home"),
                "bb",
                82,
                WifiMode::Infrastructure,
                SecurityType::Wpa2Psk,
                true,
            ),
        ]);

        assert_eq!(grouped.len(), 1);
        let network = &grouped[0];
        assert_eq!(network.strength, 82);
        assert_eq!(network.access_point_count, 2);
        assert_eq!(network.bssid.as_deref(), Some("bb"));
        assert!(network.active);
    }

    #[test]
    fn different_security_or_mode_stays_separate() {
        let grouped = group_access_points(vec![
            member(
                Some("home"),
                "aa",
                40,
                WifiMode::Infrastructure,
                SecurityType::Wpa2Psk,
                false,
            ),
            member(
                Some("home"),
                "bb",
                82,
                WifiMode::Infrastructure,
                SecurityType::None,
                false,
            ),
            member(
                Some("home"),
                "cc",
                60,
                WifiMode::Adhoc,
                SecurityType::Wpa2Psk,
                false,
            ),
        ]);

        assert_eq!(grouped.len(), 3);
    }

    #[test]
    fn networks_sort_by_strength_then_name() {
        let mut networks = vec![
            member(
                Some("b"),
                "1",
                40,
                WifiMode::Infrastructure,
                SecurityType::None,
                false,
            ),
            member(
                Some("a"),
                "2",
                70,
                WifiMode::Infrastructure,
                SecurityType::None,
                false,
            ),
            member(
                Some("c"),
                "3",
                40,
                WifiMode::Infrastructure,
                SecurityType::None,
                false,
            ),
        ];
        sort_networks(&mut networks);

        let names: Vec<_> = networks.iter().map(|n| n.display_name()).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    #[test]
    fn hidden_networks_get_a_label_instead_of_an_empty_string() {
        let network = member(
            None,
            "aa",
            40,
            WifiMode::Infrastructure,
            SecurityType::None,
            false,
        );
        assert_eq!(network.display_name(), "Hidden network");
    }

    #[test]
    fn wifi_icon_follows_connection_state() {
        let mut model = WifiModel {
            available: true,
            enabled: true,
            state: DeviceState::Activated,
            ..WifiModel::default()
        };
        model.compute_icon();
        assert_eq!(model.icon_name, "network-wireless-connected-symbolic");

        model.access_points = vec![member(
            Some("home"),
            "aa",
            85,
            WifiMode::Infrastructure,
            SecurityType::Wpa2Psk,
            true,
        )];
        model.compute_icon();
        assert_eq!(
            model.icon_name,
            "network-wireless-signal-excellent-symbolic"
        );

        model.enabled = false;
        model.compute_icon();
        assert_eq!(model.icon_name, "network-wireless-disabled-symbolic");

        model.available = false;
        model.compute_icon();
        assert_eq!(model.icon_name, "network-wireless-offline-symbolic");
    }

    #[test]
    fn ssid_bytes_are_escaped_like_networkmanager() {
        assert_eq!(ssid_to_utf8(b"home"), "home");
        assert_eq!(ssid_to_utf8(b"caf\xc3\xa9"), "café");
        assert_eq!(ssid_to_utf8(b"a\x00b"), "a\\x00b");
        assert_eq!(ssid_to_utf8(b"a\xffb"), "a\\xffb");
        assert_eq!(ssid_to_utf8(b"a\\b"), "a\\\\b");
    }

    #[test]
    fn wired_icon_and_status_follow_the_device() {
        let mut device = WiredDeviceModel {
            device_path: "/org/freedesktop/NetworkManager/Devices/1".to_string(),
            interface: "eth0".to_string(),
            state: DeviceState::Activated,
            connected: true,
            enabled: true,
            carrier: true,
            connection_name: Some("Wired connection 1".to_string()),
            ip4_address: Some("192.168.1.2".to_string()),
            speed: 1000,
        };
        assert_eq!(device.icon_name(), "network-wired-symbolic");
        assert_eq!(device.status(), "Connected");
        assert_eq!(device.title(), "Wired connection 1");

        device.enabled = false;
        assert_eq!(device.icon_name(), "network-wired-offline-symbolic");
        assert_eq!(device.status(), "Off");
    }

    #[test]
    fn summary_prefers_airplane_then_wired_then_wifi() {
        let mut model = NetworkModel {
            wifi: WifiModel {
                available: true,
                active_ssid: Some("home".to_string()),
                ..WifiModel::default()
            },
            ..NetworkModel::default()
        };
        assert_eq!(model.summary(), "home");

        model.wired.push(WiredDeviceModel {
            device_path: "/org/freedesktop/NetworkManager/Devices/2".to_string(),
            interface: "eno1".to_string(),
            state: DeviceState::Activated,
            connected: true,
            enabled: true,
            carrier: true,
            connection_name: None,
            ip4_address: Some("192.168.1.10".to_string()),
            speed: 1000,
        });
        assert_eq!(model.summary(), "eno1");

        model.airplane.enabled = true;
        assert_eq!(model.summary(), "Airplane");
    }

    #[test]
    fn an_external_active_tunnel_reads_as_external() {
        let profile = VpnProfileModel {
            uuid: "wg0".to_string(),
            name: "wg0".to_string(),
            kind: VpnKind::Wireguard,
            active: true,
            external: true,
            icon_name: "network-vpn-symbolic".to_string(),
            activation_failed: None,
        };
        assert_eq!(profile.status(), "External");
    }
}
