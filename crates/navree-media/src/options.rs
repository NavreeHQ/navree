//! [`MediaOptions`]: configuration section `media`.

use navree_config::ConfigSection;
use serde::{Deserialize, Serialize};

use crate::archive::{ArchiveLimits, safe_path};
use crate::images::ImageOptions;
#[cfg(feature = "ffmpeg")]
use crate::video::FfmpegOptions;

/// Where media rows are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreKind {
    /// The `media` table of the app database (the default; needs
    /// `DatabaseModule`).
    #[default]
    Database,
    /// Process memory, lost on restart: tests and prototypes.
    Memory,
}

/// The media module's settings (configuration section `media`, see
/// [`MediaModule::from_config`](crate::MediaModule::from_config)).
///
/// ```toml
/// [media]
/// disk = "public"              # the storage disk for originals
/// conversions_disk = "public"  # and for conversions (default: the same)
/// prefix = "media"             # files go to {prefix}/{id}/
/// max_file_size = 26214400     # 25 MiB
/// queue_conversions = true
/// queue = "media"
/// strip_metadata = true
/// delete_with_model = true
/// url_ttl_secs = 3600
/// download_timeout_secs = 30
///
/// [media.images]
/// max_pixels = 50000000
/// responsive_widths = [320, 640, 960, 1280, 1920, 2560]
///
/// [media.archives]
/// max_entries = 10000
/// max_entry_bytes = 536870912
/// max_total_bytes = 2147483648
/// max_ratio = 200
/// ```
///
/// Every key can come from the environment: `APP_MEDIA__DISK`,
/// `APP_MEDIA__IMAGES__RESPONSIVE_WIDTHS=480,960`,
/// `APP_MEDIA__FFMPEG__FFMPEG=/usr/bin/ffmpeg`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaOptions {
    /// Where media rows are kept: `database` (default) or `memory`.
    pub store: StoreKind,
    /// The storage disk originals are written to (default: the storage
    /// module's default disk). A collection can name its own.
    pub disk: Option<String>,
    /// The disk conversions are written to (default: the original's).
    pub conversions_disk: Option<String>,
    /// The directory on the disk: an item's files live under
    /// `{prefix}/{id}/` (default `media`).
    pub prefix: String,
    /// The largest file accepted, in bytes (default 25 MiB). A collection
    /// can lower it.
    pub max_file_size: u64,
    /// Generate conversions on the queue (default `true`). Without a
    /// queue in the app they run while the file is added.
    pub queue_conversions: bool,
    /// The queue conversion jobs go to (default `default`).
    pub queue: String,
    /// Remove EXIF, XMP, IPTC and text metadata from JPEG, PNG and WebP
    /// originals before they are stored (default `true`).
    pub strip_metadata: bool,
    /// Delete a model's media when the model is deleted through `Repo`
    /// (for models with a `MEDIA_TABLE`; default `true`).
    pub delete_with_model: bool,
    /// How long the temporary URLs of [`Media::resource`](crate::Media::resource)
    /// last, in seconds, for disks without a public URL (default 3600).
    pub url_ttl_secs: u64,
    /// How long a remote download may take (default 30 s).
    pub download_timeout_secs: u64,
    /// Image settings.
    pub images: ImageOptions,
    /// Archive limits.
    pub archives: ArchiveLimits,
    /// The FFmpeg sidecar (the `ffmpeg` feature).
    #[cfg(feature = "ffmpeg")]
    pub ffmpeg: FfmpegOptions,
}

impl Default for MediaOptions {
    fn default() -> Self {
        Self {
            store: StoreKind::Database,
            disk: None,
            conversions_disk: None,
            prefix: "media".to_owned(),
            max_file_size: 25 * 1024 * 1024,
            queue_conversions: true,
            queue: "default".to_owned(),
            strip_metadata: true,
            delete_with_model: true,
            url_ttl_secs: 3600,
            download_timeout_secs: 30,
            images: ImageOptions::default(),
            archives: ArchiveLimits::default(),
            #[cfg(feature = "ffmpeg")]
            ffmpeg: FfmpegOptions::default(),
        }
    }
}

impl MediaOptions {
    /// Checks the options; the error names the key.
    pub fn check(&self) -> Result<(), String> {
        if safe_path(&self.prefix).is_none() {
            return Err(format!(
                "`media.prefix` must be a relative path without `..`, not {:?}",
                self.prefix
            ));
        }
        if self.max_file_size == 0 {
            return Err("`media.max_file_size` must be at least 1".to_owned());
        }
        if self.queue.trim().is_empty() {
            return Err("`media.queue` must not be empty".to_owned());
        }
        if self.url_ttl_secs == 0 {
            return Err("`media.url_ttl_secs` must be at least 1".to_owned());
        }
        if self.download_timeout_secs == 0 {
            return Err("`media.download_timeout_secs` must be at least 1".to_owned());
        }
        if self.images.max_pixels == 0 {
            return Err("`media.images.max_pixels` must be at least 1".to_owned());
        }
        if self.images.responsive_widths.contains(&0) {
            return Err("`media.images.responsive_widths` must not contain 0".to_owned());
        }
        #[cfg(feature = "ffmpeg")]
        if self.ffmpeg.timeout_secs == 0 {
            return Err("`media.ffmpeg.timeout_secs` must be at least 1".to_owned());
        }
        Ok(())
    }

    /// The prefix without surrounding slashes.
    pub(crate) fn prefix(&self) -> String {
        safe_path(&self.prefix).unwrap_or_else(|| "media".to_owned())
    }
}

impl ConfigSection for MediaOptions {
    const KEY: &'static str = "media";

    fn validate(&self) -> Result<(), String> {
        self.check()
    }
}
