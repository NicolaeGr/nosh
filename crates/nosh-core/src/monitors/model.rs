use bitflags::bitflags;

bitflags! {
    /// Capabilities a monitor backend provides.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MonitorCapabilities: u8 {
        const READ = 1 << 0;
        const WATCH = 1 << 1;
        const WRITE = 1 << 2;
        const MIRROR = 1 << 3;
        const ENABLE = 1 << 4;
        const SET_PRIMARY = 1 << 5;
    }
}

impl MonitorCapabilities {
    /// Read and watch, the minimum needed for bars and placement.
    pub const OBSERVE: Self = Self::READ.union(Self::WATCH);
}

/// A monitor identifier.
///
/// Persist and compare [`Self::connector`], since compositor ids can change
/// across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MonitorId {
    pub compositor_id: i32,
    pub connector: String,
    pub description: String,
    pub make: String,
    pub model: String,
    pub serial: String,
}

/// The `wl_output` transform. The numeric values match Hyprland and Wayland.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Transform {
    #[default]
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl Transform {
    pub const fn from_value(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Normal),
            1 => Some(Self::Rotate90),
            2 => Some(Self::Rotate180),
            3 => Some(Self::Rotate270),
            4 => Some(Self::Flipped),
            5 => Some(Self::Flipped90),
            6 => Some(Self::Flipped180),
            7 => Some(Self::Flipped270),
            _ => None,
        }
    }

    pub const fn value(self) -> i32 {
        match self {
            Self::Normal => 0,
            Self::Rotate90 => 1,
            Self::Rotate180 => 2,
            Self::Rotate270 => 3,
            Self::Flipped => 4,
            Self::Flipped90 => 5,
            Self::Flipped180 => 6,
            Self::Flipped270 => 7,
        }
    }

    pub const fn is_flipped(self) -> bool {
        self.value() >= 4
    }
}

/// One monitor, in physical pixels plus the logical scale.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorModel {
    pub id: MonitorId,
    pub enabled: bool,
    pub focused: bool,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f64,
    pub transform: Transform,
    pub refresh_rate: f64,
    pub mirror_of: Option<String>,
}

impl MonitorModel {
    pub fn connector(&self) -> &str {
        &self.id.connector
    }

    /// The width used for layout arithmetic, in logical pixels.
    pub fn logical_width(&self) -> f64 {
        if self.scale > 0.0 {
            self.width as f64 / self.scale
        } else {
            self.width as f64
        }
    }

    pub fn logical_height(&self) -> f64 {
        if self.scale > 0.0 {
            self.height as f64 / self.scale
        } else {
            self.height as f64
        }
    }

    /// Center in compositor coordinates. Placement scores raw geometry, not
    /// logical geometry.
    pub fn center(&self) -> (f64, f64) {
        (
            self.x as f64 + self.width as f64 / 2.0,
            self.y as f64 + self.height as f64 / 2.0,
        )
    }
}

/// The complete monitor layout, including disabled monitors.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MonitorLayout {
    pub monitors: Vec<MonitorModel>,
    pub primary: Option<String>,
}

impl MonitorLayout {
    pub fn enabled(&self) -> impl Iterator<Item = &MonitorModel> {
        self.monitors.iter().filter(|monitor| monitor.enabled)
    }

    pub fn enabled_mut(&mut self) -> impl Iterator<Item = &mut MonitorModel> {
        self.monitors.iter_mut().filter(|monitor| monitor.enabled)
    }

    pub fn monitor(&self, connector: &str) -> Option<&MonitorModel> {
        self.monitors
            .iter()
            .find(|monitor| monitor.connector() == connector)
    }

    pub fn monitor_mut(&mut self, connector: &str) -> Option<&mut MonitorModel> {
        self.monitors
            .iter_mut()
            .find(|monitor| monitor.connector() == connector)
    }

    pub fn focused(&self) -> Option<&MonitorModel> {
        self.enabled().find(|monitor| monitor.focused)
    }

    /// Chooses the primary from the previous value, the focused monitor, then
    /// the first enabled monitor. Returns `None` when no monitor is enabled.
    pub fn choose_primary(&self, previous: Option<&str>) -> Option<String> {
        let enabled = |connector: &str| {
            self.monitors
                .iter()
                .any(|monitor| monitor.enabled && monitor.connector() == connector)
        };

        previous
            .filter(|connector| enabled(connector))
            .map(str::to_string)
            .or_else(|| {
                self.enabled()
                    .find(|monitor| monitor.focused)
                    .map(|monitor| monitor.connector().to_string())
            })
            .or_else(|| {
                self.enabled()
                    .next()
                    .map(|monitor| monitor.connector().to_string())
            })
    }

    /// Recomputes the primary from the previous value and ignores any primary
    /// set by a read backend.
    pub fn apply_primary(&mut self, previous: Option<&str>) -> Option<String> {
        let primary = self.choose_primary(previous);
        self.set_primary(primary.clone());
        primary
    }

    /// Keeps an explicitly requested primary while it is still enabled, then
    /// falls back to [`Self::choose_primary`].
    pub fn respect_primary(&mut self, previous: Option<&str>) -> Option<String> {
        let requested = self
            .primary
            .as_deref()
            .filter(|connector| {
                self.monitors
                    .iter()
                    .any(|monitor| monitor.enabled && monitor.connector() == *connector)
            })
            .map(str::to_string);

        let primary = requested.or_else(|| self.choose_primary(previous));
        self.set_primary(primary.clone());
        primary
    }

    /// Sets the primary connector and the per-monitor flag. The caller decides
    /// whether the connector is valid.
    pub fn set_primary(&mut self, primary: Option<String>) {
        for monitor in &mut self.monitors {
            monitor.primary = primary.as_deref() == Some(monitor.connector());
        }
        self.primary = primary;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(connector: &str, id: i32, enabled: bool, focused: bool) -> MonitorModel {
        MonitorModel {
            id: MonitorId {
                compositor_id: id,
                connector: connector.to_string(),
                description: String::new(),
                make: String::new(),
                model: String::new(),
                serial: String::new(),
            },
            enabled,
            focused,
            primary: false,
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
            transform: Transform::Normal,
            refresh_rate: 60.0,
            mirror_of: None,
        }
    }

    #[test]
    fn transforms_round_trip_the_wayland_values() {
        for value in 0..8 {
            let transform = Transform::from_value(value).unwrap();
            assert_eq!(transform.value(), value);
        }
        assert_eq!(Transform::from_value(8), None);
        assert_eq!(Transform::from_value(-1), None);
        assert!(Transform::Flipped.is_flipped());
        assert!(!Transform::Rotate180.is_flipped());
    }

    #[test]
    fn logical_size_respects_scale() {
        let mut monitor = monitor("DP-1", 0, true, false);
        monitor.width = 3840;
        monitor.height = 2160;
        monitor.scale = 2.0;

        assert_eq!(monitor.logical_width(), 1920.0);
        assert_eq!(monitor.logical_height(), 1080.0);
    }

    #[test]
    fn the_previous_primary_wins_when_it_still_exists() {
        let layout = MonitorLayout {
            monitors: vec![
                monitor("DP-1", 0, true, true),
                monitor("HDMI-A-1", 1, true, false),
            ],
            primary: None,
        };

        assert_eq!(
            layout.choose_primary(Some("HDMI-A-1")).as_deref(),
            Some("HDMI-A-1")
        );
    }

    #[test]
    fn a_removed_or_disabled_primary_falls_back_to_focus_then_order() {
        let layout = MonitorLayout {
            monitors: vec![
                monitor("DP-1", 0, false, false),
                monitor("HDMI-A-1", 1, true, true),
                monitor("DP-2", 2, true, false),
            ],
            primary: None,
        };

        assert_eq!(
            layout.choose_primary(Some("DP-1")).as_deref(),
            Some("HDMI-A-1")
        );

        let no_focus = MonitorLayout {
            monitors: vec![
                monitor("HDMI-A-1", 1, true, false),
                monitor("DP-2", 2, true, false),
            ],
            primary: None,
        };
        assert_eq!(
            no_focus.choose_primary(Some("DP-1")).as_deref(),
            Some("HDMI-A-1")
        );
    }

    #[test]
    fn no_enabled_monitor_has_no_primary() {
        let layout = MonitorLayout {
            monitors: vec![monitor("DP-1", 0, false, true)],
            primary: None,
        };

        assert_eq!(layout.choose_primary(Some("DP-1")), None);
    }

    #[test]
    fn apply_primary_sets_the_model_flags() {
        let mut layout = MonitorLayout {
            monitors: vec![
                monitor("DP-1", 0, true, false),
                monitor("HDMI-A-1", 1, true, true),
            ],
            primary: Some("DP-1".to_string()),
        };

        let primary = layout.apply_primary(None);

        assert_eq!(primary.as_deref(), Some("HDMI-A-1"));
        assert_eq!(layout.primary.as_deref(), Some("HDMI-A-1"));
        assert!(!layout.monitors[0].primary);
        assert!(layout.monitors[1].primary);
    }

    #[test]
    fn respect_primary_keeps_a_valid_explicit_request() {
        let mut layout = MonitorLayout {
            monitors: vec![
                monitor("DP-1", 0, true, true),
                monitor("HDMI-A-1", 1, true, false),
            ],
            primary: Some("HDMI-A-1".to_string()),
        };

        let primary = layout.respect_primary(None);

        assert_eq!(primary.as_deref(), Some("HDMI-A-1"));
        assert!(layout.monitors[1].primary);
    }

    #[test]
    fn respect_primary_ignores_a_request_for_a_missing_monitor() {
        let mut layout = MonitorLayout {
            monitors: vec![monitor("DP-1", 0, true, false)],
            primary: Some("HDMI-A-1".to_string()),
        };

        let primary = layout.respect_primary(None);

        assert_eq!(primary.as_deref(), Some("DP-1"));
    }
}
