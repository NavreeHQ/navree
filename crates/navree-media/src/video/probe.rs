//! [`Probe`]: what ffprobe reports about a file.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::MediaError;

/// The container and streams of a video or audio file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    /// The duration in seconds.
    pub duration: Option<f64>,
    /// The container (`mov,mp4,m4a,3gp,3g2,mj2`).
    pub format_name: Option<String>,
    /// The overall bit rate in bits per second.
    pub bit_rate: Option<u64>,
    /// Every stream, in file order.
    pub streams: Vec<ProbeStream>,
}

/// One stream of a [`Probe`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProbeStream {
    /// The stream index.
    pub index: u32,
    /// `video`, `audio`, `subtitle` or `data`.
    pub codec_type: String,
    /// The codec (`h264`, `aac`).
    pub codec_name: Option<String>,
    /// The coded width in pixels (video).
    pub width: Option<u32>,
    /// The coded height in pixels (video).
    pub height: Option<u32>,
    /// Frames per second (video).
    pub frame_rate: Option<f64>,
    /// The rotation in degrees the player applies (video from phones).
    pub rotation: Option<i32>,
    /// Samples per second (audio).
    pub sample_rate: Option<u32>,
    /// Channels (audio).
    pub channels: Option<u32>,
    /// The stream's own duration in seconds.
    pub duration: Option<f64>,
}

impl Probe {
    /// Parses `ffprobe -print_format json -show_format -show_streams`.
    pub fn from_json(json: &[u8]) -> Result<Probe, MediaError> {
        let value: Value = serde_json::from_slice(json).map_err(|error| MediaError::Ffmpeg {
            message: format!("unreadable ffprobe output: {error}"),
        })?;
        let format = &value["format"];
        let streams = value["streams"]
            .as_array()
            .map(|streams| streams.iter().map(stream).collect())
            .unwrap_or_default();
        Ok(Probe {
            duration: number(&format["duration"]),
            format_name: format["format_name"].as_str().map(str::to_owned),
            bit_rate: number(&format["bit_rate"]).map(|rate| rate as u64),
            streams,
        })
    }

    /// The first video stream.
    pub fn video(&self) -> Option<&ProbeStream> {
        self.streams
            .iter()
            .find(|stream| stream.codec_type == "video")
    }

    /// The first audio stream.
    pub fn audio(&self) -> Option<&ProbeStream> {
        self.streams
            .iter()
            .find(|stream| stream.codec_type == "audio")
    }

    /// The displayed width and height of the first video stream (swapped
    /// when the stream is rotated a quarter turn).
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        let video = self.video()?;
        let (width, height) = (video.width?, video.height?);
        match video.rotation.map(|r| r.rem_euclid(180)) {
            Some(90) => Some((height, width)),
            _ => Some((width, height)),
        }
    }
}

fn stream(value: &Value) -> ProbeStream {
    let rotation = value["side_data_list"]
        .as_array()
        .and_then(|list| list.iter().find_map(|item| item["rotation"].as_f64()))
        .or_else(|| number(&value["tags"]["rotate"]))
        .map(|rotation| rotation as i32);
    ProbeStream {
        index: value["index"].as_u64().unwrap_or_default() as u32,
        codec_type: value["codec_type"].as_str().unwrap_or("data").to_owned(),
        codec_name: value["codec_name"].as_str().map(str::to_owned),
        width: value["width"].as_u64().map(|w| w as u32),
        height: value["height"].as_u64().map(|h| h as u32),
        frame_rate: value["r_frame_rate"].as_str().and_then(ratio),
        rotation,
        sample_rate: number(&value["sample_rate"]).map(|rate| rate as u32),
        channels: value["channels"].as_u64().map(|c| c as u32),
        duration: number(&value["duration"]),
    }
}

/// A number ffprobe prints as a string (`"12.48"`) or a number.
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// `30000/1001` as frames per second.
fn ratio(text: &str) -> Option<f64> {
    let (numerator, denominator) = text.split_once('/')?;
    let numerator: f64 = numerator.parse().ok()?;
    let denominator: f64 = denominator.parse().ok()?;
    (denominator > 0.0 && numerator > 0.0).then(|| numerator / denominator)
}
