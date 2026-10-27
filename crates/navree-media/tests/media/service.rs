//! The `Media` service on memory rows and disks: adding, validating,
//! converting, presenting, ordering, deleting and regenerating.

use chrono::{Duration, Utc};
use image::ImageFormat;
use navree_media::{MediaError, MediaQuery, MediaSource, Regenerate, StoreKind};
use serde_json::json;

use crate::models::{Course, Note, service, service_with};
use crate::support::{image_bytes, jpeg_with_exif, pdf, svg_logo, zip_of};

fn course() -> Course {
    Course { id: 7 }
}

#[tokio::test]
async fn adding_a_file_stores_it_and_its_row() {
    let s = service();
    let item = s
        .media
        .add(&course(), "attachments", MediaSource::bytes("../Hand book?.pdf", pdf(&["Rules"])))
        .property("caption", "The handbook")
        .save(None)
        .await
        .unwrap();
    assert_eq!(item.model_type, "Course");
    assert_eq!(item.model_id, "7");
    assert_eq!(item.collection, "attachments");
    assert_eq!(item.mime_type, "application/pdf");
    assert_eq!(item.file_name, "Hand book-.pdf");
    assert_eq!(item.name, "Hand book-");
    assert_eq!(item.disk, "media");
    assert_eq!(item.conversions_disk, "media");
    assert_eq!(item.path, format!("media/{}/Hand book-.pdf", item.id));
    assert_eq!(item.order, 1);
    assert_eq!(item.property("caption"), Some(&json!("The handbook")));
    // A PDF gets no image conversion; that is not an error.
    assert!(item.conversions.is_empty());
    assert_eq!(
        s.media.read(&item).await.unwrap().len() as u64,
        item.size
    );
    assert_eq!(s.media.find(item.id).await.unwrap(), Some(item.clone()));

    let second = s
        .media
        .add(&course(), "attachments", MediaSource::bytes("b.txt", "notes"))
        .name("Notes")
        .file_name("renamed.txt")
        .save(None)
        .await
        .unwrap();
    assert_eq!(second.order, 2);
    assert_eq!(second.name, "Notes");
    assert_eq!(second.file_name, "renamed.txt");
    let all = s.media.get(&course(), "attachments").await.unwrap();
    assert_eq!(
        all.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![item.id, second.id]
    );
    assert_eq!(s.media.first(&course(), "attachments").await.unwrap().unwrap().id, item.id);
}

#[tokio::test]
async fn collections_refuse_wrong_types_sizes_and_names() {
    let s = service();
    let error = s
        .media
        .add(&course(), "thumbnail", MediaSource::bytes("x.pdf", pdf(&["x"])))
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::TypeNotAllowed { .. }), "{error}");
    assert_eq!((error.status(), error.code()), (400, "file_type_not_allowed"));

    // The type is sniffed from the content, not the name.
    let error = s
        .media
        .add(&course(), "thumbnail", MediaSource::bytes("cover.png", "<?php echo 1;"))
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::TypeNotAllowed { .. }), "{error}");

    let error = s
        .media
        .add(&course(), "attachments", MediaSource::bytes("big.txt", "x".repeat(70 * 1024)))
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::TooLarge { max: 65536 }), "{error}");
    assert_eq!((error.status(), error.code()), (413, "payload_too_large"));

    let error = s
        .media
        .add(&course(), "videos", MediaSource::bytes("a.txt", "x"))
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::UnknownCollection { .. }), "{error}");
    assert_eq!(error.code(), "unknown_media_collection");

    let error = s
        .media
        .add(&course(), "attachments", MediaSource::bytes("empty.txt", ""))
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::Empty), "{error}");

    // A model without collections takes any name, within media.max_file_size.
    let small = service_with(navree_media::MediaOptions {
        max_file_size: 10,
        ..Default::default()
    });
    let note = Note { id: "n1".into() };
    small
        .media
        .add(&note, "anything", MediaSource::bytes("a.txt", "short"))
        .save(None)
        .await
        .unwrap();
    let error = small
        .media
        .add(&note, "anything", MediaSource::bytes("a.txt", "far too long"))
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::TooLarge { max: 10 }), "{error}");
    assert!(s.storage.default_disk().list("media").await.unwrap().is_empty());
}

#[tokio::test]
async fn image_conversions_are_generated_without_a_queue() {
    let s = service();
    let item = s
        .media
        .add(&course(), "thumbnail", MediaSource::bytes("cover.png", image_bytes(320, 240, ImageFormat::Png)))
        .save(None)
        .await
        .unwrap();
    assert_eq!((item.width, item.height), (Some(320), Some(240)));
    let card = &item.conversions["card"];
    assert_eq!((card.width, card.height), (Some(64), Some(36)));
    assert_eq!(card.mime_type, "image/webp");
    assert_eq!(card.path, format!("media/{}/conversions/cover-card.webp", item.id));
    let bytes = s.media.read_conversion(&item, "card").await.unwrap();
    assert_eq!(bytes.len() as u64, card.size);
    assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::WebP);
    // The stored row has them too.
    assert_eq!(s.media.find_or_fail(item.id).await.unwrap().conversions, item.conversions);
    let error = s.media.read_conversion(&item, "poster").await.unwrap_err();
    assert_eq!((error.status(), error.code()), (404, "conversion_not_found"));
}

#[tokio::test]
async fn responsive_conversions_have_variants_and_srcsets() {
    let s = service();
    let item = s
        .media
        .add(&course(), "gallery", MediaSource::bytes("view.jpg", image_bytes(1200, 600, ImageFormat::Jpeg)))
        .save(None)
        .await
        .unwrap();
    let hero = &item.conversions["hero"];
    assert_eq!((hero.width, hero.height), (Some(800), Some(400)));
    assert_eq!(hero.mime_type, "image/jpeg");
    let widths: Vec<u32> = hero.variants.iter().map(|v| v.width).collect();
    assert_eq!(widths, vec![320, 640]);
    assert_eq!(hero.variants[0].height, 160);
    assert_eq!(
        hero.variants[0].path,
        format!("media/{}/responsive/view-hero-320.jpg", item.id)
    );
    let thumb = &item.conversions["thumb"];
    assert_eq!((thumb.width, thumb.height), (Some(100), Some(50)));

    // `media` is a memory disk: no public URLs.
    let error = s.media.srcset(&item, "hero").unwrap_err();
    assert!(matches!(error, MediaError::NoPublicUrl { .. }), "{error}");
    let resource = s.media.resource(&item).await.unwrap();
    assert_eq!(resource.kind, "Media");
    assert!(resource.url.starts_with(&format!("/storage/media/media/{}/view.jpg?expires=", item.id)));
    let srcset = &resource.srcsets["hero"];
    assert_eq!(srcset.matches(", ").count(), 2, "{srcset}");
    assert!(srcset.contains(" 320w, "), "{srcset}");
    assert!(srcset.ends_with(" 800w"), "{srcset}");
    assert!(resource.conversions["thumb"].contains("/conversions/view-thumb.jpg?expires="));
}

#[tokio::test]
async fn public_disks_give_public_urls() {
    let s = service_with(navree_media::MediaOptions {
        disk: Some("public".into()),
        ..Default::default()
    });
    let item = s
        .media
        .add(&course(), "gallery", MediaSource::bytes("wide.png", image_bytes(900, 300, ImageFormat::Png)))
        .save(None)
        .await
        .unwrap();
    assert_eq!(item.disk, "public");
    let base = format!("https://cdn.example.com/files/media/{}", item.id);
    assert_eq!(s.media.url(&item).unwrap(), format!("{base}/wide.png"));
    assert_eq!(
        s.media.conversion_url(&item, "thumb").unwrap(),
        format!("{base}/conversions/wide-thumb.png")
    );
    assert_eq!(
        s.media.srcset(&item, "hero").unwrap(),
        format!(
            "{base}/responsive/wide-hero-320.png 320w, {base}/responsive/wide-hero-640.png 640w, \
             {base}/conversions/wide-hero.png 800w"
        )
    );
    let resource = s.media.resource(&item).await.unwrap();
    assert_eq!(resource.url, format!("{base}/wide.png"));
    let json = serde_json::to_value(&resource).unwrap();
    assert_eq!(json["__type"], "Media");
    assert_eq!(json["width"], 900);
    let temporary = s
        .media
        .temporary_url(&item, Utc::now() + Duration::minutes(5))
        .await
        .unwrap();
    assert!(temporary.contains("expires="), "{temporary}");
    let temporary = s
        .media
        .temporary_conversion_url(&item, "thumb", Utc::now() + Duration::minutes(5))
        .await
        .unwrap();
    assert!(temporary.contains("wide-thumb.png"), "{temporary}");
}

#[tokio::test]
async fn collections_can_name_their_own_disks() {
    let s = service_with(navree_media::MediaOptions {
        conversions_disk: Some("archive".into()),
        ..Default::default()
    });
    let item = s
        .media
        .add(&course(), "thumbnail", MediaSource::bytes("c.png", image_bytes(100, 100, ImageFormat::Png)))
        .save(None)
        .await
        .unwrap();
    assert_eq!((item.disk.as_str(), item.conversions_disk.as_str()), ("media", "archive"));
    let archive = s.storage.disk("archive").unwrap();
    assert!(archive.exists(&item.conversions["card"].path).await.unwrap());
    assert!(!s.storage.default_disk().exists(&item.conversions["card"].path).await.unwrap());
    s.media.delete(None, item.id).await.unwrap();
    assert!(archive.list("media").await.unwrap().is_empty());
    assert!(s.storage.default_disk().list("media").await.unwrap().is_empty());
}

#[tokio::test]
async fn originals_lose_their_metadata_and_stand_upright() {
    let s = service();
    // Orientation 6: stored 120 x 80, displayed 80 x 120.
    let item = s
        .media
        .add(&course(), "gallery", MediaSource::bytes("phone.jpg", jpeg_with_exif(120, 80, 6)))
        .save(None)
        .await
        .unwrap();
    assert_eq!((item.width, item.height), (Some(80), Some(120)));
    let stored = s.media.read(&item).await.unwrap();
    assert!(
        !stored.windows(12).any(|w| w == b"SERIAL-12345"),
        "the camera serial number was kept"
    );
    assert_eq!(item.size, stored.len() as u64);
    assert_eq!(item.conversions["thumb"].height, Some(100));
    assert_eq!(item.conversions["thumb"].width, Some(67));

    let keep = service_with(navree_media::MediaOptions {
        strip_metadata: false,
        ..Default::default()
    });
    let item = keep
        .media
        .add(&course(), "gallery", MediaSource::bytes("phone.jpg", jpeg_with_exif(120, 80, 1)))
        .save(None)
        .await
        .unwrap();
    let stored = keep.media.read(&item).await.unwrap();
    assert!(stored.windows(12).any(|w| w == b"SERIAL-12345"));
}

#[tokio::test]
async fn svg_logos_are_measured_and_rasterized() {
    let s = service();
    let item = s
        .media
        .add(&course(), "thumbnail", MediaSource::bytes("logo.svg", svg_logo(200, 100)))
        .save(None)
        .await
        .unwrap();
    assert_eq!(item.mime_type, "image/svg+xml");
    assert_eq!((item.width, item.height), (Some(200), Some(100)));
    let card = &item.conversions["card"];
    assert_eq!((card.width, card.height, card.mime_type.as_str()), (Some(64), Some(36), "image/webp"));
}

#[tokio::test]
async fn broken_images_are_refused() {
    let s = service();
    let mut png = image_bytes(50, 50, ImageFormat::Png);
    png.truncate(40);
    let error = s
        .media
        .add(&course(), "thumbnail", MediaSource::bytes("broken.png", png))
        .save(None)
        .await
        .unwrap_err();
    assert_eq!((error.status(), error.code()), (400, "invalid_image"), "{error}");
    assert!(s.storage.default_disk().list("media").await.unwrap().is_empty());
}

#[tokio::test]
async fn single_file_collections_replace_the_previous_file() {
    let s = service();
    let first = s
        .media
        .add(&course(), "thumbnail", MediaSource::bytes("a.png", image_bytes(80, 80, ImageFormat::Png)))
        .save(None)
        .await
        .unwrap();
    let second = s
        .media
        .add(&course(), "thumbnail", MediaSource::bytes("b.png", image_bytes(90, 90, ImageFormat::Png)))
        .save(None)
        .await
        .unwrap();
    let items = s.media.get(&course(), "thumbnail").await.unwrap();
    assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![second.id]);
    let disk = s.storage.default_disk();
    assert!(!disk.exists(&first.path).await.unwrap());
    assert!(!disk.exists(&first.conversions["card"].path).await.unwrap());
    assert!(disk.exists(&second.path).await.unwrap());

    for name in ["1.txt", "2.txt", "3.txt"] {
        s.media
            .add(&course(), "handouts", MediaSource::bytes(name, "x"))
            .save(None)
            .await
            .unwrap();
    }
    let handouts = s.media.get(&course(), "handouts").await.unwrap();
    assert_eq!(
        handouts.iter().map(|i| i.file_name.as_str()).collect::<Vec<_>>(),
        vec!["2.txt", "3.txt"]
    );
}

#[tokio::test]
async fn items_are_saved_reordered_and_deleted() {
    let s = service();
    let c = course();
    let mut ids = Vec::new();
    for name in ["a.txt", "b.txt", "c.txt"] {
        ids.push(
            s.media
                .add(&c, "attachments", MediaSource::bytes(name, "x"))
                .save(None)
                .await
                .unwrap()
                .id,
        );
    }
    s.media.reorder(None, &[ids[2], ids[0], ids[1]]).await.unwrap();
    let order: Vec<_> = s
        .media
        .get(&c, "attachments")
        .await
        .unwrap()
        .iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(order, vec![ids[2], ids[0], ids[1]]);

    let mut item = s.media.find_or_fail(ids[0]).await.unwrap();
    item.name = "Renamed".into();
    item.custom_properties.insert("alt".into(), json!("Alt text"));
    item.path = "elsewhere/evil.txt".into(); // the module's, not saved
    let saved = s.media.save(None, &item).await.unwrap();
    assert_eq!(saved.name, "Renamed");
    assert_eq!(saved.property("alt"), Some(&json!("Alt text")));
    assert_eq!(saved.path, format!("media/{}/a.txt", ids[0]));

    assert!(s.media.delete(None, ids[0]).await.unwrap());
    assert!(!s.media.delete(None, ids[0]).await.unwrap());
    assert!(!s.storage.default_disk().exists(&saved.path).await.unwrap());
    let error = s.media.find_or_fail(ids[0]).await.unwrap_err();
    assert_eq!((error.status(), error.code()), (404, "media_not_found"));

    s.media
        .add(&c, "handouts", MediaSource::bytes("h.txt", "x"))
        .save(None)
        .await
        .unwrap();
    assert_eq!(s.media.clear(None, &c, "attachments").await.unwrap(), 2);
    assert_eq!(s.media.all(&c).await.unwrap().len(), 1);
    assert_eq!(s.media.delete_all(None, &c).await.unwrap(), 1);
    assert!(s.media.all(&c).await.unwrap().is_empty());
    assert!(s.storage.default_disk().list("media").await.unwrap().is_empty());
}

#[tokio::test]
async fn files_are_added_from_disks_and_local_files() {
    let s = service();
    let uploads = s.storage.disk("archive").unwrap();
    uploads.put("tmp/abc/report.pdf", pdf(&["Q1"])).await.unwrap();
    let item = s
        .media
        .add(&course(), "attachments", MediaSource::disk("archive", "tmp/abc/report.pdf"))
        .save(None)
        .await
        .unwrap();
    assert_eq!(item.file_name, "report.pdf");
    // Copied, not moved.
    assert!(uploads.exists("tmp/abc/report.pdf").await.unwrap());

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syllabus.txt");
    std::fs::write(&path, "Week 1").unwrap();
    let item = s
        .media
        .add(&course(), "attachments", MediaSource::file(&path))
        .save(None)
        .await
        .unwrap();
    assert_eq!((item.file_name.as_str(), item.mime_type.as_str()), ("syllabus.txt", "text/plain"));

    let error = s
        .media
        .add(&course(), "attachments", MediaSource::file(dir.path().join("missing.txt")))
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::File { .. }), "{error}");
    let error = s
        .media
        .add(&course(), "attachments", MediaSource::disk("nowhere", "x.txt"))
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::Storage(_)), "{error}");
}

#[tokio::test]
async fn text_archives_and_zips_come_from_items() {
    let s = service();
    let note = Note { id: "n".into() };
    let document = s
        .media
        .add(&note, "docs", MediaSource::bytes("rules.pdf", pdf(&["First page", "Second page"])))
        .save(None)
        .await
        .unwrap();
    let text = s.media.extract_text(&document).await.unwrap();
    assert_eq!(text.pages, vec!["First page", "Second page"]);

    let package = s
        .media
        .add(
            &note,
            "packages",
            MediaSource::bytes(
                "course.zip",
                zip_of(&[("imsmanifest.xml", b"<manifest/>"), ("sco/index.html", b"<p>Hi</p>")]),
            ),
        )
        .save(None)
        .await
        .unwrap();
    assert_eq!(package.mime_type, "application/zip");
    let archive = s.media.open_archive(&package).await.unwrap();
    assert!(archive.contains("imsmanifest.xml"));
    let report = s.media.unpack(&package, "public", "scorm/course-1").await.unwrap();
    assert_eq!(
        report.files,
        vec!["scorm/course-1/imsmanifest.xml", "scorm/course-1/sco/index.html"]
    );

    let error = s.media.open_archive(&document).await.unwrap_err();
    assert_eq!(error.code(), "invalid_archive");

    let again = s
        .media
        .add(&note, "docs", MediaSource::bytes("rules.pdf", pdf(&["Copy"])))
        .save(None)
        .await
        .unwrap();
    let zip = s.media.zip(&[document, again]).await.unwrap();
    let archive = navree_media::archive::Archive::open(zip, &Default::default()).unwrap();
    let names: Vec<_> = archive.files().map(|e| e.path.clone()).collect();
    assert_eq!(names, vec!["rules.pdf", "rules (2).pdf"]);
}

#[tokio::test]
async fn regenerate_converts_again_and_reports() {
    let s = service();
    let c = course();
    let png = s
        .media
        .add(&c, "gallery", MediaSource::bytes("a.png", image_bytes(400, 200, ImageFormat::Png)))
        .save(None)
        .await
        .unwrap();
    s.media
        .add(&c, "attachments", MediaSource::bytes("b.txt", "text"))
        .save(None)
        .await
        .unwrap();
    // A conversion file that went missing.
    s.storage
        .default_disk()
        .delete(&png.conversions["thumb"].path)
        .await
        .unwrap();

    let report = s.media.regenerate(&Regenerate::default()).await.unwrap();
    assert_eq!((report.media, report.conversions), (1, 2));
    assert!(report.failed.is_empty());
    assert!(
        s.storage
            .default_disk()
            .exists(&png.conversions["thumb"].path)
            .await
            .unwrap()
    );

    let report = s
        .media
        .regenerate(&Regenerate {
            conversions: vec!["thumb".into()],
            ..Regenerate::default()
        })
        .await
        .unwrap();
    assert_eq!((report.media, report.conversions), (1, 1));
    let report = s
        .media
        .regenerate(&Regenerate {
            missing_only: true,
            ..Regenerate::default()
        })
        .await
        .unwrap();
    assert_eq!((report.media, report.conversions), (0, 0));
    let report = s
        .media
        .regenerate(&Regenerate {
            model_type: Some("Other".into()),
            ..Regenerate::default()
        })
        .await
        .unwrap();
    assert_eq!(report.media, 0);

    // Without a queue there is nothing to batch on.
    let error = s.media.queue_regenerate(&Regenerate::default()).await.unwrap_err();
    assert!(matches!(error, MediaError::Queue { .. }), "{error}");
}

#[tokio::test]
async fn convert_keeps_changes_made_meanwhile() {
    let s = service();
    let item = s
        .media
        .add(&course(), "gallery", MediaSource::bytes("a.png", image_bytes(300, 300, ImageFormat::Png)))
        .save(None)
        .await
        .unwrap();
    let mut renamed = item.clone();
    renamed.name = "Renamed".into();
    s.media.save(None, &renamed).await.unwrap();
    let converted = s.media.convert(item.id, Some(&["thumb".to_owned()])).await.unwrap();
    assert_eq!(converted.name, "Renamed");
    assert!(converted.conversions["thumb"].generated_at > item.conversions["thumb"].generated_at);
    let error = s.media.convert(uuid::Uuid::now_v7(), None).await.unwrap_err();
    assert!(matches!(error, MediaError::NotFound { .. }));
}

#[tokio::test]
async fn queries_select_rows() {
    let s = service();
    let c = course();
    for name in ["a.txt", "b.txt"] {
        s.media
            .add(&c, "attachments", MediaSource::bytes(name, "x"))
            .save(None)
            .await
            .unwrap();
    }
    s.media
        .add(&Note { id: "n".into() }, "docs", MediaSource::bytes("c.txt", "x"))
        .save(None)
        .await
        .unwrap();
    let all = s.media.list(&MediaQuery::default()).await.unwrap();
    assert_eq!(all.len(), 3);
    let notes = s
        .media
        .list(&MediaQuery {
            model_type: Some("Note".into()),
            ..MediaQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(notes.len(), 1);
    let page = s
        .media
        .list(&MediaQuery {
            limit: Some(2),
            ..MediaQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(page.len(), 2);
    let rest = s
        .media
        .list(&MediaQuery {
            after: Some(page[1].id),
            limit: Some(2),
            ..MediaQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(rest.len(), 1);
    assert!(rest[0].id > page[1].id);
}

#[test]
fn options_are_checked() {
    let mut options = navree_media::MediaOptions::default();
    assert_eq!(options.store, StoreKind::Database);
    assert_eq!(options.check(), Ok(()));
    options.prefix = "../up".into();
    assert!(options.check().unwrap_err().contains("media.prefix"));
    let options = navree_media::MediaOptions {
        max_file_size: 0,
        ..Default::default()
    };
    assert!(options.check().unwrap_err().contains("media.max_file_size"));
    let mut options = navree_media::MediaOptions::default();
    options.images.responsive_widths = vec![0];
    assert!(options.check().unwrap_err().contains("responsive_widths"));
    let options: navree_media::MediaOptions =
        serde_json::from_value(json!({"store": "memory", "images": {"responsive_widths": "480, 960"}}))
            .unwrap();
    assert_eq!(options.store, StoreKind::Memory);
    assert_eq!(options.images.responsive_widths, vec![480, 960]);
}

#[test]
fn invalid_collections_fail_the_build() {
    struct Bad;
    impl navree_media::HasMedia for Bad {
        const MEDIA_TYPE: &'static str = "Bad";
        fn media_key(&self) -> String {
            String::new()
        }
        fn media_collections(collections: &mut navree_media::MediaCollections) {
            collections
                .collection("ok")
                .conversion(navree_media::Conversion::new("Thumb"));
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let error = navree_media::Media::builder(crate::models::storage(dir.path()))
        .register::<Bad>()
        .build()
        .unwrap_err();
    assert!(error.to_string().contains("Thumb"), "{error}");
}
