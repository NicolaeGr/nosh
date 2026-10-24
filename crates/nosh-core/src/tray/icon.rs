//! Pixmap decoding and icon theme path lookup.
//!
//! StatusNotifierItem pixmaps are ARGB32 in network byte order. The UI needs
//! RGBA bytes, so the core converts them once. Icon theme paths supplied by an
//! item are searched to a bounded depth and the result is cached.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::tray::model::IconPixmap;

/// The largest accepted pixmap side. A larger value is rejected instead of
/// allocating from an untrusted size.
pub const MAX_PIXMAP_SIZE: u32 = 512;

/// The largest depth searched below an item's icon theme path.
pub const MAX_ICON_DEPTH: usize = 4;

/// The largest number of lookups kept in an [`IconCache`].
pub const MAX_ICON_CACHE_ENTRIES: usize = 4096;

/// Decodes one `(width, height, bytes)` pixmap.
///
/// Rejects a side of zero or a side above [`MAX_PIXMAP_SIZE`], a short buffer,
/// and a size that overflows `usize`.
pub fn decode_argb32(width: i32, height: i32, bytes: &[u8]) -> Option<IconPixmap> {
    if width <= 0 || height <= 0 {
        return None;
    }

    let width = u32::try_from(width).ok()?;
    let height = u32::try_from(height).ok()?;
    if width > MAX_PIXMAP_SIZE || height > MAX_PIXMAP_SIZE {
        return None;
    }

    let pixels = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?;
    let needed = pixels.checked_mul(4)?;
    if bytes.len() < needed {
        return None;
    }

    let mut rgba = Vec::with_capacity(needed);
    for pixel in bytes[..needed].chunks_exact(4) {
        // Network byte order ARGB32: alpha, red, green, blue.
        rgba.extend_from_slice(&[pixel[1], pixel[2], pixel[3], pixel[0]]);
    }

    Some(IconPixmap {
        width,
        height,
        rgba,
    })
}

/// Decodes every valid pixmap in the list and returns the largest by area.
pub fn largest_pixmap(pixmaps: &[(i32, i32, Vec<u8>)]) -> Option<IconPixmap> {
    pixmaps
        .iter()
        .filter_map(|(width, height, bytes)| decode_argb32(*width, *height, bytes))
        .max_by_key(|pixmap| u64::from(pixmap.width) * u64::from(pixmap.height))
}

/// A cache of icon theme path lookups.
#[derive(Debug, Default)]
pub struct IconCache {
    entries: Mutex<HashMap<(PathBuf, String), Option<PathBuf>>>,
}

impl IconCache {
    /// Finds an icon file below `theme_path` and caches the result.
    ///
    /// The lookup does not follow directory symlinks, so it cannot loop. The
    /// depth and the number of visited entries are bounded.
    pub fn find(&self, theme_path: &Path, name: &str) -> Option<PathBuf> {
        let key = (theme_path.to_path_buf(), name.to_string());
        if let Ok(entries) = self.entries.lock() {
            if let Some(cached) = entries.get(&key) {
                return cached.clone();
            }
        }

        let found = find_icon_file(theme_path, name);
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() < MAX_ICON_CACHE_ENTRIES || entries.contains_key(&key) {
                entries.insert(key, found.clone());
            }
        }
        found
    }
}

/// The uncached lookup used by [`IconCache`]. Exposed for tests.
pub fn find_icon_file(theme_path: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('\0') {
        return None;
    }

    let direct = Path::new(name);
    if direct.is_absolute() && direct.is_file() {
        return Some(direct.to_path_buf());
    }

    let mut candidates = vec![name.to_string()];
    if Path::new(name).extension().is_none() {
        for extension in ["png", "svg", "xpm"] {
            candidates.push(format!("{name}.{extension}"));
        }
    }

    let mut visited = 0usize;
    search(theme_path, &candidates, 0, &mut visited)
}

fn search(
    directory: &Path,
    names: &[String],
    depth: usize,
    visited: &mut usize,
) -> Option<PathBuf> {
    if depth > MAX_ICON_DEPTH || *visited > 4096 {
        return None;
    }

    let entries = fs::read_dir(directory).ok()?;
    let mut directories = Vec::new();

    for entry in entries.flatten() {
        *visited += 1;
        if *visited > 4096 {
            return None;
        }

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        let path = entry.path();

        if file_type.is_file() || (file_type.is_symlink() && path.is_file()) {
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            if names.iter().any(|name| name == file_name) {
                return Some(path);
            }
        } else if file_type.is_dir() {
            directories.push(path);
        }
    }

    // Sort for a stable result when an item ships several sizes.
    directories.sort();
    for directory in directories {
        if let Some(found) = search(&directory, names, depth + 1, visited) {
            return Some(found);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn argb_pixels_become_rgba() {
        let pixmap = decode_argb32(1, 1, &[0x80, 0x10, 0x20, 0x30]).unwrap();
        assert_eq!(pixmap.width, 1);
        assert_eq!(pixmap.height, 1);
        assert_eq!(pixmap.rgba, vec![0x10, 0x20, 0x30, 0x80]);
    }

    #[test]
    fn every_pixel_is_rotated() {
        let pixmap = decode_argb32(2, 1, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(pixmap.rgba, vec![2, 3, 4, 1, 6, 7, 8, 5]);
    }

    #[test]
    fn zero_sized_pixmaps_are_rejected() {
        assert_eq!(decode_argb32(0, 1, &[]), None);
        assert_eq!(decode_argb32(1, 0, &[]), None);
        assert_eq!(decode_argb32(-1, -1, &[]), None);
    }

    #[test]
    fn oversized_pixmaps_are_rejected_without_allocating() {
        assert_eq!(decode_argb32(MAX_PIXMAP_SIZE as i32 + 1, 1, &[]), None);
        assert_eq!(decode_argb32(1, MAX_PIXMAP_SIZE as i32 + 1, &[]), None);
    }

    #[test]
    fn a_short_buffer_is_rejected() {
        assert_eq!(decode_argb32(2, 2, &[0; 15]), None);
        assert!(decode_argb32(2, 2, &[0; 16]).is_some());
    }

    #[test]
    fn the_largest_pixmap_wins() {
        let small = (1, 1, vec![0; 4]);
        let large = (2, 2, vec![0; 16]);
        let broken = (0, 4, vec![0; 16]);

        let chosen = largest_pixmap(&[small, broken, large]).unwrap();
        assert_eq!(chosen.width, 2);
        assert_eq!(chosen.height, 2);
    }

    #[test]
    fn no_valid_pixmap_means_none() {
        assert_eq!(largest_pixmap(&[]), None);
        assert_eq!(largest_pixmap(&[(0, 0, Vec::new())]), None);
    }

    #[test]
    fn a_themed_name_is_found_below_the_theme_path() {
        let directory = tempfile::tempdir().unwrap();
        let icons = directory.path().join("icons").join("hicolor");
        fs::create_dir_all(&icons).unwrap();
        let file = icons.join("app.svg");
        fs::write(&file, b"<svg/>").unwrap();

        assert_eq!(find_icon_file(directory.path(), "app"), Some(file));
    }

    #[test]
    fn a_name_with_an_extension_is_found() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("app.png");
        fs::write(&file, b"png").unwrap();

        assert_eq!(
            find_icon_file(directory.path(), "app.png"),
            Some(file.clone())
        );
        assert_eq!(find_icon_file(directory.path(), "app"), Some(file));
    }

    #[test]
    fn a_lookup_stops_at_the_depth_limit() {
        let directory = tempfile::tempdir().unwrap();
        let mut deep = directory.path().join("1");
        for index in 2..=MAX_ICON_DEPTH + 2 {
            deep = deep.join(index.to_string());
        }
        fs::create_dir_all(&deep).unwrap();
        let file = deep.join("app.svg");
        fs::write(&file, b"<svg/>").unwrap();

        assert_eq!(find_icon_file(directory.path(), "app"), None);
    }

    #[test]
    fn a_missing_icon_is_cached_as_none() {
        let directory = tempfile::tempdir().unwrap();
        let cache = IconCache::default();

        assert_eq!(cache.find(directory.path(), "missing"), None);
        assert_eq!(cache.find(directory.path(), "missing"), None);
        assert_eq!(
            cache
                .entries
                .lock()
                .unwrap()
                .get(&(directory.path().to_path_buf(), "missing".to_string())),
            Some(&None)
        );
    }

    #[test]
    fn an_absolute_icon_name_is_used_directly() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("app.png");
        fs::write(&file, b"png").unwrap();

        assert_eq!(
            find_icon_file(directory.path(), file.to_str().unwrap()),
            Some(file)
        );
    }
}
