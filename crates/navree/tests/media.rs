//! The examples of the media docs page
//! (`apps/website/content/docs/packages/media.mdx`), through the facade
//! alone: a course model with collections, a controller that takes a
//! multipart upload, the module that registers the model, deleting with
//! the model, regeneration, archives, text, images on their own, and the
//! fake.

#![cfg(feature = "media")]

use std::io::Write;

use navree::axum::extract::Multipart;
use navree::db::Db;
use navree::media::archive::{Archive, ArchiveLimits};
use navree::media::images::{self, ImageOptions};
use navree::media::{
    Conversion, Fit, HasMedia, MediaCollections, MediaFake, MediaRecord, MediaResource,
    MediaSource, OutputFormat, Regenerate, text,
};
use navree::prelude::*;
use navree::storage::{DiskConfig, StorageModule, StorageOptions};
use navree::testing::TestApp;
use serde::Serialize;
use uuid::Uuid;

/// A course, with a thumbnail, attachments and a SCORM package.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "courses")]
pub struct Course {
    /// The id.
    pub id: Uuid,
    /// The title.
    pub title: String,
}

impl HasMedia for Course {
    const MEDIA_TYPE: &'static str = "Course";
    const MEDIA_TABLE: &'static str = "courses";

    fn media_key(&self) -> String {
        self.id.to_string()
    }

    fn media_collections(collections: &mut MediaCollections) {
        collections
            .collection("thumbnail")
            .single_file()
            .accepts(["image/*"])
            .conversion(
                Conversion::new("card")
                    .fit(Fit::Cover, 640, 360)
                    .format(OutputFormat::WebP),
            );
        collections
            .collection("attachments")
            .accepts(["application/pdf", "image/*", "text/*"])
            .max_size(10 * 1024 * 1024);
        collections.collection("scorm").accepts(["application/zip"]);
    }
}

mod m20261027_000001_create_courses_table {
    use navree::schema::prelude::*;

    pub struct Migration;

    impl navree::schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create("courses", |t| {
                    t.uuid_primary();
                    t.string("title");
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("courses").await
        }
    }
}

/// A course's media routes.
#[injectable]
pub struct CourseMediaController {
    media: Arc<Media>,
}

#[controller("/courses", version = "1")]
impl CourseMediaController {
    /// Replaces the course's thumbnail with the first file of the form.
    #[post("/{id}/thumbnail")]
    #[public]
    async fn thumbnail(
        &self,
        uow: Uow,
        Path(id): Path<Uuid>,
        mut form: Multipart,
    ) -> Result<Json<MediaResource>> {
        let course = Course::find_or_fail(&uow, id).await?;
        let field = form
            .next_field()
            .await?
            .ok_or_else(|| AppError::bad_request("thumbnail should not be empty"))?;
        let file_name = field.file_name().unwrap_or("thumbnail").to_owned();
        let bytes = field.bytes().await?;
        let item = self
            .media
            .add(
                course.model(),
                "thumbnail",
                MediaSource::bytes(file_name, bytes),
            )
            .property("alt", course.title.clone())
            .save(&uow)
            .await?;
        Ok(Json(self.media.resource(&item).await?))
    }
}

/// Courses, with their media.
#[module(
    imports = [MediaModule::for_feature::<Course>()],
    controllers = [CourseMediaController],
    register = Self::wire,
)]
pub struct CoursesModule;

impl CoursesModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("courses").after(["media"]).migration(
            navree::schema::migration!(m20261027_000001_create_courses_table),
        ));
    }
}

/// A PNG of `width` x `height`: an SVG rendered by the module itself.
fn png(width: u32, height: u32) -> Vec<u8> {
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><rect width="{width}" height="{height}" fill="#1478c8"/></svg>"##
    );
    images::rasterize_svg(svg.as_bytes(), Some(width), None, &ImageOptions::default()).unwrap()
}

fn multipart(field: &str, file_name: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "navree-media-boundary";
    let mut body = Vec::new();
    write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{file_name}\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    )
    .unwrap();
    body.extend_from_slice(bytes);
    write!(body, "\r\n--{boundary}--\r\n").unwrap();
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn storage() -> StorageOptions {
    StorageOptions {
        default: "media".into(),
        disks: [("media".into(), DiskConfig::Memory)].into_iter().collect(),
        ..StorageOptions::default()
    }
}

#[tokio::test]
async fn uploads_become_media_and_go_with_the_course() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(StorageModule::for_root(storage()))
        .module(MediaModule::from_config())
        .module(CoursesModule)
        .build()
        .await;
    let db = app.get::<Db>().await;
    let media = app.get::<Media>().await;
    let course = Course::create(
        &*db,
        Course {
            title: "Phishing 101".into(),
            ..Course::default()
        },
    )
    .await
    .unwrap()
    .into_inner();

    let (content_type, body) = multipart("thumbnail", "cover.png", &png(1280, 960));
    let response = app
        .client()
        .post(&format!("/api/v1/courses/{}/thumbnail", course.id))
        .header("content-type", &content_type)
        .body(body)
        .send()
        .await
        .assert_status(200);
    let resource = response.json_value();
    assert_eq!(resource["__type"], "Media");
    assert_eq!(resource["file_name"], "cover.png");
    assert_eq!(resource["width"], 1280);
    assert_eq!(resource["custom_properties"]["alt"], "Phishing 101");
    // No queue in this app: the card was generated during the request.
    assert!(
        resource["conversions"]["card"]
            .as_str()
            .unwrap()
            .contains("/conversions/cover-card.webp")
    );
    let item = media.first(&course, "thumbnail").await.unwrap().unwrap();
    assert_eq!(item.conversions["card"].width, Some(640));

    // Reading the table with navree-orm.
    let rows = MediaRecord::query()
        .where_(MediaRecord::MIME_TYPE.eq("image/png"))
        .get(&*db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);

    // A PDF is not an image.
    let (content_type, body) = multipart("thumbnail", "cover.png", b"%PDF-1.7\n%...");
    app.client()
        .post(&format!("/api/v1/courses/{}/thumbnail", course.id))
        .header("content-type", &content_type)
        .body(body)
        .send()
        .await
        .assert_error_code(400, "file_type_not_allowed");

    // Regenerating, as `media:regenerate --model Course --only card` does.
    let report = media
        .regenerate(&Regenerate {
            model_type: Some("Course".into()),
            conversions: vec!["card".into()],
            ..Regenerate::default()
        })
        .await
        .unwrap();
    assert_eq!((report.media, report.conversions), (1, 1));
    app.run_command("media:regenerate", &["--missing"])
        .await
        .unwrap();

    // Deleting the course deletes its media.
    let uow = db.uow(None, None).await;
    let mut record = Course::find_or_fail(&uow, course.id).await.unwrap();
    record.delete(&uow).await.unwrap();
    uow.commit().await.unwrap();
    assert!(media.all(&course).await.unwrap().is_empty());
    assert!(
        !media
            .storage()
            .default_disk()
            .exists(&item.path)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn the_fake_replaces_media_in_tests() {
    let fake = MediaFake::new().register::<Course>();
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(CoursesModule)
        .override_value(Arc::new(fake.media()))
        .build()
        .await;
    let db = app.get::<Db>().await;
    let course = Course::create(
        &*db,
        Course {
            title: "Passwords".into(),
            ..Course::default()
        },
    )
    .await
    .unwrap();
    let (content_type, body) = multipart("thumbnail", "cover.png", &png(800, 600));
    app.client()
        .post(&format!("/api/v1/courses/{}/thumbnail", course.id))
        .header("content-type", &content_type)
        .body(body)
        .send()
        .await
        .assert_status(200);

    fake.assert_added("Course", "thumbnail");
    fake.assert_added_with(|item| item.has_conversion("card"));
    let item = &fake.added()[0];
    assert!(
        fake.storage()
            .default_disk()
            .exists(&item.path)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn archives_text_and_images_on_their_own() {
    // A SCORM package, checked and unpacked.
    let mut zip = navree::media::archive::ZipBuilder::new();
    zip.file("imsmanifest.xml", b"<manifest/>").unwrap();
    zip.file("sco/index.html", b"<h1>Lesson</h1>").unwrap();
    let bytes = zip.finish().unwrap();
    let mut package = Archive::open(bytes.clone(), &ArchiveLimits::default()).unwrap();
    assert!(package.contains("imsmanifest.xml"));
    assert_eq!(
        &package.read("imsmanifest.xml").unwrap()[..],
        b"<manifest/>"
    );
    assert_eq!(package.files().count(), 2);

    let fake = MediaFake::new();
    let media = fake.media();
    let course = Course {
        id: Uuid::now_v7(),
        title: "SCORM".into(),
    };
    let item = media
        .add(&course, "scorm", MediaSource::bytes("lesson.zip", bytes))
        .save(None)
        .await
        .unwrap();
    let report = media
        .unpack(&item, "media", &format!("scorm/{}", item.id))
        .await
        .unwrap();
    assert_eq!(
        report.files,
        vec![
            format!("scorm/{}/imsmanifest.xml", item.id),
            format!("scorm/{}/sco/index.html", item.id)
        ]
    );

    // Text for a search index.
    let notes = media
        .add(
            &course,
            "attachments",
            MediaSource::bytes("notes.html", "<h1>Policy</h1><p>Lock your screen.</p>"),
        )
        .save(None)
        .await
        .unwrap();
    let document = media.extract_text(&notes).await.unwrap();
    assert_eq!(document.text, "Policy\n\nLock your screen.");
    assert_eq!(
        text::extract(b"plain", "text/plain").unwrap().pages,
        vec!["plain"]
    );

    // Images without a model.
    let options = ImageOptions::default();
    let upload = png(600, 400);
    let info = images::probe(&upload).unwrap();
    assert_eq!(
        (info.width, info.height, info.mime_type),
        (600, 400, "image/png")
    );
    let logo = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect width="100" height="50"/></svg>"#;
    let rendered = images::rasterize_svg(logo, Some(512), None, &options).unwrap();
    assert_eq!(images::probe(&rendered).unwrap().width, 512);
    let avatar = images::convert(
        &upload,
        info.mime_type,
        &Conversion::new("avatar")
            .fit(Fit::Cover, 256, 256)
            .format(OutputFormat::WebP),
        &options,
    )
    .unwrap();
    assert_eq!(
        (avatar.width, avatar.height, avatar.mime_type),
        (256, 256, "image/webp")
    );
    assert_eq!(
        images::strip_metadata(&upload, info.mime_type, &options).unwrap(),
        None
    );
}
