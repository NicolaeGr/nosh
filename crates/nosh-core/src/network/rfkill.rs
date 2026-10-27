//! The rfkill switch state in `/sys/class/rfkill`.
//!
//! Airplane mode soft-blocks every wireless radio here and lets
//! NetworkManager disable its own Wi-Fi and WWAN devices. Wired interfaces have
//! no rfkill entry and are never touched.
//!
//! Writing `soft` in sysfs needs root. The `/dev/rfkill` character device is
//! usually writable by the session owner, so airplane mode sends an event there
//! first and falls back to sysfs. A failed write is logged and does not stop the
//! rest of airplane mode.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const RFKILL_ROOT: &str = "/sys/class/rfkill";
/// The character device accepts writes even when the sysfs `soft` attribute is
/// root-only. A session that owns the device can change every radio through it.
pub const RFKILL_DEVICE: &str = "/dev/rfkill";
const RFKILL_OP_CHANGE: u8 = 2;

/// The `type` attribute values from the kernel rfkill API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RfkillKind {
    All,
    Wlan,
    Bluetooth,
    UltraWideband,
    Wimax,
    Wwan,
    Gps,
    Other(u32),
}

impl RfkillKind {
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => Self::All,
            1 => Self::Wlan,
            2 => Self::Bluetooth,
            3 => Self::UltraWideband,
            4 => Self::Wimax,
            5 => Self::Wwan,
            6 => Self::Gps,
            other => Self::Other(other),
        }
    }

    /// The kernel value used by the `/dev/rfkill` event struct.
    pub fn value(self) -> u8 {
        match self {
            Self::All => 0,
            Self::Wlan => 1,
            Self::Bluetooth => 2,
            Self::UltraWideband => 3,
            Self::Wimax => 4,
            Self::Wwan => 5,
            Self::Gps => 6,
            Self::Other(other) => other.min(u32::from(u8::MAX)) as u8,
        }
    }

    /// The kernel writes names such as `wlan` and `bluetooth` into the sysfs
    /// `type` attribute. Accept the numeric values too.
    pub fn parse(value: &str) -> Self {
        let value = value.trim();
        match value.to_ascii_lowercase().as_str() {
            "all" => Self::All,
            "wlan" | "wifi" => Self::Wlan,
            "bluetooth" | "bt" => Self::Bluetooth,
            "uwb" | "ultra-wideband" | "ultrawideband" => Self::UltraWideband,
            "wimax" => Self::Wimax,
            "wwan" | "mobile" => Self::Wwan,
            "gps" => Self::Gps,
            _ => value
                .parse::<u32>()
                .map(Self::from_u32)
                .unwrap_or(Self::Other(u32::MAX)),
        }
    }

    /// A radio that airplane mode should block. Wired interfaces have no
    /// rfkill entry, so this list is exactly the wireless set.
    pub fn is_wireless(self) -> bool {
        !matches!(self, Self::All)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Wireless",
            Self::Wlan => "Wi-Fi",
            Self::Bluetooth => "Bluetooth",
            Self::UltraWideband => "UWB",
            Self::Wimax => "WiMAX",
            Self::Wwan => "Mobile",
            Self::Gps => "GPS",
            Self::Other(_) => "Wireless",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RfkillDevice {
    pub index: u32,
    pub name: String,
    pub kind: RfkillKind,
    pub soft_blocked: bool,
    pub hard_blocked: bool,
}

impl RfkillDevice {
    pub fn path(&self, root: &Path) -> PathBuf {
        root.join(format!("rfkill{}", self.index))
    }
}

/// Read every rfkill switch. A missing directory means rfkill is not available;
/// that is reported as an empty list, not an error.
pub fn read_devices(root: &Path) -> io::Result<Vec<RfkillDevice>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut devices = Vec::new();
    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(index) = name.strip_prefix("rfkill") else {
            continue;
        };
        let Ok(index) = index.parse::<u32>() else {
            continue;
        };
        let path = entry.path();

        let kind = read_string(&path.join("type"))
            .map(|value| RfkillKind::parse(&value))
            .unwrap_or(RfkillKind::Other(u32::MAX));
        let name = read_string(&path.join("name")).unwrap_or_else(|| name.to_string());
        let soft_blocked = read_u32(&path.join("soft")).unwrap_or(0) != 0;
        let hard_blocked = read_u32(&path.join("hard")).unwrap_or(0) != 0;

        devices.push(RfkillDevice {
            index,
            name,
            kind,
            soft_blocked,
            hard_blocked,
        });
    }

    devices.sort_by_key(|device| device.index);
    Ok(devices)
}

/// Whether any wireless switch exists.
pub fn is_available(root: &Path) -> bool {
    read_devices(root)
        .map(|devices| devices.iter().any(|device| device.kind.is_wireless()))
        .unwrap_or(false)
}

/// Whether a radio of this kind exists, for example a Wi-Fi radio that
/// NetworkManager removed from the bus because Wi-Fi is disabled.
pub fn has_kind(root: &Path, kind: RfkillKind) -> bool {
    read_devices(root)
        .map(|devices| devices.iter().any(|device| device.kind == kind))
        .unwrap_or(false)
}

/// Soft-block or unblock every wireless radio through sysfs. Returns the
/// devices that could not be written, so the caller can log them once.
pub fn set_wireless_soft_blocked(root: &Path, blocked: bool) -> Vec<(String, io::Error)> {
    set_wireless_soft_blocked_with(root, blocked, None)
}

/// Like [`set_wireless_soft_blocked`], but prefers the `/dev/rfkill` character
/// device. Writes there are usually allowed for the session owner even when the
/// sysfs attribute is root-only. A failed write falls back to sysfs.
pub fn set_wireless_soft_blocked_via(
    root: &Path,
    blocked: bool,
    device_path: &Path,
) -> Vec<(String, io::Error)> {
    set_wireless_soft_blocked_with(root, blocked, Some(device_path))
}

fn set_wireless_soft_blocked_with(
    root: &Path,
    blocked: bool,
    device_path: Option<&Path>,
) -> Vec<(String, io::Error)> {
    let Ok(devices) = read_devices(root) else {
        return Vec::new();
    };

    let mut failures = Vec::new();
    for device in devices
        .iter()
        .filter(|device| device.kind.is_wireless() && !device.hard_blocked)
    {
        if device.soft_blocked == blocked {
            continue;
        }
        if let Err(error) = change_soft(root, device, blocked, device_path) {
            failures.push((device.name.clone(), error));
        }
    }
    failures
}

/// Restore each wireless radio to the soft-block state captured before
/// airplane mode was enabled. This uses sysfs; [`restore_soft_state_via`]
/// prefers the rfkill character device.
pub fn restore_soft_state(
    root: &Path,
    states: &[(RfkillDevice, bool)],
) -> Vec<(String, io::Error)> {
    restore_soft_state_with(root, states, None)
}

pub fn restore_soft_state_via(
    root: &Path,
    states: &[(RfkillDevice, bool)],
    device_path: &Path,
) -> Vec<(String, io::Error)> {
    restore_soft_state_with(root, states, Some(device_path))
}

fn restore_soft_state_with(
    root: &Path,
    states: &[(RfkillDevice, bool)],
    device_path: Option<&Path>,
) -> Vec<(String, io::Error)> {
    let mut failures = Vec::new();
    for (device, soft_blocked) in states {
        if device.hard_blocked {
            continue;
        }
        if let Err(error) = change_soft(root, device, *soft_blocked, device_path) {
            failures.push((device.name.clone(), error));
        }
    }
    failures
}

/// Write one rfkill change. The sysfs fallback runs when the device is absent,
/// cannot be opened, or rejects the event.
fn change_soft(
    root: &Path,
    device: &RfkillDevice,
    blocked: bool,
    device_path: Option<&Path>,
) -> io::Result<()> {
    if let Some(device_path) = device_path {
        if let Ok(mut file) = fs::OpenOptions::new().write(true).open(device_path) {
            let event = rfkill_event(device.index, device.kind, blocked);
            if file.write_all(&event).is_ok() {
                return Ok(());
            }
        }
    }
    fs::write(
        device.path(root).join("soft"),
        if blocked { "1" } else { "0" },
    )
}

/// Encode `struct rfkill_event`: index (u32), type, op, soft, hard.
fn rfkill_event(index: u32, kind: RfkillKind, blocked: bool) -> [u8; 8] {
    let mut event = [0u8; 8];
    event[..4].copy_from_slice(&index.to_ne_bytes());
    event[4] = kind.value();
    event[5] = RFKILL_OP_CHANGE;
    event[6] = u8::from(blocked);
    event
}

fn read_string(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_u32(path: &Path) -> Option<u32> {
    read_string(path)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(root: &Path, index: u32, kind: u32, soft: u32, hard: u32, name: &str) {
        let path = root.join(format!("rfkill{index}"));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("type"), kind.to_string()).unwrap();
        fs::write(path.join("soft"), soft.to_string()).unwrap();
        fs::write(path.join("hard"), hard.to_string()).unwrap();
        fs::write(path.join("name"), name).unwrap();
    }

    #[test]
    fn rfkill_events_encode_the_kernel_layout() {
        let event = rfkill_event(3, RfkillKind::Bluetooth, true);
        assert_eq!(&event[..4], &3u32.to_ne_bytes());
        assert_eq!(event[4], 2);
        assert_eq!(event[5], 2);
        assert_eq!(event[6], 1);
        assert_eq!(event[7], 0);

        let event = rfkill_event(7, RfkillKind::Wlan, false);
        assert_eq!(event[4], 1);
        assert_eq!(event[6], 0);
    }

    #[test]
    fn devices_are_read_and_sorted_by_index() {
        let directory = tempfile::tempdir().unwrap();
        device(directory.path(), 2, 2, 0, 0, "hci0");
        device(directory.path(), 0, 1, 1, 0, "phy0");

        let devices = read_devices(directory.path()).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].index, 0);
        assert_eq!(devices[0].kind, RfkillKind::Wlan);
        assert!(devices[0].soft_blocked);
        assert_eq!(devices[1].kind, RfkillKind::Bluetooth);
    }

    #[test]
    fn a_missing_directory_is_an_empty_list() {
        assert_eq!(read_devices(Path::new("/nonexistent/rfkill")).unwrap(), []);
        assert!(!is_available(Path::new("/nonexistent/rfkill")));
    }

    #[test]
    fn soft_blocking_skips_hard_blocked_and_already_blocked_devices() {
        let directory = tempfile::tempdir().unwrap();
        device(directory.path(), 0, 1, 0, 0, "phy0");
        device(directory.path(), 1, 2, 1, 0, "hci0");
        device(directory.path(), 2, 5, 0, 1, "wwan0");

        let failures = set_wireless_soft_blocked(directory.path(), true);
        assert!(failures.is_empty());

        assert_eq!(
            fs::read_to_string(directory.path().join("rfkill0/soft")).unwrap(),
            "1"
        );
        // Already blocked stays blocked.
        assert_eq!(
            fs::read_to_string(directory.path().join("rfkill1/soft")).unwrap(),
            "1"
        );
        // Hard blocked is left alone.
        assert_eq!(
            fs::read_to_string(directory.path().join("rfkill2/soft")).unwrap(),
            "0"
        );
    }

    #[test]
    fn recorded_state_is_restored() {
        let directory = tempfile::tempdir().unwrap();
        device(directory.path(), 0, 1, 0, 0, "phy0");
        let devices = read_devices(directory.path()).unwrap();

        set_wireless_soft_blocked(directory.path(), true);
        restore_soft_state(
            directory.path(),
            &devices
                .iter()
                .map(|device| (device.clone(), device.soft_blocked))
                .collect::<Vec<_>>(),
        );

        assert_eq!(
            fs::read_to_string(directory.path().join("rfkill0/soft")).unwrap(),
            "0"
        );
    }

    #[test]
    fn sysfs_type_names_are_parsed() {
        assert_eq!(RfkillKind::parse("wlan"), RfkillKind::Wlan);
        assert_eq!(RfkillKind::parse("bluetooth"), RfkillKind::Bluetooth);
        assert_eq!(RfkillKind::parse("wwan"), RfkillKind::Wwan);
        assert_eq!(RfkillKind::parse("1"), RfkillKind::Wlan);
        assert_eq!(RfkillKind::parse("nonsense"), RfkillKind::Other(u32::MAX));
    }

    #[test]
    fn wifi_and_mobile_count_as_wireless() {
        assert!(RfkillKind::Wlan.is_wireless());
        assert!(RfkillKind::Bluetooth.is_wireless());
        assert!(RfkillKind::Wwan.is_wireless());
        assert!(!RfkillKind::All.is_wireless());
    }
}
