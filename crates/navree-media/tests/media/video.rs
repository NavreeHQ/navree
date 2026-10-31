//! The FFmpeg sidecar: probe, frames, transcodes, failures, timeouts, the
//! fake, and (when FFmpeg is installed) the real tools.

use std::path::Path;
use std::time::Duration;

use navree_media::MediaError;
use navree_media::video::{
    Ffmpeg, FfmpegFake, FfmpegOptions, FfmpegRunner, ProcessRunner, Transcode,
};
use serde_json::json;

fn has(args: &[String], window: &[&str]) -> bool {
    args.windows(window.len())
        .any(|w| w.iter().zip(window).all(|(a, b)| a == b))
}

#[tokio::test]
async fn probe_reads_ffprobe_json() {
    let fake = FfmpegFake::new().probe_json(json!({
        "format": { "format_name": "mov,mp4,m4a", "duration": "12.480000", "bit_rate": "905000" },
        "streams": [
            { "index": 0, "codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080,
              "r_frame_rate": "30000/1001", "duration": "12.480000" },
            { "index": 1, "codec_type": "audio", "codec_name": "aac", "sample_rate": "48000", "channels": 2 }
        ]
    }));
    let probe = fake.ffmpeg().probe(Path::new("in.mp4")).await.unwrap();
    assert_eq!(probe.duration, Some(12.48));
    assert_eq!(probe.bit_rate, Some(905_000));
    assert_eq!(probe.format_name.as_deref(), Some("mov,mp4,m4a"));
    let video = probe.video().unwrap();
    assert_eq!((video.width, video.height), (Some(1920), Some(1080)));
    assert!((video.frame_rate.unwrap() - 29.97).abs() < 0.01);
    let audio = probe.audio().unwrap();
    assert_eq!(audio.codec_name.as_deref(), Some("aac"));
    assert_eq!((audio.sample_rate, audio.channels), (Some(48_000), Some(2)));
    assert_eq!(probe.dimensions(), Some((1920, 1080)));

    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "ffprobe");
    assert!(has(&calls[0].args, &["-print_format", "json"]));
    assert!(calls[0].args.contains(&"-show_streams".to_owned()));
    assert_eq!(calls[0].args.last().unwrap(), "in.mp4");
    // A crafted file cannot make the tools fetch URLs.
    assert!(has(&calls[0].args, &["-protocol_whitelist", "file"]));
}

#[tokio::test]
async fn rotated_videos_report_their_displayed_size() {
    let fake = FfmpegFake::new().probe_json(json!({
        "format": { "duration": "3.0" },
        "streams": [{ "index": 0, "codec_type": "video", "width": 1920, "height": 1080,
                      "side_data_list": [{ "rotation": -90 }] }]
    }));
    let probe = fake.ffmpeg().probe(Path::new("phone.mov")).await.unwrap();
    assert_eq!(probe.dimensions(), Some((1080, 1920)));
}

#[tokio::test]
async fn frames_and_transcodes_run_ffmpeg_with_the_right_arguments() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let fake = FfmpegFake::new();
    let ffmpeg = fake.ffmpeg();

    let frame = dir.path().join("poster.png");
    ffmpeg
        .frame(Path::new("in.mp4"), 1.5, &frame)
        .await
        .unwrap();
    let png = std::fs::read(&frame).unwrap();
    let img = image::load_from_memory(&png).unwrap();
    assert_eq!(
        (img.width(), img.height()),
        (1280, 720),
        "the fake's default video"
    );

    let out = dir.path().join("out.mp4");
    let mp4 = Transcode::mp4().height(720).crf(26);
    assert_eq!((mp4.mime_type(), mp4.extension()), ("video/mp4", "mp4"));
    ffmpeg
        .transcode(Path::new("in.mov"), &out, &mp4)
        .await
        .unwrap();
    assert!(out.exists());

    let calls = fake.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].program, "ffmpeg");
    assert!(has(
        &calls[0].args,
        &["-ss", "1.5", "-protocol_whitelist", "file", "-i", "in.mp4"]
    ));
    assert!(has(&calls[0].args, &["-frames:v", "1"]));
    let args = &calls[1].args;
    assert!(has(args, &["-protocol_whitelist", "file", "-i", "in.mov"]));
    assert!(has(args, &["-c:v", "libx264"]));
    assert!(has(args, &["-crf", "26"]));
    assert!(has(args, &["-vf", "scale=-2:720"]));
    assert!(has(args, &["-c:a", "aac"]));
    assert!(has(args, &["-movflags", "+faststart"]));
    assert_eq!(args.last().unwrap(), &out.display().to_string());
    fake.assert_ran(|call| call.args.contains(&"libx264".to_owned()));
}

#[test]
fn audio_presets_drop_the_video() {
    let mp3 = Transcode::mp3();
    assert_eq!((mp3.mime_type(), mp3.extension()), ("audio/mpeg", "mp3"));
    let args = mp3.args(Path::new("talk.wav"), Path::new("talk.mp3"), 0);
    assert!(args.contains(&"-vn".to_owned()));
    assert!(has(&args, &["-c:a", "libmp3lame"]));

    let webm = Transcode::webm().height(480);
    assert_eq!(webm.mime_type(), "video/webm");
    let args = webm.args(Path::new("a.mp4"), Path::new("a.webm"), 2);
    assert!(has(&args, &["-c:v", "libvpx-vp9"]));
    assert!(has(&args, &["-threads", "2"]));
}

#[tokio::test]
async fn failures_carry_ffmpegs_message() {
    let fake = FfmpegFake::new().fail("in.mp4: Invalid data found when processing input");
    let err = fake
        .ffmpeg()
        .transcode(Path::new("in.mp4"), Path::new("out.mp4"), &Transcode::mp4())
        .await
        .unwrap_err();
    assert!(matches!(err, MediaError::Ffmpeg { .. }), "{err:?}");
    assert!(err.to_string().contains("Invalid data found"), "{err}");
}

#[tokio::test]
async fn a_missing_binary_is_reported() {
    let ffmpeg = Ffmpeg::new(FfmpegOptions {
        ffprobe: "navree-no-such-ffprobe".into(),
        ..FfmpegOptions::default()
    });
    let err = ffmpeg.probe(Path::new("in.mp4")).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("navree-no-such-ffprobe was not found"),
        "{err}"
    );
}

#[tokio::test]
async fn the_process_runner_kills_a_tool_that_runs_too_long() {
    let (program, args): (&str, Vec<String>) = if cfg!(windows) {
        (
            "powershell",
            vec![
                "-NoProfile".into(),
                "-Command".into(),
                "Start-Sleep -Seconds 20".into(),
            ],
        )
    } else {
        ("sleep", vec!["20".into()])
    };
    let started = std::time::Instant::now();
    let err = ProcessRunner
        .run(program, &args, Duration::from_millis(1500))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("did not finish within"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(15));
}

/// A tool reads files an upload chose, so it must not inherit the server's
/// secrets: `env` lists exactly what a child receives.
#[cfg(unix)]
#[tokio::test]
async fn the_process_runner_passes_only_the_path_to_the_tool() {
    let output = ProcessRunner
        .run("env", &[], Duration::from_secs(30))
        .await
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let names: Vec<&str> = text
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name))
        .collect();
    // `cargo test` itself sets `CARGO_MANIFEST_DIR` and more for this process.
    assert!(
        std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
        "the test process has variables a child could inherit"
    );
    assert_eq!(names, vec!["PATH"], "the child inherited: {names:?}");
}

#[cfg(windows)]
#[tokio::test]
async fn the_process_runner_passes_only_what_the_tool_needs_to_start() {
    let args = vec!["/c".to_owned(), "set".to_owned()];
    let output = ProcessRunner
        .run("cmd", &args, Duration::from_secs(30))
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
    assert!(!text.contains("cargo_manifest_dir"), "{text}");
    assert!(text.contains("systemroot="), "{text}");
}

/// The real tools, when FFmpeg is on the PATH (skipped with a notice
/// otherwise).
#[tokio::test]
async fn real_ffmpeg_when_installed() {
    let ffmpeg = Ffmpeg::new(FfmpegOptions::default());
    if ffmpeg.version().await.is_err() {
        eprintln!("SKIPPED real_ffmpeg_when_installed: ffmpeg is not on the PATH");
        return;
    }
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let source = dir.path().join("source.mp4");
    let args: Vec<String> = [
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=duration=2:size=320x240:rate=10",
        "-pix_fmt",
        "yuv420p",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain([source.display().to_string()])
    .collect();
    ProcessRunner
        .run("ffmpeg", &args, Duration::from_secs(60))
        .await
        .unwrap();
    let probe = ffmpeg.probe(&source).await.unwrap();
    assert_eq!(probe.dimensions(), Some((320, 240)));
    let poster = dir.path().join("poster.png");
    ffmpeg.frame(&source, 1.0, &poster).await.unwrap();
    assert!(image::open(&poster).is_ok());
}
