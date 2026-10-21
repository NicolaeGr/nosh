# Change indicator

Current files: `src/indicators/ChangeIndicator.vala`, `ChangeIndicator.blp`, `ChangeIndicator.scss`, `src/utils/system_control/VolumeControl.vala`, `BrightnessControl.vala`.

## Current behavior

`ChangeIndicator` is an `Astal.Window` from `ChangeIndicator.blp`:

- Anchor bottom, left, right.
- Exclusivity `IGNORE`.
- Layer `TOP`.
- Keymode `NONE`.
- Visible false.
- Namespace `nosh-change-indicator`.
- CSS class `ChangeIndicator`.

Content:

```text
Box indicator-card (vertical, spacing 12, center, end, margin-bottom 40)
  Box row (horizontal, spacing 8, center)
    Image icon_widget (class indicator-icon)
    Label value_label (class indicator-value)
  ProgressBar progress_bar (class indicator-bar, expanding)
```

`construct` connects:

- `volume.volume_changed` to `show_volume_indicator`.
- `brightness.brightness_changed` to `show_brightness_indicator`.

Volume:

- Fraction `volume / 1.5`.
- Label `round(volume * 100)%`.
- Icon `audio-volume-medium-symbolic`.
- Overamp when percentage > 100.

Brightness:

- Fraction `brightness / 100`.
- Label `"{brightness}%"`.
- Icon `display-brightness-symbolic`.
- No overamp.

`show_indicator` sets visible true and starts an 800 ms hide timer. A new change removes the old timer and restarts it.

The window has no input. It is a passive overlay.

### CSS

```css
.ChangeIndicator { background-color: transparent; }
.indicator-card {
  background-color: alpha(@window_bg_color, 0.7);
  border-radius: 16px;
  border: 1px solid alpha(@window_fg_color, 0.08);
  padding: 24px;
  min-width: 280px;
  backdrop-filter: blur(8px);
}
.indicator-icon { font-size: 32pt; color: alpha(@window_fg_color, 0.9); min-width: 40px; }
.indicator-value { font-weight: 700; color: alpha(@window_fg_color, 0.95); font-size: 24pt; min-width: 80px; }
.indicator-bar { min-height: 10px; border-radius: 5px; background-color: alpha(@window_fg_color, 0.1); }
.indicator-bar trough { background-color: alpha(@window_fg_color, 0.1); border-radius: 5px; min-height: 10px; }
.indicator-bar progress { background-color: @accent_bg_color; border-radius: 5px; min-height: 10px; }
```

The overamp class turns the progress bar red, from `src/style/common/_index.scss`.

## Core

The indicator is UI, but it needs to know whether a change came from the shell or from elsewhere. The core audio and brightness services must include an origin.

```rust
pub enum ChangeOrigin {
    User,
    External,
    Initial,
}
```

- `User`: the shell sent the command. Show the indicator.
- `External`: another application or keybind changed the value. Update widgets, no indicator.
- `Initial`: the first snapshot. Never show the indicator.

The current `VolumeControl.volume_changed` and `BrightnessControl.brightness_changed` only fire from the shell's own methods, so every change shows the OSD. That is acceptable for parity, but it misses external changes. Use the origin model instead of porting the limitation.

## UI

Create a `gtk::Window` with `gtk4-layer-shell`:

- Bottom, left, and right anchors.
- Layer `Top`.
- Exclusivity `Ignore`.
- Keyboard mode `None`.
- Namespace `nosh-change-indicator`.
- Monitor: the current monitor. Set `gtk4_layer_shell::set_monitor` before presenting.

Class `ChangeIndicator`. Content: a centered vertical card with the icon, value, and progress bar. Add `margin_bottom = 40` in the content or as a layer-shell bottom margin. Do not use both.

### Show

```rust
fn show_volume(&mut self, volume: f64, origin: ChangeOrigin)
fn show_brightness(&mut self, percent: f64, origin: ChangeOrigin)
```

- Ignore `Initial` and `External`.
- Set the icon, label, and fraction.
- For volume, fraction is `(volume / 1.5).clamp(0.0, 1.0)`, label is `round(volume * 100)`.
- For brightness, fraction is `(percent / 100.0).clamp(0.0, 1.0)`, label is the integer percent.
- Add `overamp` on the progress bar when the volume percent exceeds 100.
- Set the window visible.
- Restart an 800 ms hide timer. Use `glib::timeout_add_local_once` or a source ID that can be removed.

### Hide

When the timer fires, set visible false. Do not destroy the window. Reuse it for the next change.

### Window behavior

The indicator must not take focus or input. Set the keyboard mode to `None`. Do not set an exclusive zone.

If a volume and a brightness change arrive close together, the latest one wins and the timer restarts. Do not queue indicators.

## CSS

Keep the current class names and values. The overamp rule lives in the global style:

```css
scale.overamp slider, progressbar.overamp progress {
  background-color: #ff6464;
  border-color: #ff6464;
}
progressbar.overamp progress:hover { background-color: #ff7a7a; border-color: #ff7a7a; }
```

## Defects to fix

- Use `ChangeOrigin` so startup and external changes do not flash the indicator.
- Do not create a new timer source without removing the old one.
- Clamp the fraction before setting the progress bar.
- Do not show a floating-point label such as `42.0%`. Format an integer.
- Do not create the window on every change. Create it once at startup.

## Definition of done

- Changing volume from a keybind or the shell shows the indicator at the bottom for 800 ms.
- Changing brightness shows the indicator with the brightness icon.
- Volume above 100 percent turns the bar red.
- External changes update the bar and sliders without showing the indicator.
- The first snapshot does not show the indicator.
- Rapid changes restart the timer and do not flicker.
- The window is on the top layer of the current monitor, ignores input, and uses namespace `nosh-change-indicator`.
