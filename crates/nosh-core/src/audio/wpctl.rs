//! `wpctl` audio backend.
//!
//! Reads come from `wpctl status` and `wpctl list`. When `pactl` is available,
//! `pactl subscribe` supplies change events. Otherwise the backend polls at a
//! configurable interval. Writes are clamped before they reach the command.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::audio::backend::{AudioBackend, AudioBackendEvent, AudioCapabilities};
use crate::audio::model::{volume_icon, AudioEndpoint, AudioModel, EndpointKind};
use crate::error::BackendError;

#[derive(Debug, Clone)]
pub struct WpctlConfig {
    pub binary: String,
    pub pactl_binary: String,
    pub command_timeout: Duration,
    pub poll_interval: Duration,
}

impl Default for WpctlConfig {
    fn default() -> Self {
        Self {
            binary: "wpctl".to_string(),
            pactl_binary: "pactl".to_string(),
            command_timeout: Duration::from_secs(2),
            poll_interval: Duration::from_secs(2),
        }
    }
}

pub struct WpctlBackend {
    config: WpctlConfig,
    pactl_available: AtomicBool,
}

impl WpctlBackend {
    pub fn new() -> Self {
        Self::with_config(WpctlConfig::default())
    }

    pub fn with_config(config: WpctlConfig) -> Self {
        Self {
            config,
            pactl_available: AtomicBool::new(true),
        }
    }

    async fn run(&self, arguments: &[&str]) -> Result<String, BackendError> {
        let mut command = Command::new(&self.config.binary);
        command.args(arguments).kill_on_drop(true);
        let output = tokio::time::timeout(self.config.command_timeout, command.output())
            .await
            .map_err(|_| {
                BackendError::Unavailable(format!(
                    "{} {} timed out",
                    self.config.binary,
                    arguments.join(" ")
                ))
            })?
            .map_err(|error| {
                BackendError::Unavailable(format!("cannot run {}: {error}", self.config.binary))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(BackendError::Request(if stderr.is_empty() {
                format!(
                    "{} {} exited with {}",
                    self.config.binary,
                    arguments.join(" "),
                    output.status
                )
            } else {
                stderr
            }));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    async fn read_model(&self) -> Result<AudioModel, BackendError> {
        let (status, sinks, sources) = tokio::try_join!(
            self.run(&["status"]),
            self.run(&["list", "audio", "sinks"]),
            self.run(&["list", "audio", "sources"]),
        )?;
        Ok(assemble(
            parse_status(&status),
            parse_list(&sinks),
            parse_list(&sources),
        ))
    }
}

impl Default for WpctlBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AudioBackend for WpctlBackend {
    fn name(&self) -> &'static str {
        "wpctl"
    }

    async fn probe(&self) -> Result<AudioCapabilities, BackendError> {
        // `wpctl status` succeeds when the server has no default sink yet, so a
        // probe does not require one.
        self.run(&["status"]).await?;
        Ok(AudioCapabilities::READ | AudioCapabilities::WATCH | AudioCapabilities::WRITE)
    }

    async fn read(&self) -> Result<AudioModel, BackendError> {
        self.read_model().await
    }

    async fn watch(&self, events: Sender<AudioBackendEvent>) -> Result<(), BackendError> {
        if self.pactl_available.load(Ordering::SeqCst) {
            match self.watch_pactl(&events).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    tracing::debug!(%error, "pactl subscribe is unavailable; polling wpctl");
                    self.pactl_available.store(false, Ordering::SeqCst);
                }
            }
        }

        self.watch_polling(&events).await
    }

    async fn set_volume(
        &self,
        kind: EndpointKind,
        id: u32,
        volume: f64,
    ) -> Result<(), BackendError> {
        let volume = volume.clamp(0.0, kind.max_volume());
        self.run(&["set-volume", &id.to_string(), &format!("{volume:.3}")])
            .await
            .map(|_| ())
    }

    async fn set_muted(
        &self,
        kind: EndpointKind,
        id: u32,
        muted: bool,
    ) -> Result<(), BackendError> {
        let _ = kind;
        self.run(&["set-mute", &id.to_string(), if muted { "1" } else { "0" }])
            .await
            .map(|_| ())
    }

    async fn set_default(&self, kind: EndpointKind, id: u32) -> Result<(), BackendError> {
        let _ = kind;
        self.run(&["set-default", &id.to_string()])
            .await
            .map(|_| ())
    }
}

impl WpctlBackend {
    async fn watch_pactl(&self, events: &Sender<AudioBackendEvent>) -> Result<(), BackendError> {
        let mut child = Command::new(&self.config.pactl_binary)
            .arg("subscribe")
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                BackendError::Unavailable(format!("cannot start pactl subscribe: {error}"))
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BackendError::Unavailable("pactl has no stdout".to_string()))?;
        let mut lines = BufReader::new(stdout).lines();

        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let relevant =
                        line.contains("sink") || line.contains("source") || line.contains("server");
                    if relevant && events.send(AudioBackendEvent::Changed).await.is_err() {
                        return Ok(());
                    }
                }
                Ok(None) => {
                    return Err(BackendError::Unavailable(
                        "pactl subscribe ended".to_string(),
                    ))
                }
                Err(error) => {
                    return Err(BackendError::Unavailable(format!(
                        "cannot read pactl subscribe: {error}"
                    )))
                }
            }
        }
    }

    async fn watch_polling(&self, events: &Sender<AudioBackendEvent>) -> Result<(), BackendError> {
        let mut last = self.read_model().await?;

        loop {
            tokio::time::sleep(self.config.poll_interval).await;
            let next = self.read_model().await?;
            if next != last {
                last = next;
                if events.send(AudioBackendEvent::Changed).await.is_err() {
                    return Ok(());
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedEndpoint {
    pub id: u32,
    pub description: String,
    pub volume: f64,
    pub muted: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedStatus {
    pub sinks: Vec<ParsedEndpoint>,
    pub sources: Vec<ParsedEndpoint>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedListEntry {
    pub name: String,
    pub is_default: bool,
}

/// Parses the Audio section of `wpctl status`.
pub fn parse_status(output: &str) -> ParsedStatus {
    let mut status = ParsedStatus::default();
    let mut in_audio = false;
    let mut section: Option<EndpointKind> = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed == "Audio" {
            in_audio = true;
            section = None;
            continue;
        }
        if trimmed == "Video" {
            break;
        }
        if !in_audio {
            continue;
        }

        if trimmed.starts_with("├─") || trimmed.starts_with("└─") {
            section = if trimmed.contains("Sinks:") {
                Some(EndpointKind::Sink)
            } else if trimmed.contains("Sources:") {
                Some(EndpointKind::Source)
            } else {
                None
            };
            continue;
        }

        let Some(kind) = section else {
            continue;
        };
        let Some(endpoint) = parse_endpoint_line(line) else {
            continue;
        };

        match kind {
            EndpointKind::Sink => status.sinks.push(endpoint),
            EndpointKind::Source => status.sources.push(endpoint),
        }
    }

    status.sinks.sort_by_key(|endpoint| endpoint.id);
    status.sources.sort_by_key(|endpoint| endpoint.id);
    status
}

fn parse_endpoint_line(line: &str) -> Option<ParsedEndpoint> {
    let trimmed = line.trim();
    let trimmed = trimmed.trim_start_matches(['│', ' ', '├', '└', '─']);
    let is_default = trimmed.trim_start().starts_with('*');
    let trimmed = trimmed.trim_start_matches(['*', ' ']);

    let (id, rest) = trimmed.split_once(". ")?;
    let id: u32 = id.trim().parse().ok()?;

    let muted = rest.contains("MUTED") || rest.contains("muted");
    let volume = parse_volume(rest);
    let description = match rest.find("[vol:") {
        Some(index) => rest[..index].trim(),
        None => match rest.find('[') {
            Some(index) => rest[..index].trim(),
            None => rest.trim(),
        },
    };

    Some(ParsedEndpoint {
        id,
        description: description.to_string(),
        volume,
        muted,
        is_default,
    })
}

fn parse_volume(rest: &str) -> f64 {
    let Some(start) = rest.find("[vol:") else {
        return 0.0;
    };
    let values = &rest[start + 5..];
    let values = values.split(']').next().unwrap_or("");

    let mut total = 0.0;
    let mut count = 0;
    for value in values.split_whitespace() {
        if let Ok(value) = value.parse::<f64>() {
            total += value;
            count += 1;
        }
    }

    if count == 0 {
        0.0
    } else {
        total / count as f64
    }
}

/// Parses `wpctl list audio sinks` or `wpctl list audio sources`.
pub fn parse_list(output: &str) -> HashMap<u32, ParsedListEntry> {
    let mut entries = HashMap::new();

    for line in output.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 2 {
            continue;
        }
        let Ok(id) = fields[0].trim().parse::<u32>() else {
            continue;
        };
        let name = fields[1].trim().to_string();
        if name.is_empty() {
            continue;
        }
        let is_default = fields.iter().any(|field| field.trim() == "*");
        entries.insert(id, ParsedListEntry { name, is_default });
    }

    entries
}

fn assemble(
    status: ParsedStatus,
    sink_names: HashMap<u32, ParsedListEntry>,
    source_names: HashMap<u32, ParsedListEntry>,
) -> AudioModel {
    let (sinks, default_sink) = assemble_kind(EndpointKind::Sink, status.sinks, sink_names);
    let (sources, default_source) =
        assemble_kind(EndpointKind::Source, status.sources, source_names);

    AudioModel {
        default_sink,
        default_source,
        sinks,
        sources,
    }
}

fn assemble_kind(
    kind: EndpointKind,
    parsed: Vec<ParsedEndpoint>,
    names: HashMap<u32, ParsedListEntry>,
) -> (Vec<AudioEndpoint>, Option<AudioEndpoint>) {
    let mut endpoints = Vec::with_capacity(parsed.len());

    for parsed in parsed {
        let listed = names.get(&parsed.id);
        let name = listed
            .map(|entry| entry.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("{}-{}", kind.label(), parsed.id));
        let description = if parsed.description.is_empty() {
            name.clone()
        } else {
            parsed.description
        };
        let is_default = parsed.is_default || listed.is_some_and(|entry| entry.is_default);
        let volume = AudioModel::clamped_volume(kind, parsed.volume.max(0.0));

        endpoints.push(AudioEndpoint {
            id: parsed.id,
            name,
            description,
            volume,
            muted: parsed.muted,
            icon_name: volume_icon(kind, volume, parsed.muted).to_string(),
            is_default,
        });
    }

    let default = endpoints
        .iter()
        .find(|endpoint| endpoint.is_default)
        .cloned();
    (endpoints, default)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = r#"PipeWire 'pipewire-0' [1.6.9, user@host, cookie:1]
 └─ Clients:
        1. WirePlumber

Audio
 ├─ Devices:
 │      55. Built-in Audio [alsa]
 │
 ├─ Sinks:
 │      68. HDMI Digital Stereo [Q27G42XE] [vol: 0.72]
 │  *   69. USB Headset Analog Stereo       [vol: 0.30]
 │      71. Analog Stereo                   [vol: 0.50 0.60 MUTED]
 │
 ├─ Sources:
 │  *   70. USB Headset Mono                [vol: 0.94]
 │      72. Analog Stereo                   [vol: 1.00]
 │
 └─ Streams:
       100. Player
            103. output_FR       > USB Headset:playback_FR [active]

Video
 ├─ Sinks:
 │      80. Video sink [vol: 0.10]
"#;

    #[test]
    fn status_parser_reads_both_audio_sections_only() {
        let status = parse_status(STATUS);

        assert_eq!(status.sinks.len(), 3);
        assert_eq!(status.sources.len(), 2);
        assert_eq!(
            status.sinks[1],
            ParsedEndpoint {
                id: 69,
                description: "USB Headset Analog Stereo".to_string(),
                volume: 0.3,
                muted: false,
                is_default: true,
            }
        );
        assert_eq!(status.sinks[2].volume, 0.55);
        assert!(status.sinks[2].muted);
        assert_eq!(status.sources[0].id, 70);
    }

    #[test]
    fn status_parser_copes_with_an_empty_audio_section() {
        let status = parse_status("Audio\n ├─ Sinks:\n ├─ Sources:\nVideo\n");
        assert!(status.sinks.is_empty());
        assert!(status.sources.is_empty());
    }

    #[test]
    fn list_parser_reads_names_and_the_default_marker() {
        let entries = parse_list(
            "68\talsa_output.hdmi\taudio/sink\t\n\
             69\talsa_output.usb\taudio/sink\t*\n\
             71\talsa_output.analog\taudio/sink\n",
        );

        assert_eq!(entries[&68].name, "alsa_output.hdmi");
        assert!(!entries[&68].is_default);
        assert_eq!(entries[&69].name, "alsa_output.usb");
        assert!(entries[&69].is_default);
    }

    #[test]
    fn assembled_model_uses_names_and_clamps_volume() {
        let status = parse_status(STATUS);
        let sinks = parse_list(
            "68\talsa_output.hdmi\taudio/sink\n\
             69\talsa_output.usb\taudio/sink\t*\n\
             71\talsa_output.analog\taudio/sink\n",
        );
        let sources = parse_list(
            "70\talsa_input.usb\taudio/source\t*\n\
             72\talsa_input.analog\taudio/source\n",
        );

        let model = assemble(status, sinks, sources);

        assert_eq!(model.sinks[0].name, "alsa_output.hdmi");
        assert_eq!(
            model.default_sink.as_ref().map(|endpoint| endpoint.id),
            Some(69)
        );
        assert_eq!(
            model.default_source.as_ref().map(|endpoint| endpoint.id),
            Some(70)
        );
        assert!(!model.sinks[0].is_default);
        assert_eq!(model.sources[1].volume, 1.0);
    }

    #[test]
    fn a_missing_default_is_not_an_error() {
        let model = assemble(
            parse_status("Audio\n ├─ Sinks:\n │      1. Sink [vol: 0.20]\n"),
            HashMap::new(),
            HashMap::new(),
        );

        assert!(model.default_sink.is_none());
        assert_eq!(model.sinks[0].name, "sink-1");
    }
}
