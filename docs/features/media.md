# Media

The bar shows the current track and the centre panel has a media tab with cover
art, transport controls, seek, and volume.

## Current behavior

The Vala bar built a `Gtk.Box` and bound the first MPRIS player's metadata to a
label as `"{artist} - {title}"`. It had no controls and no cover art. The Rust
shell replaces that with a shared model, one selected player, and a full media
tab in the centre panel.

## Core

`nosh-core/src/media/` holds the model, the backend trait, the MPRIS backend, and
the service.

### Model

`MediaModel` is one track and the controls that apply to it.

```rust
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
```

`player` is the MPRIS bus name. Transport commands use it, so the shell always
controls the player it shows. A default model means no player runs.

Helpers on the model:

| Method      | Result                                                           |
| ----------- | ---------------------------------------------------------------- |
| `is_active` | true when a player name and a title exist                        |
| `subtitle`  | artist, else album, else player identity, else `Unknown artist`  |
| `progress`  | position divided by length, clamped to 0..1, zero without length |
| `pick`      | the player the shell shows                                       |

`pick` sorts the players by bus name, takes the first playing player, then the
first paused one, then the first player. An empty list returns the default
model. The sort makes the choice stable across reads, so the bar does not
flicker between two players with the same state.

### Backend

`MediaBackend` is the seam for tests and for a future non-MPRIS source. Reads
return one `MediaModel`, not a list. Writes take the bus name.

```rust
async fn probe(&self) -> Result<(), BackendError>;
async fn read(&self) -> Result<MediaModel, BackendError>;
async fn watch(&self, events: Sender<MediaBackendEvent>) -> Result<(), BackendError>;
async fn play_pause(&self, player: &str) -> Result<(), BackendError>;
async fn next(&self, player: &str) -> Result<(), BackendError>;
async fn previous(&self, player: &str) -> Result<(), BackendError>;
async fn seek(&self, player: &str, position: Duration) -> Result<(), BackendError>;
async fn set_volume(&self, player: &str, volume: f64) -> Result<(), BackendError>;
```

An empty session bus is not a probe failure. If no player runs, `read` returns
the default model and writes do nothing.

`MprisBackend` implements the trait:

- List bus names and keep every `org.mpris.MediaPlayer2.` name.
- Read `GetAll` on `org.mpris.MediaPlayer2.Player`, and on
  `org.mpris.MediaPlayer2` for `Identity`. A failed root read is not fatal.
- Watch `NameOwnerChanged`, `PropertiesChanged` on both interfaces, and `Seeked`.
  Each one emits `MediaBackendEvent::Changed`.
- `Seek` tries `SetPosition` with the stored track id, then falls back to a
  relative `Seek`.
- `SetVolume` clamps to 0..1 and writes the `Volume` property.
- `xesam:artist` accepts a string list or a single string and joins a list with
  `", "`.
- `mpris:length` of zero or less becomes `None`.

### Service

`MediaService` owns the connection and the model the shell sees. It publishes a
snapshot at startup, sends `MediaEvent::Changed` when the model differs from the
last one, and keeps the player selected by `pick`.

After every change signal, every transport command, and every poll tick, the
service re-reads the player. The poll defaults to one second. Players publish
metadata in stages, and Firefox sometimes writes artwork after the change
signal without sending a second one, so the signal alone misses fields.

A failed transport write sends `MediaEvent::WriteFailed` and keeps the service
running. A dead bus sends `MediaEvent::Unavailable`, marks the service
unavailable, and reconnects with the shared backoff. A connection that lasted
five seconds resets the backoff.

### Events and commands

```rust
pub enum MediaEvent {
    Snapshot(MediaModel),
    Changed(MediaModel),
    WriteFailed(String),
    Unavailable,
}

pub enum Command {
    MediaPlayPause,
    MediaNext,
    MediaPrevious,
    MediaSeek(u64),
    MediaSetVolume(f64),
}
```

`MediaSeek` takes milliseconds from the start of the track. `MediaSetVolume`
takes 0..1.

### Limits

- MPRIS exposes one bus name per application, so two browser tabs arrive as one
  player and the shell cannot switch between them. Showing per-tab entries needs
  a browser extension.
- The core does not download remote art. The panel loads local files only.
- The core does not cache artwork. The panel loads the file each time the track
  changes.

## UI

### Bar

The centre island holds a divider and a button. The section hides when the model
is not active, so an idle island has no media item.

The label shows the title with a middle ellipsis and a cap of 18 characters.
The tooltip shows the title and the subtitle. A click opens the centre panel on
the media tab.

### Centre panel

The media tab is narrower than the calendar tab because it holds less text. Its
header button shows the title, also with a middle ellipsis and a cap of 28
characters. The button hides when no player is active. Clicking the clock or the
media label switches tabs, and an open media tab falls back to the calendar when
the player stops.

The page holds, top to bottom, cover art, a seek row, and a volume row. The art
is an overlay:

- The picture fills the box and crops with `ContentFit::Cover`.
- The track title scrolls back and forth when it is wider than the box. This is
  the `Marquee` widget, which uses a scrolling viewport and a tick callback.
- The subtitle sits under the title, centered, with a middle ellipsis.
- Previous, play/pause, and next sit at the bottom of the art over a gradient.
- Without art, the box is a square with the generic audio icon centered.

Controls:

| Control  | Command                                         |
| -------- | ----------------------------------------------- |
| Previous | `MediaPrevious` when `can_previous`             |
| Play     | `MediaPlayPause` when `can_play` or `can_pause` |
| Next     | `MediaNext` when `can_next`                     |
| Seek     | `MediaSeek` when `can_seek` and a length exists |
| Volume   | `MediaSetVolume` while a player is active       |

A one-second tick advances the elapsed time and the seek slider from the stored
position while the player is playing. The tick also retries cover art.

### Cover art

The art keeps the source aspect ratio inside a 354 by 168 box. Wide art fills
the width. Tall art takes the height. The height never drops below 132. A square
placeholder of 168 pixels stands in when the file does not load.

The source is `mpris:artUrl` when it is a local file. The panel retries a failed
read once a second for ten ticks, because a player may publish the URL before it
finishes writing the image.

Firefox, Zen, and the other Firefox-family browsers sometimes omit the URL while
still writing the image. For those players the panel falls back to the newest
image in the browser's `firefox-mpris` directory. A file older than the track
start, with a three second tolerance, belongs to the previous track and is
ignored. The first track after startup accepts any file, since the browser may
have written it long before the shell started.

Searched directories:

```text
~/.zen
~/.mozilla/firefox
$XDG_CONFIG_HOME/mozilla/firefox
~/.librewolf
~/.waterfox
~/.floorp
```

### CSS classes

The bar uses `MediaButton`, `MediaRow`, `MediaIcon`, `MediaLabel`, and
`MediaSection` in `style.css`.

The panel uses `CenterPanelMedia`, `CenterPanelMediaLabel`, and
`CenterPanelMediaIcon` for the header, and `MediaPage`, `MediaSheet`,
`MediaArt`, `MediaArtImage`, `MediaArtIcon`, `MediaArtScrim`, `MediaMarquee`,
`MediaMarqueeTrack`, `MediaTitle`, `MediaArtist`, `MediaControls`,
`MediaControl`, `MediaPlay`, `MediaSeek`, `MediaTime`, `MediaVolume`,
`MediaVolumeIcon`, `MediaVolumeValue`, and `MediaSlider` for the tab.

## Definition of done

- A playing player fills the bar label and opens the media tab.
- Pausing, changing track, or changing artist updates the label and the tab
  without rebuilding the widgets.
- Closing the player hides the bar section and switches an open media tab to the
  calendar.
- Two players resolve to the same one on every read, and transport commands
  reach that player.
- The media tab shows art, title, subtitle, transport, seek, and volume, and
  falls back to the placeholder without art.
- The palette and the corner radius match the rest of the shell in both themes.
- The shell starts and the bar stays stable when the session bus is missing or
  no player runs.
