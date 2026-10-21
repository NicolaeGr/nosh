# Audio

Current files: `src/utils/system_control/VolumeControl.vala`, `src/bar/widgets/Volume.vala`, `src/quick_settings/widgets/VolumeSlider.vala`, `src/quick_settings/widgets/Microphone.vala`, `src/indicators/ChangeIndicator.vala`.

## Current behavior

`VolumeControl` uses `AstalWp.get_default().audio`.

- `MAX_VOLUME = 1.5`.
- `VOLUME_STEP = 0.05`.
- `increase`, `decrease`, and `set` clamp to `0..1.5`.
- `get` returns the default speaker volume.
- `get_percentage` returns `volume * 100`.
- `volume_changed(new, old)` fires only from its own methods.
- On construction, it connects `notify["volume"]` on the default speaker and clamps values above `1.5`.

`Volume` in the bar is a `Gtk.Button` with class `square-icon`.

- Binds `volume-icon` to the image `icon-name`.
- Scroll up or down changes volume by `0.05` and clamps to `0..1`.
- Tooltip `Volume: N%`, updated on volume changes.
- No click action and no mute.

`VolumeSlider` is a vertical `Gtk.Box` with classes `slider` and `volume`.

- It hides itself if `volume_control.get() == 0` and the default speaker is null.
- Header: icon `audio-volume-medium-symbolic`, label `Volume`, percentage label with class `numeric`.
- `Gtk.Scale` range `0..150`, step 1, no drawn value.
- On change, sets volume to `value / 100`.
- On `volume_changed`, sets the scale to `volume * 100`.
- Adds class `overamp` above 100.

`Microphone` is a vertical box with class `slider`.

- Uses `audio.default_microphone`; returns early if null.
- Header: icon `audio-input-microphone-symbolic`, label `Microphone`, percentage label.
- Scale range `0..100`, step 1.
- On change, sets `mic.volume = value / 100`.
- On `mic.notify["volume"]`, updates the scale and percentage.

`ChangeIndicator` shows the volume OSD:

- Fraction is `volume / 1.5`.
- Label is `round(volume * 100)%`.
- `overamp` when percentage > 100.

## Core

Add an `AudioService` to `nosh-core`.

### Model

```rust
pub struct AudioModel {
    pub default_sink: Option<AudioEndpoint>,
    pub default_source: Option<AudioEndpoint>,
    pub sinks: Vec<AudioEndpoint>,
    pub sources: Vec<AudioEndpoint>,
}

pub struct AudioEndpoint {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub volume: f64,       // 0.0..=1.5 for output, 0.0..=1.0 for input
    pub muted: bool,
    pub icon_name: String,
    pub is_default: bool,
}
```

### Backend choice

The current implementation uses libwireplumber through AstalWp. Rust has no equivalent high-level binding. Use the try tree from [backends.md](../backends.md):

| Order | Backend | Probe | Notes |
| --- | --- | --- | --- |
| 1 | `WirePlumberNativeBackend` | direct libwireplumber binding | Not planned for v1. High complexity. |
| 2 | `PulseBackend` | open a Pulse context and read server info | Default. Works with `pipewire-pulse` and PulseAudio. |
| 3 | `WpctlBackend` | `wpctl get-volume @DEFAULT_AUDIO_SINK@` succeeds | CLI fallback. Uses `pactl subscribe` for changes. |
| 4 | `NullAudioBackend` | always | Shell starts, audio widgets hide. |

Why Pulse is the default:

- It provides `Context::subscribe` callbacks for sink, source, and server changes.
- It exposes the default sink and source.
- It exposes volume and mute per channel.
- It does not shell out on every change.

Trade-off: it adds a C dependency on `libpulse`. Most PipeWire desktops already have it because they run `pipewire-pulse`.

CLI fallback:

- `wpctl get-volume @DEFAULT_AUDIO_SINK@` for volume and mute.
- `wpctl set-volume @DEFAULT_AUDIO_SINK@ <value>` to set.
- `wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle`.
- `pactl subscribe` for change events.
- Parse the event stream and re-read the values.

Do not use two backends at once for the same capability. Select one at startup, log it, and emit it in the state snapshot. If the selected backend disconnects, fall back to the next candidate and re-probe the better backend with backoff.

Do not FFI into libwireplumber in v1. The full AstalWp C API is about 4,900 lines and the shell needs a small fraction of it. Leave the native backend as a documented future option.

### Range and constants

- Output maximum `1.5`, step `0.05`.
- Input maximum `1.0`.
- Clamp every value in the core.
- Keep `1.5` in one constant. The bar and slider must agree.

Decision: amplification is available everywhere in the logic and the UI, except the bar scroll action. The core and the quick settings slider allow `0..1.5`. The bar scroll wheel stops at `1.0` so a stray scroll cannot push the output into amplified range. If the volume is already above `1.0`, scrolling down reduces it normally; scrolling up does nothing until it falls below `1.0`.

### Origin

```rust
pub enum ChangeOrigin {
    User,
    External,
    Initial,
}

pub enum AudioEvent {
    Snapshot(AudioModel),
    VolumeChanged { endpoint: EndpointKind, volume: f64, old: f64, origin: ChangeOrigin },
    MuteChanged { endpoint: EndpointKind, muted: bool, origin: ChangeOrigin },
    DefaultChanged { kind: EndpointKind, endpoint: AudioEndpoint },
}
```

The change indicator shows only for `origin == User`. A volume change from another application emits `External` and updates the widgets without an OSD. The first snapshot emits `Initial`.

### Commands

```rust
Command::AudioSetVolume(f64)
Command::AudioStepVolume(f64)
Command::AudioSetMuted(bool)
Command::AudioSetDefault(u32)
```

`AudioSetVolume` applies to the default sink in v1. A future device selector adds an endpoint ID.

### Icon

Use the backend's volume icon when available. If the Pulse backend does not expose one, compute it:

```text
muted or volume == 0 -> audio-volume-muted-symbolic
< 0.33 -> audio-volume-low-symbolic
< 0.66 -> audio-volume-medium-symbolic
else -> audio-volume-high-symbolic
```

AstalWp exposes `volume_icon`; the bar binds it directly. Keep the same icon names.

## UI

### Bar volume

`Volume` is a `gtk::Button` with class `square-icon`, a `gtk::Image` child, and a scroll controller.

- Icon from `AudioModel.default_sink.icon_name`.
- Scroll up: send `AudioStepVolume(0.05)` only when the current volume is below `1.0`. Scroll down: send `-0.05`. Use a vertical scroll controller and return `true` to consume the event.
- Tooltip `Volume: {floor(volume * 100)}%`. The current code uses `floor`. Keep `floor` for parity.
- Add a click action for mute if the design allows. The current code has none. If added, send `AudioSetMuted(!muted)`.
- The bar never shows the `overamp` class. It is not an amplification control.

### Volume slider

`VolumeSlider` is a vertical `gtk::Box` with classes `slider` and `volume`.

- Hide when there is no default sink.
- Header icon, label, percentage.
- `gtk::Scale` with range `0..150`, step 1.
- On value change, send `AudioSetVolume(value / 100)`. Avoid a feedback loop: use a guard flag or compare with the model value before sending.
- On `VolumeChanged`, set the scale and percentage. The guard prevents the setter from sending a second command.
- Add class `overamp` when percentage > 100.
- Keep the red overamp styling from `scale.overamp slider`.

### Microphone

`Microphone` is a vertical `gtk::Box` with class `slider`.

- Hide when there is no default source.
- Header icon, label, percentage.
- Scale `0..100`, step 1.
- On change, send a source volume command. Add `Command::AudioSetSourceVolume(f64)` if the service does not infer the endpoint from the command.
- On source volume change, update the scale and percentage.

### Change indicator

See `features/change-indicator.md`. The volume event must carry `ChangeOrigin::User` when the shell initiated the change.

### Debouncing

The slider emits a value on every pixel movement. Do not send one D-Bus call per pixel. Coalesce updates in the core with a 20 to 50 ms debounce and apply the latest value. The UI can still update the percentage label immediately.

## CSS

Keep the current `.slider`, `.volume`, `.overamp`, `.numeric`, and scale rules. The volume scale trough has a linear gradient that marks the 100 percent point at two thirds of a 150 percent range. Keep it.

## Definition of done

- The bar icon and tooltip follow the default sink volume.
- Scroll changes volume by 5 percent and clamps the bar action at 0 and 100 percent.
- The quick settings slider and the OSD allow up to 150 percent and show overamp above 100 percent.
- The quick settings slider shows 0 to 150 percent and updates from external changes.
- The microphone slider shows 0 to 100 percent.
- Volume changes initiated by the shell show the OSD; external changes do not.
- The core emits a snapshot on startup and after a backend reconnect.
- No GTK code is in the audio service.
- No shell command runs per slider pixel; writes are debounced.
- The audio service starts even when PipeWire or Pulse is unavailable, and the widgets hide until it appears.
