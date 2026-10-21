# Media

Current file: `src/bar/widgets/Media.vala`.

## Current behavior

`Media` is a `Gtk.Box` with CSS class `Media`. It gets `AstalMpris.get_default()`, connects `notify["players"]` to `sync`, and calls `sync`.

`sync` clears the box. If there are no players, it returns. Otherwise it takes the first player, creates a `Gtk.Label`, and binds `player.metadata` to the label with a transform that sets:

```text
"{artist} - {title}"
```

The cover art code is commented out. The intended code would create a `Gtk.Picture` with height 32, class `Cover`, load `player.cover_art`, and update on `cover_art` changes.

There are no controls. The label is not clickable. The first player is not chosen by playback state, only by list order.

## Core

Add an `MprisService` to `nosh-core`. It tracks every `org.mpris.MediaPlayer2.*` name on the session bus.

### Player model

```rust
pub struct PlayerModel {
    pub bus_name: String,
    pub identity: String,
    pub playback_status: PlaybackStatus,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    pub cover_art_path: Option<PathBuf>,
    pub length: Option<Duration>,
    pub position: Option<Duration>,
    pub can_control: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_seek: bool,
    pub available: bool,
}
```

`artist` is the `xesam:artist` string array joined with `", "`. `album_artist` uses `", "`. Keep the current display format `"{artist} - {title}"`, even when one side is empty. The current code does not trim the separator.

### Discovery

- Subscribe to `NameOwnerChanged` on the session bus.
- Add a player when a name with the MPRIS prefix appears.
- Remove it when the name loses its owner.
- Create a proxy for `org.mpris.MediaPlayer2` and `org.mpris.MediaPlayer2.Player`.
- Read `Identity` from the first interface.
- Read `PlaybackStatus`, `Metadata`, `CanControl`, `CanPlay`, `CanPause`, `CanGoNext`, `CanGoPrevious`, `CanSeek`, `Volume`, `LoopStatus`, `Shuffle`, `Rate` from the second.
- Subscribe to `PropertiesChanged` on both interfaces.
- Subscribe to `Seeked` and update the position.

### Selection

The current UI always uses the first player. Keep that rule for parity, but define it explicitly: the first player in discovery order. Prefer a player whose `playback_status` is `Playing` if no player is playing, then the first. Do not rely on a `HashMap` iteration order. Store players in a `Vec` and preserve insertion order.

Emit `MediaEvent::PlayersChanged(Vec<PlayerModel>)` when the list changes. Emit `MediaEvent::PlayerChanged(PlayerModel)` when a property changes.

### Position

Do not poll every second unless a UI shows progress. The current bar does not. If the future central console shows a progress bar, add a 1-second poll only while the player is playing, like AstalMpris. Do not poll in the background for an unused value.

### Cover art

The current bar does not display cover art. Implement the cache only if the UI will use it:

- If `mpris:artUrl` is a local file, use its path.
- If it is `http` or `https`, download it with `reqwest` or `ureq`.
- If it is `data:image/...;base64,`, decode it.
- Cache under `~/.cache/nosh/covers/<sha1-of-url>`.
- Never cache in the source tree.
- Cap the download size and the decoded image size.

### Controls

Expose commands only for methods the UI uses:

```rust
Command::MediaPlayPause(String)
Command::MediaNext(String)
Command::MediaPrevious(String)
Command::MediaSeek(String, Duration)
```

If the first rewrite keeps the bar label only, do not add these commands. Add them with the central console or a clickable media widget.

## UI

`Media` is a `gtk::Box` with class `Media`.

- If there are no players, hide the box or show nothing.
- Otherwise show a `gtk::Label` with `"{artist} - {title}"`.
- Use the first player from the core snapshot.
- If the label is wider than the available space, ellipsize it. The current code does not, but the center box can shrink. Add `ellipsize(END)` and a reasonable max width.
- Do not recreate the label on every property change. Update its text.

Optional, if the feature is requested:

- A `gtk::Picture` with height 32 and class `Cover`, before the label.
- A click on the label toggles play/pause.
- Scroll up and down skip to the next and previous track.
- A tooltip with the album and player identity.

Do not add these silently. They change the interaction model.

### CSS

Current `.Media` has no dedicated rules. The commented cover code used `.Cover`. Add only the classes the UI actually uses.

## Defects and scope

- The current code binds `metadata` to a label with a transform but does not disconnect if the player list changes. The Rust UI updates by player ID and drops the old binding.
- The commented cover art code is not part of the behavior. Do not port commented code.
- The current code takes `players.nth_data(0)` without checking that the player is available. The core should expose only available players, or mark `available` and let the UI skip unavailable ones.
- The current code does not handle a player name with no owner. The core removes it.

## Definition of done

- A playing MPRIS player adds a label with `"{artist} - {title}"`.
- Pausing, changing track, or changing artist updates the label without recreating the widget.
- Closing the player removes the label.
- Multiple players follow the documented selection rule and do not flicker between players.
- The core emits a full player snapshot on startup.
- No cover art is downloaded when the UI does not display it.
- The bar remains stable when no MPRIS service is available.
