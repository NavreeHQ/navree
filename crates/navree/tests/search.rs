//! The `search` feature through the facade: `#[derive(Searchable)]` expands
//! to `::navree::search` paths, and `SearchModule` comes with the prelude.
//! Run with `cargo test -p navree --features search --test search`.
#![cfg(feature = "search")]

use navree::db::{DatabaseModule, Db, MigrationSet, PageQuery, Repo};
use navree::prelude::*;
use navree::search::{Engine, Search, Searchable};
use navree::testing::TestApp;
use sea_orm::{ActiveValue::Set, ConnectionTrait};

mod note {
    use navree::search::Searchable;
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Searchable)]
    #[sea_orm(table_name = "notes")]
    #[searchable(name = "Note")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        #[searchable(boost = 2)]
        pub title: String,
        #[searchable(filter)]
        pub pinned: bool,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

struct CreateNotes;

impl sea_orm_migration::MigrationName for CreateNotes {
    fn name(&self) -> &str {
        "m20261026_000001_create_notes"
    }
}

#[sea_orm_migration::async_trait::async_trait]
impl sea_orm_migration::MigrationTrait for CreateNotes {
    async fn up(&self, manager: &sea_orm_migration::SchemaManager) -> Result<(), sea_orm::DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE notes (id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL, pinned BOOLEAN NOT NULL)",
            )
            .await
            .map(drop)
    }
}

#[module(
    imports = [SearchModule::for_feature::<note::Entity>()],
    register = Self::wire,
)]
struct NotesModule;

impl NotesModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("notes").migration(CreateNotes));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_facade_derives_and_searches() {
    assert_eq!(note::Entity::settings().name(), "Note");
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
    Repo::<note::Entity>::insert(
        &uow,
        note::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            title: Set("Lovelace renewal".into()),
            pinned: Set(true),
        },
    )
    .await
    .unwrap();
    uow.commit().await.unwrap();
    let search = app.get::<Search>().await;
    search.flush_pending().await.unwrap();
    let page = search
        .query::<note::Entity>("lovelace")
        .filter("pinned", true)
        .paginate(PageQuery::default())
        .await
        .unwrap();
    assert_eq!(page.meta.total, 1);
    assert_eq!(page.data[0].title, "Lovelace renewal");
}
