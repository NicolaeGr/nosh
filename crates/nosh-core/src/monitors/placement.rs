//! Pure monitor placement rules.
//!
//! The score is defined by `docs/features/monitors.md`. Ties are broken by the
//! distance from the primary center (farther wins), then by connector name.

use std::str::FromStr;

use crate::error::MonitorError;
use crate::monitors::model::MonitorLayout;
use crate::monitors::model::MonitorModel;

/// The corner used by notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Self; 4] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
    ];

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "top-left" | "top_left" | "topleft" => Some(Self::TopLeft),
            "top-right" | "top_right" | "topright" => Some(Self::TopRight),
            "bottom-left" | "bottom_left" | "bottomleft" => Some(Self::BottomLeft),
            "bottom-right" | "bottom_right" | "bottomright" => Some(Self::BottomRight),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
        }
    }
}

impl FromStr for Corner {
    type Err = MonitorError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value).ok_or_else(|| MonitorError::UnknownPolicy(value.to_string()))
    }
}

/// The exact score from the feature spec.
pub fn score(monitor: &MonitorModel, corner: Corner) -> i64 {
    let left = monitor.x as i64;
    let top = monitor.y as i64;
    let right = (monitor.x + monitor.width) as i64;
    let bottom = (monitor.y + monitor.height) as i64;

    match corner {
        Corner::TopLeft => -left * 2 - top,
        Corner::TopRight => right * 2 - top,
        Corner::BottomLeft => -left * 2 + bottom,
        Corner::BottomRight => right * 2 + bottom,
    }
}

/// How to choose the monitor for notifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitorPolicy {
    /// The default: the highest corner score.
    Farthest,
    /// Always the primary monitor.
    Primary,
    /// The focused monitor.
    Current,
    /// A specific connector.
    Named(String),
}

impl MonitorPolicy {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        match value.to_ascii_lowercase().as_str() {
            "farthest" | "" => Some(Self::Farthest),
            "primary" => Some(Self::Primary),
            "current" | "focused" => Some(Self::Current),
            _ => value
                .strip_prefix("named:")
                .filter(|connector| !connector.is_empty())
                .map(|connector| Self::Named(connector.to_string())),
        }
    }
}

impl FromStr for MonitorPolicy {
    type Err = MonitorError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value).ok_or_else(|| MonitorError::UnknownPolicy(value.to_string()))
    }
}

/// Chooses the enabled monitor for a corner under the given policy.
///
/// When no monitor is enabled the whole list is used, which matches the spec's
/// fallback to the primary or first monitor.
pub fn select_monitor<'a>(
    layout: &'a MonitorLayout,
    corner: Corner,
    policy: &MonitorPolicy,
) -> Option<&'a MonitorModel> {
    let candidates: Vec<&MonitorModel> = layout.enabled().collect();
    if candidates.is_empty() {
        // With nothing enabled the spec asks for the primary or first monitor,
        // not a score over disabled geometry.
        return layout
            .primary
            .as_deref()
            .and_then(|connector| layout.monitor(connector))
            .or_else(|| layout.monitors.first());
    }

    match policy {
        MonitorPolicy::Farthest => farthest(layout, &candidates, corner),
        MonitorPolicy::Primary => primary(layout, &candidates),
        MonitorPolicy::Current => current(layout, &candidates),
        MonitorPolicy::Named(connector) => candidates
            .into_iter()
            .find(|monitor| monitor.connector() == connector),
    }
}

fn farthest<'a>(
    layout: &MonitorLayout,
    candidates: &[&'a MonitorModel],
    corner: Corner,
) -> Option<&'a MonitorModel> {
    let primary_center = layout
        .primary
        .as_deref()
        .and_then(|connector| layout.monitor(connector))
        .map(MonitorModel::center);

    let mut best: Option<&MonitorModel> = None;
    for candidate in candidates {
        match best {
            None => best = Some(candidate),
            Some(current) => {
                if is_better(candidate, current, corner, primary_center) {
                    best = Some(candidate);
                }
            }
        }
    }
    best
}

fn is_better(
    candidate: &MonitorModel,
    current: &MonitorModel,
    corner: Corner,
    primary_center: Option<(f64, f64)>,
) -> bool {
    match score(candidate, corner).cmp(&score(current, corner)) {
        std::cmp::Ordering::Greater => return true,
        std::cmp::Ordering::Less => return false,
        std::cmp::Ordering::Equal => {}
    }

    match distance(candidate, primary_center).total_cmp(&distance(current, primary_center)) {
        std::cmp::Ordering::Greater => return true,
        std::cmp::Ordering::Less => return false,
        std::cmp::Ordering::Equal => {}
    }

    candidate.connector() < current.connector()
}

fn distance(monitor: &MonitorModel, primary_center: Option<(f64, f64)>) -> f64 {
    let Some((primary_x, primary_y)) = primary_center else {
        return 0.0;
    };
    let (x, y) = monitor.center();
    (x - primary_x).hypot(y - primary_y)
}

fn primary<'a>(
    layout: &MonitorLayout,
    candidates: &[&'a MonitorModel],
) -> Option<&'a MonitorModel> {
    layout
        .primary
        .as_deref()
        .and_then(|connector| {
            candidates
                .iter()
                .copied()
                .find(|monitor| monitor.connector() == connector)
        })
        .or_else(|| candidates.iter().copied().find(|monitor| monitor.focused))
        .or_else(|| candidates.first().copied())
}

fn current<'a>(
    _layout: &MonitorLayout,
    candidates: &[&'a MonitorModel],
) -> Option<&'a MonitorModel> {
    candidates
        .iter()
        .copied()
        .find(|monitor| monitor.focused)
        .or_else(|| candidates.iter().copied().find(|monitor| monitor.primary))
        .or_else(|| candidates.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::model::{MonitorId, Transform};

    fn monitor(connector: &str, x: i32, y: i32, width: i32, height: i32) -> MonitorModel {
        MonitorModel {
            id: MonitorId {
                compositor_id: 0,
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
            height,
            scale: 1.0,
            transform: Transform::Normal,
            refresh_rate: 60.0,
            mirror_of: None,
        }
    }

    fn layout(monitors: Vec<MonitorModel>, primary: &str) -> MonitorLayout {
        let mut layout = MonitorLayout {
            monitors,
            primary: None,
        };
        layout.set_primary(Some(primary.to_string()));
        layout
    }

    #[test]
    fn the_score_matches_the_spec_for_all_four_corners() {
        let monitor = monitor("DP-1", 100, 50, 1000, 500);

        assert_eq!(score(&monitor, Corner::TopLeft), -250);
        assert_eq!(score(&monitor, Corner::TopRight), 2150);
        assert_eq!(score(&monitor, Corner::BottomLeft), 350);
        assert_eq!(score(&monitor, Corner::BottomRight), 2750);
    }

    #[test]
    fn each_corner_picks_the_monitor_that_extends_furthest() {
        let layout = layout(
            vec![
                monitor("TOP-LEFT", -1920, -1080, 1920, 1080),
                monitor("TOP-RIGHT", 1920, -1080, 1920, 1080),
                monitor("BOTTOM-LEFT", -1920, 1080, 1920, 1080),
                monitor("BOTTOM-RIGHT", 1920, 1080, 1920, 1080),
                monitor("PRIMARY", 0, 0, 1920, 1080),
            ],
            "PRIMARY",
        );

        let choose = |corner| {
            select_monitor(&layout, corner, &MonitorPolicy::Farthest)
                .unwrap()
                .connector()
                .to_string()
        };

        assert_eq!(choose(Corner::TopLeft), "TOP-LEFT");
        assert_eq!(choose(Corner::TopRight), "TOP-RIGHT");
        assert_eq!(choose(Corner::BottomLeft), "BOTTOM-LEFT");
        assert_eq!(choose(Corner::BottomRight), "BOTTOM-RIGHT");
    }

    #[test]
    fn equal_scores_prefer_the_monitor_further_from_the_primary() {
        let primary = monitor("PRIMARY", 0, 0, 1000, 1000);
        let near = monitor("NEAR", 0, 0, 2000, 1000);
        let far = monitor("FAR", 1000, 0, 1000, 1000);
        let layout = layout(vec![primary, near, far], "PRIMARY");

        let chosen = select_monitor(&layout, Corner::TopRight, &MonitorPolicy::Farthest).unwrap();

        assert_eq!(chosen.connector(), "FAR");
    }

    #[test]
    fn a_full_tie_is_broken_by_connector_name() {
        let primary = monitor("PRIMARY", 0, 0, 1000, 1000);
        let first = monitor("DP-1", 2000, 0, 1000, 1000);
        let second = monitor("HDMI-A-1", 2000, 0, 1000, 1000);
        let layout = layout(vec![primary, second, first], "PRIMARY");

        let chosen = select_monitor(&layout, Corner::TopRight, &MonitorPolicy::Farthest).unwrap();

        assert_eq!(chosen.connector(), "DP-1");
    }

    #[test]
    fn no_enabled_monitor_falls_back_to_the_list() {
        let mut first = monitor("DP-1", 0, 0, 1920, 1080);
        first.enabled = false;
        let mut second = monitor("DP-2", 1920, 0, 1920, 1080);
        second.enabled = false;
        let layout = layout(vec![first, second], "DP-1");

        let chosen = select_monitor(&layout, Corner::TopRight, &MonitorPolicy::Farthest).unwrap();

        assert_eq!(chosen.connector(), "DP-1");
    }

    #[test]
    fn an_empty_layout_selects_nothing() {
        let layout = MonitorLayout::default();

        assert_eq!(
            select_monitor(&layout, Corner::TopRight, &MonitorPolicy::Farthest),
            None
        );
    }

    #[test]
    fn the_primary_policy_uses_the_primary_then_falls_back() {
        let mut layout = layout(
            vec![
                monitor("DP-1", 0, 0, 1920, 1080),
                monitor("DP-2", 1920, 0, 1920, 1080),
            ],
            "DP-2",
        );

        assert_eq!(
            select_monitor(&layout, Corner::TopRight, &MonitorPolicy::Primary)
                .unwrap()
                .connector(),
            "DP-2"
        );

        layout.set_primary(Some("MISSING".to_string()));
        layout.monitors[0].focused = true;
        assert_eq!(
            select_monitor(&layout, Corner::TopRight, &MonitorPolicy::Primary)
                .unwrap()
                .connector(),
            "DP-1"
        );
    }

    #[test]
    fn the_current_policy_uses_the_focused_monitor() {
        let mut layout = layout(
            vec![
                monitor("DP-1", 0, 0, 1920, 1080),
                monitor("DP-2", 1920, 0, 1920, 1080),
            ],
            "DP-1",
        );
        layout.monitors[1].focused = true;

        assert_eq!(
            select_monitor(&layout, Corner::TopRight, &MonitorPolicy::Current)
                .unwrap()
                .connector(),
            "DP-2"
        );
    }

    #[test]
    fn the_named_policy_returns_only_that_connector() {
        let layout = layout(
            vec![
                monitor("DP-1", 0, 0, 1920, 1080),
                monitor("DP-2", 1920, 0, 1920, 1080),
            ],
            "DP-1",
        );

        assert_eq!(
            select_monitor(
                &layout,
                Corner::TopRight,
                &MonitorPolicy::Named("DP-2".to_string())
            )
            .unwrap()
            .connector(),
            "DP-2"
        );
        assert_eq!(
            select_monitor(
                &layout,
                Corner::TopRight,
                &MonitorPolicy::Named("HDMI-A-1".to_string())
            ),
            None
        );
    }

    #[test]
    fn policies_and_corners_parse_from_config_strings() {
        assert_eq!(
            MonitorPolicy::parse("named:DP-1"),
            Some(MonitorPolicy::Named("DP-1".to_string()))
        );
        assert_eq!(
            MonitorPolicy::parse("farthest"),
            Some(MonitorPolicy::Farthest)
        );
        assert_eq!(
            MonitorPolicy::parse("primary"),
            Some(MonitorPolicy::Primary)
        );
        assert_eq!(
            MonitorPolicy::parse("current"),
            Some(MonitorPolicy::Current)
        );
        assert_eq!(MonitorPolicy::parse("named:"), None);
        assert_eq!(MonitorPolicy::parse("nonsense"), None);
        assert_eq!(Corner::parse("top-right"), Some(Corner::TopRight));
        assert_eq!(Corner::parse("nonsense"), None);
        assert_eq!("bottom-left".parse::<Corner>().unwrap(), Corner::BottomLeft);
        assert!("nonsense".parse::<Corner>().is_err());
    }
}
