//! The polkit authentication helper client.
//!
//! polkitd only accepts an authentication answer from a privileged process.
//! The helper does that job: it runs PAM with the `polkit-1` stack, calls
//! `AuthenticationAgentResponse2`, and speaks a small line protocol with the
//! agent.
//!
//! Newer polkit versions expose `/run/polkit/agent-helper.socket`, and polkitd
//! starts the helper as root. Older versions run a setuid
//! `polkit-agent-helper-1`. Both speak the same protocol. The socket path sends
//! the user name first, the binary path takes it as an argument.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

use crate::error::BackendError;

pub const HELPER_SOCKET: &str = "/run/polkit/agent-helper.socket";

const HELPER_BINARIES: [&str; 3] = [
    "/run/current-system/sw/lib/polkit-1/polkit-agent-helper-1",
    "/usr/lib/polkit-1/polkit-agent-helper-1",
    "/usr/libexec/polkit-agent-helper-1",
];

/// One line from the helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperMessage {
    /// PAM asks for a value. `echo` is false for a password.
    Prompt { text: String, echo: bool },
    /// Informational text that needs no answer.
    Info(String),
    /// An error to show, followed by another prompt or the end.
    Error(String),
    /// The helper authenticated the user and told polkitd.
    Success,
    /// PAM rejected the attempt.
    Failure,
}

pub struct HelperSession {
    reader: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
    _child: Option<Child>,
    line: String,
}

impl HelperSession {
    /// Starts a session for one polkit cookie.
    pub async fn start(username: &str, cookie: &str) -> Result<Self, BackendError> {
        let (reader, writer, child, over_socket) = connect(username).await?;
        let mut session = Self {
            reader: BufReader::new(reader),
            writer,
            _child: child,
            line: String::new(),
        };

        if over_socket {
            session.write_line(username).await?;
        }
        session.write_line(cookie).await?;
        Ok(session)
    }

    /// Reads the next meaningful line from the helper.
    ///
    /// Unknown lines are skipped. `None` means the helper closed the stream
    /// without reporting a result.
    pub async fn next(&mut self) -> Option<HelperMessage> {
        loop {
            self.line.clear();
            match self.reader.read_line(&mut self.line).await {
                Ok(0) | Err(_) => return None,
                Ok(_) => {}
            }

            let line = unescape(self.line.trim_end_matches(['\n', '\r']));
            if let Some(message) = parse_line(&line) {
                return Some(message);
            }
        }
    }

    pub async fn respond(&mut self, response: &[u8]) -> Result<(), BackendError> {
        self.writer.write_all(response).await.map_err(io_error)?;
        self.writer.write_all(b"\n").await.map_err(io_error)?;
        self.writer.flush().await.map_err(io_error)
    }

    async fn write_line(&mut self, value: &str) -> Result<(), BackendError> {
        self.writer
            .write_all(value.as_bytes())
            .await
            .map_err(io_error)?;
        self.writer.write_all(b"\n").await.map_err(io_error)?;
        self.writer.flush().await.map_err(io_error)
    }
}

type Transport = (
    Box<dyn AsyncRead + Unpin + Send>,
    Box<dyn AsyncWrite + Unpin + Send>,
    Option<Child>,
    bool,
);

async fn connect(username: &str) -> Result<Transport, BackendError> {
    if Path::new(HELPER_SOCKET).exists() {
        match UnixStream::connect(HELPER_SOCKET).await {
            Ok(stream) => {
                let (reader, writer) = stream.into_split();
                return Ok((Box::new(reader), Box::new(writer), None, true));
            }
            Err(error) => {
                tracing::warn!(%error, "the polkit helper socket did not accept a connection");
            }
        }
    }

    let Some(path) = helper_binary() else {
        return Err(BackendError::Unavailable(
            "polkit-agent-helper-1 was not found".to_string(),
        ));
    };

    let mut child = Command::new(&path)
        .arg(username)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(io_error)?;

    let stdin = child.stdin.take().ok_or_else(|| {
        BackendError::Unavailable("the polkit helper has no input pipe".to_string())
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        BackendError::Unavailable("the polkit helper has no output pipe".to_string())
    })?;

    Ok((Box::new(stdout), Box::new(stdin), Some(child), false))
}

fn helper_binary() -> Option<PathBuf> {
    HELPER_BINARIES
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

fn io_error(error: std::io::Error) -> BackendError {
    BackendError::Unavailable(error.to_string())
}

fn parse_line(line: &str) -> Option<HelperMessage> {
    if let Some(text) = line.strip_prefix("PAM_PROMPT_ECHO_OFF ") {
        return Some(HelperMessage::Prompt {
            text: text.to_string(),
            echo: false,
        });
    }
    if let Some(text) = line.strip_prefix("PAM_PROMPT_ECHO_ON ") {
        return Some(HelperMessage::Prompt {
            text: text.to_string(),
            echo: true,
        });
    }
    if let Some(text) = line.strip_prefix("PAM_TEXT_INFO ") {
        return Some(HelperMessage::Info(text.to_string()));
    }
    if let Some(text) = line.strip_prefix("PAM_ERROR_MSG ") {
        return Some(HelperMessage::Error(text.to_string()));
    }
    match line {
        "SUCCESS" => Some(HelperMessage::Success),
        "FAILURE" => Some(HelperMessage::Failure),
        _ => None,
    }
}

/// Reverses the C escaping the helper applies to PAM messages.
fn unescape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match chars.next() {
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some('t') => output.push('\t'),
            Some('"') => output.push('"'),
            Some('\\') => output.push('\\'),
            Some(digit @ '0'..='7') => {
                let mut octal = String::from(digit);
                for _ in 0..2 {
                    match chars.clone().next() {
                        Some(next @ '0'..='7') => {
                            octal.push(next);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                if let Ok(byte) = u8::from_str_radix(&octal, 8) {
                    output.push(byte as char);
                }
            }
            Some(other) => output.push(other),
            None => output.push('\\'),
        }
    }
    output
}

/// The name the helper passes to PAM, resolved the way polkit does it.
///
/// `getpwuid_r` reads the system user database, so identities from NSS (LDAP,
/// SSSD, systemd-homed) resolve the same way they do for polkit.
pub fn username_for(uid: u32) -> Option<String> {
    // The first call usually fits. NSS may ask for a bigger buffer.
    let mut buffer = vec![0u8; 1024];
    let mut passwd: libc::passwd = unsafe { std::mem::zeroed() };
    loop {
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                uid,
                &mut passwd,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE {
            if buffer.len() >= 1 << 20 {
                return None;
            }
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if status != 0 || result.is_null() || passwd.pw_name.is_null() {
            return None;
        }
        let name = unsafe { std::ffi::CStr::from_ptr(passwd.pw_name) };
        return name.to_str().ok().map(str::to_string);
    }
}

pub fn current_uid() -> u32 {
    // No safe wrapper exists in std for the real user id.
    unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_prompts_are_parsed() {
        assert_eq!(
            parse_line("PAM_PROMPT_ECHO_OFF Password: "),
            Some(HelperMessage::Prompt {
                text: "Password: ".to_string(),
                echo: false,
            })
        );
        assert_eq!(
            parse_line("PAM_PROMPT_ECHO_ON Login: "),
            Some(HelperMessage::Prompt {
                text: "Login: ".to_string(),
                echo: true,
            })
        );
    }

    #[test]
    fn results_and_messages_are_recognized() {
        assert_eq!(parse_line("SUCCESS"), Some(HelperMessage::Success));
        assert_eq!(parse_line("FAILURE"), Some(HelperMessage::Failure));
        assert_eq!(
            parse_line("PAM_ERROR_MSG Authentication failure"),
            Some(HelperMessage::Error("Authentication failure".to_string()))
        );
        assert_eq!(parse_line("nonsense"), None);
    }

    #[test]
    fn c_escapes_are_reversed() {
        assert_eq!(unescape("a\\nb\\tc\\\\d"), "a\nb\tc\\d");
        assert_eq!(unescape("\\120assword"), "Password");
    }

    #[test]
    fn the_current_user_has_a_name() {
        let uid = current_uid();
        assert!(username_for(uid).is_some());
    }

    #[test]
    fn an_unknown_uid_has_no_name() {
        assert_eq!(username_for(u32::MAX), None);
    }
}
