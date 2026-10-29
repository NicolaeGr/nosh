# App launcher

Current files: `src/app_launcher/Window.vala`, `widgets/AppGrid.vala`, `widgets/SearchBar.vala`, `utils/AppProvider.vala`, `utils/AppEntry.vala`, `utils/ConfigManager.vala`, `utils/DatabaseManager.vala`, `utils/Plugin.vala`, `plugins/RunCommandPlugin.vala`, `plugins/RestartNoshPlugin.vala`, `src/app_launcher/AppLauncher.scss`, and the three Blueprint files.

## Current behavior

### Window

`AppLauncher.Window` is an `Astal.Window` from `Window.blp`:

- Anchor top, bottom, left, right.
- Exclusivity `IGNORE`.
- Layer `OVERLAY`.
- Keymode `EXCLUSIVE`.
- Visible false.
- Namespace `nosh-app-launcher`.
- CSS class `AppLauncher`.

Content:

```text
Box background (hexpand, vexpand, class background)
  Box main_container (vertical, center, class container, size 500x312)
    SearchBar
    AppGrid
```

`construct`:

- Gets `AppProvider` and `AppState`.
- Sets `main_container` size to 500 by 312 (16:10).
- Connects search changes, activation, arrow keys, grid exit, and close.
- Connects app activation.
- Adds a background click gesture that closes when the background is picked.
- Binds `AppState.app_launcher_open` to `visible` bidirectionally.
- On visible: reloads apps, clears the search text, sets all apps, presents, and focuses the search bar.

### Search bar

`SearchBar` is a horizontal `Gtk.Box` from `SearchBar.blp`, spacing 8:

- `Gtk.SearchEntry` with placeholder `Search applications...`, expanding, class `SearchBar`.
- `Gtk.Image` for the active plugin, 24 px, hidden by default, class `plugin-icon`.

Signals: `query_changed`, `activate_selected`, `close_requested`, `arrow_up`, `arrow_down`, `arrow_left`, `arrow_right`, `exit_grid_mode`.

Key handling:

- If Control is held, ignore the key.
- Escape: if in grid mode, exit grid mode and consume. Else request close.
- In grid mode: Up, Down, Left, Right emit the matching signal.
- Down when not in grid mode enters grid mode and emits `arrow_down`.
- Text changes exit grid mode before emitting `query_changed`.
- Enter emits `activate_selected`.

### App grid

`AppGrid` is a vertical `Gtk.Box` from `AppGrid.blp` with class `AppGrid`:

- `Gtk.ScrolledWindow` with horizontal policy never, vertical automatic, expand both.
- `Gtk.FlowBox` at the top, `max-children-per-line = 2`, `min-children-per-line = 2`, column and row spacing 8, homogeneous, single selection.

`set_apps`:

- Replaces the app list and resets `selected_index` to 0.
- Resets scroll to 0.
- Removes every flow box child.
- Creates an item for each app.
- Selects the first child.

Each item is a horizontal box with class `AppItem`, spacing 8:

- Icon 32 px from `app.icon` or `application-x-executable`.
- Label with the app name, ellipsize end, xalign 0, expanding.

Keyboard movement:

- Up: index minus 2 if non-negative.
- Down: index plus 2 if below the list length.
- Left: index minus 1 if not in the first column.
- Right: index plus 1 if not in the last column and below the last index.
- After movement, select the child and scroll it into view.
- Enter activates `apps[selected_index]`.
- Flow box child activation also activates the app.

Scroll to selected computes the child's y position relative to the flow box and adjusts the vertical adjustment.

### App entry

`AppEntry` fields:

```text
name, icon, desktop_id, description, categories, app_info,
launch_func, exec_command, frequency
```

`launch` tries `launch_func`, then `exec_command`, then `app_info`.

`launch_command`:

- Replaces `$HOME` with the home directory.
- Builds `hl.dsp.exec_cmd("<command>")`.
- Spawns `hyprctl dispatch <lua>` with stdout and stderr to `/dev/null`.

`launch_desktop_app`:

- Gets `app_info.get_commandline()`.
- Removes `%f`, `%F`, `%u`, `%U`, `%d`, `%D`, `%n`, `%N`, `%v`, `%m`, `%i`, `%c`, `%k`.
- Strips and launches.
- If there is no command line, calls `app_info.launch`.

### App provider

`load_apps`:

- Iterates `AppInfo.get_all()`.
- Keeps apps where `should_show()`.
- Uses the display name.
- Extracts the first name from a `ThemedIcon`, or `icon.to_string()` otherwise.
- Sets description.
- Splits `DesktopAppInfo.get_categories()` on `;`.
- Reads frequency from SQLite by `desktop_id`.
- Adds custom entries from the config.
- Sorts by frequency descending.

`search(query)`:

- Empty or whitespace query returns all apps.
- If a plugin matches, returns exactly one entry from the plugin.
- Else scores every app:
  - Name match.
  - If score is 0, description match divided by 2.
  - If score is still 0, category match divided by 3.
- Sorts by score descending, then frequency descending.
- If no results and `is_valid_command(query)`, returns a `Run: <query>` entry with icon `utilities-terminal`.
- Otherwise returns the scored list.

`is_valid_command`:

- Rejects empty.
- Splits on spaces and takes the first token.
- Returns true if the token contains `/`.
- Else true if `Environment.find_program_in_path(token)` finds it.

`record_launch` increments the SQLite frequency and the in-memory frequency, then re-sorts.

`reload` clears the list, reloads config, and reloads apps.

### Fuzzy score

`fuzzy_match_score(text, pattern)`:

1. If `text` contains `pattern`, start at 1000.
   - Add 500 if the match starts at position 0.
   - Add 200 if the character before the match is a space, `-`, or `_`.
   - Return.
2. Else fuzzy walk:
   - Iterate text and pattern.
   - On a match:
     - Track consecutive matches and the maximum run.
     - Add 100 if both indexes are 0.
     - Add 30 if the previous text character is a space, `-`, or `_`.
     - Add `consecutive * 20` if consecutive > 1, else 10.
     - Increment `matched_at_start` when `text_idx == pattern_idx`.
     - Advance the pattern.
   - On a mismatch: count a gap if there was a run, reset consecutive.
3. Return 0 if the pattern is not fully matched.
4. Return 0 if `max_consecutive < pattern.length * 0.6`.
5. Divide by `gaps * 2` if gaps > 2.
6. Return 0 if the score is below 30.

Keep these constants in one module. They are part of the ranking users feel.

### SQLite

Path `~/.local/share/nosh/app_launcher.db`. The Rust menu engine replaced this SQLite database with the JSON launch log at `~/.local/share/nosh/app_usage.json`. See [menus.md](menus.md#ordering-by-launch-history).

```sql
CREATE TABLE IF NOT EXISTS app_usage (
    desktop_id TEXT PRIMARY KEY,
    frequency INTEGER DEFAULT 0,
    last_used INTEGER DEFAULT 0
);
```

`increment_frequency`:

```sql
INSERT INTO app_usage (desktop_id, frequency, last_used)
VALUES (?, 1, ?)
ON CONFLICT(desktop_id) DO UPDATE SET
    frequency = frequency + 1,
    last_used = ?
```

`last_used` is Unix seconds.

### TOML config

Path `~/.config/nosh/app_launcher.toml`.

On first run, create the directory and an example file. The first line is `# EXAMPLE_CONFIG`. If the file starts with that line, the parser returns without loading entries.

The file format:

```toml
[[entry]]
name = "App Name"
icon = "icon-name"
exec = "command"
```

The current parser reads lines, starts a new entry on `[[entry]]`, and reads `name`, `icon`, and `exec` by splitting on the first `=`. It strips matching single or double quotes. It does not support inline comments or escapes.

The example file includes a custom script and a Firefox profile example.

### Plugins

`Plugin` interface:

```text
matches(query) -> bool
get_display_label() -> string
get_icon() -> string
get_name() -> string
create_entry(query) -> AppEntry?
get_priority() -> int
```

`PluginManager` sorts by priority descending and returns the first match.

Built-ins:

- `RunCommandPlugin`, priority 100. Matches `run:`, `kt:`, or `kitty:`. `run:` executes the command directly. `kt:` and `kitty:` execute with `kitty -e`.
- `RestartNoshPlugin`, priority 200. Matches a lowercase query containing both `restart` and `nosh`. Launches `systemctl --user restart nosh`, then `nosh &` as a fallback.

The search bar shows the active plugin icon when a plugin matches.

## Core

Add an `AppService` to `nosh-core`. The UI receives snapshots and pure search results.

### Types

```rust
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub categories: Vec<String>,
    pub desktop_id: Option<String>,
    pub exec: Option<String>,
    pub launch: LaunchTarget,
    pub frequency: i64,
}

pub enum LaunchTarget {
    Desktop(PathBuf),       // .desktop file id or path
    Command(String),
    Builtin(BuiltinAction),
}

pub enum BuiltinAction {
    RestartNosh,
}
```

Do not store a `GAppInfo` pointer in the core. Store the desktop id and use `gio::DesktopAppInfo` in the UI only if needed, or store the parsed command line.

### Load

- Enumerate `.desktop` entries with `freedesktop-desktop-entry` or `gio` from the UI. If the core must stay GTK-free, use a pure Rust desktop entry parser such as `freedesktop-desktop-entry`, or parse `$XDG_DATA_DIRS/applications`.
- Keep entries where `should_show` is true. The desktop spec rule: `NoDisplay=true`, `Hidden=true`, or `OnlyShowIn`/`NotShowIn` mismatch.
- Extract the display name, icon, description, categories, keywords, executable, and `StartupWMClass`.
- Read frequency from SQLite.
- Add custom TOML entries.
- Sort by frequency descending for the initial list.
- Watch the applications directories with `notify` and reload on change, or reload when the launcher opens. The current code reloads on open. Keep that for v1.

Emit `AppsEvent::IndexChanged(Vec<AppEntry>)`.

### Search

Search is a pure function over the index and the providers:

```rust
pub fn search(index: &[AppEntry], query: &str, providers: &[Box<dyn SearchProvider>]) -> Vec<AppEntry>
```

Keep the current algorithm and constants. Put it in `nosh-core/src/apps/search.rs` with table tests.

The plugin check happens before scoring and returns exactly one entry.

Description and category scoring only apply when the name score is 0. Keep the divisors 2 and 3.

Sort by score descending, then frequency descending. Use a stable sort so equal scores keep index order.

Command fallback: if the result is empty and the query looks like a command, append `Run: <query>`.

### Command detection

Keep the current rule:

- Split on whitespace.
- If the first token contains `/`, it is a command.
- Else look it up in `PATH`.

In Rust, `which` or a manual `PATH` search. Do not run the command to test it.

### Launch

The user's concern is real: if the launcher spawns a child and nosh crashes, the child must not die with it or become a zombie. The current Vala code used Hyprland `exec_cmd` because children were not disowned.

Proposal: a `spawn_detached` helper in `nosh-core`.

```rust
pub fn spawn_detached(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    env: &[(String, String)],
) -> Result<(), LaunchError>
```

Implementation rules:

- Use a double fork or `setsid` plus a second fork so the final child is reparented to init. `setsid` alone is not enough if the parent process group is killed later.
- Set stdin, stdout, and stderr to `/dev/null`.
- Reset the signal mask and signal dispositions that the shell may have changed.
- Set the working directory to the user's home unless the entry overrides it.
- Inherit the current environment and apply the entry's extra variables.
- Close file descriptors above 2, or use `O_CLOEXEC` everywhere.
- Return after the first fork. Do not wait for the app.

Use `libc` or the `nix` crate. Do not use `Command::spawn` and drop the `Child` without a fork; that still leaves a direct child that becomes a zombie until the shell reaps it.

Launch targets:

- `LaunchTarget::Desktop`: remove `%` field codes, parse the Exec line with the desktop entry rules, and call `spawn_detached`.
- `LaunchTarget::Command`: call `spawn_detached` with `sh -c <command>` only because the user explicitly configured a shell command.
- `LaunchTarget::Builtin(RestartNosh)`: run `systemctl --user restart nosh`, fallback to spawning the current executable through `spawn_detached`.

Do not use Hyprland `exec_cmd`, and do not add a `spawn_via` switch. Spawn through the session manager instead, so the app leaves nosh's service cgroup and picks up the session environment.

The core tries `uwsm app -t service -- <argv>` in a UWSM session, then `systemd-run --user --scope --quiet -- <argv>`, then the detached spawn above. UWSM gets a short probe, so a setup failure falls through to the next step. `systemd-run --scope` runs in the foreground for the app's lifetime, so only a spawn failure falls through. A menu action can force a step with `launch = "uwsm"`, `"systemd-run"`, or `"detached"`. Desktop entries use `auto`.

This removes the compositor dependency, the shell quoting bug, and the child lifetime problem.

### SQLite

Keep the schema and path. Use `rusqlite` on the core thread or a dedicated blocking thread. Do not block the Tokio runtime with a long SQLite call; the calls are small, but a dedicated `spawn_blocking` is safer.

Increment frequency inside a transaction. Update the in-memory entry and emit the new index or a targeted `AppsEvent::FrequencyChanged`.

`last_used` stays for a future recency ranker.

### TOML

Use `serde` and `toml`:

```rust
#[derive(Deserialize)]
struct LauncherConfig {
    entry: Vec<CustomEntry>,
}

#[derive(Deserialize)]
struct CustomEntry {
    name: String,
    icon: Option<String>,
    exec: String,
}
```

Keep the `# EXAMPLE_CONFIG` sentinel. Keep the example file text. Accept single and double quoted strings, inline comments, and escapes through the TOML parser.

Validate on load. A malformed file logs an error and keeps the previous entries. It does not crash the launcher.

### Plugins

Keep the two providers and the priority order. Use a trait:

```rust
pub trait SearchProvider: Send + Sync {
    fn matches(&self, query: &str) -> bool;
    fn create_entry(&self, query: &str) -> Option<AppEntry>;
    fn icon(&self) -> &'static str;
    fn priority(&self) -> i32;
}
```

Match the exact prefixes and the lowercase `restart` plus `nosh` rule. Return the same display labels and icons.

Do not build a dynamic plugin loader in v1. A trait with two built-ins is enough.

## UI

> The generic menu engine now owns this window. The launcher is the `apps` builtin, opened with `nosh menu apps`. See [menus.md](menus.md) for the shared window, view, and focus model. The rest of this section records the original launcher behavior and remains the visual reference for the `apps` menu.

### Window

Create a `gtk::Window` with `gtk4-layer-shell`:

- All four anchors.
- Layer `Overlay`.
- Exclusivity `Ignore`.
- Keyboard mode `Exclusive`.
- Namespace `nosh-app-launcher`.

Class `AppLauncher`. Content: background box, centered container, search bar, grid.

Set the container to 500 by 312. If the content changes later, use `set_size_request(500, -1)` and let the grid expand.

On show:

- Reload the app index through the core, or use the last snapshot.
- Clear the search text.
- Set the grid to all apps.
- Set the layer-shell monitor to the current monitor. If the window was previously on another monitor, hide it, set the monitor, then present.
- Present and focus the search entry.

### Search bar

Use `gtk::SearchEntry` and a plugin icon. Keep the signals as Rust callbacks or a small `SearchBar` struct with closures.

Key handling with `gtk::EventControllerKey` in the capture phase:

- Control held: return `Propagation::Proceed`.
- Escape: exit grid mode if active, else close.
- Grid mode: arrows move the selection.
- Down outside grid mode: enter grid mode and move down.
- Enter: activate selection.

Do not use raw numeric key values. Use `gdk::Key`.

### Grid

Use `gtk::FlowBox` with two columns, homogeneous, single selection, row and column spacing 8. Or use a `gtk::GridView` with a `gio::ListStore`. The flow box matches the current layout.

Do not rebuild every item on every keystroke if the list is large. The current app count is small, so a rebuild is acceptable, but keyed updates are better. At minimum, keep the selected index stable across updates with the same query.

Keyboard movement uses the same `COLUMNS = 2` logic. Keep the current bounds:

- Up: `index - 2 >= 0`.
- Down: `index + 2 < len`.
- Left: `index % 2 != 0`.
- Right: `index % 2 != 1 && index < len - 1`.

Scroll to the selected child after moving.

### App item

Horizontal box, class `AppItem`, spacing 8:

- Icon 32 px.
- Label, ellipsize end, xalign 0, expanding.
- Tooltip with the description or the desktop id if present.

### Custom entries and script providers

Keep the current `[[entry]]` table and add the optional fields from `features/custom-applets.md`. Add script search providers there too. The launcher UI treats custom entries and provider results exactly like normal app entries, with the provider icon shown in the search bar.

### Visibility

Bind visibility to the UI `app_launcher_open` boolean. Clicking the background closes. Escape closes. Activating an app records the launch, launches, and closes.

## CSS

Keep the `AppLauncher.scss` selectors:

- `.container` background `alpha(@window_bg_color, 0.95)`, radius 12, padding 16, border `alpha(@window_fg_color, 0.1)`.
- `.SearchBar` margin bottom 12, padding 8 12, radius 8, background `alpha(@card_bg_color, 0.8)`, border, caret accent. Focus border accent and a 2 px shadow.
- `.AppGrid` radius 8.
- `flowboxchild` radius 6, selected background `alpha(@accent_bg_color, 0.2)` and outline, focus outline.
- `.AppItem` padding 8 12, radius 6, hover background `alpha(@accent_bg_color, 0.1)`, label 0.9 em.

## Defects to fix

- Replace the line-based TOML parser with a real TOML parser.
- Remove the Hyprland launch dependency by default.
- Do not rebuild the app list on every open unless the file monitor is unavailable.
- Keep `last_used` meaningful even if it is not used yet.
- Do not hardcode the plugin manager as a singleton. Inject the providers.
- Handle a malformed desktop file and a missing icon without a warning per keystroke.
- The `RestartNosh` fallback `nosh &` should use the current executable, not a PATH lookup.
- The command fallback should not appear for a query with a trailing space or an empty command.

## Definition of done

- `nosh menu apps` opens the window centered over the current workspace.
- The search entry is focused.
- An empty query lists all apps, most frequent first.
- Typing filters with the current score, then frequency order.
- Down enters grid mode; arrows navigate; Enter launches; Escape exits grid mode, then closes.
- A plugin prefix shows the plugin icon and returns one entry.
- Launching an app increments its frequency and reorders the list the next time it opens.
- A custom TOML entry appears and launches.
- A malformed TOML file logs an error and leaves the launcher usable.
- A valid command query with no app match shows `Run: <query>`.
- The launcher opens on the current monitor, not always the primary.
- The launcher works when Hyprland is not running.
- A launched child survives a shell crash and does not become a zombie.
- Custom entries and script providers from `features/custom-applets.md` appear and launch.
- The core owns the index, database, config, and scoring. The UI owns the window, selection, and keyboard state.
