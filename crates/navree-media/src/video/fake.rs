//! [`FfmpegFake`]: FFmpeg for tests, without FFmpeg.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use futures_util::future::BoxFuture;
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use serde_json::{Value, json};

use super::{Ffmpeg, FfmpegOptions, FfmpegOutput, FfmpegRunner};
use crate::MediaError;

/// One run the fake answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FfmpegCall {
    /// `ffmpeg` or `ffprobe` (as configured).
    pub program: String,
    /// The arguments.
    pub args: Vec<String>,
}

#[derive(Default)]
struct State {
    probe: Option<Value>,
    failure: Option<String>,
    calls: Vec<FfmpegCall>,
}

/// An [`FfmpegRunner`] that never starts a process. `ffprobe` answers
/// with the configured JSON (by default a 10-second 1280x720 H.264 video
/// with AAC audio); `ffmpeg` writes its output file: a frame of the
/// probed size for `.png` and `.jpg` outputs, placeholder bytes
/// otherwise. Every run is recorded.
///
/// ```
/// use std::path::Path;
/// use navree_media::video::FfmpegFake;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_media::MediaError> {
/// let fake = FfmpegFake::new();
/// let probe = fake.ffmpeg().probe(Path::new("intro.mp4")).await?;
/// assert_eq!(probe.dimensions(), Some((1280, 720)));
/// fake.assert_ran(|call| call.program == "ffprobe");
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Default)]
pub struct FfmpegFake {
    state: Arc<Mutex<State>>,
}

impl std::fmt::Debug for FfmpegFake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FfmpegFake")
            .field("calls", &self.lock().calls)
            .finish()
    }
}

impl FfmpegFake {
    /// A fake with the default probe.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Answers `ffprobe` with `json` (its `-print_format json` shape).
    pub fn probe_json(self, json: Value) -> Self {
        self.lock().probe = Some(json);
        self
    }

    /// Fails every run with `message` as FFmpeg's error output.
    pub fn fail(self, message: impl Into<String>) -> Self {
        self.lock().failure = Some(message.into());
        self
    }

    /// The runner, for [`Ffmpeg::with_runner`] or
    /// `MediaFake`/`override_value(Arc<dyn FfmpegRunner>)`.
    pub fn runner(&self) -> Arc<dyn FfmpegRunner> {
        Arc::new(self.clone())
    }

    /// An [`Ffmpeg`] on this fake with the default options.
    pub fn ffmpeg(&self) -> Ffmpeg {
        Ffmpeg::with_runner(FfmpegOptions::default(), self.runner())
    }

    /// Every run, in order.
    pub fn calls(&self) -> Vec<FfmpegCall> {
        self.lock().calls.clone()
    }

    /// Panics unless a run matches `predicate`.
    #[track_caller]
    pub fn assert_ran(&self, predicate: impl Fn(&FfmpegCall) -> bool) {
        let calls = self.calls();
        assert!(
            calls.iter().any(predicate),
            "no matching ffmpeg run; runs: {calls:#?}"
        );
    }

    /// Panics if anything ran.
    #[track_caller]
    pub fn assert_nothing_ran(&self) {
        let calls = self.calls();
        assert!(
            calls.is_empty(),
            "expected no ffmpeg runs; runs: {calls:#?}"
        );
    }

    fn probe_value(&self) -> Value {
        self.lock().probe.clone().unwrap_or_else(|| {
            json!({
                "format": { "format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "10.000000", "bit_rate": "1200000" },
                "streams": [
                    { "index": 0, "codec_type": "video", "codec_name": "h264", "width": 1280, "height": 720,
                      "r_frame_rate": "30/1", "duration": "10.000000" },
                    { "index": 1, "codec_type": "audio", "codec_name": "aac", "sample_rate": "48000",
                      "channels": 2, "duration": "10.000000" }
                ]
            })
        })
    }

    async fn answer(&self, program: &str, args: &[String]) -> Result<FfmpegOutput, MediaError> {
        let failure = {
            let mut state = self.lock();
            state.calls.push(FfmpegCall {
                program: program.to_owned(),
                args: args.to_vec(),
            });
            state.failure.clone()
        };
        if let Some(message) = failure {
            return Err(MediaError::Ffmpeg {
                message: format!("{program} failed (exit code: 1): {message}"),
            });
        }
        if args.iter().any(|arg| arg == "-print_format") {
            return Ok(FfmpegOutput {
                stdout: serde_json::to_vec(&self.probe_value()).unwrap_or_default(),
                stderr: Vec::new(),
            });
        }
        if args.iter().any(|arg| arg == "-version") {
            return Ok(FfmpegOutput {
                stdout: b"ffmpeg version navree-fake".to_vec(),
                stderr: Vec::new(),
            });
        }
        if let Some(output) = args.last() {
            let path = Path::new(output);
            let bytes = match path.extension().and_then(|e| e.to_str()) {
                Some("png") => self.frame(ImageFormat::Png),
                Some("jpg" | "jpeg") => self.frame(ImageFormat::Jpeg),
                _ => b"navree fake media".to_vec(),
            };
            tokio::fs::write(path, bytes)
                .await
                .map_err(|error| MediaError::Ffmpeg {
                    message: format!("{program}: {error}"),
                })?;
        }
        Ok(FfmpegOutput::default())
    }

    /// A grey frame of the probed video's size.
    fn frame(&self, format: ImageFormat) -> Vec<u8> {
        let probe =
            super::Probe::from_json(&serde_json::to_vec(&self.probe_value()).unwrap_or_default())
                .unwrap_or_default();
        let (width, height) = probe.dimensions().unwrap_or((1280, 720));
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb([90, 90, 90])));
        let mut out = std::io::Cursor::new(Vec::new());
        let _ = img.write_to(&mut out, format);
        out.into_inner()
    }
}

impl FfmpegRunner for FfmpegFake {
    fn run<'a>(
        &'a self,
        program: &'a str,
        args: &'a [String],
        _timeout: Duration,
    ) -> BoxFuture<'a, Result<FfmpegOutput, MediaError>> {
        Box::pin(self.answer(program, args))
    }
}
