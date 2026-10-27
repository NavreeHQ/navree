//! Every public item the media docs page names
//! (`apps/website/content/docs/packages/media.mdx`): types, methods,
//! configuration keys, the command, the job, the table and the error
//! codes. A rename fails this file.

use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use navree_db::Uow;
use navree_kernel::ModuleBuilder;
use navree_media::archive::{Archive, ArchiveEntry, ArchiveLimits, ExtractReport, ZipBuilder};
use navree_media::images::{self, ConvertedImage, ImageInfo, ImageOptions};
use navree_media::text::{self, ExtractedText};
use navree_media::video::{
    Ffmpeg, FfmpegCall, FfmpegFake, FfmpegOptions, FfmpegRunner, Probe, ProbeStream, ProcessRunner,
    Transcode,
};
use navree_media::{
    ArchiveError, Conversion, DEFAULT_QUALITY, DatabaseStore, Fit, GenerateConversions,
    GeneratedConversion, HasMedia, MEDIA_MIGRATIONS, MEDIA_TABLE, Media, MediaBuilder,
    MediaCollection, MediaCollections, MediaError, MediaFake, MediaItem, MediaModule, MediaOptions,
    MediaQuery, MediaRecord, MediaResource, MediaSource, MediaStore, MemoryStore,
    ModuleBuilderMediaExt, OutputFormat, PendingMedia, Regenerate, RegenerateReport,
    ResponsiveVariant, StoreKind,
};
use navree_storage::Disk;
use serde_json::json;
use uuid::Uuid;

struct Course;

impl HasMedia for Course {
    const MEDIA_TYPE: &'static str = "Course";
    const MEDIA_TABLE: &'static str = "courses";

    fn media_key(&self) -> String {
        "1".into()
    }

    fn media_collections(collections: &mut MediaCollections) {
        let collection: &mut MediaCollection = collections
            .collection("thumbnail")
            .single_file()
            .only_keep_latest(1)
            .accepts(["image/*"])
            .max_size(1024)
            .disk("public")
            .conversions_disk("public")
            .conversion(Conversion::new("card"));
        let _: &str = collection.name();
        let _: &[Conversion] = collection.conversions();
        let _: Option<&Conversion> = collection.conversion_named("card");
        let _: bool = collection.accepts_type("image/png");
    }
}

/// The `Media` methods the page names (compiled, never run).
#[allow(dead_code)]
async fn media_methods(media: &Media, uow: &Uow, item: MediaItem) -> Result<(), MediaError> {
    let course = Course;
    let pending: PendingMedia = media
        .add(
            &course,
            "thumbnail",
            MediaSource::bytes("a.png", Bytes::new()),
        )
        .name("A")
        .file_name("a.png")
        .property("alt", "x");
    let _: MediaItem = pending.save(uow).await?;
    let _: MediaItem = media
        .add_from_url(&course, "thumbnail", "https://cdn.example.com/a.png")
        .save(None)
        .await?;
    let _ = MediaSource::url("https://cdn.example.com/a.png");
    let _ = MediaSource::disk("uploads", "tmp/a.png");
    let _ = MediaSource::file("/tmp/a.png");
    let _: Vec<MediaItem> = media.get(&course, "thumbnail").await?;
    let _: Option<MediaItem> = media.first(&course, "thumbnail").await?;
    let _: Vec<MediaItem> = media.all(&course).await?;
    let _: Option<MediaItem> = media.find(item.id).await?;
    let _: MediaItem = media.find_or_fail(item.id).await?;
    let _: Vec<MediaItem> = media
        .list(&MediaQuery {
            model_type: Some("Course".into()),
            model_id: None,
            collection: None,
            ids: None,
            after: None,
            limit: Some(10),
        })
        .await?;
    let _ = MediaQuery::owner("Course", "1").collection("thumbnail");
    let _: MediaResource = media.resource(&item).await?;
    let _: String = media.url(&item)?;
    let _: String = media.conversion_url(&item, "card")?;
    let _: String = media.srcset(&item, "card")?;
    let _: String = media.temporary_url(&item, Utc::now()).await?;
    let _: String = media
        .temporary_conversion_url(&item, "card", Utc::now())
        .await?;
    let _: Bytes = media.read(&item).await?;
    let _: Bytes = media.read_conversion(&item, "card").await?;
    let _: MediaItem = media.save(uow, &item).await?;
    media.reorder(uow, &[item.id]).await?;
    let _: bool = media.delete(uow, item.id).await?;
    let _: u64 = media.clear(uow, &course, "thumbnail").await?;
    let _: u64 = media.delete_all(uow, &course).await?;
    let _: MediaItem = media.convert(item.id, Some(&["card".to_owned()])).await?;
    let report: RegenerateReport = media.regenerate(&Regenerate::default()).await?;
    let _: (u64, u64, Vec<(Uuid, String)>) = (report.media, report.conversions, report.failed);
    let _: Option<navree_queue::JobBatch> = media.queue_regenerate(&Regenerate::default()).await?;
    let _: ExtractedText = media.extract_text(&item).await?;
    let _: Archive = media.open_archive(&item).await?;
    let _: ExtractReport = media.unpack(&item, "public", "scorm/1").await?;
    let _: Bytes = media.zip(std::slice::from_ref(&item)).await?;
    let _: Arc<MediaCollections> = media.register::<Course>()?;
    let _: &MediaOptions = media.options();
    let _ = media.storage();
    let _: &Arc<dyn MediaStore> = media.store();
    let _: &Ffmpeg = media.ffmpeg();
    Ok(())
}

/// What a `MediaItem` carries (compiled, never run).
#[allow(dead_code)]
fn item_fields(item: &MediaItem) {
    let _: (Uuid, &str, &str, &str, &str, &str, &str) = (
        item.id,
        &item.model_type,
        &item.model_id,
        &item.collection,
        &item.name,
        &item.file_name,
        &item.mime_type,
    );
    let _: (&str, &str, &str, u64) = (&item.disk, &item.conversions_disk, &item.path, item.size);
    let _: (Option<u32>, Option<u32>, i32) = (item.width, item.height, item.order);
    let generated: &GeneratedConversion = &item.conversions["card"];
    let variant: &ResponsiveVariant = &generated.variants[0];
    let _: (&str, &str, u64, Option<u32>, u32, DateTime<Utc>) = (
        &generated.path,
        &generated.mime_type,
        generated.size,
        generated.width,
        variant.width,
        generated.generated_at,
    );
    let _ = (
        &item.metadata,
        &item.custom_properties,
        item.created_at,
        item.updated_at,
    );
    let _: bool =
        item.has_conversion("card") && item.is_image() && item.is_video() && item.is_audio();
    let _ = item.property("alt");
}

/// The free functions and the archive, text and video types (compiled,
/// never run).
#[allow(dead_code)]
async fn functions(bytes: &[u8], disk: &Disk) -> Result<(), MediaError> {
    let options = ImageOptions::default();
    let info: ImageInfo = images::probe(bytes)?;
    let _: (u32, u32, &str) = (info.width, info.height, info.mime_type);
    let converted: ConvertedImage =
        images::convert(bytes, "image/png", &Conversion::new("x"), &options)?;
    let _ = (
        converted.bytes,
        converted.mime_type,
        converted.extension,
        converted.width,
        converted.height,
        converted.variants,
    );
    let _: Option<Vec<u8>> = images::strip_metadata(bytes, "image/jpeg", &options)?;
    let _: Vec<u8> = images::rasterize_svg(bytes, Some(512), None, &options)?;

    let mut archive = Archive::open(bytes.to_vec(), &ArchiveLimits::default())?;
    let _: &[ArchiveEntry] = archive.entries();
    let _: Vec<&ArchiveEntry> = archive.files().collect();
    let _: bool = archive.contains("imsmanifest.xml");
    let _: Bytes = archive.read("imsmanifest.xml")?;
    let _: ExtractReport = archive.extract_to(disk, "scorm").await?;
    let mut zip = ZipBuilder::new();
    zip.file("a.txt", b"a")?.directory("dir")?;
    let _: Bytes = zip.finish()?;

    let text: ExtractedText = text::extract(bytes, "application/pdf")?;
    let _ = (text.text, text.pages, text.title, text.mime_type);
    let _: bool = text::supports(text::DOCX);

    let ffmpeg = Ffmpeg::new(FfmpegOptions::default());
    let probe: Probe = ffmpeg.probe(std::path::Path::new("in.mp4")).await?;
    let _: Option<&ProbeStream> = probe.video();
    let _ = (probe.duration, probe.dimensions(), probe.audio());
    ffmpeg
        .frame(
            std::path::Path::new("in.mp4"),
            1.0,
            std::path::Path::new("poster.png"),
        )
        .await?;
    ffmpeg
        .transcode(
            std::path::Path::new("in.mp4"),
            std::path::Path::new("out.mp4"),
            &Transcode::mp4()
                .height(720)
                .crf(23)
                .preset("fast")
                .audio_bitrate("96k")
                .max_duration(10),
        )
        .await?;
    let _: String = ffmpeg.version().await?;
    let _ = (
        Transcode::webm().without_audio().arg("-an"),
        Transcode::mp3(),
        Transcode::m4a(),
    );
    let _: Arc<dyn FfmpegRunner> = Arc::new(ProcessRunner);
    let fake = FfmpegFake::new().probe_json(json!({})).fail("x");
    let _: Vec<FfmpegCall> = fake.calls();
    Ok(())
}

/// The builder, the module and its extension (compiled, never run).
#[allow(dead_code)]
fn modules(m: &mut ModuleBuilder, storage: Arc<navree_storage::Storage>, db: navree_db::Db) {
    let _ = MediaModule::for_root(MediaOptions::default());
    let _ = MediaModule::from_config();
    let _ = MediaModule::for_feature::<Course>();
    m.media_model::<Course>();
    let builder: MediaBuilder = Media::builder(storage)
        .options(MediaOptions::default())
        .store(Arc::new(DatabaseStore::new(db)))
        .store(Arc::new(MemoryStore::new()))
        .register::<Course>()
        .ffmpeg_runner(FfmpegFake::new().runner());
    let _: Result<Media, MediaError> = builder.build();
    let conversion = Conversion::new("hero")
        .width(1920)
        .height(1080)
        .fit(Fit::Contain, 1, 1)
        .fit(Fit::Cover, 1, 1)
        .fit(Fit::Fill, 1, 1)
        .upscale()
        .format(OutputFormat::Jpeg)
        .format(OutputFormat::Png)
        .format(OutputFormat::WebP)
        .quality(DEFAULT_QUALITY)
        .greyscale()
        .blur(2.0)
        .responsive()
        .nonqueued()
        .queued()
        .frame_at(2.0)
        .transcode(Transcode::mp4());
    let _: (&str, bool) = (conversion.name(), conversion.is_transcode());
    let fake = MediaFake::new().register::<Course>();
    let _ = MediaFake::with_options(MediaOptions::default());
    let _: Media = fake.media();
    let _ = (
        fake.storage(),
        fake.http(),
        fake.ffmpeg(),
        fake.added(),
        fake.deleted(),
    );
    fake.assert_nothing_added();
    let _ = |id| fake.assert_deleted(id);
    let _ = || fake.assert_added("Course", "thumbnail");
    let _ = || fake.assert_added_with(|item| item.has_conversion("card"));
    let _ = || fake.assert_added_count(1);
}

#[test]
fn every_configuration_key_is_read() {
    let options: MediaOptions = serde_json::from_value(json!({
        "store": "memory",
        "disk": "public",
        "conversions_disk": "public",
        "prefix": "uploads",
        "max_file_size": 1024,
        "queue_conversions": false,
        "queue": "media",
        "strip_metadata": false,
        "delete_with_model": false,
        "url_ttl_secs": 60,
        "download_timeout_secs": 5,
        "images": { "max_pixels": 100, "responsive_widths": "480,960" },
        "archives": { "max_entries": 1, "max_entry_bytes": 2, "max_total_bytes": 3, "max_ratio": 4 },
        "ffmpeg": { "ffmpeg": "/usr/bin/ffmpeg", "ffprobe": "/usr/bin/ffprobe", "timeout_secs": 7, "threads": 2 },
    }))
    .unwrap();
    assert_eq!(options.store, StoreKind::Memory);
    assert_eq!(options.disk.as_deref(), Some("public"));
    assert_eq!(options.conversions_disk.as_deref(), Some("public"));
    assert_eq!(options.prefix, "uploads");
    assert_eq!(options.max_file_size, 1024);
    assert!(!options.queue_conversions && !options.strip_metadata && !options.delete_with_model);
    assert_eq!(options.queue, "media");
    assert_eq!(
        (options.url_ttl_secs, options.download_timeout_secs),
        (60, 5)
    );
    assert_eq!(options.images.max_pixels, 100);
    assert_eq!(options.images.responsive_widths, vec![480, 960]);
    assert_eq!(
        (
            options.archives.max_entries,
            options.archives.max_entry_bytes,
            options.archives.max_total_bytes,
            options.archives.max_ratio
        ),
        (1, 2, 3, 4)
    );
    assert_eq!(options.ffmpeg.ffmpeg, "/usr/bin/ffmpeg");
    assert_eq!(options.ffmpeg.ffprobe, "/usr/bin/ffprobe");
    assert_eq!(
        (options.ffmpeg.timeout_secs, options.ffmpeg.threads),
        (7, 2)
    );
}

#[test]
fn the_defaults_are_as_documented() {
    let options = MediaOptions::default();
    assert_eq!(options.store, StoreKind::Database);
    assert_eq!(
        (options.disk.clone(), options.conversions_disk.clone()),
        (None, None)
    );
    assert_eq!(options.prefix, "media");
    assert_eq!(options.max_file_size, 26_214_400);
    assert!(options.queue_conversions && options.strip_metadata && options.delete_with_model);
    assert_eq!(options.queue, "default");
    assert_eq!(
        (options.url_ttl_secs, options.download_timeout_secs),
        (3600, 30)
    );
    assert_eq!(options.images.max_pixels, 50_000_000);
    assert_eq!(
        options.images.responsive_widths,
        vec![320, 640, 960, 1280, 1920, 2560]
    );
    let archives = ArchiveLimits::default();
    assert_eq!(
        (
            archives.max_entries,
            archives.max_entry_bytes,
            archives.max_total_bytes,
            archives.max_ratio
        ),
        (10_000, 536_870_912, 2_147_483_648, 200)
    );
    let ffmpeg = FfmpegOptions::default();
    assert_eq!(
        (ffmpeg.ffmpeg.as_str(), ffmpeg.ffprobe.as_str()),
        ("ffmpeg", "ffprobe")
    );
    assert_eq!((ffmpeg.timeout_secs, ffmpeg.threads), (600, 0));
    assert_eq!(DEFAULT_QUALITY, 82);
}

#[test]
fn names_of_the_job_table_and_migrations() {
    use navree_contracts::Job;
    assert_eq!(GenerateConversions::NAME, "media.conversions");
    assert_eq!(GenerateConversions::options().timeout_secs, 900);
    assert_eq!(MEDIA_TABLE, "media");
    assert_eq!(MEDIA_MIGRATIONS, "media");
    use navree_orm::Model as _;
    assert_eq!(MediaRecord::meta().table(), "media");
    for column in [
        "id",
        "model_type",
        "model_id",
        "collection",
        "name",
        "file_name",
        "mime_type",
        "disk",
        "conversions_disk",
        "path",
        "size",
        "width",
        "height",
        "conversions",
        "metadata",
        "custom_properties",
        "order_column",
        "created_at",
        "updated_at",
    ] {
        assert!(
            MediaRecord::meta().has_column(column),
            "the media table has no `{column}`"
        );
    }
}

#[test]
fn the_documented_error_codes() {
    let id = Uuid::nil();
    let cases: Vec<(MediaError, u16, &str)> = vec![
        (
            MediaError::UnknownCollection {
                model_type: "C".into(),
                collection: "x".into(),
            },
            400,
            "unknown_media_collection",
        ),
        (
            MediaError::TypeNotAllowed {
                collection: "x".into(),
                mime_type: "a/b".into(),
            },
            400,
            "file_type_not_allowed",
        ),
        (MediaError::TooLarge { max: 1 }, 413, "payload_too_large"),
        (MediaError::Empty, 400, "empty_file"),
        (
            MediaError::Download {
                url: "u".into(),
                reason: "r".into(),
            },
            400,
            "remote_file_unavailable",
        ),
        (
            MediaError::Blocked {
                url: "u".into(),
                reason: "r".into(),
            },
            400,
            "remote_file_blocked",
        ),
        (
            MediaError::Image {
                message: "m".into(),
            },
            400,
            "invalid_image",
        ),
        (
            MediaError::Text {
                message: "m".into(),
            },
            400,
            "invalid_document",
        ),
        (
            MediaError::Unsupported {
                operation: "text extraction",
                mime_type: "a/b".into(),
            },
            415,
            "unsupported_media_type",
        ),
        (MediaError::NotFound { id }, 404, "media_not_found"),
        (
            MediaError::MissingConversion {
                id,
                conversion: "c".into(),
            },
            404,
            "conversion_not_found",
        ),
        (
            MediaError::NoPublicUrl { disk: "d".into() },
            500,
            "internal_error",
        ),
        (
            MediaError::Ffmpeg {
                message: "m".into(),
            },
            500,
            "internal_error",
        ),
        (
            MediaError::File {
                path: "p".into(),
                message: "m".into(),
            },
            500,
            "internal_error",
        ),
        (
            MediaError::Queue {
                message: "m".into(),
            },
            500,
            "internal_error",
        ),
    ];
    for (error, status, code) in cases {
        assert_eq!((error.status(), error.code()), (status, code), "{error}");
    }
    let archive: Vec<(ArchiveError, u16, &str)> = vec![
        (
            ArchiveError::Invalid {
                message: "m".into(),
            },
            400,
            "invalid_archive",
        ),
        (
            ArchiveError::UnsafePath {
                path: "../x".into(),
            },
            400,
            "unsafe_archive",
        ),
        (
            ArchiveError::Symlink { path: "x".into() },
            400,
            "unsafe_archive",
        ),
        (
            ArchiveError::Encrypted { path: "x".into() },
            400,
            "unsafe_archive",
        ),
        (
            ArchiveError::Duplicate { path: "x".into() },
            400,
            "unsafe_archive",
        ),
        (
            ArchiveError::TooManyEntries { max: 1 },
            413,
            "archive_too_large",
        ),
        (
            ArchiveError::EntryTooLarge {
                path: "x".into(),
                max: 1,
            },
            413,
            "archive_too_large",
        ),
        (ArchiveError::TooLarge { max: 1 }, 413, "archive_too_large"),
        (
            ArchiveError::Ratio {
                path: "x".into(),
                max: 1,
            },
            413,
            "archive_too_large",
        ),
        (
            ArchiveError::NotFound { path: "x".into() },
            404,
            "not_found",
        ),
    ];
    for (error, status, code) in archive {
        assert_eq!((error.status(), error.code()), (status, code), "{error}");
        let wrapped = MediaError::from(error);
        assert_eq!(wrapped.status(), status);
    }
}

#[tokio::test]
async fn the_command_is_registered() {
    let mut storage = navree_storage::StorageOptions::default();
    storage.disks.clear();
    storage
        .disks
        .insert("local".into(), navree_storage::DiskConfig::Memory);
    let app = navree_testing::TestApp::builder()
        .module(navree_storage::StorageModule::for_root(storage))
        .config("media.store", "memory")
        .module(MediaModule::from_config())
        .build()
        .await;
    app.run_command("media:regenerate", &["--missing"])
        .await
        .unwrap();
}
