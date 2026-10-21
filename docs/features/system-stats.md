# System stats

Current file: `src/bar/widgets/SystemStats.vala`, `src/bar/Bar.scss`.

## Current behavior

`SystemStats` is a horizontal `Gtk.Box` with CSS class `SystemStats`. It has two labels:

- CPU label, initial text `"  0%"`, CSS class `cpu-stat`.
- Memory label, initial text `"  0%"`, CSS class `mem-stat`.

Every 3 seconds it runs:

```sh
top -bn1
```

It scans the output for a line containing `Cpu(s):` and a line containing `Mem`, then parses them.

CPU: splits the CPU line on commas, finds the part containing `" id"`, removes `id` and `%`, parses the idle percentage, and sets CPU percent to `100 - idle`.

Memory: splits the memory line on commas. With at least three parts, it takes part 0 after the colon as total, part 2 without `used` as used, and sets percent to `used / total * 100`.

Each label updates with `"{icon} {percent}%"`. It removes the four level classes and adds one based on the percentage:

| Range | CSS class | Color |
| --- | --- | --- |
| `< 50` | `level-low` | `#22c55e` |
| `50..69` | `level-medium` | `#eab308` |
| `70..89` | `level-high` | `#f97316` |
| `>= 90` | `level-critical` | `#ef4444` |

When the widget is unparented, it removes the timeout.

The CSS sets `margin: 0 8px`, bold labels, `min-width: 32px`, right-aligned text, and a 200 ms color transition.

The icons are Nerd Font private-use glyphs. They render only when a Nerd Font is active.

## Core

Add a `SystemStatsService` to `nosh-core`. It owns the sampling and emits a snapshot plus changes.

### CPU

Read `/proc/stat` and use the aggregate `cpu` line:

```text
cpu user nice system idle iowait irq softirq steal guest guest_nice
```

Compute:

```text
idle = idle + iowait
total = sum of all fields
busy_fraction = 1 - (idle_delta / total_delta)
```

Keep the previous sample. On the first sample, emit 0 percent and do not show a misleading value from boot. If `total_delta` is 0, keep the previous value.

Use `u64` and guard against counter wrap or a container restart.

### Memory

Read `/proc/meminfo`:

```text
MemTotal: ... kB
MemAvailable: ... kB
```

Compute:

```text
used = MemTotal - MemAvailable
percent = used / MemTotal * 100
```

If `MemAvailable` is missing, fall back to `MemFree + Buffers + Cached`. Do not use `used` from `top`, which includes cache differently.

### Timing

Emit every 3000 ms. Use `tokio::time::interval` with the first tick skipped, or a `sleep` loop. Keep the CPU delta interval equal to the emit interval.

Do not spawn a process. Do not parse localized text.

### Event

```rust
pub struct SystemStats {
    pub cpu_percent: u8,
    pub mem_percent: u8,
}

pub enum SystemStatsEvent {
    Changed(SystemStats),
}
```

Round to the nearest whole percent. Clamp to `0..=100`.

## UI

`SystemStats` is a horizontal `gtk::Box` with class `SystemStats`.

- CPU label with classes `cpu-stat` plus the level class.
- Memory label with classes `mem-stat` plus the level class.
- Label text `"{icon} {percent}%"`.
- Update only the text and the level class. Do not recreate labels.

The level classes and the update function are pure:

```rust
fn level_class(percent: u8) -> &'static str {
    match percent {
        0..=49 => "level-low",
        50..=69 => "level-medium",
        70..=89 => "level-high",
        _ => "level-critical",
    }
}
```

Keep the current lower bounds. `50` is medium, `70` is high, `90` is critical.

### Icons

The current glyphs are `\u{f2db}` (CPU) and `\u{f0c9}` (memory in the current label string, though the visual intent is a memory icon). Use the same code points for visual parity. If the rewrite should not depend on a Nerd Font, use a symbolic icon or a text label and document the change. Do not silently replace one glyph with a different one.

### Startup

Render `0%` with `level-low` until the first snapshot. Do not show a spinner.

## CSS

Keep the existing `.SystemStats` rules. The classes `level-low`, `level-medium`, `level-high`, and `level-critical` are applied to the labels, not the box.

## Definition of done

- CPU percent comes from `/proc/stat` deltas, not from `top`.
- Memory percent comes from `MemTotal` and `MemAvailable`.
- The widget updates every 3 seconds.
- The first CPU sample is 0 percent, not a since-boot average.
- The four color thresholds match the current values.
- No subprocess is spawned.
- The labels keep their CSS classes and do not flicker or get recreated.
- A missing or unreadable `/proc` file hides the widget or leaves the last value and logs once, without panicking.
