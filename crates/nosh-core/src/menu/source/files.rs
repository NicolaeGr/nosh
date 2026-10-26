//! The `files` builtin. It lists files in one directory, optionally recursing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use super::{MenuSource, SourceEnv, SourceError};
use crate::menu::model::{MenuIcon, MenuItem, MenuPreview, MenuValue};
use crate::menu::query::filter_and_sort;

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "avif", "bmp"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilesOptions {
    path: String,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    hidden: bool,
}

pub struct FilesSource {
    root: PathBuf,
    extensions: Vec<String>,
    recursive: bool,
    hidden: bool,
}

impl FilesSource {
    pub fn from_options(
        options: Option<&toml::Value>,
        env: Arc<SourceEnv>,
    ) -> Result<Self, SourceError> {
        let Some(value) = options else {
            return Err(SourceError::Options {
                builtin: "files".to_string(),
                message: "missing options with a path".to_string(),
            });
        };
        let options: FilesOptions =
            value
                .clone()
                .try_into()
                .map_err(|error: toml::de::Error| SourceError::Options {
                    builtin: "files".to_string(),
                    message: error.to_string(),
                })?;

        if options.path.trim().is_empty() {
            return Err(SourceError::Options {
                builtin: "files".to_string(),
                message: "path is empty".to_string(),
            });
        }

        Ok(Self {
            root: expand_tilde(&options.path, &env.home),
            extensions: options
                .extensions
                .into_iter()
                .map(|extension| {
                    extension
                        .trim()
                        .trim_start_matches('.')
                        .to_ascii_lowercase()
                })
                .filter(|extension| !extension.is_empty())
                .collect(),
            recursive: options.recursive,
            hidden: options.hidden,
        })
    }

    async fn scan(&self) -> Result<Vec<MenuItem>, SourceError> {
        let mut items = Vec::new();
        let mut stack = vec![self.root.clone()];
        let mut first = true;

        while let Some(directory) = stack.pop() {
            let mut entries = match tokio::fs::read_dir(&directory).await {
                Ok(entries) => entries,
                Err(error) if first => {
                    return Err(SourceError::Io {
                        path: directory.display().to_string(),
                        message: error.to_string(),
                    });
                }
                Err(_) => continue,
            };
            first = false;

            loop {
                let entry = match entries.next_entry().await {
                    Ok(Some(entry)) => entry,
                    Ok(None) => break,
                    Err(error) => {
                        return Err(SourceError::Io {
                            path: directory.display().to_string(),
                            message: error.to_string(),
                        });
                    }
                };

                let file_name = entry.file_name().to_string_lossy().to_string();
                if !self.hidden && file_name.starts_with('.') {
                    continue;
                }

                let path = entry.path();
                let file_type = match entry.file_type().await {
                    Ok(file_type) => file_type,
                    Err(_) => continue,
                };

                if file_type.is_dir() {
                    if self.recursive {
                        stack.push(path);
                    }
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }

                let extension = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if !self.extensions.is_empty() && !self.extensions.contains(&extension) {
                    continue;
                }

                let Some(item) = self.item_for(&path, &extension, entry.metadata().await.ok())
                else {
                    continue;
                };
                items.push(item);
            }
        }

        items.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(items)
    }

    fn item_for(
        &self,
        path: &Path,
        extension: &str,
        metadata: Option<std::fs::Metadata>,
    ) -> Option<MenuItem> {
        let name = path
            .file_stem()
            .or_else(|| path.file_name())
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            return None;
        }

        let mut item = MenuItem::new(path.display().to_string(), name.clone());
        item.description = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|value| value.to_str())
            .map(ToString::to_string);

        if IMAGE_EXTENSIONS.contains(&extension) {
            item.preview = Some(MenuPreview::Image {
                image: path.to_path_buf(),
            });
        } else {
            item.icon = Some(MenuIcon::Named("text-x-generic".to_string()));
        }

        item.fields.insert(
            "path".to_string(),
            MenuValue::String(path.display().to_string()),
        );
        item.fields
            .insert("name".to_string(), MenuValue::String(name));
        if let Some(directory) = path.parent() {
            item.fields.insert(
                "dir".to_string(),
                MenuValue::String(directory.display().to_string()),
            );
        }
        item.fields.insert(
            "extension".to_string(),
            MenuValue::String(extension.to_string()),
        );
        if let Some(metadata) = metadata {
            item.fields
                .insert("size".to_string(), MenuValue::Number(metadata.len() as f64));
        }

        Some(item)
    }
}

#[async_trait]
impl MenuSource for FilesSource {
    async fn query(&self, query: &str) -> Result<Vec<MenuItem>, SourceError> {
        let items = self.scan().await?;
        Ok(filter_and_sort(&items, query))
    }
}

fn expand_tilde(path: &str, home: &Path) -> PathBuf {
    if path == "~" {
        return home.to_path_buf();
    }
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(text: &str) -> toml::Value {
        toml::from_str(text).unwrap()
    }

    fn env(home: &Path) -> Arc<SourceEnv> {
        Arc::new(SourceEnv {
            home: home.to_path_buf(),
            config_dir: home.join(".config/nosh"),
            data_dirs: vec![home.join(".local/share")],
            usage: Arc::new(crate::menu::usage::Usage::empty()),
        })
    }

    #[tokio::test]
    async fn lists_and_filters_files() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a.png"), b"x").unwrap();
        std::fs::write(directory.path().join("b.txt"), b"x").unwrap();
        std::fs::write(directory.path().join(".hidden.png"), b"x").unwrap();

        let source = FilesSource::from_options(
            Some(&options(&format!(
                "path = {:?}\nextensions = [\"png\"]",
                directory.path()
            ))),
            env(directory.path()),
        )
        .unwrap();

        let items = source.query("").await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "a");
        assert_eq!(
            items[0].preview,
            Some(MenuPreview::Image {
                image: directory.path().join("a.png"),
            })
        );
        assert_eq!(items[0].field("extension").as_deref(), Some("png"));
    }

    #[tokio::test]
    async fn recursion_is_optional() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("deep.png"), b"x").unwrap();

        let flat = FilesSource::from_options(
            Some(&options(&format!("path = {:?}", directory.path()))),
            env(directory.path()),
        )
        .unwrap();
        assert!(flat.query("").await.unwrap().is_empty());

        let recursive = FilesSource::from_options(
            Some(&options(&format!(
                "path = {:?}\nrecursive = true",
                directory.path()
            ))),
            env(directory.path()),
        )
        .unwrap();
        assert_eq!(recursive.query("").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn missing_directory_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let source = FilesSource::from_options(
            Some(&options(&format!(
                "path = {:?}",
                directory.path().join("missing")
            ))),
            env(directory.path()),
        )
        .unwrap();

        assert!(matches!(
            source.query("").await.unwrap_err(),
            SourceError::Io { .. }
        ));
    }

    #[test]
    fn expands_the_home_prefix() {
        assert_eq!(
            expand_tilde("~/Pictures", Path::new("/home/user")),
            PathBuf::from("/home/user/Pictures")
        );
        assert_eq!(
            expand_tilde("/tmp/x", Path::new("/home/user")),
            PathBuf::from("/tmp/x")
        );
    }

    #[test]
    fn missing_path_option_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let error =
            FilesSource::from_options(Some(&options("recursive = true")), env(directory.path()));
        assert!(matches!(error, Err(SourceError::Options { .. })));
    }
}
