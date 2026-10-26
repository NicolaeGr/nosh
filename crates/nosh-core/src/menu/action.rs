//! Action argument expansion and process spawning.
//!
//! Actions are argv lists, never shell strings. When a shell is genuinely
//! needed, write the shell out explicitly: `["sh", "-c", "script \"$1\"", "sh",
//! "{item.path}"]`.

use std::io;
use std::process::{Command, Stdio};

use super::model::MenuItem;

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

    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
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

    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });

    Ok(())
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
}
