//! The `search` feature through the facade: `#[derive(Searchable)]` on a
//! navree-orm model expands to `::navree::search` paths, and
//! `SearchModule` comes with the prelude.
//! Run with `cargo test -p navree --features search --test search`.

use navree::db::{DatabaseModule, Db, MigrationSet, PageQuery};
use navree::prelude::*;
use navree::search::{Engine, Search, Searchable};
use navree::testing::TestApp;

#[derive(Model, Searchable, Clone, Debug, Default, PartialEq, serde::Serialize)]
#[model(table = "notes")]
#[searchable(name = "Note")]
/// A note: its title searched, `pinned` filterable.
pub struct Note {
    /// The id (UUID v7).
    pub id: uuid::Uuid,
    /// The title.
    #[searchable(boost = 2)]
    pub title: String,
    /// Whether it is pinned.
    #[searchable(filter)]
    pub pinned: bool,
}

mod m20261026_000001_create_notes {
    use navree::schema::prelude::*;

    pub struct Migration;

    impl navree::schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create("notes", |t| {
                    t.uuid("id").primary();
                    t.string("title");
                    t.boolean("pinned");
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("notes").await
        }
    }
}

#[module(
    imports = [SearchModule::for_feature::<Note>()],
    register = Self::wire,
)]
struct NotesModule;

impl NotesModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("notes")
                .migration(navree::schema::migration!(m20261026_000001_create_notes)),
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_facade_derives_and_searches() {
    assert_eq!(Note::settings().name(), "Note");
    assert_eq!(Note::settings().index(), "notes");
    let app = TestApp::builder()
        .temp_database()
        .config("search.sync", "inline")
        .module(DatabaseModule::from_config())
        .module(SearchModule::for_root(navree::search::SearchOptions {
            sync: navree::search::SyncMode::Inline,
            ..Engine::memory().into()
        }))
        .module(NotesModule)
        .build()
        .await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    Note::create(
        &uow,
        Note {
            id: uuid::Uuid::now_v7(),
            title: "Lovelace renewal".into(),
            pinned: true,
        },
    )
    .await
    .unwrap();
    uow.commit().await.unwrap();
    let search = app.get::<Search>().await;
    search.flush_pending().await.unwrap();
    let page = search
        .query::<Note>("lovelace")
        .filter("pinned", true)
        .paginate(PageQuery::default())
        .await
        .unwrap();
    assert_eq!(page.meta.total, 1);
    assert_eq!(page.data[0].title, "Lovelace renewal");
}
