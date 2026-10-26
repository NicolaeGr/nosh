//! Source resolution and the external NDJSON runner.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{ChildStdout, Command};

use super::model::{
    dedupe_ids, MenuItem, SourceDef, SourceKind, Template, MAX_LINE_BYTES, MAX_OUTPUT_BYTES,
};
use super::usage::{self, Usage};

pub mod apps;
pub mod files;
pub mod static_source;

/// The environment a builtin source reads.
#[derive(Debug, Clone)]
pub struct SourceEnv {
    pub home: PathBuf,
    pub config_dir: PathBuf,
    pub data_dirs: Vec<PathBuf>,
    pub usage: Arc<Usage>,
}

impl SourceEnv {
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();

        let config_dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("nosh");

        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"));

        let usage_path = data_home.join("nosh").join("app_usage.json");

        let mut data_dirs = vec![data_home];
        match std::env::var_os("XDG_DATA_DIRS") {
            Some(value) => data_dirs.extend(std::env::split_paths(&value)),
            None => {
                data_dirs.push(PathBuf::from("/usr/local/share"));
                data_dirs.push(PathBuf::from("/usr/share"));
            }
        }

        let mut seen = BTreeSet::new();
        data_dirs.retain(|directory| seen.insert(directory.clone()));

        Self {
            home,
            config_dir,
            data_dirs,
            usage: Arc::new(Usage::load(usage_path, usage::now())),
        }
    }
}

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("unknown builtin source {0:?}")]
    UnknownBuiltin(String),
    #[error("invalid options for {builtin}: {message}")]
    Options { builtin: String, message: String },
    #[error("cannot read {path}: {message}")]
    Io { path: String, message: String },
    #[error("source timed out")]
    Timeout,
    #[error("cannot start the source: {0}")]
    Spawn(String),
    #[error("source exited with {status}")]
    Exit { status: String },
}

/// A function from a query to a list of items.
#[async_trait]
pub trait MenuSource: Send + Sync {
    async fn query(&self, query: &str) -> Result<Vec<MenuItem>, SourceError>;

    /// Called after an item from this source launches. Sources that rank by
    /// usage record the launch here. The default does nothing.
    fn record_use(&self, _item: &MenuItem) {}
}

/// Resolves a source definition into a runnable source.
pub fn menu_source(
    def: &SourceDef,
    template: Template,
    env: Arc<SourceEnv>,
) -> Result<Arc<dyn MenuSource>, SourceError> {
    match def.kind {
        SourceKind::Builtin => {
            let name = def.name.as_deref().unwrap_or("");
            let source = builtin_source(name, def.options.as_ref(), env)?;
            Ok(Arc::from(source))
        }
        SourceKind::External => Ok(Arc::new(ExternalSource {
            def: def.clone(),
            template,
        })),
    }
}

pub fn builtin_source(
    name: &str,
    options: Option<&toml::Value>,
    env: Arc<SourceEnv>,
) -> Result<Box<dyn MenuSource>, SourceError> {
    match name {
        "apps" => Ok(Box::new(apps::AppsSource::new(env))),
        "files" => Ok(Box::new(files::FilesSource::from_options(options, env)?)),
        "static" => Ok(Box::new(static_source::StaticSource::from_options(
            options,
        )?)),
        other => Err(SourceError::UnknownBuiltin(other.to_string())),
    }
}

struct ExternalSource {
    def: SourceDef,
    template: Template,
}

#[async_trait]
impl MenuSource for ExternalSource {
    async fn query(&self, query: &str) -> Result<Vec<MenuItem>, SourceError> {
        run_external(&self.def, query, &self.template).await
    }
}

/// Runs an external source and parses its NDJSON output.
///
/// The query is appended as the final argument after the configured command
/// words, so `["python", "list.py"]` becomes `python list.py <query>`.
pub async fn run_external(
    def: &SourceDef,
    query: &str,
    template: &Template,
) -> Result<Vec<MenuItem>, SourceError> {
    let argv = def
        .command
        .as_deref()
        .filter(|argv| !argv.is_empty())
        .ok_or_else(|| SourceError::Options {
            builtin: "external".to_string(),
            message: "command is empty".to_string(),
        })?;

    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .arg(query)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .map_err(|error| SourceError::Spawn(error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| SourceError::Spawn("the source has no stdout".to_string()))?;

    let limit = def.resolved_limit();
    let timeout = Duration::from_millis(def.resolved_timeout_ms());

    let result = tokio::time::timeout(timeout, read_output(stdout, template, limit)).await;

    match result {
        Ok(Ok((mut items, over_limit))) => {
            if over_limit {
                let _ = child.kill().await;
                dedupe_ids(&mut items);
                return Ok(items);
            }

            let status = child
                .wait()
                .await
                .map_err(|error| SourceError::Spawn(error.to_string()))?;
            if status.success() {
                dedupe_ids(&mut items);
                Ok(items)
            } else {
                Err(SourceError::Exit {
                    status: status.to_string(),
                })
            }
        }
        Ok(Err(error)) => {
            let _ = child.kill().await;
            Err(error)
        }
        Err(_) => {
            let _ = child.kill().await;
            Err(SourceError::Timeout)
        }
    }
}

async fn read_output(
    stdout: ChildStdout,
    template: &Template,
    limit: usize,
) -> Result<(Vec<MenuItem>, bool), SourceError> {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    let mut items = Vec::new();
    let mut total = 0usize;
    let mut warned = false;
    let mut over_limit = false;

    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .await
            .map_err(|error| SourceError::Io {
                path: "source stdout".to_string(),
                message: error.to_string(),
            })?;
        if read == 0 {
            break;
        }

        total += read;
        if total > MAX_OUTPUT_BYTES || line.len() > MAX_LINE_BYTES {
            over_limit = true;
            break;
        }

        let Ok(text) = std::str::from_utf8(&line) else {
            if !warned {
                tracing::warn!("an external menu source emitted a non UTF-8 line");
                warned = true;
            }
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(error) => {
                if !warned {
                    tracing::warn!(%error, "an external menu source emitted invalid JSON");
                    warned = true;
                }
                continue;
            }
        };

        let values = match value {
            serde_json::Value::Array(values) => values,
            value => vec![value],
        };

        for value in values {
            match template.apply(&value) {
                Ok(item) => {
                    items.push(item);
                    if items.len() >= limit {
                        over_limit = true;
                        break;
                    }
                }
                Err(error) => {
                    if !warned {
                        tracing::warn!(%error, "an external menu source emitted a bad item");
                        warned = true;
                    }
                }
            }
        }

        if over_limit {
            break;
        }
    }

    Ok((items, over_limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_env_always_has_a_data_home() {
        let env = SourceEnv::from_env();
        assert!(!env.data_dirs.is_empty());
    }

    #[test]
    fn unknown_builtins_are_errors() {
        let env = Arc::new(SourceEnv::from_env());
        let error = builtin_source("nope", None, env)
            .err()
            .expect("an unknown builtin should fail");
        assert!(matches!(error, SourceError::UnknownBuiltin(name) if name == "nope"));
    }

    fn external(command: Vec<&str>, timeout_ms: u64) -> SourceDef {
        SourceDef {
            kind: SourceKind::External,
            name: None,
            command: Some(command.into_iter().map(ToString::to_string).collect()),
            options: None,
            search: None,
            debounce_ms: None,
            timeout_ms: Some(timeout_ms),
            limit: None,
        }
    }

    #[tokio::test]
    async fn external_source_parses_ndjson() {
        let def = external(
            vec![
                "sh",
                "-c",
                "printf '%s\\n' '{\"name\":\"One\"}' '{\"name\":\"Two\"}'",
                "sh",
            ],
            2000,
        );

        let items = run_external(&def, "query", &Template::default_fields())
            .await
            .unwrap();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "One");
        assert_eq!(items[1].name, "Two");
    }

    #[tokio::test]
    async fn external_source_skips_bad_lines_and_items() {
        let def = external(
            vec![
                "sh",
                "-c",
                "printf '%s\\n' 'not json' '{\"nope\":1}' '{\"name\":\"Good\"}'",
                "sh",
            ],
            2000,
        );

        let items = run_external(&def, "", &Template::default_fields())
            .await
            .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Good");
    }

    #[tokio::test]
    async fn external_source_times_out() {
        let def = external(vec!["sh", "-c", "sleep 5", "sh"], 20);
        let error = run_external(&def, "", &Template::default_fields())
            .await
            .unwrap_err();
        assert!(matches!(error, SourceError::Timeout));
    }
}
