//! The MPRIS2 D-Bus backend.
//!
//! Every player publishes `org.mpris.MediaPlayer2.<name>`. The backend reads all
//! of them, watches the bus for player and property changes, and sends
//! transport commands to the player it reported. No player is not an error, so
//! the shell starts before any music app does.

use std::collections::HashMap;
use std::time::Duration;

use async_channel::Sender;
use async_trait::async_trait;
use futures_util::TryStreamExt;
use tokio::sync::OnceCell;
use zbus::message::Type;
use zbus::names::OwnedBusName;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream, Proxy};

use crate::error::BackendError;
use crate::media::backend::{MediaBackend, MediaBackendEvent};
use crate::media::model::{MediaModel, PlaybackStatus};

pub const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
pub const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
pub const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
pub const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
pub const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// Reads and controls MPRIS players over the session bus.
pub struct MprisBackend {
    connection: OnceCell<Connection>,
}

impl MprisBackend {
    pub fn new() -> Self {
        Self {
            connection: OnceCell::new(),
        }
    }

    async fn connection(&self) -> Result<&Connection, BackendError> {
        self.connection
            .get_or_try_init(|| async {
                Connection::session()
                    .await
                    .map_err(|error| BackendError::Unavailable(error.to_string()))
            })
            .await
    }

    /// Reads every running player. `read` picks the one the shell shows.
    async fn players(&self, connection: &Connection) -> Result<Vec<MediaModel>, BackendError> {
        let dbus = zbus::fdo::DBusProxy::new(connection)
            .await
            .map_err(dbus_error)?;
        let names: Vec<OwnedBusName> = dbus
            .list_names()
            .await
            .map_err(|error| BackendError::Unavailable(error.to_string()))?;

        let mut players = Vec::new();
        for name in names {
            let name = name.as_str();
            if !name.starts_with(MPRIS_PREFIX) {
                continue;
            }
            match read_player(connection, name).await {
                Ok(model) => players.push(model),
                Err(error) => tracing::debug!(
                    player = name,
                    %error,
                    "an MPRIS player could not be read"
                ),
            }
        }
        Ok(players)
    }

    async fn player_proxy(&self, player: &str) -> Result<Proxy<'static>, BackendError> {
        let connection = self.connection().await?;
        Proxy::new_owned(
            connection.clone(),
            player.to_string(),
            MPRIS_PATH.to_string(),
            PLAYER_INTERFACE.to_string(),
        )
        .await
        .map_err(dbus_error)
    }

    async fn call_player(&self, player: &str, method: &str) -> Result<(), BackendError> {
        let proxy = self.player_proxy(player).await?;
        proxy
            .call::<_, _, ()>(method, &())
            .await
            .map_err(dbus_error)
    }
}

impl Default for MprisBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MediaBackend for MprisBackend {
    fn name(&self) -> &'static str {
        "mpris"
    }

    async fn probe(&self) -> Result<(), BackendError> {
        self.connection().await?;
        Ok(())
    }

    async fn read(&self) -> Result<MediaModel, BackendError> {
        let connection = self.connection().await?;
        Ok(MediaModel::pick(self.players(connection).await?))
    }

    async fn watch(&self, events: Sender<MediaBackendEvent>) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let name_rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.DBus")
            .map_err(dbus_error)?
            .member("NameOwnerChanged")
            .map_err(dbus_error)?
            .build();
        let property_rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(PROPERTIES_INTERFACE)
            .map_err(dbus_error)?
            .member("PropertiesChanged")
            .map_err(dbus_error)?
            .build();
        let seek_rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(PLAYER_INTERFACE)
            .map_err(dbus_error)?
            .member("Seeked")
            .map_err(dbus_error)?
            .build();

        let mut names = MessageStream::for_match_rule(name_rule, connection, Some(64))
            .await
            .map_err(dbus_error)?;
        let mut properties = MessageStream::for_match_rule(property_rule, connection, Some(256))
            .await
            .map_err(dbus_error)?;
        let mut seeked = MessageStream::for_match_rule(seek_rule, connection, Some(64))
            .await
            .map_err(dbus_error)?;

        loop {
            tokio::select! {
                message = names.try_next() => {
                    let message = message.map_err(dbus_error)?;
                    let Some(message) = message else { break };
                    let Ok((name, _, _)) = message.body().deserialize::<(String, String, String)>() else {
                        continue;
                    };
                    if !name.starts_with(MPRIS_PREFIX) {
                        continue;
                    }
                    if events.send(MediaBackendEvent::Changed).await.is_err() {
                        return Ok(());
                    }
                }
                message = properties.try_next() => {
                    let message = message.map_err(dbus_error)?;
                    let Some(message) = message else { break };
                    if !is_mpris_path(&message) {
                        continue;
                    }
                    let Ok((interface, _, _)) = message
                        .body()
                        .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                    else {
                        continue;
                    };
                    if interface != PLAYER_INTERFACE && interface != ROOT_INTERFACE {
                        continue;
                    }
                    if events.send(MediaBackendEvent::Changed).await.is_err() {
                        return Ok(());
                    }
                }
                message = seeked.try_next() => {
                    let message = message.map_err(dbus_error)?;
                    let Some(message) = message else { break };
                    if !is_mpris_path(&message) {
                        continue;
                    }
                    if events.send(MediaBackendEvent::Changed).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }

        Err(BackendError::Unavailable(
            "the session bus left the player".to_string(),
        ))
    }

    async fn play_pause(&self, player: &str) -> Result<(), BackendError> {
        self.call_player(player, "PlayPause").await
    }

    async fn next(&self, player: &str) -> Result<(), BackendError> {
        self.call_player(player, "Next").await
    }

    async fn previous(&self, player: &str) -> Result<(), BackendError> {
        self.call_player(player, "Previous").await
    }

    async fn seek(&self, player: &str, position: Duration) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let model = read_player(connection, player).await?;
        let proxy = self.player_proxy(player).await?;
        let target = position.as_micros().min(i64::MAX as u128) as i64;

        if let Ok(track) = OwnedObjectPath::try_from(model.track_id.clone()) {
            let result = proxy
                .call::<_, _, ()>("SetPosition", &(track, target))
                .await;
            if result.is_ok() {
                return Ok(());
            }
        }

        let current = model.position.as_micros().min(i64::MAX as u128) as i64;
        proxy
            .call::<_, _, ()>("Seek", &(target - current))
            .await
            .map_err(dbus_error)
    }

    async fn set_volume(&self, player: &str, volume: f64) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let proxy = Proxy::new_owned(
            connection.clone(),
            player.to_string(),
            MPRIS_PATH.to_string(),
            PROPERTIES_INTERFACE.to_string(),
        )
        .await
        .map_err(dbus_error)?;
        proxy
            .call::<_, _, ()>(
                "Set",
                &(
                    PLAYER_INTERFACE,
                    "Volume",
                    Value::from(volume.clamp(0.0, 1.0)),
                ),
            )
            .await
            .map_err(dbus_error)
    }
}

async fn read_player(connection: &Connection, player: &str) -> Result<MediaModel, BackendError> {
    let player_properties = get_all(connection, player, PLAYER_INTERFACE).await?;
    let root_properties = get_all(connection, player, ROOT_INTERFACE)
        .await
        .unwrap_or_default();
    Ok(model_from(player, &root_properties, &player_properties))
}

async fn get_all(
    connection: &Connection,
    player: &str,
    interface: &str,
) -> Result<HashMap<String, OwnedValue>, BackendError> {
    let proxy = Proxy::new_owned(
        connection.clone(),
        player.to_string(),
        MPRIS_PATH.to_string(),
        PROPERTIES_INTERFACE.to_string(),
    )
    .await
    .map_err(dbus_error)?;
    proxy
        .call("GetAll", &(interface,))
        .await
        .map_err(dbus_error)
}

/// Turns the MPRIS property maps into the shared model.
pub fn model_from(
    player: &str,
    root: &HashMap<String, OwnedValue>,
    values: &HashMap<String, OwnedValue>,
) -> MediaModel {
    let metadata = values
        .get("Metadata")
        .and_then(|value| HashMap::<String, OwnedValue>::try_from(value.clone()).ok())
        .unwrap_or_default();

    MediaModel {
        player: Some(player.to_string()),
        identity: string(root, "Identity"),
        title: metadata_string(&metadata, "xesam:title"),
        artist: metadata_string(&metadata, "xesam:artist"),
        album: metadata_string(&metadata, "xesam:album"),
        art_url: metadata_string(&metadata, "mpris:artUrl"),
        track_id: metadata
            .get("mpris:trackid")
            .and_then(|value| OwnedObjectPath::try_from(value.clone()).ok())
            .map(|path| path.as_str().to_string())
            .unwrap_or_default(),
        status: PlaybackStatus::parse(&string(values, "PlaybackStatus")),
        position: duration(values, "Position"),
        length: metadata_duration(&metadata, "mpris:length"),
        can_previous: boolean(values, "CanGoPrevious"),
        can_next: boolean(values, "CanGoNext"),
        can_play: boolean(values, "CanPlay"),
        can_pause: boolean(values, "CanPause"),
        can_seek: boolean(values, "CanSeek"),
        volume: values
            .get("Volume")
            .and_then(|value| f64::try_from(value.clone()).ok())
            .unwrap_or(1.0),
    }
}

fn string(values: &HashMap<String, OwnedValue>, key: &str) -> String {
    values
        .get(key)
        .and_then(|value| String::try_from(value.clone()).ok())
        .unwrap_or_default()
}

/// `xesam:artist` is a list, but some players send a single string anyway.
fn metadata_string(values: &HashMap<String, OwnedValue>, key: &str) -> String {
    let Some(value) = values.get(key).cloned() else {
        return String::new();
    };
    if let Ok(list) = Vec::<String>::try_from(value.clone()) {
        return list.join(", ");
    }
    if let Ok(single) = String::try_from(value.clone()) {
        return single;
    }
    if let Ok(single) = <&str>::try_from(&value) {
        return single.to_string();
    }
    String::new()
}

fn duration(values: &HashMap<String, OwnedValue>, key: &str) -> Duration {
    let micros = values
        .get(key)
        .and_then(|value| i64::try_from(value.clone()).ok())
        .unwrap_or(0);
    Duration::from_micros(micros.max(0) as u64)
}

fn metadata_duration(values: &HashMap<String, OwnedValue>, key: &str) -> Option<Duration> {
    let micros = i64::try_from(values.get(key).cloned()?).ok()?;
    (micros > 0).then(|| Duration::from_micros(micros as u64))
}

fn boolean(values: &HashMap<String, OwnedValue>, key: &str) -> bool {
    values
        .get(key)
        .and_then(|value| bool::try_from(value.clone()).ok())
        .unwrap_or(false)
}

fn is_mpris_path(message: &zbus::Message) -> bool {
    message
        .header()
        .path()
        .is_some_and(|path| path.as_str().starts_with(MPRIS_PATH))
}

fn dbus_error(error: zbus::Error) -> BackendError {
    BackendError::Unavailable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<T: Into<Value<'static>>>(value: T) -> OwnedValue {
        OwnedValue::try_from(value.into()).unwrap()
    }

    fn player_values() -> HashMap<String, OwnedValue> {
        let metadata = HashMap::from([
            ("xesam:title".to_string(), value("Midnight City")),
            (
                "xesam:artist".to_string(),
                value(vec!["M83".to_string(), "Guest".to_string()]),
            ),
            ("xesam:album".to_string(), value("Hurry Up")),
            ("mpris:artUrl".to_string(), value("file:///tmp/art.png")),
            ("mpris:length".to_string(), value(238_000_000i64)),
            (
                "mpris:trackid".to_string(),
                value(zbus::zvariant::ObjectPath::try_from("/track/1").unwrap()),
            ),
        ]);

        HashMap::from([
            ("PlaybackStatus".to_string(), value("Playing")),
            ("Position".to_string(), value(84_000_000i64)),
            ("CanGoNext".to_string(), value(true)),
            ("CanGoPrevious".to_string(), value(true)),
            ("CanPlay".to_string(), value(true)),
            ("CanPause".to_string(), value(true)),
            ("CanSeek".to_string(), value(true)),
            ("Volume".to_string(), value(0.42f64)),
            ("Metadata".to_string(), value(metadata)),
        ])
    }

    #[test]
    fn all_player_properties_are_read() {
        let root = HashMap::from([("Identity".to_string(), value("Spotify"))]);
        let model = model_from("org.mpris.MediaPlayer2.spotify", &root, &player_values());

        assert_eq!(
            model.player.as_deref(),
            Some("org.mpris.MediaPlayer2.spotify")
        );
        assert_eq!(model.identity, "Spotify");
        assert_eq!(model.title, "Midnight City");
        assert_eq!(model.artist, "M83, Guest");
        assert_eq!(model.album, "Hurry Up");
        assert_eq!(model.track_id, "/track/1");
        assert_eq!(model.status, PlaybackStatus::Playing);
        assert_eq!(model.position, Duration::from_secs(84));
        assert_eq!(model.length, Some(Duration::from_secs(238)));
        assert_eq!(model.volume, 0.42);
        assert!(model.can_next && model.can_previous && model.can_seek);
    }

    #[test]
    fn missing_properties_use_neutral_values() {
        let model = model_from("player", &HashMap::new(), &HashMap::new());

        assert_eq!(model.title, "");
        assert_eq!(model.status, PlaybackStatus::Stopped);
        assert_eq!(model.length, None);
        assert_eq!(model.volume, 1.0);
        assert!(!model.can_next);
    }

    #[test]
    fn a_single_artist_string_is_accepted() {
        let metadata = HashMap::from([("xesam:artist".to_string(), value("Solo"))]);
        let values = HashMap::from([("Metadata".to_string(), value(metadata))]);

        assert_eq!(
            model_from("player", &HashMap::new(), &values).artist,
            "Solo"
        );
    }
}
