#!/usr/bin/env bash
#
# Capture the README screenshots in a private nested Hyprland session.
#
# This is a maintainer tool, not part of the shell. It writes docs/images.
#
# The nested compositor runs at 1920x1080 inside the current Wayland session.
# It uses a private session bus, private XDG config, data, state, and cache
# directories, and its own config tree. The script saves and restores the
# session lock marker, unsets the UWSM variables for the nested process, and
# restores the systemd user environment if the nested session changes it.
#
# Requirements: Hyprland with Lua config support, hyprctl, grim, cwebp, jq,
# ffmpeg, flock, awww, awww-daemon, sonora, playerctl, dbus-daemon, notify-send,
# curl, cargo, and a Wayland session. A Hyprland host is used to resize the nested
# window to 1080p; other hosts must be resized by hand.
#
# Usage:
#   scripts/screenshots.sh                 capture every scene
#   scripts/screenshots.sh --list          list the scene names
#   scripts/screenshots.sh --only a,b      capture named scenes
#   KEEP=1 scripts/screenshots.sh          keep the temporary session tree
#
# NOSH_BIN=/path/to/nosh skips the build. The shell picks its GTK theme from
# `ht list`; catppuccin-latte and catppuccin-mocha are used by default. The
# wallpaper menu uses $NOSH_SCREENSHOT_WALLPAPER_DIR, defaulting to the NixOS
# hypr module directory.

set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output_dir="$root/docs/images"
fixtures="$root/scripts/screenshots"
width=1920
height=1080

light_theme=${NOSH_SCREENSHOT_LIGHT_THEME:-catppuccin-latte}
dark_theme=${NOSH_SCREENSHOT_DARK_THEME:-catppuccin-mocha}
keep=${KEEP:-0}
only=""

light_wallpaper=https://raw.githubusercontent.com/saint-13/Linux_Dynamic_Wallpapers/main/Dynamic_Wallpapers/RockyMountain/RockyMountain1.jpg
dark_wallpaper=https://raw.githubusercontent.com/saint-13/Linux_Dynamic_Wallpapers/main/Dynamic_Wallpapers/RockyMountain/RockyMountain2.jpg
system_wallpapers=${NOSH_SCREENSHOT_WALLPAPER_DIR:-/etc/nixos/flake/home/_modules/hypr/bg/images}

scenes=(
  workspace-light
  workspace-dark
  centre-panel
  music
  wallpapers
  lock-screen
)

usage() {
  sed -n '/^# Usage:/,/^# NOSH_BIN/p' "$0" | sed 's/^# \{0,1\}//'
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --list)
      printf '%s\n' "${scenes[@]}"
      exit 0
      ;;
    --only)
      only=${2:?--only needs a comma separated list}
      shift 2
      ;;
    --output)
      output_dir=${2:?--output needs a directory}
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [[ -n $only ]]; then
  IFS=, read -r -a requested <<<"$only"
  selected=()
  for name in "${requested[@]}"; do
    found=0
    for scene in "${scenes[@]}"; do
      if [[ $scene == "$name" ]]; then
        found=1
        break
      fi
    done
    if [[ $found == 0 ]]; then
      echo "unknown scene: $name" >&2
      exit 2
    fi
    selected+=("$name")
  done
  scenes=("${selected[@]}")
fi

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing command: $1" >&2
    exit 1
  fi
}

for command in Hyprland hyprctl grim cwebp jq ffmpeg dbus-daemon notify-send \
  awww awww-daemon sonora playerctl; do
  need "$command"
done

if [[ -z ${WAYLAND_DISPLAY:-} ]]; then
  echo "run this inside a Wayland session" >&2
  exit 1
fi

if [[ -n ${NOSH_BIN:-} ]]; then
  bin=$NOSH_BIN
else
  need cargo
  cargo build --manifest-path "$root/Cargo.toml" -p nosh-ui --no-default-features --features stable
  bin="$root/target/debug/nosh"
fi
if [[ ! -x $bin ]]; then
  echo "not executable: $bin" >&2
  exit 1
fi

mkdir -p "$output_dir"

tmp=$(mktemp -d "${TMPDIR:-/tmp}/nosh-shots.XXXXXX")
host_runtime=$XDG_RUNTIME_DIR
host_display=$WAYLAND_DISPLAY
host_signature=${HYPRLAND_INSTANCE_SIGNATURE:-}
nested_pid=""
nosh_pid=""
bus_pid=""
awww_pid=""
sonora_pid=""
nested_signature=""
nested_display=""

# The nested session shares the lock marker path with the running shell. Save
# the current state and restore it on exit.
lock_marker="$host_runtime/nosh/session-lock"
lock_marker_backup=""
if [[ -f $lock_marker ]]; then
  lock_marker_backup="$tmp/session-lock"
  cp "$lock_marker" "$lock_marker_backup"
fi

# Record the systemd user environment so a nested compositor that calls uwsm
# cannot leave the running session's values behind.
systemd_wayland=""
systemd_signature=""
if command -v systemctl >/dev/null 2>&1; then
  systemd_env=$(systemctl --user show-environment 2>/dev/null || true)
  systemd_wayland=$(printf '%s\n' "$systemd_env" |
    sed -n 's/^WAYLAND_DISPLAY=//p' | head -1)
  systemd_signature=$(printf '%s\n' "$systemd_env" |
    sed -n 's/^HYPRLAND_INSTANCE_SIGNATURE=//p' | head -1)
fi

stop_pid() {
  local pid=$1
  [[ -n $pid ]] || return 0
  kill "$pid" 2>/dev/null || true
  local attempt
  for attempt in $(seq 1 20); do
    kill -0 "$pid" 2>/dev/null || return 0
    sleep 0.1
  done
  kill -9 "$pid" 2>/dev/null || true
}

cleanup() {
  set +e
  stop_pid "$nosh_pid"
  stop_pid "$sonora_pid"
  stop_pid "$awww_pid"
  stop_pid "$nested_pid"
  stop_pid "$bus_pid"
  # Hyprland forks a watchdog process. Match the unique config path so both
  # processes go away.
  pkill -9 -f "Hyprland --config $tmp/hyprland.lua" 2>/dev/null
  if [[ -n $nested_display && $nested_display != "$host_display" ]]; then
    rm -f "$XDG_RUNTIME_DIR/$nested_display" "$XDG_RUNTIME_DIR/$nested_display.lock"
  fi
  if [[ -n $lock_marker_backup ]]; then
    mkdir -p "$host_runtime/nosh"
    cp "$lock_marker_backup" "$lock_marker"
  else
    rm -f "$lock_marker"
  fi
  if [[ -n $systemd_wayland ]] && command -v systemctl >/dev/null 2>&1; then
    current_wayland=$(systemctl --user show-environment 2>/dev/null |
      sed -n 's/^WAYLAND_DISPLAY=//p' | head -1)
    current_signature=$(systemctl --user show-environment 2>/dev/null |
      sed -n 's/^HYPRLAND_INSTANCE_SIGNATURE=//p' | head -1)
    if [[ $current_wayland == "$nested_display" ||
      $current_signature == "$nested_signature" ]]; then
      systemctl --user set-environment \
        WAYLAND_DISPLAY="$systemd_wayland" \
        HYPRLAND_INSTANCE_SIGNATURE="$systemd_signature" \
        >/dev/null 2>&1 || true
    fi
  fi
  if [[ $keep == 1 ]]; then
    echo "kept $tmp"
  else
    rm -rf "$tmp"
  fi
}
trap cleanup EXIT INT TERM

# A nested compositor killed with SIGKILL can leave its socket behind. Remove
# stale sockets belonging to nobody, so the next launch gets a clean name.
for lock in "$XDG_RUNTIME_DIR"/wayland-*.lock; do
  [[ -e $lock ]] || continue
  [[ ${lock%.lock} == "$XDG_RUNTIME_DIR/$host_display" ]] && continue
  if flock -n "$lock" true 2>/dev/null; then
    rm -f "$lock" "${lock%.lock}"
  fi
done

# A private session bus keeps the nested notification daemon, tray, and media
# players separate from the ones on the running desktop.
bus_output=$(dbus-daemon --session --fork --print-address=1 --print-pid=1)
bus_address=$(printf '%s\n' "$bus_output" | head -1)
bus_pid=$(printf '%s\n' "$bus_output" | tail -1)

config_home="$tmp/config"
data_home="$tmp/data"
state_home="$tmp/state"
cache_home="$tmp/cache"
wallpaper_dir="$tmp/wallpapers"
mkdir -p "$config_home/nosh/menus.d" "$config_home/nosh/apps.d" \
  "$data_home" "$state_home" "$cache_home" "$wallpaper_dir"

download() {
  local url=$1 destination=$2
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 2 --max-time 60 "$url" -o "$destination" && return 0
  fi
  return 1
}

# The workspace background comes from the Linux_Dynamic_Wallpapers collection.
# The gradient is only a fallback for a machine without network access.
if ! download "$light_wallpaper" "$tmp/background-light.jpg"; then
  ffmpeg -loglevel error -f lavfi \
    -i "gradients=s=${width}x${height}:c0=0xe9e5dd:c1=0xb7c2cc:seed=8" \
    -frames:v 1 -y "$tmp/background-light.jpg"
fi
if ! download "$dark_wallpaper" "$tmp/background-dark.jpg"; then
  ffmpeg -loglevel error -f lavfi \
    -i "gradients=s=${width}x${height}:c0=0x101820:c1=0x3a2340:seed=4" \
    -frames:v 1 -y "$tmp/background-dark.jpg"
fi

# Wallpaper picker cards. Use the wallpaper directory from the running system
# plus the two photographs used for the workspace shots. Gradients are only a
# fallback when the directory is missing.
cp "$tmp/background-light.jpg" "$wallpaper_dir/rocky-mountain-light.jpg"
cp "$tmp/background-dark.jpg" "$wallpaper_dir/rocky-mountain-dark.jpg"
if [[ -d $system_wallpapers ]]; then
  find "$system_wallpapers" -maxdepth 1 -type f \
    \( -iname '*.jpg' -o -iname '*.jpeg' -o -iname '*.png' -o -iname '*.webp' \) \
    -exec cp -n {} "$wallpaper_dir/" \; 2>/dev/null || true
else
  palette=(
    "0x1f2a44 0x8a5b6b"
    "0x173a34 0x6f8f72"
    "0x3b2440 0xc07a63"
    "0x1b2b38 0x5f7f8c"
    "0x35261f 0xb08b63"
    "0x2a2036 0x7a6b9c"
  )
  for index in "${!palette[@]}"; do
    read -r from to <<<"${palette[$index]}"
    number=$((index + 1))
    ffmpeg -loglevel error -f lavfi \
      -i "gradients=s=600x400:c0=$from:c1=$to:seed=$number" \
      -frames:v 1 -y "$wallpaper_dir/wallpaper-$number.png"
  done
fi

sed "s|@WALLPAPER_DIR@|$wallpaper_dir|g" \
  "$fixtures/menus.d/wallpapers.toml" >"$config_home/nosh/menus.d/wallpapers.toml"
cp "$fixtures/apps.d/demo.toml" "$config_home/nosh/apps.d/demo.toml"
cp "$fixtures/hyprland.lua" "$tmp/hyprland.lua"

before_signatures=$(ls "$XDG_RUNTIME_DIR/hypr" 2>/dev/null | sort || true)
before_displays=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' \
  -printf '%f\n' 2>/dev/null | sort || true)

DBUS_SESSION_BUS_ADDRESS="$bus_address" \
  WAYLAND_DISPLAY="$host_display" \
  env -u UWSM_FINALIZE_VARNAMES -u UWSM_WAIT_VARNAMES \
  Hyprland --config "$tmp/hyprland.lua" >"$tmp/hyprland.log" 2>&1 &
nested_pid=$!

wait_for_nested() {
  local attempt
  for attempt in $(seq 1 100); do
    nested_signature=$(ls "$XDG_RUNTIME_DIR/hypr" 2>/dev/null | sort | comm -13 \
      <(printf '%s\n' "$before_signatures") - | head -1 || true)
    nested_display=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' \
      ! -name '*.lock' -printf '%f\n' 2>/dev/null | sort | comm -13 \
      <(printf '%s\n' "$before_displays") - | head -1 || true)
    if [[ -n $nested_signature && -n $nested_display ]]; then
      if HYPRLAND_INSTANCE_SIGNATURE="$nested_signature" \
        WAYLAND_DISPLAY="$nested_display" \
        hyprctl monitors -j 2>/dev/null | jq -e '.[0].width > 0' >/dev/null 2>&1; then
        return 0
      fi
    fi
    sleep 0.1
  done
  return 1
}

if ! wait_for_nested; then
  echo "the nested Hyprland session did not start; see $tmp/hyprland.log" >&2
  keep=1
  exit 1
fi

nested() {
  HYPRLAND_INSTANCE_SIGNATURE="$nested_signature" WAYLAND_DISPLAY="$nested_display" "$@"
}

nested_dispatch() {
  local lua=$1 legacy=$2
  if ! nested hyprctl dispatch "$lua" 2>&1 | grep -q '^ok$'; then
    nested hyprctl dispatch "$legacy" >/dev/null 2>&1 || true
  fi
}

# Resize the compositor window in the host session. Hyprland's Aquamarine
# backend sizes its only output to the client area of that window.
resize_host_window() {
  [[ -n $host_signature ]] || return 1

  local address attempt
  for attempt in $(seq 1 50); do
    address=$(HYPRLAND_INSTANCE_SIGNATURE="$host_signature" hyprctl clients -j |
      jq -r '.[] | select((.class // "") | test("aquamarine")) | .address' | head -1)
    [[ -n $address && $address != "null" ]] && break
    sleep 0.1
  done
  [[ -n $address && $address != "null" ]] || return 1

  HYPRLAND_INSTANCE_SIGNATURE="$host_signature" hyprctl dispatch \
    "hl.dsp.window.float({ window = \"address:$address\", action = \"enable\" })" \
    >/dev/null 2>&1 || true
  sleep 0.2
  HYPRLAND_INSTANCE_SIGNATURE="$host_signature" hyprctl dispatch \
    "hl.dsp.window.resize({ window = \"address:$address\", x = $width, y = $height, relative = false })" \
    >/dev/null 2>&1 || {
    # Hyprland before the Lua dispatchers used this form.
    HYPRLAND_INSTANCE_SIGNATURE="$host_signature" hyprctl dispatch \
      "resizewindowpixel exact $width $height,address:$address" >/dev/null 2>&1 || true
  }
}

resize_host_window

size=""
for attempt in $(seq 1 50); do
  size=$(nested hyprctl monitors -j | jq -r '.[0] | "\(.width)x\(.height)"')
  [[ $size == "${width}x${height}" ]] && break
  sleep 0.2
done
if [[ $size != "${width}x${height}" ]]; then
  echo "the nested output is $size, not ${width}x${height}" >&2
  echo "resize the nested Hyprland window to ${width}x${height} and run again" >&2
  keep=1
  exit 1
fi

# Every program started below talks to the nested session instead of the host.
export HYPRLAND_INSTANCE_SIGNATURE=$nested_signature
export WAYLAND_DISPLAY=$nested_display
export DBUS_SESSION_BUS_ADDRESS=$bus_address
export XDG_CONFIG_HOME=$config_home
export XDG_DATA_HOME=$data_home
export XDG_STATE_HOME=$state_home
export XDG_CACHE_HOME=$cache_home
unset XDG_SESSION_TYPE

populate_workspaces() {
  local workspace command
  while read -r workspace command; do
    nested_dispatch "hl.dsp.focus({ workspace = \"$workspace\" })" \
      "workspace $workspace"
    sleep 0.2
    nested_dispatch "hl.dsp.exec_cmd(\"$command\")" "exec $command"
    sleep 0.9
  done <<'EOF'
1 kitty
2 nautilus
3 kitty -e btop
5 kitty -e htop
8 kitty -e sh -c 'fastfetch; sleep 600'
EOF
  nested_dispatch 'hl.dsp.focus({ workspace = "4" })' 'workspace 4'
  sleep 3
}

populate_workspaces

awww-daemon >"$tmp/awww.log" 2>&1 &
awww_pid=$!
for attempt in $(seq 1 50); do
  if [[ -S "$XDG_RUNTIME_DIR/$nested_display-awww-daemon.sock" ]]; then
    break
  fi
  sleep 0.1
done

set_background() {
  awww img --transition-type none "$1" >/dev/null 2>&1 ||
    awww img "$1" >/dev/null 2>&1 || true
  sleep 0.5
}

# `ht` owns the same themes the real session uses. Read the theme's GTK
# stylesheet instead of applying it, so the host session keeps its own theme.
theme_dir() {
  local name=$1 thumbnail
  command -v ht >/dev/null 2>&1 || return 1
  thumbnail=$(ht list --json 2>/dev/null |
    jq -r --arg name "$name" 'select(.theme == $name or .id == $name) | .thumbnail' |
    head -1)
  [[ -n $thumbnail && $thumbnail != "null" ]] || return 1
  dirname "$thumbnail"
}

notifications() {
  notify-send -a "nosh" -i "dialog-information" "Build finished" \
    "nosh-core and nosh-ui compiled in 41 seconds."
  notify-send -a "nosh" -i "dialog-warning" "Battery low" \
    "12 percent left. Plug in a charger."
}

stop_shell() {
  if [[ -n $nosh_pid ]]; then
    stop_pid "$nosh_pid"
    wait "$nosh_pid" 2>/dev/null || true
    nosh_pid=""
  fi
  if [[ -n $sonora_pid ]]; then
    stop_pid "$sonora_pid"
    wait "$sonora_pid" 2>/dev/null || true
    sonora_pid=""
  fi
}

start_shell() {
  local name=$1
  stop_shell

  # Each scene starts with empty notification history, no pending lock, and the
  # pointer away from the bar so a new bar does not open a hover panel.
  rm -rf "$state_home/nosh" "$cache_home/nosh"
  rm -f "$XDG_RUNTIME_DIR/nosh/session-lock"
  nested hyprctl dispatch 'hl.dsp.cursor.move({ x = 1918, y = 1078 })' \
    >/dev/null 2>&1 || true

  local dark=1
  case "$name" in
    *latte*|*light*) dark=0 ;;
  esac

  mkdir -p "$config_home/gtk-4.0"
  rm -f "$config_home/gtk-4.0/gtk.css"
  local directory
  if directory=$(theme_dir "$name"); then
    cp "$directory/gtk.css" "$config_home/gtk-4.0/gtk.css"
  fi
  printf '[Settings]\ngtk-application-prefer-dark-theme=%s\n' "$dark" \
    >"$config_home/gtk-4.0/settings.ini"

  local gtk_theme=Adwaita
  [[ $dark == 1 ]] && gtk_theme=Adwaita:dark

  GTK_THEME="$gtk_theme" "$bin" >"$tmp/nosh.log" 2>&1 &
  nosh_pid=$!

  local attempt namespaces
  for attempt in $(seq 1 80); do
    namespaces=$(nested hyprctl layers -j 2>/dev/null |
      jq -r '.. | .namespace? // empty' 2>/dev/null || true)
    if printf '%s\n' "$namespaces" | grep -qx 'nosh-topbar'; then
      sleep 0.6
      return 0
    fi
    sleep 0.1
  done
  echo "the shell did not map its bar; see $tmp/nosh.log" >&2
  keep=1
  exit 1
}

capture() {
  local name=$1
  local park=${2:-1}
  local png="$tmp/$name.png"
  local target="$output_dir/$name.webp"
  # Drop compositor notifications such as the start-hyprland warning. Park the
  # pointer in the corner unless the scene needs it to stay over a widget.
  nested hyprctl dismissnotify 999 >/dev/null 2>&1 || true
  if [[ $park == 1 ]]; then
    nested hyprctl dispatch 'hl.dsp.cursor.move({ x = 1918, y = 1078 })' \
      >/dev/null 2>&1 || true
    sleep 0.2
  fi
  grim "$png"
  cwebp -quiet -q 82 -m 6 "$png" -o "$target"
  rm -f "$png"
  echo "$target"
}

scene_workspace_light() {
  set_background "$tmp/background-light.jpg"
  start_shell "$light_theme"
  "$bin" quick-settings
  sleep 1
  capture workspace-light
}

scene_workspace_dark() {
  set_background "$tmp/background-dark.jpg"
  start_shell "$dark_theme"
  "$bin" quick-settings
  sleep 1
  capture workspace-dark
}

scene_centre_panel() {
  set_background "$tmp/background-dark.jpg"
  start_shell "$dark_theme"
  notifications
  nested_dispatch 'hl.dsp.cursor.move({ x = 960, y = 14 })' \
    'movecursor 960 14'
  sleep 1.2
  capture centre-panel 0
}

# Sonora runs on the private bus so nosh sees it over MPRIS. Its config, data,
# and cache are copied so the real instance keeps its database and tokens.
launch_sonora() {
  local base="$tmp/sonora"
  local runtime="$tmp/sonora-runtime"
  mkdir -p "$base/config" "$base/data" "$base/cache" "$runtime"
  chmod 700 "$runtime"
  ln -sf "$host_runtime/$nested_display" "$runtime/wayland-sonora"
  cp -a "$HOME/.config/sonora" "$base/config/" 2>/dev/null || true
  cp -a "$HOME/.local/share/sonora" "$base/data/" 2>/dev/null || true
  cp -a "$HOME/.cache/sonora" "$base/cache/" 2>/dev/null || true

  # Sonora keys its single-instance socket off XDG_RUNTIME_DIR. Give it a
  # private one and point PipeWire and PulseAudio back at the host.
  XDG_CONFIG_HOME="$base/config" XDG_DATA_HOME="$base/data" \
    XDG_CACHE_HOME="$base/cache" \
    XDG_RUNTIME_DIR="$runtime" \
    WAYLAND_DISPLAY=wayland-sonora \
    PIPEWIRE_RUNTIME_DIR="$host_runtime" \
    PULSE_SERVER="unix:$host_runtime/pulse/native" \
    sonora >"$tmp/sonora.log" 2>&1 &
  sonora_pid=$!

  local attempt
  for attempt in $(seq 1 60); do
    if [[ -n $(playerctl status 2>/dev/null) ]]; then
      break
    fi
    sleep 0.2
  done
  playerctl play >/dev/null 2>&1 || true
  sleep 1

  # Give the operator time to click Sonora's expand button by hand before the
  # panel is opened.
  local pause=${NOSH_SCREENSHOT_MUSIC_PAUSE:-4}
  echo "music scene: click Sonora's expand button, capturing in ${pause}s" >&2
  sleep "$pause"

  nested_dispatch \
    'hl.dsp.window.float({ window = "class:sonora", action = "enable" })' \
    'togglefloating class:sonora'
  nested_dispatch \
    'hl.dsp.window.resize({ window = "class:sonora", x = 480, y = 715, relative = false })' \
    'resizewindowpixel exact 480 715,class:sonora'
  nested_dispatch \
    'hl.dsp.window.move({ window = "class:sonora", x = 120, y = 160, relative = false })' \
    'movewindowpixel exact 120 160,class:sonora'
  sleep 0.5
}

scene_music() {
  set_background "$tmp/background-dark.jpg"
  start_shell "$dark_theme"
  launch_sonora
  local hover_x=${NOSH_SCREENSHOT_MUSIC_X:-1015}
  nested_dispatch "hl.dsp.cursor.move({ x = $hover_x, y = 14 })" \
    "movecursor $hover_x 14"
  sleep 1.5
  capture music 0
}

scene_wallpapers() {
  set_background "$tmp/background-dark.jpg"
  start_shell "$dark_theme"
  "$bin" menu wallpapers
  sleep 3
  capture wallpapers
}

scene_lock_screen() {
  set_background "$tmp/background-dark.jpg"
  start_shell "$dark_theme"
  "$bin" lock
  sleep 1
  capture lock-screen
}

for scene in "${scenes[@]}"; do
  "scene_${scene//-/_}"
done

stop_shell

# Splice the two workspace shots along the bottom-left to top-right diagonal.
# The top-left half is the light session and the bottom-right half is dark.
if [[ -f $output_dir/workspace-light.webp && -f $output_dir/workspace-dark.webp ]]; then
  ffmpeg -loglevel error \
    -i "$output_dir/workspace-light.webp" \
    -i "$output_dir/workspace-dark.webp" \
    -filter_complex \
    "[0:v]format=gbrp[light];[1:v]format=gbrp[dark];[light][dark]blend=all_expr='if(lt(X/W+Y/H,1),A,B)'" \
    -frames:v 1 -y "$tmp/workspace-split.png"
  cwebp -quiet -q 88 -m 6 "$tmp/workspace-split.png" \
    -o "$output_dir/workspace-split.webp"
  echo "$output_dir/workspace-split.webp"
fi
