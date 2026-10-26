use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::PathError;

/// Resolved config, data, cache, and state directories for nosh.
///
/// Paths are resolved once at startup and passed into services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoshPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub state_dir: PathBuf,
    pub runtime_dir: Option<PathBuf>,
}

impl NoshPaths {
    pub fn from_env() -> Result<Self, PathError> {
        Self::from_lookup(|key| env::var_os(key))
    }

    pub fn from_lookup<F>(mut lookup: F) -> Result<Self, PathError>
    where
        F: FnMut(&str) -> Option<OsString>,
    {
        let config_dir = xdg_dir(&mut lookup, "XDG_CONFIG_HOME", &[".config"])?.join("nosh");
        let data_dir = xdg_dir(&mut lookup, "XDG_DATA_HOME", &[".local", "share"])?.join("nosh");
        let cache_dir = xdg_dir(&mut lookup, "XDG_CACHE_HOME", &[".cache"])?.join("nosh");
        let state_dir = xdg_dir(&mut lookup, "XDG_STATE_HOME", &[".local", "state"])?.join("nosh");
        let runtime_dir = lookup("XDG_RUNTIME_DIR").map(|value| PathBuf::from(value).join("nosh"));

        Ok(Self {
            config_dir,
            data_dir,
            cache_dir,
            state_dir,
            runtime_dir,
        })
    }

    pub fn app_launcher_config(&self) -> PathBuf {
        self.config_dir.join("app_launcher.toml")
    }

    pub fn config(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn monitors_config(&self) -> PathBuf {
        self.config_dir.join("monitors.toml")
    }

    pub fn menus_config(&self) -> PathBuf {
        self.config_dir.join("menus.toml")
    }

    pub fn menus_dir(&self) -> PathBuf {
        self.config_dir.join("menus.d")
    }

    pub fn user_styles(&self) -> PathBuf {
        self.config_dir.join("style.css")
    }

    pub fn app_usage(&self) -> PathBuf {
        self.data_dir.join("app_usage.json")
    }

    pub fn covers_cache(&self) -> PathBuf {
        self.cache_dir.join("covers")
    }

    pub fn notifications_cache(&self) -> PathBuf {
        self.cache_dir.join("notifications")
    }

    pub fn brightness_snapshot(&self) -> PathBuf {
        self.cache_dir.join("brightness").join("snapshot.json")
    }

    pub fn notifications_store(&self) -> PathBuf {
        self.state_dir.join("notifications.json")
    }
}

fn home<F>(lookup: &mut F) -> Result<PathBuf, PathError>
where
    F: FnMut(&str) -> Option<OsString>,
{
    lookup("HOME").map(PathBuf::from).ok_or(PathError::NoHome)
}

fn xdg_dir<F>(lookup: &mut F, key: &str, fallback: &[&str]) -> Result<PathBuf, PathError>
where
    F: FnMut(&str) -> Option<OsString>,
{
    match lookup(key) {
        Some(value) => Ok(PathBuf::from(value)),
        None => {
            let mut path = home(lookup)?;
            for part in fallback {
                path.push(part);
            }
            Ok(path)
        }
    }
}

/// The two Hyprland sockets for the running instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyprlandPaths {
    pub command_socket: PathBuf,
    pub event_socket: PathBuf,
}

impl HyprlandPaths {
    /// Returns `None` when Hyprland is not the running compositor.
    pub fn from_env() -> Result<Option<Self>, PathError> {
        Self::from_lookup(|key| env::var_os(key))
    }

    pub fn from_lookup<F>(mut lookup: F) -> Result<Option<Self>, PathError>
    where
        F: FnMut(&str) -> Option<OsString>,
    {
        let Some(signature) = lookup("HYPRLAND_INSTANCE_SIGNATURE") else {
            return Ok(None);
        };
        let signature = signature
            .into_string()
            .map_err(|value| PathError::NonUtf8 {
                name: "HYPRLAND_INSTANCE_SIGNATURE",
                value,
            })?;
        let runtime = lookup("XDG_RUNTIME_DIR").ok_or(PathError::NoRuntimeDir)?;
        let directory = PathBuf::from(runtime).join("hypr").join(signature);

        Ok(Some(Self {
            command_socket: directory.join(".socket.sock"),
            event_socket: directory.join(".socket2.sock"),
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn lookup(values: &[(&str, &str)]) -> impl FnMut(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = values
            .iter()
            .map(|(key, value)| (key.to_string(), OsString::from(value)))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn xdg_variables_win_over_home() {
        let paths = NoshPaths::from_lookup(lookup(&[
            ("HOME", "/home/nicolae"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_DATA_HOME", "/data"),
            ("XDG_CACHE_HOME", "/cache"),
            ("XDG_STATE_HOME", "/state"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]))
        .unwrap();

        assert_eq!(paths.config_dir, PathBuf::from("/cfg/nosh"));
        assert_eq!(paths.data_dir, PathBuf::from("/data/nosh"));
        assert_eq!(paths.cache_dir, PathBuf::from("/cache/nosh"));
        assert_eq!(paths.state_dir, PathBuf::from("/state/nosh"));
        assert_eq!(
            paths.runtime_dir,
            Some(PathBuf::from("/run/user/1000/nosh"))
        );
    }

    #[test]
    fn home_is_the_fallback() {
        let paths = NoshPaths::from_lookup(lookup(&[("HOME", "/home/nicolae")])).unwrap();

        assert_eq!(
            paths.config_dir,
            PathBuf::from("/home/nicolae/.config/nosh")
        );
        assert_eq!(
            paths.data_dir,
            PathBuf::from("/home/nicolae/.local/share/nosh")
        );
        assert_eq!(paths.cache_dir, PathBuf::from("/home/nicolae/.cache/nosh"));
        assert_eq!(
            paths.state_dir,
            PathBuf::from("/home/nicolae/.local/state/nosh")
        );
        assert_eq!(paths.runtime_dir, None);
    }

    #[test]
    fn missing_home_is_an_error() {
        let error = NoshPaths::from_lookup(lookup(&[])).unwrap_err();
        assert!(matches!(error, PathError::NoHome));
    }

    #[test]
    fn derived_paths_match_the_spec() {
        let paths = NoshPaths::from_lookup(lookup(&[("HOME", "/home/nicolae")])).unwrap();

        assert_eq!(
            paths.app_launcher_config(),
            PathBuf::from("/home/nicolae/.config/nosh/app_launcher.toml")
        );
        assert_eq!(
            paths.app_usage(),
            PathBuf::from("/home/nicolae/.local/share/nosh/app_usage.json")
        );
        assert_eq!(
            paths.brightness_snapshot(),
            PathBuf::from("/home/nicolae/.cache/nosh/brightness/snapshot.json")
        );
        assert_eq!(
            paths.menus_config(),
            PathBuf::from("/home/nicolae/.config/nosh/menus.toml")
        );
        assert_eq!(
            paths.menus_dir(),
            PathBuf::from("/home/nicolae/.config/nosh/menus.d")
        );
        assert_eq!(
            paths.user_styles(),
            PathBuf::from("/home/nicolae/.config/nosh/style.css")
        );

        assert_eq!(
            paths.notifications_store(),
            PathBuf::from("/home/nicolae/.local/state/nosh/notifications.json")
        );
    }

    #[test]
    fn hyprland_paths_use_the_instance_signature() {
        let paths = HyprlandPaths::from_lookup(lookup(&[
            ("HYPRLAND_INSTANCE_SIGNATURE", "abc123"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]))
        .unwrap()
        .unwrap();

        assert_eq!(
            paths.command_socket,
            PathBuf::from("/run/user/1000/hypr/abc123/.socket.sock")
        );
        assert_eq!(
            paths.event_socket,
            PathBuf::from("/run/user/1000/hypr/abc123/.socket2.sock")
        );
    }

    #[test]
    fn missing_signature_means_no_hyprland() {
        let paths =
            HyprlandPaths::from_lookup(lookup(&[("XDG_RUNTIME_DIR", "/run/user/1000")])).unwrap();
        assert_eq!(paths, None);
    }

    #[test]
    fn signature_without_runtime_dir_is_an_error() {
        let error =
            HyprlandPaths::from_lookup(lookup(&[("HYPRLAND_INSTANCE_SIGNATURE", "abc123")]))
                .unwrap_err();
        assert!(matches!(error, PathError::NoRuntimeDir));
    }
}
