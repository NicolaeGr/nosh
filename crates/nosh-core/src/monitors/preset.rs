//! Pure layout presets and target resolution.
//!
//! These functions never touch the compositor. They produce a new
//! [`MonitorLayout`] that the service can hand to a write backend.

use std::str::FromStr;

use crate::error::MonitorError;
use crate::monitors::model::{MonitorLayout, MonitorModel};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayoutPreset {
    Extend,
    Mirror,
    Single,
    Panning,
}

impl LayoutPreset {
    /// The default cycle order.
    pub const CYCLE: [Self; 4] = [Self::Extend, Self::Mirror, Self::Single, Self::Panning];

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "extend" => Some(Self::Extend),
            "mirror" => Some(Self::Mirror),
            "single" => Some(Self::Single),
            "panning" | "pan" => Some(Self::Panning),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Extend => "extend",
            Self::Mirror => "mirror",
            Self::Single => "single",
            Self::Panning => "panning",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Extend => Self::Mirror,
            Self::Mirror => Self::Single,
            Self::Single => Self::Panning,
            Self::Panning => Self::Extend,
        }
    }
}

impl FromStr for LayoutPreset {
    type Err = MonitorError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value).ok_or_else(|| MonitorError::UnknownPreset(value.to_string()))
    }
}

/// Resolves a connector name or a 1-based index in the enabled list sorted by
/// compositor id.
pub fn resolve_target<'a>(
    layout: &'a MonitorLayout,
    target: &str,
) -> Result<&'a MonitorModel, MonitorError> {
    let target = target.trim();
    if target.is_empty() {
        return layout
            .focused()
            .or_else(|| layout.enabled().next())
            .ok_or(MonitorError::NoTarget);
    }

    if target.chars().all(|character| character.is_ascii_digit()) {
        let index: usize = target
            .parse()
            .map_err(|_| MonitorError::TargetNotFound(target.to_string()))?;
        if index == 0 {
            return Err(MonitorError::TargetNotFound(target.to_string()));
        }

        let mut enabled: Vec<&MonitorModel> = layout.enabled().collect();
        enabled.sort_by_key(|monitor| monitor.id.compositor_id);
        return enabled
            .get(index - 1)
            .copied()
            .ok_or_else(|| MonitorError::TargetNotFound(target.to_string()));
    }

    layout
        .monitor(target)
        .ok_or_else(|| MonitorError::TargetNotFound(target.to_string()))
}

/// The default layout: enabled monitors left to right by logical width.
pub fn extend(layout: &MonitorLayout) -> MonitorLayout {
    let mut next = layout.clone();
    for monitor in &mut next.monitors {
        monitor.mirror_of = None;
    }
    lay_out_horizontally(&mut next);
    next
}

/// Extend with scale fixed at 1.0.
pub fn panning(layout: &MonitorLayout) -> MonitorLayout {
    let mut next = layout.clone();
    for monitor in &mut next.monitors {
        if monitor.enabled {
            monitor.scale = 1.0;
            monitor.mirror_of = None;
        }
    }
    lay_out_horizontally(&mut next);
    next
}

fn lay_out_horizontally(layout: &mut MonitorLayout) {
    let mut enabled: Vec<usize> = layout
        .monitors
        .iter()
        .enumerate()
        .filter_map(|(index, monitor)| monitor.enabled.then_some(index))
        .collect();
    enabled.sort_by_key(|index| layout.monitors[*index].id.compositor_id);

    let mut x: f64 = 0.0;
    for index in enabled {
        let monitor = &mut layout.monitors[index];
        monitor.x = x.round() as i32;
        monitor.y = 0;
        x += monitor.logical_width();
    }
}

/// Mirrors the target monitor from the primary, or from another enabled
/// monitor when the target is the primary.
pub fn mirror(layout: &MonitorLayout, target: Option<&str>) -> Result<MonitorLayout, MonitorError> {
    let mut next = layout.clone();
    let target = match target {
        Some(target) => resolve_target(&next, target)?,
        None => next
            .primary
            .as_deref()
            .and_then(|connector| next.monitor(connector))
            .or_else(|| next.focused())
            .or_else(|| next.enabled().next())
            .ok_or(MonitorError::NoMirrorSource)?,
    };
    let target_connector = target.connector().to_string();

    let source = next
        .primary
        .as_deref()
        .filter(|connector| *connector != target_connector)
        .and_then(|connector| next.monitor(connector))
        .filter(|monitor| monitor.enabled)
        .or_else(|| {
            let mut enabled: Vec<&MonitorModel> = next
                .enabled()
                .filter(|monitor| monitor.connector() != target_connector)
                .collect();
            enabled.sort_by_key(|monitor| monitor.id.compositor_id);
            enabled.first().copied()
        })
        .ok_or(MonitorError::NoMirrorSource)?;

    let source_connector = source.connector().to_string();
    let source_position = (source.x, source.y);
    let target = next
        .monitor_mut(&target_connector)
        .ok_or_else(|| MonitorError::TargetNotFound(target_connector.clone()))?;
    target.enabled = true;
    target.x = source_position.0;
    target.y = source_position.1;
    target.mirror_of = Some(source_connector);
    Ok(next)
}

/// Disables every monitor except the target.
pub fn single(layout: &MonitorLayout, target: Option<&str>) -> Result<MonitorLayout, MonitorError> {
    let mut next = layout.clone();
    let target_connector = match target {
        Some(target) => resolve_target(&next, target)?.connector().to_string(),
        None => next
            .primary
            .clone()
            .or_else(|| {
                next.focused()
                    .map(|monitor| monitor.connector().to_string())
            })
            .or_else(|| {
                next.enabled()
                    .next()
                    .map(|monitor| monitor.connector().to_string())
            })
            .ok_or(MonitorError::NoTarget)?,
    };

    for monitor in &mut next.monitors {
        monitor.enabled = monitor.connector() == target_connector;
        monitor.mirror_of = None;
        if monitor.enabled {
            monitor.x = 0;
            monitor.y = 0;
        }
    }
    next.set_primary(Some(target_connector));
    Ok(next)
}

/// Applies a preset. `Extend` and `Panning` ignore the target.
pub fn preset_layout(
    layout: &MonitorLayout,
    preset: LayoutPreset,
    target: Option<&str>,
) -> Result<MonitorLayout, MonitorError> {
    match preset {
        LayoutPreset::Extend => Ok(extend(layout)),
        LayoutPreset::Mirror => mirror(layout, target),
        LayoutPreset::Single => single(layout, target),
        LayoutPreset::Panning => Ok(panning(layout)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::model::{MonitorId, Transform};

    fn monitor(connector: &str, id: i32, x: i32, y: i32, width: i32, scale: f64) -> MonitorModel {
        MonitorModel {
            id: MonitorId {
                compositor_id: id,
                connector: connector.to_string(),
                description: String::new(),
                make: String::new(),
                model: String::new(),
                serial: String::new(),
            },
            enabled: true,
            focused: false,
            primary: false,
            x,
            y,
            width,
            height: 1080,
            scale,
            transform: Transform::Normal,
            refresh_rate: 60.0,
            mirror_of: None,
        }
    }

    fn layout() -> MonitorLayout {
        let mut layout = MonitorLayout {
            monitors: vec![
                monitor("HDMI-A-1", 2, 3000, 100, 1920, 1.0),
                monitor("DP-1", 0, 0, 0, 2560, 1.0),
                monitor("DP-2", 1, 5000, 0, 3840, 2.0),
            ],
            primary: None,
        };
        layout.set_primary(Some("DP-1".to_string()));
        layout
    }

    #[test]
    fn extend_clears_mirrors_and_places_by_logical_width_in_id_order() {
        let mut layout = layout();
        layout.monitors[1].mirror_of = Some("DP-2".to_string());

        let extended = extend(&layout);

        let dp1 = extended.monitor("DP-1").unwrap();
        let dp2 = extended.monitor("DP-2").unwrap();
        let hdmi = extended.monitor("HDMI-A-1").unwrap();
        assert_eq!((dp1.x, dp1.y), (0, 0));
        assert_eq!((dp2.x, dp2.y), (2560, 0));
        assert_eq!((hdmi.x, hdmi.y), (2560 + 1920, 0));
        assert!(extended.monitors.iter().all(|m| m.mirror_of.is_none()));
    }

    #[test]
    fn extend_leaves_disabled_monitors_in_place_and_out_of_the_strip() {
        let mut layout = layout();
        layout.monitors[0].enabled = false;

        let extended = extend(&layout);

        assert_eq!(extended.monitor("HDMI-A-1").unwrap().x, 3000);
        assert_eq!(extended.monitor("DP-2").unwrap().x, 2560);
    }

    #[test]
    fn panning_uses_one_scale_with_linear_positions() {
        let extended = panning(&layout());

        assert_eq!(extended.monitor("DP-1").unwrap().x, 0);
        assert_eq!(extended.monitor("DP-2").unwrap().x, 2560);
        assert_eq!(extended.monitor("HDMI-A-1").unwrap().x, 2560 + 3840);
        assert!(extended.enabled().all(|monitor| monitor.scale == 1.0));
        assert!(extended.monitors.iter().all(|m| m.mirror_of.is_none()));
    }

    #[test]
    fn mirror_uses_the_primary_and_keeps_the_targets_geometry() {
        let mirrored = mirror(&layout(), Some("HDMI-A-1")).unwrap();

        let hdmi = mirrored.monitor("HDMI-A-1").unwrap();
        assert_eq!(hdmi.mirror_of.as_deref(), Some("DP-1"));
        assert_eq!(hdmi.width, 1920);
        assert_eq!(hdmi.scale, 1.0);
        assert_eq!((hdmi.x, hdmi.y), (0, 0));
    }

    #[test]
    fn mirroring_the_primary_picks_another_enabled_source() {
        let mirrored = mirror(&layout(), Some("DP-1")).unwrap();

        let dp1 = mirrored.monitor("DP-1").unwrap();
        assert_eq!(dp1.mirror_of.as_deref(), Some("DP-2"));
    }

    #[test]
    fn mirror_without_a_target_uses_the_primary() {
        let mirrored = mirror(&layout(), None).unwrap();

        assert_eq!(
            mirrored.monitor("DP-1").unwrap().mirror_of.as_deref(),
            Some("DP-2")
        );
    }

    #[test]
    fn single_disables_every_other_monitor() {
        let single = single(&layout(), Some("DP-2")).unwrap();

        assert!(single.monitor("DP-2").unwrap().enabled);
        assert!(!single.monitor("DP-1").unwrap().enabled);
        assert!(!single.monitor("HDMI-A-1").unwrap().enabled);
        assert_eq!(single.primary.as_deref(), Some("DP-2"));
        assert_eq!(
            (
                single.monitor("DP-2").unwrap().x,
                single.monitor("DP-2").unwrap().y
            ),
            (0, 0)
        );
    }

    #[test]
    fn presets_dispatch_and_reject_a_missing_target() {
        assert!(preset_layout(&layout(), LayoutPreset::Extend, None).is_ok());
        assert!(preset_layout(&layout(), LayoutPreset::Panning, None).is_ok());
        assert!(preset_layout(&layout(), LayoutPreset::Mirror, Some("NOPE")).is_err());
        assert!(preset_layout(&layout(), LayoutPreset::Single, Some("NOPE")).is_err());
    }

    #[test]
    fn numeric_targets_are_one_based_in_enabled_compositor_id_order() {
        let layout = layout();

        assert_eq!(resolve_target(&layout, "1").unwrap().connector(), "DP-1");
        assert_eq!(resolve_target(&layout, "2").unwrap().connector(), "DP-2");
        assert_eq!(
            resolve_target(&layout, "3").unwrap().connector(),
            "HDMI-A-1"
        );
        assert!(matches!(
            resolve_target(&layout, "0"),
            Err(MonitorError::TargetNotFound(_))
        ));
        assert!(matches!(
            resolve_target(&layout, "4"),
            Err(MonitorError::TargetNotFound(_))
        ));
    }

    #[test]
    fn an_empty_target_uses_the_focused_monitor_then_the_first_enabled_one() {
        let mut layout = layout();
        layout.monitors[1].focused = true;

        assert_eq!(resolve_target(&layout, "  ").unwrap().connector(), "DP-1");

        layout.monitors[1].focused = false;
        assert_eq!(resolve_target(&layout, "").unwrap().connector(), "HDMI-A-1");

        layout.monitors[0].enabled = false;
        layout.monitors[1].enabled = false;
        assert_eq!(resolve_target(&layout, "").unwrap().connector(), "DP-2");

        layout.monitors[2].enabled = false;
        assert!(matches!(
            resolve_target(&layout, ""),
            Err(MonitorError::NoTarget)
        ));
    }

    #[test]
    fn numeric_targets_skip_disabled_monitors() {
        let mut layout = layout();
        layout.monitors[1].enabled = false;

        assert_eq!(resolve_target(&layout, "1").unwrap().connector(), "DP-2");
        assert!(matches!(
            resolve_target(&layout, "3"),
            Err(MonitorError::TargetNotFound(_))
        ));
    }

    #[test]
    fn named_targets_accept_disabled_monitors() {
        let mut layout = layout();
        layout.monitors[0].enabled = false;

        assert_eq!(
            resolve_target(&layout, "HDMI-A-1")
                .unwrap()
                .id
                .compositor_id,
            2
        );
        assert!(matches!(
            resolve_target(&layout, "MISSING"),
            Err(MonitorError::TargetNotFound(_))
        ));
    }

    #[test]
    fn presets_parse_and_cycle_in_the_spec_order() {
        assert_eq!(LayoutPreset::parse("extend"), Some(LayoutPreset::Extend));
        assert_eq!(LayoutPreset::parse("mirror"), Some(LayoutPreset::Mirror));
        assert_eq!(LayoutPreset::parse("single"), Some(LayoutPreset::Single));
        assert_eq!(LayoutPreset::parse("panning"), Some(LayoutPreset::Panning));
        assert_eq!(LayoutPreset::parse("nonsense"), None);
        assert_eq!(LayoutPreset::Extend.next(), LayoutPreset::Mirror);
        assert_eq!(LayoutPreset::Mirror.next(), LayoutPreset::Single);
        assert_eq!(LayoutPreset::Single.next(), LayoutPreset::Panning);
        assert_eq!(LayoutPreset::Panning.next(), LayoutPreset::Extend);
        assert_eq!(
            "panning".parse::<LayoutPreset>().unwrap(),
            LayoutPreset::Panning
        );
        assert!("nonsense".parse::<LayoutPreset>().is_err());
    }
}
