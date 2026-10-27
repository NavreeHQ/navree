//! The `database` store: the `media` table and its migration, units of
//! work, change events, paging, and a model's media deleted with the
//! model — on SQLite always, and on PostgreSQL (with TimescaleDB) and
//! MySQL when their servers are configured.
//!
//! Opt-in servers: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL`, or `--features mysql` and
//! `NAVREE_TEST_MYSQL_URL`, to a URL of a throwaway server whose user may
//! create databases. Each test creates its own database and drops it.

use chrono::{DateTime, Utc};
use image::ImageFormat;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};
use navree_db::{ChangeOp, DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt};
use navree_kernel::{Module, ModuleBuilder};
use navree_media::{
    HasMedia, Media, MediaCollections, MediaModule, MediaQuery, MediaRecord, MediaSource,
};
use navree_orm::Model;
use navree_storage::{DiskConfig, StorageModule, StorageOptions};
use navree_testing::{TestApp, TestAppBuilder};
use serde::Serialize;
use uuid::Uuid;

use crate::support::image_bytes;

/// A lecture, stored in `lectures`, with slides.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "lectures")]
pub struct Lecture {
    pub id: Uuid,
    pub title: String,
}

impl HasMedia for Lecture {
    const MEDIA_TYPE: &'static str = "Lecture";
    const MEDIA_TABLE: &'static str = "lectures";

    fn media_key(&self) -> String {
        self.id.to_string()
    }

    fn media_collections(collections: &mut MediaCollections) {
        collections
            .collection("slides")
            .conversion(navree_media::Conversion::new("thumb").width(50));
        collections.collection("cover").single_file();
    }
}

/// A handout, stored in `handouts` with soft deletes: its media stay when
/// it is deleted.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "handouts", soft_deletes)]
pub struct Handout {
    pub id: Uuid,
    pub title: String,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl HasMedia for Handout {
    const MEDIA_TYPE: &'static str = "Handout";
    const MEDIA_TABLE: &'static str = "handouts";

    fn media_key(&self) -> String {
        self.id.to_string()
    }
}

/// Creates `lectures` and `handouts` after the media table.
struct CreateLectures;

fn uuid<'a>(t: &'a mut Blueprint, name: &str, mysql: bool) -> ColumnBuilder<'a> {
    if mysql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

impl Migration for CreateLectures {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("lectures", |t| {
                uuid(t, "id", mysql).primary();
                t.string("title");
            })
            .await?;
        schema
            .create("handouts", |t| {
                uuid(t, "id", mysql).primary();
                t.string("title");
                t.timestamp_tz("deleted_at").nullable();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("handouts").await?;
        schema.drop_if_exists("lectures").await
    }
}

struct LecturesModule;

impl Module for LecturesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("lectures")
                .after(["media"])
                .migration(SchemaMigration::new("m20261027_000001_create_lectures", CreateLectures)),
        );
    }
}

fn storage() -> StorageOptions {
    let mut options = StorageOptions::default();
    options.default = "media".into();
    options.disks.clear();
    options.disks.insert("media".into(), DiskConfig::Memory);
    options
}

fn app(builder: TestAppBuilder) -> TestAppBuilder {
    builder
        .module(DatabaseModule::from_config())
        .module(StorageModule::for_root(storage()))
        .module(MediaModule::from_config())
        .module(MediaModule::for_feature::<Lecture>())
        .module(MediaModule::for_feature::<Handout>())
        .module(LecturesModule)
}

/// Times as the database keeps them (microseconds).
fn micros(time: DateTime<Utc>) -> i64 {
    time.timestamp_micros()
}

/// Everything the store does, on the app's database.
async fn store_flow(app: &TestApp) {
    let db = app.get::<Db>().await;
    let media = app.get::<Media>().await;
    let disk = media.storage().default_disk();

    // Rows written in a unit of work, with change events.
    let uow = db.uow(None, None).await;
    let lecture = Lecture::create(
        &uow,
        Lecture {
            title: "Phishing 101".into(),
            ..Lecture::default()
        },
    )
    .await
    .unwrap()
    .into_inner();
    let slide = media
        .add(&lecture, "slides", MediaSource::bytes("one.png", image_bytes(200, 100, ImageFormat::Png)))
        .property("alt", "Slide one")
        .save(&uow)
        .await
        .unwrap();
    let notes = media
        .add(&lecture, "slides", MediaSource::bytes("notes.txt", "Speaker notes"))
        .save(&uow)
        .await
        .unwrap();
    assert_eq!(notes.order, 2, "the uncommitted first slide counts");
    let changes = uow.changes();
    assert!(
        changes
            .iter()
            .any(|c| c.entity == "media" && c.op == ChangeOp::Created && c.id == slide.id.to_string()),
        "{changes:?}"
    );
    uow.commit().await.unwrap();

    let stored = media.find_or_fail(slide.id).await.unwrap();
    assert_eq!(stored.model_type, "Lecture");
    assert_eq!(stored.model_id, lecture.id.to_string());
    assert_eq!((stored.width, stored.height), (Some(200), Some(100)));
    assert_eq!(stored.conversions["thumb"].width, Some(50));
    assert_eq!(stored.conversions["thumb"].path, slide.conversions["thumb"].path);
    assert_eq!(stored.property("alt"), slide.property("alt"));
    assert_eq!(stored.size, slide.size);
    assert_eq!(micros(stored.created_at), micros(slide.created_at));
    let rows = MediaRecord::query()
        .where_(MediaRecord::MODEL_ID.eq(lecture.id.to_string()))
        .get(&*db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        media
            .get(&lecture, "slides")
            .await
            .unwrap()
            .iter()
            .map(|i| i.id)
            .collect::<Vec<_>>(),
        vec![slide.id, notes.id]
    );

    // A rolled-back add leaves no row.
    let uow = db.uow(None, None).await;
    let dropped = media
        .add(&lecture, "slides", MediaSource::bytes("draft.txt", "draft"))
        .save(&uow)
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert!(media.find(dropped.id).await.unwrap().is_none());

    // Updates and order.
    let mut renamed = stored.clone();
    renamed.name = "First slide".into();
    media.save(None, &renamed).await.unwrap();
    media.reorder(None, &[notes.id, slide.id]).await.unwrap();
    let slides = media.get(&lecture, "slides").await.unwrap();
    assert_eq!(slides[0].id, notes.id);
    assert_eq!(slides[1].name, "First slide");
    assert_eq!(micros(slides[1].created_at), micros(slide.created_at));

    // A single-file collection replaces its file in the same unit of work.
    let uow = db.uow(None, None).await;
    let first = media
        .add(&lecture, "cover", MediaSource::bytes("a.txt", "a"))
        .save(&uow)
        .await
        .unwrap();
    let second = media
        .add(&lecture, "cover", MediaSource::bytes("b.txt", "b"))
        .save(&uow)
        .await
        .unwrap();
    // The replaced file is deleted after the commit.
    assert!(disk.exists(&first.path).await.unwrap());
    uow.commit().await.unwrap();
    assert!(!disk.exists(&first.path).await.unwrap());
    let cover = media.get(&lecture, "cover").await.unwrap();
    assert_eq!(cover.iter().map(|i| i.id).collect::<Vec<_>>(), vec![second.id]);

    // Paging in id order.
    let page = media
        .list(&MediaQuery {
            model_type: Some("Lecture".into()),
            limit: Some(2),
            ..MediaQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(page.len(), 2);
    assert!(page[0].id < page[1].id);
    let rest = media
        .list(&MediaQuery {
            model_type: Some("Lecture".into()),
            after: Some(page[1].id),
            ..MediaQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(rest.len(), 1);
    let some = media
        .list(&MediaQuery {
            ids: Some(vec![slide.id, second.id]),
            ..MediaQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(some.len(), 2);

    // A delete in a unit of work: the files go after the commit.
    let uow = db.uow(None, None).await;
    assert!(media.delete(&uow, notes.id).await.unwrap());
    assert!(disk.exists(&notes.path).await.unwrap());
    uow.commit().await.unwrap();
    assert!(!disk.exists(&notes.path).await.unwrap());
    assert!(media.find(notes.id).await.unwrap().is_none());

    // Deleting the lecture deletes its media; a rollback keeps them.
    let uow = db.uow(None, None).await;
    let mut record = Lecture::find(&uow, lecture.id).await.unwrap().unwrap();
    record.delete(&uow).await.unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(media.all(&lecture).await.unwrap().len(), 2);

    let uow = db.uow(None, None).await;
    let mut record = Lecture::find(&uow, lecture.id).await.unwrap().unwrap();
    record.delete(&uow).await.unwrap();
    uow.commit().await.unwrap();
    assert!(media.all(&lecture).await.unwrap().is_empty());
    assert!(!disk.exists(&slide.path).await.unwrap());
    assert!(!disk.exists(&slide.conversions["thumb"].path).await.unwrap());
    assert!(!disk.exists(&second.path).await.unwrap());

    // A soft-deleting model keeps its media: a restore finds them.
    let handout = Handout::create(
        &*db,
        Handout {
            title: "Checklist".into(),
            ..Handout::default()
        },
    )
    .await
    .unwrap()
    .into_inner();
    let sheet = media
        .add(&handout, "sheets", MediaSource::bytes("list.txt", "1. Lock"))
        .save(None)
        .await
        .unwrap();
    let uow = db.uow(None, None).await;
    let mut record = Handout::find(&uow, handout.id).await.unwrap().unwrap();
    record.delete(&uow).await.unwrap();
    uow.commit().await.unwrap();
    assert!(Handout::find(&*db, handout.id).await.unwrap().is_none());
    assert_eq!(media.all(&handout).await.unwrap().len(), 1);
    assert!(disk.exists(&sheet.path).await.unwrap());
}

#[tokio::test]
async fn the_store_works_on_sqlite() {
    let app = app(TestApp::builder().temp_database()).build().await;
    store_flow(&app).await;
}

#[tokio::test]
async fn delete_with_model_can_be_turned_off() {
    let app = app(TestApp::builder().temp_database())
        .config("media.delete_with_model", false)
        .build()
        .await;
    let db = app.get::<Db>().await;
    let media = app.get::<Media>().await;
    let lecture = Lecture::create(
        &*db,
        Lecture {
            title: "Kept".into(),
            ..Lecture::default()
        },
    )
    .await
    .unwrap();
    media
        .add(lecture.model(), "slides", MediaSource::bytes("a.txt", "a"))
        .save(None)
        .await
        .unwrap();
    let uow = db.uow(None, None).await;
    let mut record = Lecture::find(&uow, lecture.id).await.unwrap().unwrap();
    record.delete(&uow).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(media.all(lecture.model()).await.unwrap().len(), 1);
}

/// A database of its own on a test server.
#[cfg(any(feature = "postgres", feature = "mysql"))]
struct Scratch {
    admin: String,
    name: String,
    url: String,
    mysql: bool,
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
impl Scratch {
    async fn new(var: &str, test: &str, mysql: bool) -> Option<Scratch> {
        use navree_db::sea_orm::{ConnectionTrait, Database};
        let admin = match std::env::var(var) {
            Ok(url) if !url.is_empty() => url,
            _ => {
                println!("SKIPPED {test}: set {var}");
                return None;
            }
        };
        let name = format!("navree_media_{}", Uuid::now_v7().simple());
        let conn = Database::connect(&admin).await.unwrap();
        let create = if mysql {
            format!("CREATE DATABASE `{name}` CHARACTER SET utf8mb4")
        } else {
            format!("CREATE DATABASE {name}")
        };
        conn.execute_unprepared(&create).await.unwrap();
        conn.close().await.unwrap();
        let (base, rest) = admin.rsplit_once('/').unwrap();
        let options = rest.find('?').map_or("", |at| &rest[at..]);
        let url = format!("{base}/{name}{options}");
        Some(Scratch {
            admin,
            name,
            url,
            mysql,
        })
    }

    async fn drop(self) {
        use navree_db::sea_orm::{ConnectionTrait, Database};
        let conn = Database::connect(&self.admin).await.unwrap();
        let drop = if self.mysql {
            format!("DROP DATABASE IF EXISTS `{}`", self.name)
        } else {
            format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name)
        };
        conn.execute_unprepared(&drop).await.unwrap();
    }

    async fn run(self) {
        let app = app(
            TestApp::builder()
                .config("database.url", self.url.as_str())
                .config("database.migrate_on_boot", true),
        )
        .build()
        .await;
        store_flow(&app).await;
        app.shutdown().await.unwrap();
        self.drop().await;
    }
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn the_store_works_on_postgres() {
    if let Some(scratch) =
        Scratch::new("NAVREE_TEST_POSTGRES_URL", "the_store_works_on_postgres", false).await
    {
        scratch.run().await;
    }
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn the_store_works_on_mysql() {
    if let Some(scratch) =
        Scratch::new("NAVREE_TEST_MYSQL_URL", "the_store_works_on_mysql", true).await
    {
        scratch.run().await;
    }
}
