//! `MediaFake`: the real pipeline on memory, recorded, with fake HTTP and
//! FFmpeg; and video and audio conversions through the FFmpeg fake.

use http::Method;
use image::ImageFormat;
use navree_http_client::FakeResponse;
use navree_media::video::Transcode;
use navree_media::{
    Conversion, Fit, HasMedia, MediaCollections, MediaFake, MediaSource, OutputFormat,
};
use serde_json::json;

use crate::models::Course;
use crate::support::image_bytes;

/// An MP4 header `infer` recognizes (`ftyp` box, brand `isom`).
fn mp4_bytes() -> Vec<u8> {
    let mut bytes = vec![0, 0, 0, 0x18];
    bytes.extend_from_slice(b"ftypisom");
    bytes.extend_from_slice(&[0, 0, 2, 0]);
    bytes.extend_from_slice(b"isomiso2");
    bytes.extend_from_slice(&[0u8; 64]);
    bytes
}

/// An MP3 with an ID3 tag.
fn mp3_bytes() -> Vec<u8> {
    let mut bytes = b"ID3\x04\x00\x00\x00\x00\x00\x0a".to_vec();
    bytes.extend_from_slice(&[0u8; 10]);
    bytes.extend_from_slice(&[0xff, 0xfb, 0x90, 0x64]);
    bytes.extend_from_slice(&[0u8; 64]);
    bytes
}

/// A lesson with videos (a poster frame, an MP4 rendition) and recordings
/// (an M4A rendition).
struct Lesson;

impl HasMedia for Lesson {
    const MEDIA_TYPE: &'static str = "Lesson";

    fn media_key(&self) -> String {
        "lesson-1".into()
    }

    fn media_collections(collections: &mut MediaCollections) {
        collections
            .collection("videos")
            .accepts(["video/*"])
            .conversion(
                Conversion::new("poster")
                    .fit(Fit::Cover, 320, 180)
                    .format(OutputFormat::Jpeg)
                    .frame_at(2.5),
            )
            .conversion(Conversion::new("web").transcode(Transcode::mp4().height(480)));
        collections
            .collection("recordings")
            .accepts(["audio/*"])
            .conversion(Conversion::new("web").transcode(Transcode::m4a()))
            .conversion(Conversion::new("cover").width(100));
    }
}

#[tokio::test]
async fn the_fake_records_adds_and_deletes() {
    let fake = MediaFake::new().register::<Course>();
    let media = fake.media();
    fake.assert_nothing_added();
    let item = media
        .add(
            &Course { id: 3 },
            "thumbnail",
            MediaSource::bytes("c.png", image_bytes(200, 100, ImageFormat::Png)),
        )
        .save(None)
        .await
        .unwrap();
    // Conversions are generated at once.
    assert_eq!(item.conversions["card"].width, Some(64));
    fake.assert_added("Course", "thumbnail");
    fake.assert_added_with(|added| added.id == item.id && added.width == Some(200));
    fake.assert_added_count(1);
    assert!(
        fake.storage()
            .default_disk()
            .exists(&item.path)
            .await
            .unwrap()
    );
    assert!(fake.deleted().is_empty());

    media.delete(None, item.id).await.unwrap();
    fake.assert_deleted(item.id);
    assert!(
        fake.storage()
            .default_disk()
            .list("media")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
#[should_panic(expected = "no added media matched")]
async fn assert_added_fails_without_a_match() {
    MediaFake::new().assert_added("Course", "thumbnail");
}

#[tokio::test]
async fn remote_files_come_from_the_fake_server() {
    let fake = MediaFake::new();
    fake.http().on(
        Method::GET,
        "https://cdn.example.com/*",
        FakeResponse::new(200).body(image_bytes(40, 40, ImageFormat::Png)),
    );
    let item = fake
        .media()
        .add_from_url(
            &Course { id: 1 },
            "gallery",
            "https://cdn.example.com/a/team.png",
        )
        .save(None)
        .await
        .unwrap();
    assert_eq!(item.file_name, "team.png");
    assert_eq!(item.width, Some(40));
    fake.http()
        .assert_sent(|request| request.url().as_str() == "https://cdn.example.com/a/team.png");

    let error = fake
        .media()
        .add_from_url(
            &Course { id: 1 },
            "gallery",
            "https://elsewhere.example.com/x.png",
        )
        .save(None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "remote_file_unavailable", "{error}");
}

#[tokio::test]
async fn videos_are_probed_framed_and_transcoded() {
    let fake = MediaFake::new();
    let item = fake
        .media()
        .add(
            &Lesson,
            "videos",
            MediaSource::bytes("intro.mp4", mp4_bytes()),
        )
        .save(None)
        .await
        .unwrap();
    assert_eq!(item.mime_type, "video/mp4");
    // The fake's default probe: 10 s of 1280x720 H.264 with AAC.
    assert_eq!((item.width, item.height), (Some(1280), Some(720)));
    assert_eq!(item.metadata["duration"], json!(10.0));
    assert_eq!(item.metadata["video_codec"], json!("h264"));
    assert_eq!(item.metadata["audio_codec"], json!("aac"));

    let poster = &item.conversions["poster"];
    assert_eq!((poster.width, poster.height), (Some(320), Some(180)));
    assert_eq!(poster.mime_type, "image/jpeg");
    assert_eq!(
        poster.path,
        format!("media/{}/conversions/intro-poster.jpg", item.id)
    );
    let web = &item.conversions["web"];
    assert_eq!(web.mime_type, "video/mp4");
    assert_eq!(
        web.path,
        format!("media/{}/conversions/intro-web.mp4", item.id)
    );
    assert_eq!((web.width, web.height), (Some(1280), Some(720)));
    assert!(item.is_video());

    let ffmpeg = fake.ffmpeg();
    ffmpeg.assert_ran(|call| {
        call.program == "ffmpeg" && call.args.windows(2).any(|w| w[0] == "-ss" && w[1] == "2.5")
    });
    ffmpeg.assert_ran(|call| call.args.iter().any(|arg| arg == "scale=-2:480"));
}

#[tokio::test]
async fn frames_of_short_videos_come_from_the_middle() {
    let fake = MediaFake::new();
    fake.ffmpeg().clone().probe_json(json!({
        "format": { "duration": "2.0" },
        "streams": [{ "index": 0, "codec_type": "video", "codec_name": "vp9", "width": 640, "height": 360 }]
    }));
    fake.media()
        .add(
            &Lesson,
            "videos",
            MediaSource::bytes("clip.mp4", mp4_bytes()),
        )
        .save(None)
        .await
        .unwrap();
    fake.ffmpeg()
        .assert_ran(|call| call.args.windows(2).any(|w| w[0] == "-ss" && w[1] == "1"));
}

#[tokio::test]
async fn audio_gets_transcodes_and_no_image_conversions() {
    let fake = MediaFake::new();
    let item = fake
        .media()
        .add(
            &Lesson,
            "recordings",
            MediaSource::bytes("talk.mp3", mp3_bytes()),
        )
        .save(None)
        .await
        .unwrap();
    assert_eq!(item.mime_type, "audio/mpeg");
    assert!(item.is_audio());
    assert_eq!(item.conversions["web"].mime_type, "audio/mp4");
    assert!(!item.conversions.contains_key("cover"));
}

#[tokio::test]
async fn a_failing_conversion_keeps_nothing() {
    let fake = MediaFake::new();
    fake.ffmpeg().clone().fail("ffmpeg was not found");
    let error = fake
        .media()
        .add(
            &Lesson,
            "videos",
            MediaSource::bytes("intro.mp4", mp4_bytes()),
        )
        .save(None)
        .await
        .unwrap_err();
    // The probe failure is only logged; the poster frame, generated at
    // once without a queue, fails the add.
    assert!(
        matches!(error, navree_media::MediaError::Ffmpeg { .. }),
        "{error}"
    );
    assert_eq!(error.status(), 500);
    assert!(
        fake.storage()
            .default_disk()
            .list("media")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        fake.media()
            .list(&Default::default())
            .await
            .unwrap()
            .is_empty()
    );
    fake.assert_nothing_added();
}
