//! Image hint normalization.
//!
//! `image-data` is a raw `(iiibiiay)` structure. The whole hint is replaced by
//! an `image-path` pointing at a cached PNG. Unsupported formats are dropped
//! with a debug log, not converted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use zbus::zvariant::{OwnedValue, Value};

use crate::notifications::model::{HINT_IMAGE_DATA, HINT_IMAGE_PATH};

/// The largest accepted source image, in pixels per side.
pub const MAX_DIMENSION: u32 = 4096;
/// The largest accepted raw pixel payload.
pub const MAX_RAW_BYTES: usize = 32 * 1024 * 1024;

type RawImageData = (i32, i32, i32, bool, i32, i32, Vec<u8>);

/// Replaces `image-data` and the deprecated `image_data` hint with an
/// `image-path`. Returns the cached path when one was written.
pub fn normalize_image_hints(
    hints: &mut HashMap<String, OwnedValue>,
    cache_dir: &Path,
) -> Option<PathBuf> {
    let raw = hints
        .remove(HINT_IMAGE_DATA)
        .or_else(|| hints.remove("image_data"));
    hints.remove("icon_data");

    let raw = raw?;
    match cache_image_value(&raw, cache_dir) {
        Ok(Some(path)) => {
            hints.insert(
                HINT_IMAGE_PATH.to_string(),
                OwnedValue::try_from(Value::from(path.to_string_lossy().into_owned()))
                    .unwrap_or_else(|_| {
                        OwnedValue::try_from(Value::from(String::new())).expect("a string is valid")
                    }),
            );
            Some(path)
        }
        Ok(None) => None,
        Err(error) => {
            tracing::debug!(%error, "dropping an unsupported notification image");
            None
        }
    }
}

/// Converts one `image-data` value to a cached PNG. Returns `Ok(None)` when the
/// value is not a well-formed image structure.
pub fn cache_image_value(value: &OwnedValue, cache_dir: &Path) -> Result<Option<PathBuf>, String> {
    let (width, height, rowstride, _has_alpha, bits_per_sample, channels, data) =
        RawImageData::try_from(value.clone()).map_err(|error| error.to_string())?;

    if width <= 0 || height <= 0 {
        return Err("the image has no area".to_string());
    }
    let width = width as u32;
    let height = height as u32;
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(format!("the image is larger than {MAX_DIMENSION} pixels"));
    }
    if bits_per_sample != 8 {
        return Err(format!("unsupported bit depth {bits_per_sample}"));
    }
    if channels != 3 && channels != 4 {
        return Err(format!("unsupported channel count {channels}"));
    }
    let channels = channels as usize;
    let row_bytes = width as usize * channels;
    let stride = if rowstride <= 0 {
        row_bytes
    } else {
        rowstride as usize
    };
    if stride < row_bytes {
        return Err("the row stride is smaller than one row".to_string());
    }
    let expected = stride
        .checked_mul(height as usize)
        .ok_or_else(|| "the image dimensions overflow".to_string())?;
    if data.len() < expected {
        return Err("the image data is shorter than its dimensions".to_string());
    }
    if data.len() > MAX_RAW_BYTES {
        return Err("the image data is too large".to_string());
    }

    let mut pixels = Vec::with_capacity(row_bytes * height as usize);
    for row in 0..height as usize {
        let start = row * stride;
        pixels.extend_from_slice(&data[start..start + row_bytes]);
    }

    let png = encode_png(width, height, channels, &pixels)?;
    let digest = Sha256::digest(&png);
    let hash = format!("{digest:x}");
    let path = cache_dir.join(format!("{hash}.png"));

    if path.exists() {
        return Ok(Some(path));
    }

    std::fs::create_dir_all(cache_dir).map_err(|error| error.to_string())?;
    let tmp = cache_dir.join(format!(".{hash}.png.tmp"));
    std::fs::write(&tmp, &png).map_err(|error| error.to_string())?;
    if let Err(error) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error.to_string());
    }

    Ok(Some(path))
}

fn encode_png(width: u32, height: u32, channels: usize, pixels: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_color(if channels == 4 {
            png::ColorType::Rgba
        } else {
            png::ColorType::Rgb
        });
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer
            .write_image_data(pixels)
            .map_err(|error| error.to_string())?;
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_value(
        width: i32,
        height: i32,
        rowstride: i32,
        bits: i32,
        channels: i32,
        data: Vec<u8>,
    ) -> OwnedValue {
        let structure: (i32, i32, i32, bool, i32, i32, Vec<u8>) = (
            width,
            height,
            rowstride,
            channels == 4,
            bits,
            channels,
            data,
        );
        OwnedValue::try_from(Value::from(structure)).unwrap()
    }

    #[test]
    fn rgb_data_becomes_a_cached_png() {
        let directory = tempfile::tempdir().unwrap();
        let mut hints = HashMap::new();
        hints.insert(
            HINT_IMAGE_DATA.to_string(),
            image_value(2, 2, 6, 8, 3, vec![255; 2 * 6]),
        );

        let path = normalize_image_hints(&mut hints, directory.path()).unwrap();

        assert!(path.exists());
        assert!(!hints.contains_key(HINT_IMAGE_DATA));
        let cached = hints
            .get(HINT_IMAGE_PATH)
            .and_then(|value| String::try_from(value.clone()).ok())
            .unwrap();
        assert_eq!(cached, path.to_string_lossy());
    }

    #[test]
    fn rows_with_padding_are_unpadded() {
        let directory = tempfile::tempdir().unwrap();
        let mut hints = HashMap::new();
        hints.insert(
            HINT_IMAGE_DATA.to_string(),
            image_value(2, 2, 8, 8, 3, vec![0; 2 * 8]),
        );

        assert!(normalize_image_hints(&mut hints, directory.path()).is_some());
    }

    #[test]
    fn rgba_data_is_accepted() {
        let directory = tempfile::tempdir().unwrap();
        let mut hints = HashMap::new();
        hints.insert(
            HINT_IMAGE_DATA.to_string(),
            image_value(1, 1, 4, 8, 4, vec![1, 2, 3, 4]),
        );

        assert!(normalize_image_hints(&mut hints, directory.path()).is_some());
    }

    #[test]
    fn unsupported_formats_are_dropped() {
        let directory = tempfile::tempdir().unwrap();

        for (bits, channels) in [(16, 3), (8, 2), (8, 5)] {
            let mut hints = HashMap::new();
            hints.insert(
                HINT_IMAGE_DATA.to_string(),
                image_value(1, 1, 3, bits, channels, vec![0; 8]),
            );

            assert!(normalize_image_hints(&mut hints, directory.path()).is_none());
            assert!(!hints.contains_key(HINT_IMAGE_DATA));
        }
    }

    #[test]
    fn short_or_oversized_images_are_rejected() {
        let directory = tempfile::tempdir().unwrap();

        let mut hints = HashMap::new();
        hints.insert(
            HINT_IMAGE_DATA.to_string(),
            image_value(10, 10, 30, 8, 3, vec![0; 10]),
        );
        assert!(normalize_image_hints(&mut hints, directory.path()).is_none());

        let mut hints = HashMap::new();
        hints.insert(
            HINT_IMAGE_DATA.to_string(),
            image_value(MAX_DIMENSION as i32 + 1, 1, 3, 8, 3, vec![0; 3]),
        );
        assert!(normalize_image_hints(&mut hints, directory.path()).is_none());
    }

    #[test]
    fn the_deprecated_hints_are_removed() {
        let directory = tempfile::tempdir().unwrap();
        let mut hints = HashMap::new();
        hints.insert("image_data".to_string(), OwnedValue::from(1u8));
        hints.insert("icon_data".to_string(), OwnedValue::from(2u8));

        assert!(normalize_image_hints(&mut hints, directory.path()).is_none());
        assert!(hints.is_empty());
    }

    #[test]
    fn identical_images_share_a_file() {
        let directory = tempfile::tempdir().unwrap();
        let value = || image_value(1, 1, 3, 8, 3, vec![10, 20, 30]);

        let first = cache_image_value(&value(), directory.path()).unwrap();
        let second = cache_image_value(&value(), directory.path()).unwrap();

        assert_eq!(first, second);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
