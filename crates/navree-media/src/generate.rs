//! Generating conversions: images, video frames and transcodes, written
//! next to the original.

use bytes::Bytes;
use chrono::Utc;
use navree_storage::{Disk, StorageError};
#[cfg(feature = "ffmpeg")]
use serde_json::{Map, Value};

use crate::conversion::Conversion;
use crate::images::{self, ConvertedImage};
use crate::item::{GeneratedConversion, ResponsiveVariant};
use crate::service::{Media, blocking};
use crate::{MediaError, MediaItem};

/// The image types the decoders read (and SVG, rendered with resvg).
const IMAGE_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/svg+xml",
];

/// Whether the image decoders (or resvg) read files of type `mime_type`:
/// those originals are measured, stripped of metadata and converted;
/// other images (TIFF, HEIC, icons) are stored as they are.
pub(crate) fn is_decodable_image(mime_type: &str) -> bool {
    IMAGE_TYPES.contains(&mime_type)
}

/// Whether `conversion` can be generated from a file of type `mime_type`:
/// image conversions from images (and, with the `ffmpeg` feature, from a
/// video's frame), transcodes from video and audio. Other files keep no
/// conversion of that name; that is not an error.
pub(crate) fn applies(conversion: &Conversion, mime_type: &str) -> bool {
    let video = mime_type.starts_with("video/");
    if conversion.is_transcode() {
        return cfg!(feature = "ffmpeg") && (video || mime_type.starts_with("audio/"));
    }
    IMAGE_TYPES.contains(&mime_type) || (cfg!(feature = "ffmpeg") && video)
}

/// The file name without its extension, for the names of conversions.
fn stem(file_name: &str) -> &str {
    match file_name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => file_name,
    }
}

impl Media {
    /// Generates `conversions` of `item` from its original (`original`, or
    /// read from its disk), writes them to the conversions disk and
    /// records them on the item, replacing earlier generations (whose
    /// files are deleted when the new ones have other paths). Conversions
    /// that do not apply to the item's type are skipped.
    pub(crate) async fn generate(
        &self,
        mut item: MediaItem,
        conversions: &[Conversion],
        original: Option<Bytes>,
    ) -> Result<MediaItem, MediaError> {
        let conversions: Vec<&Conversion> = conversions
            .iter()
            .filter(|conversion| applies(conversion, &item.mime_type))
            .collect();
        if conversions.is_empty() {
            return Ok(item);
        }
        let original = match original {
            Some(bytes) => bytes,
            None => self.read(&item).await?,
        };
        let disk = self.disk(&item.conversions_disk)?;
        #[cfg(feature = "ffmpeg")]
        let mut scratch: Option<Scratch> = None;
        for conversion in conversions {
            let is_image = IMAGE_TYPES.contains(&item.mime_type.as_str());
            let generated = if is_image {
                let bytes = original.clone();
                let mime_type = item.mime_type.clone();
                let options = self.options().images.clone();
                let conversion_owned = conversion.clone();
                let image = blocking(move || {
                    images::convert(&bytes, &mime_type, &conversion_owned, &options)
                })
                .await?;
                store_image(&disk, &item, &conversion.name, image).await?
            } else {
                #[cfg(feature = "ffmpeg")]
                {
                    if scratch.is_none() {
                        scratch = Some(Scratch::write(&original, &item.mime_type).await?);
                    }
                    let scratch = scratch.as_ref().ok_or_else(|| MediaError::Ffmpeg {
                        message: "no scratch file".to_owned(),
                    })?;
                    self.generate_av(&disk, &item, conversion, scratch).await?
                }
                #[cfg(not(feature = "ffmpeg"))]
                {
                    continue;
                }
            };
            if let Some(previous) = item.conversions.get(&conversion.name) {
                let kept = generated_paths(&generated);
                for path in generated_paths(previous) {
                    if !kept.contains(&path) {
                        delete_quietly(&disk, &path).await;
                    }
                }
            }
            item.conversions.insert(conversion.name.clone(), generated);
        }
        item.updated_at = Utc::now();
        Ok(item)
    }

    /// A video or audio conversion: a transcode, or an image conversion of
    /// a video's frame.
    #[cfg(feature = "ffmpeg")]
    async fn generate_av(
        &self,
        disk: &Disk,
        item: &MediaItem,
        conversion: &Conversion,
        scratch: &Scratch,
    ) -> Result<GeneratedConversion, MediaError> {
        let ffmpeg = self.ffmpeg();
        if let Some(transcode) = &conversion.transcode {
            let output =
                scratch
                    .dir
                    .path()
                    .join(format!("{}.{}", conversion.name, transcode.extension()));
            ffmpeg.transcode(&scratch.input, &output, transcode).await?;
            let bytes = read_file(&output).await?;
            let dimensions = if transcode.has_video() {
                match ffmpeg.probe(&output).await {
                    Ok(probe) => probe.dimensions(),
                    Err(_) => None,
                }
            } else {
                None
            };
            let path = format!(
                "{}/conversions/{}-{}.{}",
                item.directory(),
                stem(&item.file_name),
                conversion.name,
                transcode.extension()
            );
            let size = bytes.len() as u64;
            disk.put(&path, bytes).await?;
            return Ok(GeneratedConversion {
                path,
                mime_type: transcode.mime_type().to_owned(),
                size,
                width: dimensions.map(|(w, _)| w),
                height: dimensions.map(|(_, h)| h),
                variants: Vec::new(),
                generated_at: Utc::now(),
            });
        }
        let duration = item.metadata.get("duration").and_then(Value::as_f64);
        let at = frame_time(conversion.frame_at, duration);
        let frame = scratch
            .dir
            .path()
            .join(format!("frame-{}.png", conversion.name));
        ffmpeg.frame(&scratch.input, at, &frame).await?;
        let png = read_file(&frame).await?;
        let options = self.options().images.clone();
        let conversion_owned = conversion.clone();
        let image =
            blocking(move || images::convert(&png, "image/png", &conversion_owned, &options))
                .await?;
        store_image(disk, item, &conversion.name, image).await
    }

    /// The size and metadata ffprobe reads from a video or audio file;
    /// `None` (logged) when FFmpeg is missing or fails, so the file is
    /// still added.
    #[cfg(feature = "ffmpeg")]
    pub(crate) async fn probe(
        &self,
        bytes: &Bytes,
        mime_type: &str,
    ) -> Option<(Option<(u32, u32)>, Map<String, Value>)> {
        let scratch = match Scratch::write(bytes, mime_type).await {
            Ok(scratch) => scratch,
            Err(error) => {
                tracing::warn!(target: "navree::media", %error, "cannot probe the file");
                return None;
            }
        };
        match self.ffmpeg().probe(&scratch.input).await {
            Ok(probe) => {
                let mut metadata = Map::new();
                if let Some(duration) = probe.duration {
                    metadata.insert("duration".into(), Value::from(duration));
                }
                if let Some(codec) = probe.video().and_then(|s| s.codec_name.clone()) {
                    metadata.insert("video_codec".into(), Value::from(codec));
                }
                if let Some(codec) = probe.audio().and_then(|s| s.codec_name.clone()) {
                    metadata.insert("audio_codec".into(), Value::from(codec));
                }
                Some((probe.dimensions(), metadata))
            }
            Err(error) => {
                tracing::warn!(target: "navree::media", %error, "ffprobe failed; the file is added without its metadata");
                None
            }
        }
    }
}

/// The second a frame conversion grabs: `frame_at` (default 1 s), or the
/// middle of a video shorter than that.
#[cfg(feature = "ffmpeg")]
pub(crate) fn frame_time(frame_at: Option<f64>, duration: Option<f64>) -> f64 {
    let at = frame_at.unwrap_or(1.0);
    match duration {
        Some(duration) if duration > 0.0 && at >= duration => duration / 2.0,
        _ => at,
    }
}

/// A temporary directory holding the original for FFmpeg; removed when
/// dropped.
#[cfg(feature = "ffmpeg")]
pub(crate) struct Scratch {
    dir: tempfile::TempDir,
    input: std::path::PathBuf,
}

#[cfg(feature = "ffmpeg")]
impl Scratch {
    async fn write(bytes: &[u8], mime_type: &str) -> Result<Scratch, MediaError> {
        let dir = tempfile::tempdir().map_err(|error| MediaError::File {
            path: std::env::temp_dir().display().to_string(),
            message: error.to_string(),
        })?;
        let input = dir
            .path()
            .join(format!("original.{}", crate::mime::extension(mime_type)));
        tokio::fs::write(&input, bytes)
            .await
            .map_err(|error| MediaError::File {
                path: input.display().to_string(),
                message: error.to_string(),
            })?;
        Ok(Scratch { dir, input })
    }
}

#[cfg(feature = "ffmpeg")]
async fn read_file(path: &std::path::Path) -> Result<Bytes, MediaError> {
    tokio::fs::read(path)
        .await
        .map(Bytes::from)
        .map_err(|error| MediaError::File {
            path: path.display().to_string(),
            message: error.to_string(),
        })
}

/// Writes an image conversion (and its responsive variants) under the
/// item's directory: `conversions/{stem}-{name}.{ext}` and
/// `responsive/{stem}-{name}-{width}.{ext}`.
async fn store_image(
    disk: &Disk,
    item: &MediaItem,
    name: &str,
    image: ConvertedImage,
) -> Result<GeneratedConversion, MediaError> {
    let directory = item.directory();
    let stem = stem(&item.file_name);
    let path = format!("{directory}/conversions/{stem}-{name}.{}", image.extension);
    let size = image.bytes.len() as u64;
    disk.put(&path, Bytes::from(image.bytes)).await?;
    let mut variants = Vec::with_capacity(image.variants.len());
    for variant in image.variants {
        let variant_path = format!(
            "{directory}/responsive/{stem}-{name}-{}.{}",
            variant.width, variant.extension
        );
        let variant_size = variant.bytes.len() as u64;
        disk.put(&variant_path, Bytes::from(variant.bytes)).await?;
        variants.push(ResponsiveVariant {
            path: variant_path,
            width: variant.width,
            height: variant.height,
            size: variant_size,
        });
    }
    Ok(GeneratedConversion {
        path,
        mime_type: image.mime_type.to_owned(),
        size,
        width: Some(image.width),
        height: Some(image.height),
        variants,
        generated_at: Utc::now(),
    })
}

/// Every path of a generated conversion.
fn generated_paths(generated: &GeneratedConversion) -> Vec<String> {
    std::iter::once(generated.path.clone())
        .chain(generated.variants.iter().map(|v| v.path.clone()))
        .collect()
}

/// Deletes a file, logging failures other than a missing file.
async fn delete_quietly(disk: &Disk, path: &str) {
    match disk.delete(path).await {
        Ok(()) | Err(StorageError::NotFound { .. }) => {}
        Err(error) => {
            tracing::warn!(target: "navree::media", path, %error, "cannot delete a stale conversion");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_apply_by_type() {
        let thumb = Conversion::new("thumb");
        assert!(applies(&thumb, "image/png"));
        assert!(applies(&thumb, "image/svg+xml"));
        assert!(!applies(&thumb, "image/tiff"));
        assert!(!applies(&thumb, "application/pdf"));
        assert_eq!(applies(&thumb, "video/mp4"), cfg!(feature = "ffmpeg"));
        assert!(!applies(&thumb, "audio/mpeg"));
    }

    #[test]
    fn stems() {
        assert_eq!(stem("cover.png"), "cover");
        assert_eq!(stem("archive.tar.gz"), "archive.tar");
        assert_eq!(stem("README"), "README");
        assert_eq!(stem(".env"), ".env");
    }

    #[cfg(feature = "ffmpeg")]
    #[test]
    fn frames_fall_inside_short_videos() {
        assert_eq!(frame_time(None, Some(10.0)), 1.0);
        assert_eq!(frame_time(None, Some(0.6)), 0.3);
        assert_eq!(frame_time(Some(4.0), None), 4.0);
        assert_eq!(frame_time(Some(12.0), Some(10.0)), 5.0);
    }
}
