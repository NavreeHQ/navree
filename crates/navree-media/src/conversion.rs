//! [`Conversion`]: a derived file generated from an original (a thumbnail,
//! a WebP copy, a poster frame, a transcoded video).

use serde::{Deserialize, Serialize};

#[cfg(feature = "ffmpeg")]
use crate::video::Transcode;

/// How a resized image fills the box given to [`Conversion::fit`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Fits inside the box, keeping the aspect ratio; one side may be
    /// shorter than the box (the default).
    #[default]
    Contain,
    /// Fills the box exactly, keeping the aspect ratio; the overflow is
    /// cropped evenly from both sides.
    Cover,
    /// Stretches to the box exactly, ignoring the aspect ratio.
    Fill,
}

/// The encoding of a converted image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    /// JPEG at the conversion's quality; transparency is flattened onto
    /// white.
    Jpeg,
    /// PNG.
    Png,
    /// WebP (lossless).
    #[serde(rename = "webp")]
    WebP,
}

impl OutputFormat {
    /// The MIME type (`image/webp`).
    pub fn mime_type(self) -> &'static str {
        match self {
            OutputFormat::Jpeg => "image/jpeg",
            OutputFormat::Png => "image/png",
            OutputFormat::WebP => "image/webp",
        }
    }

    /// The file extension, without the dot (`webp`).
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Jpeg => "jpg",
            OutputFormat::Png => "png",
            OutputFormat::WebP => "webp",
        }
    }

    /// The format an image of type `mime_type` keeps when a conversion
    /// names none: JPEG, PNG and WebP stay as they are, everything else
    /// (GIF, BMP, SVG, video frames) becomes PNG.
    pub fn keep(mime_type: &str) -> OutputFormat {
        match mime_type {
            "image/jpeg" | "image/jpg" => OutputFormat::Jpeg,
            "image/webp" => OutputFormat::WebP,
            _ => OutputFormat::Png,
        }
    }
}

/// The default JPEG quality.
pub const DEFAULT_QUALITY: u8 = 82;

/// A derived file generated from an original: an image resized, cropped,
/// re-encoded or filtered, a frame grabbed from a video (with the `ffmpeg`
/// feature), or a transcoded video or audio file.
///
/// Declared on a collection
/// ([`MediaCollection::conversion`](crate::MediaCollection::conversion))
/// and generated when a file is added (queued unless
/// [`nonqueued`](Conversion::nonqueued) or `media.queue_conversions =
/// false`), or with `media:regenerate`.
///
/// ```
/// use navree_media::{Conversion, Fit, OutputFormat};
///
/// let thumb = Conversion::new("thumb")
///     .fit(Fit::Cover, 320, 180)
///     .format(OutputFormat::WebP);
/// let hero = Conversion::new("hero").width(1920).responsive();
/// # let _ = (thumb, hero);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Conversion {
    pub(crate) name: String,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) fit: Fit,
    pub(crate) upscale: bool,
    pub(crate) format: Option<OutputFormat>,
    pub(crate) quality: u8,
    pub(crate) greyscale: bool,
    pub(crate) blur: Option<f32>,
    pub(crate) responsive: bool,
    pub(crate) queued: Option<bool>,
    pub(crate) frame_at: Option<f64>,
    #[cfg(feature = "ffmpeg")]
    pub(crate) transcode: Option<Transcode>,
}

impl Conversion {
    /// A conversion named `name` that re-encodes the image as it is
    /// (orientation applied, metadata dropped) until sizes, a format or
    /// filters are added. Names are lower-case letters, digits, `-` and
    /// `_`; they name the generated file.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            width: None,
            height: None,
            fit: Fit::Contain,
            upscale: false,
            format: None,
            quality: DEFAULT_QUALITY,
            greyscale: false,
            blur: None,
            responsive: false,
            queued: None,
            frame_at: None,
            #[cfg(feature = "ffmpeg")]
            transcode: None,
        }
    }

    /// The name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// At most `width` pixels wide; the height follows the aspect ratio
    /// unless [`height`](Conversion::height) is set too.
    pub fn width(mut self, width: u32) -> Self {
        self.width = Some(width);
        self
    }

    /// At most `height` pixels high; the width follows the aspect ratio
    /// unless [`width`](Conversion::width) is set too.
    pub fn height(mut self, height: u32) -> Self {
        self.height = Some(height);
        self
    }

    /// A `width` x `height` box filled as `fit` says.
    pub fn fit(mut self, fit: Fit, width: u32, height: u32) -> Self {
        self.fit = fit;
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Lets the image grow beyond its own size to reach the box (by
    /// default smaller images keep their size). SVGs always scale.
    pub fn upscale(mut self) -> Self {
        self.upscale = true;
        self
    }

    /// The output encoding (by default JPEG, PNG and WebP keep theirs and
    /// other images become PNG).
    pub fn format(mut self, format: OutputFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// The JPEG quality, 1 to 100 (default 82).
    pub fn quality(mut self, quality: u8) -> Self {
        self.quality = quality.clamp(1, 100);
        self
    }

    /// Converts to shades of grey.
    pub fn greyscale(mut self) -> Self {
        self.greyscale = true;
        self
    }

    /// Applies a Gaussian blur of `sigma` pixels.
    pub fn blur(mut self, sigma: f32) -> Self {
        self.blur = Some(sigma);
        self
    }

    /// Also generates narrower copies for `srcset`, one for each width of
    /// `media.images.responsive_widths` below the conversion's own width.
    pub fn responsive(mut self) -> Self {
        self.responsive = true;
        self
    }

    /// Generates this conversion while the file is added, inside the
    /// request, even when `media.queue_conversions` is on.
    pub fn nonqueued(mut self) -> Self {
        self.queued = Some(false);
        self
    }

    /// Generates this conversion on the queue, even when
    /// `media.queue_conversions` is off.
    pub fn queued(mut self) -> Self {
        self.queued = Some(true);
        self
    }

    /// For video originals (the `ffmpeg` feature): the frame at
    /// `seconds` is the image the conversion starts from (default 1 s, or
    /// the middle of shorter videos).
    pub fn frame_at(mut self, seconds: f64) -> Self {
        self.frame_at = Some(seconds.max(0.0));
        self
    }

    /// Transcodes video or audio originals with FFmpeg instead of
    /// converting an image (the `ffmpeg` feature).
    #[cfg(feature = "ffmpeg")]
    pub fn transcode(mut self, transcode: Transcode) -> Self {
        self.transcode = Some(transcode);
        self
    }

    /// Whether this conversion transcodes video or audio.
    pub fn is_transcode(&self) -> bool {
        #[cfg(feature = "ffmpeg")]
        {
            self.transcode.is_some()
        }
        #[cfg(not(feature = "ffmpeg"))]
        {
            false
        }
    }

    /// Whether the conversion runs on the queue when the module's default
    /// is `queue_by_default`.
    pub(crate) fn runs_queued(&self, queue_by_default: bool) -> bool {
        self.queued.unwrap_or(queue_by_default)
    }

    /// Checks the name: lower-case letters, digits, `-` and `_`.
    pub(crate) fn check(&self) -> Result<(), String> {
        let valid = !self.name.is_empty()
            && self.name.len() <= 64
            && self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
        if valid {
            Ok(())
        } else {
            Err(format!(
                "conversion name `{}` must be 1 to 64 lower-case letters, digits, `-` or `_`",
                self.name
            ))
        }
    }
}
