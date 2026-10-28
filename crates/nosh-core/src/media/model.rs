//! The now-playing model shared by the bar, the panel, and the backend.

use std::time::Duration;

/// The MPRIS playback state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    #[default]
    Stopped,
}

impl PlaybackStatus {
    pub fn parse(value: &str) -> Self {
        match value {
            "Playing" => Self::Playing,
            "Paused" => Self::Paused,
            _ => Self::Stopped,
        }
    }

    pub const fn is_playing(self) -> bool {
        matches!(self, Self::Playing)
    }
}

/// The track and controls of one MPRIS player.
///
/// An empty model means no player is running. `player` holds the bus name, so
/// a transport command reaches the player the model was read from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaModel {
    pub player: Option<String>,
    pub identity: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    pub track_id: String,
    pub status: PlaybackStatus,
    pub position: Duration,
    pub length: Option<Duration>,
    pub can_previous: bool,
    pub can_next: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_seek: bool,
    pub volume: f64,
}

impl MediaModel {
    /// Whether there is a player with a track to show.
    pub fn is_active(&self) -> bool {
        self.player.is_some() && !self.title.is_empty()
    }

    /// The line under the title. An empty artist falls back to the album, then
    /// to a neutral label.
    pub fn subtitle(&self) -> String {
        if !self.artist.is_empty() {
            self.artist.clone()
        } else if !self.album.is_empty() {
            self.album.clone()
        } else if !self.identity.is_empty() {
            self.identity.clone()
        } else {
            "Unknown artist".to_string()
        }
    }

    /// How much of the track has played, from 0 to 1.
    pub fn progress(&self) -> f64 {
        let Some(length) = self.length else {
            return 0.0;
        };
        if length.is_zero() {
            return 0.0;
        }
        (self.position.as_secs_f64() / length.as_secs_f64()).clamp(0.0, 1.0)
    }

    /// Picks the player the shell shows. A playing player wins, then a paused
    /// one, then the first bus name. Returns an empty model when no player is
    /// running.
    pub fn pick(mut players: Vec<Self>) -> Self {
        players.sort_by(|a, b| a.player.cmp(&b.player));
        let index = players
            .iter()
            .position(|model| model.status.is_playing())
            .or_else(|| {
                players
                    .iter()
                    .position(|model| model.status == PlaybackStatus::Paused)
            })
            .unwrap_or(0);
        players.into_iter().nth(index).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> MediaModel {
        MediaModel {
            player: Some("org.mpris.MediaPlayer2.spotify".to_string()),
            identity: "Spotify".to_string(),
            title: "Midnight City".to_string(),
            artist: "M83".to_string(),
            length: Some(Duration::from_secs(238)),
            position: Duration::from_secs(84),
            status: PlaybackStatus::Playing,
            ..MediaModel::default()
        }
    }

    #[test]
    fn a_titled_player_is_active() {
        assert!(track().is_active());
    }

    #[test]
    fn a_missing_player_or_title_is_inactive() {
        let mut model = track();
        model.player = None;
        assert!(!model.is_active());

        let mut model = track();
        model.title.clear();
        assert!(!model.is_active());
    }

    #[test]
    fn the_subtitle_prefers_the_artist_then_the_album() {
        let mut model = track();
        assert_eq!(model.subtitle(), "M83");

        model.artist.clear();
        model.album = "Hurry Up, We're Dreaming".to_string();
        assert_eq!(model.subtitle(), "Hurry Up, We're Dreaming");

        model.album.clear();
        assert_eq!(model.subtitle(), "Spotify");
    }

    #[test]
    fn progress_is_clamped_and_needs_a_length() {
        assert!((track().progress() - 84.0 / 238.0).abs() < 0.0001);

        let mut model = track();
        model.length = None;
        assert_eq!(model.progress(), 0.0);

        let mut model = track();
        model.position = Duration::from_secs(10_000);
        assert_eq!(model.progress(), 1.0);
    }

    #[test]
    fn playback_status_is_parsed() {
        assert_eq!(PlaybackStatus::parse("Playing"), PlaybackStatus::Playing);
        assert_eq!(PlaybackStatus::parse("Paused"), PlaybackStatus::Paused);
        assert_eq!(PlaybackStatus::parse("Stopped"), PlaybackStatus::Stopped);
        assert_eq!(PlaybackStatus::parse("unknown"), PlaybackStatus::Stopped);
    }

    fn player(bus: &str, status: PlaybackStatus) -> MediaModel {
        MediaModel {
            player: Some(bus.to_string()),
            title: bus.to_string(),
            status,
            ..MediaModel::default()
        }
    }

    fn picked(players: Vec<MediaModel>) -> Option<String> {
        MediaModel::pick(players).player
    }

    #[test]
    fn a_playing_player_is_picked() {
        assert_eq!(
            picked(vec![
                player("org.mpris.MediaPlayer2.zed", PlaybackStatus::Paused),
                player("org.mpris.MediaPlayer2.firefox", PlaybackStatus::Playing),
                player("org.mpris.MediaPlayer2.mpd", PlaybackStatus::Stopped),
            ])
            .as_deref(),
            Some("org.mpris.MediaPlayer2.firefox")
        );
    }

    #[test]
    fn a_paused_player_wins_when_nothing_plays() {
        assert_eq!(
            picked(vec![
                player("org.mpris.MediaPlayer2.b", PlaybackStatus::Paused),
                player("org.mpris.MediaPlayer2.a", PlaybackStatus::Stopped),
            ])
            .as_deref(),
            Some("org.mpris.MediaPlayer2.b")
        );
    }

    #[test]
    fn the_first_bus_name_wins_when_nothing_plays_or_pauses() {
        assert_eq!(
            picked(vec![
                player("org.mpris.MediaPlayer2.b", PlaybackStatus::Stopped),
                player("org.mpris.MediaPlayer2.a", PlaybackStatus::Stopped),
            ])
            .as_deref(),
            Some("org.mpris.MediaPlayer2.a")
        );
    }

    #[test]
    fn no_players_pick_an_empty_model() {
        assert_eq!(MediaModel::pick(Vec::new()), MediaModel::default());
    }
}
