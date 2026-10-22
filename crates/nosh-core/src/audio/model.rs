//! Audio endpoints and volume rules.

/// The maximum output volume, including amplification.
pub const MAX_VOLUME: f64 = 1.5;
/// The maximum input volume. Microphones are not amplified.
pub const MAX_SOURCE_VOLUME: f64 = 1.0;
/// One keyboard or scroll step.
pub const VOLUME_STEP: f64 = 0.05;
/// The bar stops scrolling up here. The slider and command line can go higher.
pub const BAR_MAX_VOLUME: f64 = 1.0;
/// A difference smaller than this is not reported as a change.
pub const VOLUME_EPSILON: f64 = 0.0005;

/// Which device list an endpoint belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndpointKind {
    Sink,
    Source,
}

impl EndpointKind {
    pub const fn max_volume(self) -> f64 {
        match self {
            Self::Sink => MAX_VOLUME,
            Self::Source => MAX_SOURCE_VOLUME,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sink => "sink",
            Self::Source => "source",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioEndpoint {
    pub id: u32,
    pub name: String,
    pub description: String,
    /// `0.0..=1.5` for output, `0.0..=1.0` for input.
    pub volume: f64,
    pub muted: bool,
    pub icon_name: String,
    pub is_default: bool,
}

/// The complete audio state the UI renders.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioModel {
    pub default_sink: Option<AudioEndpoint>,
    pub default_source: Option<AudioEndpoint>,
    pub sinks: Vec<AudioEndpoint>,
    pub sources: Vec<AudioEndpoint>,
}

impl AudioModel {
    pub fn endpoints(&self, kind: EndpointKind) -> &[AudioEndpoint] {
        match kind {
            EndpointKind::Sink => &self.sinks,
            EndpointKind::Source => &self.sources,
        }
    }

    pub fn endpoint(&self, kind: EndpointKind, id: u32) -> Option<&AudioEndpoint> {
        self.endpoints(kind)
            .iter()
            .find(|endpoint| endpoint.id == id)
    }

    pub fn endpoint_mut(&mut self, kind: EndpointKind, id: u32) -> Option<&mut AudioEndpoint> {
        match kind {
            EndpointKind::Sink => self.sinks.iter_mut().find(|endpoint| endpoint.id == id),
            EndpointKind::Source => self.sources.iter_mut().find(|endpoint| endpoint.id == id),
        }
    }

    pub fn default_for(&self, kind: EndpointKind) -> Option<&AudioEndpoint> {
        match kind {
            EndpointKind::Sink => self.default_sink.as_ref(),
            EndpointKind::Source => self.default_source.as_ref(),
        }
    }

    pub fn default_for_mut(&mut self, kind: EndpointKind) -> Option<&mut AudioEndpoint> {
        match kind {
            EndpointKind::Sink => self.default_sink.as_mut(),
            EndpointKind::Source => self.default_source.as_mut(),
        }
    }

    /// Clamps a requested value for the endpoint kind and rounds it so repeated
    /// steps do not drift.
    pub fn clamped_volume(kind: EndpointKind, volume: f64) -> f64 {
        round_volume(volume.clamp(0.0, kind.max_volume()))
    }
}

/// The icon for a volume level.
pub fn volume_icon(kind: EndpointKind, volume: f64, muted: bool) -> &'static str {
    let _ = kind;
    if muted || volume <= 0.0 {
        "audio-volume-muted-symbolic"
    } else if volume < 0.33 {
        "audio-volume-low-symbolic"
    } else if volume < 0.66 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

/// Returns the next volume for a bar scroll, or `None` when scrolling up would
/// pass the bar limit.
pub fn bar_step_volume(current: f64, delta: f64) -> Option<f64> {
    if delta > 0.0 {
        if current >= BAR_MAX_VOLUME {
            None
        } else {
            Some(round_volume((current + delta).min(BAR_MAX_VOLUME)))
        }
    } else {
        Some(round_volume((current + delta).max(0.0)))
    }
}

pub fn round_volume(volume: f64) -> f64 {
    (volume * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(id: u32, volume: f64) -> AudioEndpoint {
        AudioEndpoint {
            id,
            name: format!("endpoint-{id}"),
            description: format!("Endpoint {id}"),
            volume,
            muted: false,
            icon_name: String::new(),
            is_default: false,
        }
    }

    #[test]
    fn output_and_input_use_different_maxima() {
        assert_eq!(AudioModel::clamped_volume(EndpointKind::Sink, 2.0), 1.5);
        assert_eq!(AudioModel::clamped_volume(EndpointKind::Sink, -1.0), 0.0);
        assert_eq!(AudioModel::clamped_volume(EndpointKind::Source, 2.0), 1.0);
    }

    #[test]
    fn clamping_rounds_away_float_drift() {
        let mut volume = 0.0;
        for _ in 0..30 {
            volume = AudioModel::clamped_volume(EndpointKind::Sink, volume + VOLUME_STEP);
        }
        assert_eq!(volume, 1.5);
    }

    #[test]
    fn icons_follow_the_documented_thresholds() {
        assert_eq!(
            volume_icon(EndpointKind::Sink, 0.0, false),
            "audio-volume-muted-symbolic"
        );
        assert_eq!(
            volume_icon(EndpointKind::Sink, 0.2, false),
            "audio-volume-low-symbolic"
        );
        assert_eq!(
            volume_icon(EndpointKind::Sink, 0.33, false),
            "audio-volume-medium-symbolic"
        );
        assert_eq!(
            volume_icon(EndpointKind::Sink, 0.8, false),
            "audio-volume-high-symbolic"
        );
        assert_eq!(
            volume_icon(EndpointKind::Sink, 0.8, true),
            "audio-volume-muted-symbolic"
        );
    }

    #[test]
    fn the_bar_stops_at_one_when_scrolling_up() {
        assert_eq!(bar_step_volume(0.95, 0.05), Some(1.0));
        assert_eq!(bar_step_volume(1.0, 0.05), None);
        assert_eq!(bar_step_volume(1.2, 0.05), None);
    }

    #[test]
    fn scrolling_down_from_amplified_volume_reduces_normally() {
        assert_eq!(bar_step_volume(1.2, -0.05), Some(1.15));
        assert_eq!(bar_step_volume(0.02, -0.05), Some(0.0));
    }

    #[test]
    fn defaults_are_found_by_kind() {
        let model = AudioModel {
            default_sink: Some(endpoint(1, 0.5)),
            default_source: Some(endpoint(2, 0.5)),
            sinks: vec![endpoint(1, 0.5), endpoint(3, 0.2)],
            sources: vec![endpoint(2, 0.5)],
        };

        assert_eq!(
            model.endpoint(EndpointKind::Sink, 3).unwrap().name,
            "endpoint-3"
        );
        assert_eq!(
            model
                .default_for(EndpointKind::Source)
                .map(|endpoint| endpoint.id),
            Some(2)
        );
    }
}
