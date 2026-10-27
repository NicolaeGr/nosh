//! Automatic keybind registration.
//!
//! Menus can declare a bind in `menus.toml`, and the standard media keys are
//! always registered. This module writes those chords over the Hyprland
//! command socket, skipping chords that another program already owns unless
//! the bind is forced, and avoiding duplicates in the set.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::broadcast::error::RecvError;

use crate::hyprland::connection::Hyprland;
use crate::hyprland::dispatch::{escape_lua, ConfigProvider};
use crate::hyprland::events::HyprlandEvent;
use crate::hyprland::json::HyprBind;
use crate::menu::model::MenuDef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Shift,
    Caps,
    Ctrl,
    Alt,
    Mod2,
    Mod3,
    Super,
    Mod5,
}

impl Modifier {
    pub fn bit(self) -> u32 {
        match self {
            Self::Shift => 1,
            Self::Caps => 2,
            Self::Ctrl => 4,
            Self::Alt => 8,
            Self::Mod2 => 16,
            Self::Mod3 => 32,
            Self::Super => 64,
            Self::Mod5 => 128,
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_uppercase().as_str() {
            "SHIFT" => Some(Self::Shift),
            "CAPS" => Some(Self::Caps),
            "CTRL" | "CONTROL" => Some(Self::Ctrl),
            "ALT" => Some(Self::Alt),
            "MOD2" => Some(Self::Mod2),
            "MOD3" => Some(Self::Mod3),
            "SUPER" => Some(Self::Super),
            "MOD5" => Some(Self::Mod5),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shift => "SHIFT",
            Self::Caps => "CAPS",
            Self::Ctrl => "CTRL",
            Self::Alt => "ALT",
            Self::Mod2 => "MOD2",
            Self::Mod3 => "MOD3",
            Self::Super => "SUPER",
            Self::Mod5 => "MOD5",
        }
    }

    /// Canonical order used when rebuilding a chord from a `modmask`.
    pub const ALL: [Self; 8] = [
        Self::Shift,
        Self::Caps,
        Self::Ctrl,
        Self::Alt,
        Self::Mod2,
        Self::Mod3,
        Self::Super,
        Self::Mod5,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub modifiers: Vec<Modifier>,
    pub key: String,
}

impl Chord {
    /// Parses `"MODS, KEY"`. Modifiers are separated by whitespace or `+`.
    pub fn parse(text: &str) -> Result<Self, BindError> {
        let (modifiers, key) = text.split_once(',').ok_or_else(|| BindError::Chord {
            chord: text.to_string(),
            message: "expected `MODIFIERS, KEY`".to_string(),
        })?;

        let mut parsed = Vec::new();
        for token in modifiers
            .split(|character: char| character.is_whitespace() || character == '+')
            .filter(|token| !token.is_empty())
        {
            let modifier = Modifier::parse(token).ok_or_else(|| BindError::Chord {
                chord: text.to_string(),
                message: format!("unknown modifier {token:?}"),
            })?;
            if !parsed.contains(&modifier) {
                parsed.push(modifier);
            }
        }

        let key = key.trim();
        if key.is_empty() {
            return Err(BindError::Chord {
                chord: text.to_string(),
                message: "the key is empty".to_string(),
            });
        }

        Ok(Self {
            modifiers: parsed,
            key: normalize_key(key),
        })
    }

    /// Rebuilds a chord from an existing bind, in canonical modifier order.
    pub fn from_bind(bind: &HyprBind) -> Self {
        let modifiers = Modifier::ALL
            .into_iter()
            .filter(|modifier| bind.modmask & modifier.bit() != 0)
            .collect();
        Self {
            modifiers,
            key: bind.key.clone(),
        }
    }

    pub fn modmask(&self) -> u32 {
        self.modifiers
            .iter()
            .fold(0, |mask, modifier| mask | modifier.bit())
    }

    pub fn is_taken_by(&self, bind: &HyprBind) -> bool {
        bind.key.eq_ignore_ascii_case(&self.key)
            && bind.modmask == self.modmask()
            && bind.submap.is_empty()
    }

    /// Renders the chord for `keyword bind`, for example `"SUPER SHIFT, Space"`.
    pub fn render_hyprlang(&self) -> String {
        let modifiers = self
            .modifiers
            .iter()
            .map(|modifier| modifier.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        format!("{modifiers}, {}", self.key)
    }

    /// Renders the chord for `hl.bind`, for example `"SUPER + SHIFT + Space"`.
    pub fn render_lua(&self) -> String {
        let mut parts: Vec<&str> = self
            .modifiers
            .iter()
            .map(|modifier| modifier.as_str())
            .collect();
        parts.push(&self.key);
        parts.join(" + ")
    }

    fn same_chord(&self, other: &Self) -> bool {
        self.modmask() == other.modmask() && self.key.eq_ignore_ascii_case(&other.key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindSpec {
    pub chord: Chord,
    pub forced: bool,
    /// Repeat while the chord is held.
    pub repeating: bool,
    pub command: Vec<String>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindDecision {
    Install,
    Skip { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedBind {
    pub spec: BindSpec,
    pub decision: BindDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindPlan {
    pub entries: Vec<PlannedBind>,
    /// Managed binds that are still registered but no longer installed.
    pub stale: Vec<Chord>,
}

impl BindPlan {
    pub fn installed(&self) -> Vec<&BindSpec> {
        self.entries
            .iter()
            .filter(|entry| entry.decision == BindDecision::Install)
            .map(|entry| &entry.spec)
            .collect()
    }

    pub fn skipped(&self) -> Vec<(&BindSpec, &str)> {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.decision {
                BindDecision::Install => None,
                BindDecision::Skip { reason } => Some((&entry.spec, reason.as_str())),
            })
            .collect()
    }

    /// Requests that bring the runtime binds in line with the plan.
    pub fn requests(&self, provider: ConfigProvider) -> Vec<String> {
        let mut requests = Vec::new();

        for chord in &self.stale {
            requests.push(unbind_request(provider, chord));
        }

        for spec in self.installed() {
            requests.push(unbind_request(provider, &spec.chord));
            requests.push(bind_request(provider, spec));
        }

        requests
    }
}

#[derive(Debug, Error)]
pub enum BindError {
    #[error("invalid chord {chord:?}: {message}")]
    Chord { chord: String, message: String },
    #[error("Hyprland request failed: {0}")]
    Request(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindSyncOutcome {
    pub changed: bool,
    pub installed: Vec<String>,
    pub skipped: Vec<(String, String)>,
    pub missing: Vec<String>,
}

/// Binds the standard media keys to the matching shell command.
///
/// The keysym names are fixed by the XDG keyboard specification, so these
/// binds need no configuration. They are forced because the shell owns the
/// media keys, and an existing bind would otherwise hide the change indicator.
pub fn media_binds(executable: &str) -> Vec<BindSpec> {
    const BINDS: [(&str, &str, &str); 6] = [
        ("XF86AudioRaiseVolume", "volume-up", "volume up"),
        ("XF86AudioLowerVolume", "volume-down", "volume down"),
        ("XF86AudioMute", "volume-mute", "volume mute"),
        ("XF86AudioMicMute", "mic-mute", "microphone mute"),
        ("XF86MonBrightnessUp", "brightness-up", "brightness up"),
        (
            "XF86MonBrightnessDown",
            "brightness-down",
            "brightness down",
        ),
    ];

    BINDS
        .into_iter()
        .map(|(key, command, label)| BindSpec {
            chord: Chord {
                modifiers: Vec::new(),
                key: key.to_string(),
            },
            forced: true,
            repeating: true,
            command: vec![executable.to_string(), command.to_string()],
            label: label.to_string(),
        })
        .collect()
}

/// Builds the specs for every menu that declares a bind.
pub fn menu_binds(
    menus: &BTreeMap<String, MenuDef>,
    executable: &str,
) -> (Vec<BindSpec>, Vec<String>) {
    let mut specs = Vec::new();
    let mut errors = Vec::new();

    for (id, menu) in menus {
        let Some(bind) = &menu.bind else {
            continue;
        };

        match Chord::parse(&bind.chord) {
            Ok(chord) => specs.push(BindSpec {
                chord,
                forced: bind.forced,
                repeating: false,
                command: vec![executable.to_string(), "menu".to_string(), id.clone()],
                label: format!("menu {id}"),
            }),
            Err(error) => errors.push(format!("menu {id}: {error}")),
        }
    }

    (specs, errors)
}

/// Plans which specs may be installed against the current binds.
pub fn plan(specs: &[BindSpec], existing: &[HyprBind]) -> BindPlan {
    let foreign: Vec<&HyprBind> = existing
        .iter()
        .filter(|bind| !bind.mouse && !is_ours(bind))
        .collect();

    let mut seen: Vec<Chord> = Vec::new();
    let mut entries = Vec::new();

    for spec in specs {
        if seen.iter().any(|chord| chord.same_chord(&spec.chord)) {
            entries.push(PlannedBind {
                spec: spec.clone(),
                decision: BindDecision::Skip {
                    reason: "duplicate chord".to_string(),
                },
            });
            continue;
        }
        seen.push(spec.chord.clone());

        let conflict = foreign
            .iter()
            .find(|bind| spec.chord.is_taken_by(bind))
            .copied();

        match conflict {
            Some(bind) if !spec.forced => entries.push(PlannedBind {
                spec: spec.clone(),
                decision: BindDecision::Skip {
                    reason: format!(
                        "the chord is already bound to `{}{}`",
                        bind.dispatcher,
                        if bind.arg.is_empty() {
                            String::new()
                        } else {
                            format!(" {}", bind.arg)
                        }
                    ),
                },
            }),
            Some(bind) => {
                tracing::warn!(
                    chord = %spec.chord.render_hyprlang(),
                    dispatcher = %bind.dispatcher,
                    arg = %bind.arg,
                    label = %spec.label,
                    "forced keybind overrides an existing bind"
                );
                entries.push(PlannedBind {
                    spec: spec.clone(),
                    decision: BindDecision::Install,
                });
            }
            None => entries.push(PlannedBind {
                spec: spec.clone(),
                decision: BindDecision::Install,
            }),
        }
    }

    let installed_chords: Vec<Chord> = entries
        .iter()
        .filter(|entry| entry.decision == BindDecision::Install)
        .map(|entry| entry.spec.chord.clone())
        .collect();

    let mut stale: Vec<Chord> = Vec::new();
    for bind in existing.iter().filter(|bind| !bind.mouse && is_ours(bind)) {
        let chord = Chord::from_bind(bind);
        if installed_chords
            .iter()
            .any(|installed| installed.same_chord(&chord))
        {
            continue;
        }
        if stale.iter().any(|known| known.same_chord(&chord)) {
            continue;
        }
        if foreign.iter().any(|bind| chord.is_taken_by(bind)) {
            continue;
        }
        stale.push(chord);
    }

    BindPlan { entries, stale }
}

/// Reads the current binds, then unbinds and binds to match the plan.
pub async fn sync(hypr: &Hyprland, specs: &[BindSpec]) -> Result<BindSyncOutcome, BindError> {
    let existing = hypr
        .binds()
        .await
        .map_err(|error| BindError::Request(error.to_string()))?;
    let provider = hypr.provider().await;
    let plan = plan(specs, &existing);
    let requests = plan.requests(provider);

    let installed: Vec<String> = plan
        .installed()
        .iter()
        .map(|spec| spec.label.clone())
        .collect();
    let skipped: Vec<(String, String)> = plan
        .skipped()
        .iter()
        .map(|(spec, reason)| (spec.label.clone(), (*reason).to_string()))
        .collect();

    if requests.is_empty() {
        return Ok(BindSyncOutcome {
            changed: false,
            installed,
            skipped,
            missing: Vec::new(),
        });
    }

    hypr.requests(&requests)
        .await
        .map_err(|error| BindError::Request(error.to_string()))?;

    let after = hypr
        .binds()
        .await
        .map_err(|error| BindError::Request(error.to_string()))?;
    let missing = plan
        .installed()
        .iter()
        .filter(|spec| !after.iter().any(|bind| spec.chord.is_taken_by(bind)))
        .map(|spec| spec.label.clone())
        .collect();

    Ok(BindSyncOutcome {
        changed: true,
        installed,
        skipped,
        missing,
    })
}

/// Runs one sync at startup, then again after a reconnect or a config reload.
pub async fn run(hypr: Arc<Hyprland>, menus: BTreeMap<String, MenuDef>) {
    let executable = executable();
    let (mut specs, errors) = menu_binds(&menus, &executable);
    specs.extend(media_binds(&executable));
    for error in errors {
        tracing::warn!(%error, "a menu bind was ignored");
    }

    match sync(&hypr, &specs).await {
        Ok(outcome) => report(&outcome),
        Err(error) => tracing::warn!(%error, "the keybind sync failed"),
    }

    let mut events = hypr.events();
    loop {
        match events.recv().await {
            Ok(HyprlandEvent::ConfigReloaded | HyprlandEvent::Connected) => {
                match sync(&hypr, &specs).await {
                    Ok(outcome) => report(&outcome),
                    Err(error) => tracing::warn!(%error, "the keybind sync failed"),
                }
            }
            Ok(_) => {}
            Err(RecvError::Lagged(skipped)) => {
                tracing::debug!(skipped, "the bind sync event receiver lagged");
            }
            Err(RecvError::Closed) => return,
        }
    }
}

fn report(outcome: &BindSyncOutcome) {
    tracing::info!(
        installed = outcome.installed.len(),
        changed = outcome.changed,
        "keybinds are in sync"
    );

    for (label, reason) in &outcome.skipped {
        tracing::warn!(label, %reason, "a keybind was skipped");
    }

    for label in &outcome.missing {
        tracing::warn!(
            label,
            "a keybind is missing: the compositor did not report the bind after registration"
        );
    }
}

fn is_ours(bind: &HyprBind) -> bool {
    if bind.has_description {
        return bind.description.starts_with("nosh ");
    }
    if bind.dispatcher != "exec" {
        return false;
    }

    let program = bind.arg.split_whitespace().next().unwrap_or("");
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "nosh")
}

/// The binary that registered the binds, so a keybind does not depend on
/// `nosh` being on the compositor's `PATH`.
fn executable() -> String {
    let Some(argument) = std::env::args_os().next().map(PathBuf::from) else {
        return "nosh".to_string();
    };

    let path = if argument.is_absolute() {
        Some(argument)
    } else if argument.components().count() > 1 {
        std::path::absolute(&argument).ok()
    } else {
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .map(|directory| directory.join(&argument))
            .find(|candidate| is_executable(candidate))
    };

    path.map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "nosh".to_string())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn unbind_request(provider: ConfigProvider, chord: &Chord) -> String {
    match provider {
        ConfigProvider::Hyprlang => {
            format!("keyword unbind \"{}\"", chord.render_hyprlang())
        }
        ConfigProvider::Lua => {
            format!("eval hl.unbind(\"{}\")", chord.render_lua())
        }
    }
}

fn bind_request(provider: ConfigProvider, spec: &BindSpec) -> String {
    let command = spec.command.join(" ");
    match provider {
        ConfigProvider::Hyprlang => format!(
            "keyword {} \"{}, exec, {}\"",
            if spec.repeating { "bindr" } else { "bind" },
            spec.chord.render_hyprlang(),
            command
        ),
        ConfigProvider::Lua => {
            let mut options = vec![format!(
                "description = \"{}\"",
                escape_lua(&format!("nosh {}", spec.label))
            )];
            if spec.repeating {
                options.push("repeating = true".to_string());
            }
            format!(
                "eval hl.bind(\"{}\", hl.dsp.exec_cmd(\"{}\"), {{ {} }})",
                spec.chord.render_lua(),
                escape_lua(&command),
                options.join(", ")
            )
        }
    }
}

fn normalize_key(key: &str) -> String {
    let lower = key.to_ascii_lowercase();
    if lower.starts_with("code:") || lower.starts_with("xf86") {
        key.to_string()
    } else {
        key.to_ascii_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bind(key: &str, modmask: u32, dispatcher: &str, arg: &str) -> HyprBind {
        HyprBind {
            key: key.to_string(),
            modmask,
            dispatcher: dispatcher.to_string(),
            arg: arg.to_string(),
            ..HyprBind::default()
        }
    }

    fn spec(chord: &str, forced: bool, label: &str) -> BindSpec {
        BindSpec {
            chord: Chord::parse(chord).unwrap(),
            forced,
            repeating: false,
            command: vec!["nosh".to_string(), "menu".to_string(), label.to_string()],
            label: label.to_string(),
        }
    }

    #[test]
    fn modifiers_have_the_upstream_bits() {
        assert_eq!(Modifier::Shift.bit(), 1);
        assert_eq!(Modifier::Caps.bit(), 2);
        assert_eq!(Modifier::Ctrl.bit(), 4);
        assert_eq!(Modifier::Alt.bit(), 8);
        assert_eq!(Modifier::Mod2.bit(), 16);
        assert_eq!(Modifier::Mod3.bit(), 32);
        assert_eq!(Modifier::Super.bit(), 64);
        assert_eq!(Modifier::Mod5.bit(), 128);
    }

    #[test]
    fn chord_parsing_accepts_plus_and_whitespace() {
        let plus = Chord::parse("SUPER + SHIFT, Space").unwrap();
        let spaces = Chord::parse("super shift, Space").unwrap();

        assert_eq!(plus, spaces);
        assert_eq!(plus.modmask(), 65);
        assert_eq!(plus.key, "SPACE");
        assert_eq!(plus.render_hyprlang(), "SUPER SHIFT, SPACE");
        assert_eq!(plus.render_lua(), "SUPER + SHIFT + SPACE");
    }

    #[test]
    fn chord_parsing_accepts_the_ctrl_alias_and_an_empty_modifier_list() {
        assert_eq!(
            Chord::parse("CONTROL, q").unwrap().modifiers,
            vec![Modifier::Ctrl]
        );

        let bare = Chord::parse(", Space").unwrap();
        assert!(bare.modifiers.is_empty());
        assert_eq!(bare.modmask(), 0);
        assert_eq!(bare.render_hyprlang(), ", SPACE");
        assert_eq!(bare.render_lua(), "SPACE");
    }

    #[test]
    fn chord_parsing_rejects_missing_commas_empty_keys_and_unknown_modifiers() {
        assert!(matches!(
            Chord::parse("SUPER Space"),
            Err(BindError::Chord { .. })
        ));
        assert!(matches!(
            Chord::parse("SUPER,   "),
            Err(BindError::Chord { .. })
        ));
        assert!(matches!(
            Chord::parse("HYPER, Space"),
            Err(BindError::Chord { .. })
        ));
    }

    #[test]
    fn a_chord_only_matches_an_empty_submap() {
        let chord = Chord::parse("SUPER, Space").unwrap();
        let mut submap = bind("SPACE", 64, "exec", "kitty");
        submap.submap = "resize".to_string();

        assert!(chord.is_taken_by(&bind("SPACE", 64, "exec", "kitty")));
        assert!(!chord.is_taken_by(&submap));
        assert!(!chord.is_taken_by(&bind("SPACE", 0, "exec", "kitty")));
    }

    #[test]
    fn a_chord_is_rebuilt_from_a_bind_in_canonical_modifier_order() {
        let chord = Chord::from_bind(&bind("SPACE", 64 | 1 | 4, "exec", "kitty"));

        assert_eq!(
            chord.modifiers,
            vec![Modifier::Shift, Modifier::Ctrl, Modifier::Super]
        );
        assert_eq!(chord.key, "SPACE");
        assert_eq!(chord.render_hyprlang(), "SHIFT CTRL SUPER, SPACE");
    }

    #[test]
    fn menu_binds_use_the_menu_id_and_report_invalid_chords() {
        let config = crate::menu::parse_menus(
            r#"
schema = 1

[menu.apps]
title = "Apps"

[menu.apps.source]
type = "builtin"
name = "apps"

[menu.apps.bind]
chord = "SUPER, Space"

[menu.broken]
title = "Broken"

[menu.broken.source]
type = "builtin"
name = "static"

[menu.broken.bind]
chord = "nope"
"#,
        )
        .unwrap();

        let (specs, errors) = menu_binds(&config.menus, "nosh");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].label, "menu apps");
        assert_eq!(specs[0].command, vec!["nosh", "menu", "apps"]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("menu broken:"));
    }

    #[test]
    fn plan_skips_mouse_binds_and_our_own_binds() {
        let specs = vec![spec("SUPER, Space", false, "apps")];
        let existing = vec![
            HyprBind {
                mouse: true,
                key: "SPACE".to_string(),
                modmask: 64,
                ..HyprBind::default()
            },
            bind("SPACE", 64, "exec", "nosh menu other"),
        ];

        let plan = plan(&specs, &existing);

        assert_eq!(plan.installed().len(), 1);
        assert!(plan.skipped().is_empty());
    }

    #[test]
    fn a_managed_nosh_command_is_cleaned_up() {
        let existing = vec![bind("V", 64, "exec", "nosh volume-up")];

        let plan = plan(&[spec("SUPER, Space", false, "apps")], &existing);

        assert_eq!(plan.installed().len(), 1);
        assert_eq!(
            plan.requests(ConfigProvider::Hyprlang),
            vec![
                "keyword unbind \"SUPER, V\"",
                "keyword unbind \"SUPER, SPACE\"",
                "keyword bind \"SUPER, SPACE, exec, nosh menu apps\"",
            ]
        );
    }

    #[test]
    fn a_lua_description_marks_a_bind_as_ours() {
        let mut managed = bind("SPACE", 64, "__lua", "7");
        managed.has_description = true;
        managed.description = "nosh menu apps".to_string();
        let mut foreign = bind("RETURN", 64, "__lua", "8");
        foreign.has_description = true;
        foreign.description = "other".to_string();

        let plan = plan(&[], &[managed, foreign]);

        assert_eq!(plan.stale.len(), 1);
        assert_eq!(
            plan.requests(ConfigProvider::Lua),
            vec!["eval hl.unbind(\"SUPER + SPACE\")"]
        );
    }

    #[test]
    fn media_binds_cover_the_standard_keys() {
        let specs = media_binds("nosh");
        let keys: Vec<&str> = specs.iter().map(|spec| spec.chord.key.as_str()).collect();

        assert_eq!(
            keys,
            [
                "XF86AudioRaiseVolume",
                "XF86AudioLowerVolume",
                "XF86AudioMute",
                "XF86AudioMicMute",
                "XF86MonBrightnessUp",
                "XF86MonBrightnessDown",
            ]
        );
        assert!(specs.iter().all(|spec| spec.chord.modifiers.is_empty()));
        assert!(specs.iter().all(|spec| spec.forced));
        assert!(specs.iter().all(|spec| spec.repeating));
        assert_eq!(specs[0].command, vec!["nosh", "volume-up"]);
        assert_eq!(specs[0].chord.render_lua(), "XF86AudioRaiseVolume");
        assert_eq!(specs[0].chord.render_hyprlang(), ", XF86AudioRaiseVolume");
        assert_eq!(specs[3].command, vec!["nosh", "mic-mute"]);
    }

    #[test]
    fn plan_skips_a_conflicting_chord_but_a_forced_bind_installs() {
        let existing = vec![bind("SPACE", 64, "exec", "rofi -show drun")];

        let skipped = plan(&[spec("SUPER, Space", false, "apps")], &existing);
        assert_eq!(skipped.installed().len(), 0);
        assert_eq!(skipped.skipped().len(), 1);
        assert!(skipped.requests(ConfigProvider::Hyprlang).is_empty());
        assert!(skipped.stale.is_empty());

        let forced = plan(&[spec("SUPER, Space", true, "apps")], &existing);
        assert_eq!(forced.installed().len(), 1);
        assert!(forced.skipped().is_empty());
        assert_eq!(
            forced.requests(ConfigProvider::Hyprlang),
            vec![
                "keyword unbind \"SUPER, SPACE\"",
                "keyword bind \"SUPER, SPACE, exec, nosh menu apps\"",
            ]
        );
    }

    #[test]
    fn the_first_duplicate_chord_wins() {
        let specs = vec![
            spec("SUPER, Space", false, "apps"),
            spec("super, SPACE", false, "again"),
        ];

        let plan = plan(&specs, &[]);

        assert_eq!(plan.installed().len(), 1);
        assert_eq!(plan.installed()[0].label, "apps");
        let skipped = plan.skipped();
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].0.label, "again");
        assert_eq!(skipped[0].1, "duplicate chord");
    }

    #[test]
    fn hyprlang_requests_match_the_runtime_syntax() {
        let plan = plan(&[spec("SUPER, Space", false, "apps")], &[]);

        assert_eq!(
            plan.requests(ConfigProvider::Hyprlang),
            vec![
                "keyword unbind \"SUPER, SPACE\"",
                "keyword bind \"SUPER, SPACE, exec, nosh menu apps\"",
            ]
        );
    }

    #[test]
    fn lua_requests_match_the_runtime_syntax() {
        let plan = plan(&[spec("SUPER, Space", false, "apps")], &[]);

        assert_eq!(
            plan.requests(ConfigProvider::Lua),
            vec![
                "eval hl.unbind(\"SUPER + SPACE\")",
                "eval hl.bind(\"SUPER + SPACE\", hl.dsp.exec_cmd(\"nosh menu apps\"), { description = \"nosh apps\" })",
            ]
        );
    }

    #[test]
    fn repeating_requests_use_bindr_and_the_lua_option() {
        let entry = BindSpec {
            chord: Chord {
                modifiers: Vec::new(),
                key: "XF86AudioRaiseVolume".to_string(),
            },
            forced: true,
            repeating: true,
            command: vec!["/usr/bin/nosh".to_string(), "volume-up".to_string()],
            label: "volume up".to_string(),
        };
        let plan = plan(&[entry], &[]);

        assert_eq!(
            plan.requests(ConfigProvider::Hyprlang),
            vec![
                "keyword unbind \", XF86AudioRaiseVolume\"",
                "keyword bindr \", XF86AudioRaiseVolume, exec, /usr/bin/nosh volume-up\"",
            ]
        );
        assert_eq!(
            plan.requests(ConfigProvider::Lua),
            vec![
                "eval hl.unbind(\"XF86AudioRaiseVolume\")",
                "eval hl.bind(\"XF86AudioRaiseVolume\", hl.dsp.exec_cmd(\"/usr/bin/nosh volume-up\"), { description = \"nosh volume up\", repeating = true })",
            ]
        );
    }

    #[test]
    fn lua_strings_are_escaped() {
        let mut entry = spec("SUPER, Space", false, "apps");
        entry.command = vec!["notify-send".to_string(), "\"hi\" C:\\tmp".to_string()];
        let plan = plan(&[entry], &[]);

        assert_eq!(
            plan.requests(ConfigProvider::Lua),
            vec![
                "eval hl.unbind(\"SUPER + SPACE\")",
                "eval hl.bind(\"SUPER + SPACE\", hl.dsp.exec_cmd(\"notify-send \\\"hi\\\" C:\\\\tmp\"), { description = \"nosh apps\" })",
            ]
        );
    }

    #[test]
    fn stale_managed_binds_are_unbound_first() {
        let specs = vec![spec("SUPER, Space", false, "apps")];
        let existing = vec![
            bind("RETURN", 64, "exec", "nosh menu old"),
            bind("P", 64, "exec", "nosh volume-up"),
        ];

        let plan = plan(&specs, &existing);
        assert_eq!(plan.stale.len(), 2);

        assert_eq!(
            plan.requests(ConfigProvider::Hyprlang),
            vec![
                "keyword unbind \"SUPER, RETURN\"",
                "keyword unbind \"SUPER, P\"",
                "keyword unbind \"SUPER, SPACE\"",
                "keyword bind \"SUPER, SPACE, exec, nosh menu apps\"",
            ]
        );
        assert_eq!(
            plan.requests(ConfigProvider::Lua),
            vec![
                "eval hl.unbind(\"SUPER + RETURN\")",
                "eval hl.unbind(\"SUPER + P\")",
                "eval hl.unbind(\"SUPER + SPACE\")",
                "eval hl.bind(\"SUPER + SPACE\", hl.dsp.exec_cmd(\"nosh menu apps\"), { description = \"nosh apps\" })",
            ]
        );
    }

    #[test]
    fn a_foreign_bind_on_a_shared_chord_is_not_stale() {
        let existing = vec![
            bind("SPACE", 64, "exec", "nosh menu old"),
            bind("SPACE", 64, "exec", "rofi -show drun"),
        ];

        let plan = plan(&[], &existing);

        assert!(plan.stale.is_empty());
        assert!(plan.requests(ConfigProvider::Hyprlang).is_empty());
    }

    #[test]
    fn an_unchanged_managed_bind_is_not_stale() {
        let specs = vec![spec("SUPER, Space", false, "apps")];
        let existing = vec![bind("SPACE", 64, "exec", "nosh menu apps")];

        let plan = plan(&specs, &existing);

        assert!(plan.stale.is_empty());
        assert_eq!(
            plan.requests(ConfigProvider::Hyprlang),
            vec![
                "keyword unbind \"SUPER, SPACE\"",
                "keyword bind \"SUPER, SPACE, exec, nosh menu apps\"",
            ]
        );
    }
}
