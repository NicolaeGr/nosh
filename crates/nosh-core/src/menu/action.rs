//! Action argument expansion and process launching.
//!
//! Actions are argv lists, never shell strings. When a shell is genuinely
//! needed, write the shell out explicitly: `["sh", "-c", "script \"$1\"", "sh",
//! "{item.path}"]`.
//!
//! `spawn_action` tries UWSM, then a systemd user scope, then a direct detached
//! spawn. A wrapper that cannot start falls through to the next step.

use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::model::{LaunchMode, MenuItem};

/// Replaces `{query}` and `{item.<field>}` tokens. Unknown tokens stay literal
/// so a typo is visible in the error from the spawned command instead of
/// silently becoming an empty argument.
pub fn expand_argv(argv: &[String], item: &MenuItem, query: &str) -> Vec<String> {
    argv.iter()
        .map(|argument| expand_argument(argument, item, query))
        .collect()
}

fn expand_argument(argument: &str, item: &MenuItem, query: &str) -> String {
    let mut output = String::with_capacity(argument.len());
    let mut rest = argument;

    while let Some(open) = rest.find('{') {
        output.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find('}') else {
            output.push_str(&rest[open..]);
            return output;
        };
        let token = &after_open[..close];
        push_token(&mut output, token, item, query);
        rest = &after_open[close + 1..];
    }

    output.push_str(rest);
    output
}

fn push_token(output: &mut String, token: &str, item: &MenuItem, query: &str) {
    if token == "query" {
        output.push_str(query);
        return;
    }

    if let Some(key) = token.strip_prefix("item.") {
        if let Some(value) = item.field(key) {
            output.push_str(&value);
            return;
        }
    }

    output.push('{');
    output.push_str(token);
    output.push('}');
}

/// A wrapper that starts the action inside the session manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Launcher {
    Uwsm,
    SystemdRun,
}

impl Launcher {
    /// The wrapper program and its leading arguments. The action argv follows.
    fn wrapper(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Uwsm => ("uwsm", &["app", "-t", "service", "--"]),
            Self::SystemdRun => ("systemd-run", &["--user", "--scope", "--quiet", "--"]),
        }
    }

    /// Whether the wrapper exits after it starts the app. `uwsm` reports a
    /// setup error by exiting. `systemd-run --scope` stays in the foreground for
    /// the app's lifetime, so the only error it reports is a failed spawn.
    fn exits_after_setup(self) -> bool {
        matches!(self, Self::Uwsm)
    }
}

/// Time allowed for a wrapper to report a setup error. A wrapper that is still
/// running after this has started the app.
const LAUNCH_PROBE: Duration = Duration::from_millis(100);

/// Starts a menu action through the session manager when one is available.
///
/// UWSM comes first because it imports the graphical session environment into
/// the systemd user manager. Then it tries a systemd user scope. Both keep the
/// app out of nosh's service cgroup, so restarting nosh does not restart the
/// app. A direct detached spawn is the last fallback.
///
/// A UWSM failure within the probe window falls through to the next launcher.
/// `systemd-run --scope` runs in the foreground, so only a spawn failure falls
/// through.
pub fn spawn_action(argv: &[String], mode: LaunchMode) -> io::Result<()> {
    if argv.is_empty() || argv[0].is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an action needs a program",
        ));
    }

    for launcher in session_launchers(mode) {
        match launch_with(launcher, argv) {
            Ok(()) => {
                tracing::info!(
                    program = %argv[0],
                    launcher = ?launcher,
                    "started an action through the session manager"
                );
                return Ok(());
            }
            Err(error) => tracing::warn!(
                program = %argv[0],
                launcher = ?launcher,
                %error,
                "cannot start an action through this launcher, falling back"
            ),
        }
    }

    spawn_detached(argv)
}

fn session_launchers(mode: LaunchMode) -> Vec<Launcher> {
    let mut launchers = Vec::with_capacity(2);
    match mode {
        LaunchMode::Auto => {
            if uwsm_session() {
                launchers.push(Launcher::Uwsm);
            }
            if systemd_user_manager() {
                launchers.push(Launcher::SystemdRun);
            }
        }
        LaunchMode::Uwsm => launchers.push(Launcher::Uwsm),
        LaunchMode::SystemdRun => launchers.push(Launcher::SystemdRun),
        LaunchMode::Detached => {}
    }
    launchers
}

/// UWSM exports these variables for the compositor plugins. Their presence means
/// the graphical session runs under UWSM.
fn uwsm_session() -> bool {
    ["UWSM_WAIT_VARNAMES", "UWSM_FINALIZE_VARNAMES"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// A reachable systemd user manager has a private socket in the runtime
/// directory. Without it, `systemd-run --user` fails immediately.
fn systemd_user_manager() -> bool {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|dir| Path::new(&dir).join("systemd/private").exists())
        .unwrap_or(false)
}

fn launch_with(launcher: Launcher, argv: &[String]) -> io::Result<()> {
    let (program, prefix) = launcher.wrapper();
    let mut args = Vec::with_capacity(prefix.len() + argv.len());
    args.extend(prefix.iter().map(|argument| argument.to_string()));
    args.extend(argv.iter().cloned());

    let mut child = spawn_process(program, &args)?;
    if !launcher.exits_after_setup() {
        reap(child);
        return Ok(());
    }

    let deadline = Instant::now() + LAUNCH_PROBE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(io::Error::other(format!("{program} exited with {status}")));
            }
            Ok(None) if Instant::now() >= deadline => {
                reap(child);
                return Ok(());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    }
}

/// Starts a program in a new session and reaps it on a background thread.
///
/// `setsid` detaches the child from the shell's process group, so closing the
/// menu does not send it a signal and the compositor does not tie it to the
/// menu window.
pub fn spawn_detached(argv: &[String]) -> io::Result<()> {
    if argv.is_empty() || argv[0].is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an action needs a program",
        ));
    }

    match spawn_process(&argv[0], &argv[1..]) {
        Ok(child) => {
            reap(child);
            Ok(())
        }
        Err(error) => {
            tracing::error!(program = %argv[0], ?argv, %error, "cannot spawn an action");
            Err(error)
        }
    }
}

fn spawn_process(program: &str, args: &[String]) -> io::Result<Child> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and does not touch shared state.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    command.spawn()
}

fn reap(mut child: Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::model::MenuValue;

    fn item() -> MenuItem {
        let mut item = MenuItem::new("id-1", "Cyberpunk City");
        item.description = Some("4K wall".to_string());
        item.fields.insert(
            "path".to_string(),
            MenuValue::String("/home/user/city.png".to_string()),
        );
        item.fields
            .insert("width".to_string(), MenuValue::Number(3840.0));
        item
    }

    #[test]
    fn expands_slots_and_fields() {
        let argv = vec![
            "swww".to_string(),
            "img".to_string(),
            "{item.path}".to_string(),
            "--name".to_string(),
            "{item.name}".to_string(),
            "{item.width}".to_string(),
            "{query}".to_string(),
        ];

        let expanded = expand_argv(&argv, &item(), "city wallpaper");

        assert_eq!(
            expanded,
            vec![
                "swww",
                "img",
                "/home/user/city.png",
                "--name",
                "Cyberpunk City",
                "3840",
                "city wallpaper"
            ]
        );
    }

    #[test]
    fn numbers_render_without_a_trailing_zero() {
        let expanded = expand_argv(&["{item.width}".to_string()], &item(), "");
        assert_eq!(expanded, vec!["3840"]);
    }

    #[test]
    fn tokens_inside_longer_arguments_expand() {
        let expanded = expand_argv(&["--file={item.path}".to_string()], &item(), "");
        assert_eq!(expanded, vec!["--file=/home/user/city.png"]);
    }

    #[test]
    fn unknown_tokens_stay_literal() {
        let expanded = expand_argv(
            &["{item.nope}".to_string(), "{nope}".to_string()],
            &item(),
            "",
        );
        assert_eq!(expanded, vec!["{item.nope}", "{nope}"]);
    }

    #[test]
    fn an_unclosed_brace_is_literal() {
        let expanded = expand_argv(&["{item.path".to_string()], &item(), "");
        assert_eq!(expanded, vec!["{item.path"]);
    }

    #[test]
    fn empty_argv_is_rejected() {
        let error = spawn_detached(&[]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn uwsm_starts_a_transient_service() {
        let (program, prefix) = Launcher::Uwsm.wrapper();
        assert_eq!(program, "uwsm");
        assert_eq!(prefix, ["app", "-t", "service", "--"]);
    }

    #[test]
    fn systemd_run_starts_a_user_scope() {
        let (program, prefix) = Launcher::SystemdRun.wrapper();
        assert_eq!(program, "systemd-run");
        assert_eq!(prefix, ["--user", "--scope", "--quiet", "--"]);
    }

    #[test]
    fn only_uwsm_reports_a_setup_error_through_an_exit() {
        assert!(Launcher::Uwsm.exits_after_setup());
        assert!(!Launcher::SystemdRun.exits_after_setup());
    }

    #[test]
    fn detached_mode_skips_the_session_manager() {
        assert!(session_launchers(LaunchMode::Detached).is_empty());
    }

    #[test]
    fn a_forced_launcher_has_no_other_session_launcher() {
        assert_eq!(session_launchers(LaunchMode::Uwsm), [Launcher::Uwsm]);
        assert_eq!(
            session_launchers(LaunchMode::SystemdRun),
            [Launcher::SystemdRun]
        );
    }
}
