# Build and packaging

Pseudocode build plan. Pin exact versions in `Cargo.lock` and the package manifest. Verify crate names and feature flags before the first build.

## Workspace

```text
workspace/
  Cargo.toml
  crates/
    nosh-core/
      Cargo.toml
      src/
    nosh-ui/
      Cargo.toml
      src/
      resources/
  docs/
  data/
    com.nicolaegr.nosh.gschema.xml
    nosh.service
    pam.d/nosh-auth
    icons/
    style/
```

Workspace manifest:

```toml
[workspace]
members = ["crates/nosh-core", "crates/nosh-ui"]
resolver = "2"

[workspace.package]
edition = "2021"
license = "MIT"
rust-version = "1.80"
```

Adjust the license to the one you choose. Keep Astal attribution in `README.md` or `CREDITS.md`.

## nosh-core

Pseudocode manifest:

```toml
[package]
name = "nosh-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "sync", "process", "fs", "signal"] }
zbus = { version = "4", features = ["tokio"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
rusqlite = { version = "0.32", features = ["bundled"] }
libpulse-binding = "2"
wayland-client = "0.31"
wayland-protocols = { version = "0.31", features = ["client", "unstable"] }
freedesktop-desktop-entry = "0.5"
image = "0.25"
sha2 = "0.10"
reqwest = { version = "0.12", features = ["blocking", "rustls-tls"], default-features = false }
notify = "6"
tracing = "0.1"
thiserror = "1"
async-channel = "2"
bitflags = "2"
zeroize = "1"
secrecy = "0.8"
libc = "0.2"
nix = { version = "0.29", features = ["process", "signal", "fs"] }
gio = "0.20"
```

Notes:

- `gio` is for GSettings. It does not pull GTK.
- `libpulse-binding` is optional behind an `audio-pulse` feature if you want a pure `wpctl` build.
- `reqwest` is only for MPRIS cover art. Drop it if the UI does not display covers.
- `freedesktop-desktop-entry` is one option. A small parser is acceptable if the crate is not a good fit.
- `zbus` major version must match the rest of the project. Pin it.

Features:

```toml
[features]
default = ["audio-pulse"]
audio-pulse = ["libpulse-binding"]
```

## nosh-ui

Pseudocode manifest:

```toml
[package]
name = "nosh-ui"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[[bin]]
name = "nosh"
path = "src/main.rs"

[dependencies]
nosh-core = { path = "../nosh-core" }
gtk4 = "0.9"
glib = "0.20"
gio = "0.20"
gtk4-layer-shell = "0.4"
async-channel = "2"
tracing = "0.1"
tracing-subscriber = "0.3"

[build-dependencies]
grass = "0.13"
glib-build-tools = "0.20"
```

Pin the GTK and gtk4-layer-shell versions to the versions available in the target distribution. gtk-rs crates must match the same GTK minor version.

## System dependencies

Typical packages:

```text
gtk4
gtk4-layer-shell
glib2
networkmanager
bluez
upower
power-profiles-daemon
wireplumber
pipewire-pulse or pulseaudio
sqlite
pam
ddcutil
brightnessctl
sass or dart-sass
pkg-config
```

Optional:

```text
libpulse
libdrm
wayland-protocols
```

Exact package names differ by distribution. Document the names for the distributions you support.

## GSettings schema

Pseudocode XML:

```xml
<schemalist>
  <schema id="com.nicolaegr.nosh" path="/com/nicolaegr/nosh/">
    <key name="notifications-ignore-timeout" type="b"><default>false</default></key>
    <key name="notifications-dont-disturb" type="b"><default>false</default></key>
    <key name="notifications-default-timeout" type="i"><default>-1</default></key>
    <key name="notifications-max-visible" type="i"><default>3</default></key>
    <key name="notifications-display-timeout" type="i"><default>5000</default></key>
    <key name="notifications-position" type="s"><default>"top-right"</default></key>
    <key name="notifications-monitor-policy" type="s"><default>"farthest"</default></key>
    <key name="notifications-server-capabilities" type="as">
      <default>["action-icons","actions","body","body-hyperlinks","body-images","body-markup","icon-static","persistence","sound"]</default>
    </key>
    <key name="bar-time-format" type="s"><default>"%H:%M - %a %e"</default></key>
  </schema>
</schemalist>
```

Install to `$DATADIR/glib-2.0/schemas/` and run `glib-compile-schemas`. The core uses a `SettingsStore` trait. The binary wires a GSettings backend. Tests use an in-memory backend.

If you prefer nested paths instead of flat key names, use `path` and child schemas. Keep the key names stable once users write scripts against them.

## PAM service

Pseudocode file `/etc/pam.d/nosh-auth`:

```text
# include the distribution's auth stack
include system-auth
include common-auth
```

Do not copy a stack blindly across distributions. Use the correct include for the target. On NixOS, declare the service in `security.pam.services.nosh-auth`.

## systemd user unit

Pseudocode service:

```ini
[Unit]
Description=nosh shell
PartOf=graphical-session.target
After=graphical-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/nosh
Restart=on-failure
RestartSec=1
Environment=RUST_LOG=nosh=info

[Install]
WantedBy=graphical-session.target
```

Adjust the executable path for the install prefix. Use `systemctl --user daemon-reload` after installation.

## Build script

`crates/nosh-ui/build.rs`:

```rust
fn main() {
    compile_scss("resources/style/style.scss", "style.css");
    compile_gresource("resources/resources.gresource.xml");
    println!("cargo:rerun-if-changed=resources");
}
```

SCSS rules:

- Compile with `grass` first.
- If `grass` cannot reproduce the GTK-specific output, run the `sass` CLI and fail with a clear message when it is missing.
- Keep GTK functions such as `alpha()`, `mix()`, `lighter()`, and `darker()` as function calls in the CSS output. Do not evaluate them in Sass.
- Add a test that the generated CSS contains the expected functions and selectors.

Resource rules:

- Use `glib-build-tools` or `glib-compile-resources` for the GResource.
- Keep the resource prefix `com.nicolaegr.nosh`.
- Load CSS with `gtk::CssProvider::load_from_string` or from the resource.

## Build commands

```sh
cargo build -p nosh-core
cargo test -p nosh-core
cargo build -p nosh-ui
cargo run -p nosh-ui
```

Release:

```sh
cargo build --release -p nosh-ui
install -Dm755 target/release/nosh "$DESTDIR/usr/bin/nosh"
install -Dm644 data/com.nicolaegr.nosh.gschema.xml "$DESTDIR/usr/share/glib-2.0/schemas/com.nicolaegr.nosh.gschema.xml"
glib-compile-schemas "$DESTDIR/usr/share/glib-2.0/schemas"
install -Dm644 data/nosh.service "$DESTDIR/usr/lib/systemd/user/nosh.service"
install -Dm644 data/pam.d/nosh-auth "$DESTDIR/etc/pam.d/nosh-auth"
```

Install the CSS and icons according to the chosen resource strategy. If everything is embedded, skip the separate files and install only the binary, schema, service, and PAM file.

## Nix outline

```nix
{
  buildRustPackage,
  pkg-config,
  gtk4,
  gtk4-layer-shell,
  glib,
  networkmanager,
  bluez,
  upower,
  power-profiles-daemon,
  wireplumber,
  pipewire,
  sqlite,
  pam,
  ddcutil,
  brightnessctl,
  sass,
}:
buildRustPackage {
  pname = "nosh";
  version = "0.1.0";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ pkg-config sass ];
  buildInputs = [
    gtk4 gtk4-layer-shell glib networkmanager bluez upower
    power-profiles-daemon wireplumber pipewire sqlite pam ddcutil brightnessctl
  ];
  postInstall = ''
    install -Dm644 data/com.nicolaegr.nosh.gschema.xml \
      $out/share/glib-2.0/schemas/com.nicolaegr.nosh.gschema.xml
    glib-compile-schemas $out/share/glib-2.0/schemas
    install -Dm644 data/nosh.service $out/lib/systemd/user/nosh.service
  '';
}
```

Add the PAM service through the NixOS module, not through `postInstall`, so `security.pam.services.nosh-auth` is declared.

## Install layout

```text
/usr/bin/nosh
/usr/share/glib-2.0/schemas/com.nicolaegr.nosh.gschema.xml
/usr/lib/systemd/user/nosh.service
/etc/pam.d/nosh-auth
/usr/share/nosh/style.css         optional if not embedded
/usr/share/nosh/icons/            optional if not embedded
```

## Verification

- `cargo test -p nosh-core` passes without a display.
- `cargo build -p nosh-ui` links GTK4 and gtk4-layer-shell.
- `glib-compile-schemas --strict` accepts the schema.
- `gsettings list-keys com.nicolaegr.nosh` lists every key.
- `systemd-analyze verify` accepts the user unit.
- `pamtester nosh-auth <user> authenticate` succeeds when the PAM stack is correct.
- The binary starts under a Wayland session with layer shell support.
- The binary refuses to start with a clear error when layer shell is unavailable, or starts without the bar if that is the chosen fallback.
