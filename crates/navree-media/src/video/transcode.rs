//! [`Transcode`]: how FFmpeg re-encodes a video or audio file.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// How [`Ffmpeg::transcode`](super::Ffmpeg::transcode) (and a
/// [`Conversion::transcode`](crate::Conversion::transcode)) re-encodes
/// video or audio. Start from a preset and adjust it:
///
/// | Preset | Container | Video | Audio |
/// |---|---|---|---|
/// | [`mp4`](Transcode::mp4) | MP4, fast start | H.264, CRF 23, `yuv420p` | AAC 128k |
/// | [`webm`](Transcode::webm) | WebM | VP9, CRF 32 | Opus 96k |
/// | [`mp3`](Transcode::mp3) | MP3 | none | MP3 192k |
/// | [`m4a`](Transcode::m4a) | M4A | none | AAC 128k |
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcode {
    container: String,
    mime_type: String,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    height: Option<u32>,
    crf: Option<u8>,
    preset: Option<String>,
    audio_bitrate: Option<String>,
    max_duration: Option<u32>,
    extra: Vec<String>,
}

impl Transcode {
    fn new(container: &str, mime_type: &str) -> Self {
        Self {
            container: container.to_owned(),
            mime_type: mime_type.to_owned(),
            video_codec: None,
            audio_codec: None,
            height: None,
            crf: None,
            preset: None,
            audio_bitrate: None,
            max_duration: None,
            extra: Vec::new(),
        }
    }

    /// MP4 for every browser: H.264 (CRF 23, preset `medium`, `yuv420p`)
    /// and AAC at 128 kbit/s, with the index at the front for streaming.
    pub fn mp4() -> Self {
        let mut transcode = Self::new("mp4", "video/mp4");
        transcode.video_codec = Some("libx264".into());
        transcode.audio_codec = Some("aac".into());
        transcode.crf = Some(23);
        transcode.preset = Some("medium".into());
        transcode.audio_bitrate = Some("128k".into());
        transcode.extra = vec!["-movflags".into(), "+faststart".into()];
        transcode
    }

    /// WebM: VP9 (CRF 32, constant quality) and Opus at 96 kbit/s.
    pub fn webm() -> Self {
        let mut transcode = Self::new("webm", "video/webm");
        transcode.video_codec = Some("libvpx-vp9".into());
        transcode.audio_codec = Some("libopus".into());
        transcode.crf = Some(32);
        transcode.audio_bitrate = Some("96k".into());
        transcode.extra = vec!["-b:v".into(), "0".into()];
        transcode
    }

    /// MP3 audio at 192 kbit/s (the video is dropped).
    pub fn mp3() -> Self {
        let mut transcode = Self::new("mp3", "audio/mpeg");
        transcode.audio_codec = Some("libmp3lame".into());
        transcode.audio_bitrate = Some("192k".into());
        transcode
    }

    /// AAC audio in an M4A container at 128 kbit/s (the video is
    /// dropped).
    pub fn m4a() -> Self {
        let mut transcode = Self::new("m4a", "audio/mp4");
        transcode.audio_codec = Some("aac".into());
        transcode.audio_bitrate = Some("128k".into());
        transcode.extra = vec!["-movflags".into(), "+faststart".into()];
        transcode
    }

    /// Scales the video to `height` pixels, the width following the
    /// aspect ratio (rounded to an even number).
    pub fn height(mut self, height: u32) -> Self {
        self.height = Some(height);
        self
    }

    /// The constant rate factor: lower is better quality and bigger files.
    pub fn crf(mut self, crf: u8) -> Self {
        self.crf = Some(crf);
        self
    }

    /// The encoder preset (`ultrafast` to `veryslow` for H.264).
    pub fn preset(mut self, preset: impl Into<String>) -> Self {
        self.preset = Some(preset.into());
        self
    }

    /// The audio bit rate (`128k`).
    pub fn audio_bitrate(mut self, bitrate: impl Into<String>) -> Self {
        self.audio_bitrate = Some(bitrate.into());
        self
    }

    /// Keeps only the first `seconds` (previews).
    pub fn max_duration(mut self, seconds: u32) -> Self {
        self.max_duration = Some(seconds);
        self
    }

    /// Drops the audio track.
    pub fn without_audio(mut self) -> Self {
        self.audio_codec = None;
        self.audio_bitrate = None;
        self
    }

    /// Adds raw FFmpeg output arguments (placed before the output file).
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.extra.push(arg.into());
        self
    }

    /// The output type (`video/mp4`).
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }

    /// The output extension, without the dot (`mp4`).
    pub fn extension(&self) -> &str {
        &self.container
    }

    /// Whether the output has a video track.
    pub fn has_video(&self) -> bool {
        self.video_codec.is_some()
    }

    /// The FFmpeg arguments that turn `input` into `output` (`threads` 0
    /// lets FFmpeg choose).
    pub fn args(&self, input: &Path, output: &Path, threads: u32) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "-y".into(),
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-protocol_whitelist".into(),
            "file".into(),
            "-i".into(),
            input.display().to_string(),
        ];
        if let Some(seconds) = self.max_duration {
            args.extend(["-t".into(), seconds.to_string()]);
        }
        match &self.video_codec {
            Some(codec) => {
                args.extend(["-c:v".into(), codec.clone()]);
                if let Some(crf) = self.crf {
                    args.extend(["-crf".into(), crf.to_string()]);
                }
                if let Some(preset) = &self.preset {
                    args.extend(["-preset".into(), preset.clone()]);
                }
                if codec == "libx264" {
                    args.extend(["-pix_fmt".into(), "yuv420p".into()]);
                }
                if let Some(height) = self.height {
                    args.extend(["-vf".into(), format!("scale=-2:{height}")]);
                }
            }
            None => args.push("-vn".into()),
        }
        match &self.audio_codec {
            Some(codec) => {
                args.extend(["-c:a".into(), codec.clone()]);
                if let Some(bitrate) = &self.audio_bitrate {
                    args.extend(["-b:a".into(), bitrate.clone()]);
                }
            }
            None => args.push("-an".into()),
        }
        if threads > 0 {
            args.extend(["-threads".into(), threads.to_string()]);
        }
        args.extend(self.extra.iter().cloned());
        args.push(output.display().to_string());
        args
    }
}
