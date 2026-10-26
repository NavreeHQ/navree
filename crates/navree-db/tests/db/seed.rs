//! Seeders (`db:seed`, `migrate:fresh --seed`) and model factories.

use std::sync::{Arc, Mutex};

use navree_db::{
    DatabaseModule, Db, Factory, MigrationSet, ModuleBuilderDbExt, Seeder, create, create_many,
    run_seeders,
};
use navree_kernel::{
    App, AppContext, BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree,
    ProviderKey, Resolver,
};
use sea_orm::ActiveValue::Set;
use sea_orm::EntityTrait;

use crate::fixtures::{CreateNotes, count, note, notes_db};
use crate::support::TempDb;

/// Notes titled `note-<n>`.
struct NoteFactory;

impl Factory for NoteFactory {
    type Entity = note::Entity;

    fn definition(&self, n: usize) -> note::ActiveModel {
        note::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            title: Set(format!("note-{n}")),
            body: Set("generated".into()),
        }
    }
}

#[tokio::test]
async fn factory_creates_numbered_records() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let notes = create_many(&NoteFactory, db.conn(), 3).await.unwrap();
    assert_eq!(
        notes.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(),
        ["note-0", "note-1", "note-2"]
    );
    assert_eq!(note::Entity::find().all(db.conn()).await.unwrap(), notes);

    let uow = db.uow(None, None).await;
    let one = create(&NoteFactory, &*uow.tx().await.unwrap()).await;
    assert!(one.is_err(), "note-0 again violates the unique title");
}

/// Order in which seeders ran.
#[derive(Default)]
struct SeedLog(Mutex<Vec<&'static str>>);

/// Seeds three notes with the factory and logs its run.
struct NoteSeeder(Arc<SeedLog>);

impl Injectable for NoteSeeder {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<SeedLog>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(Self(r.get::<SeedLog>().await?)) })
    }
}

impl Seeder for NoteSeeder {
    fn name(&self) -> &'static str {
        "notes"
    }

    fn run<'a>(&'a self, _ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push("notes");
            create_many(&NoteFactory, db.conn(), 3).await?;
            Ok(())
        })
    }
}

/// Only logs; registered by a module the notes module imports.
struct BaseSeeder(Arc<SeedLog>);

impl Injectable for BaseSeeder {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<SeedLog>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(Self(r.get::<SeedLog>().await?)) })
    }
}

impl Seeder for BaseSeeder {
    fn name(&self) -> &'static str {
        "base"
    }

    fn run<'a>(&'a self, _ctx: &'a AppContext, _db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push("base");
            Ok(())
        })
    }
}

struct BaseModule;

impl Module for BaseModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global()
            .value(SeedLog::default())
            .exports::<SeedLog>()
            .seeder::<BaseSeeder>();
    }
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(BaseModule)
            .migrations(MigrationSet::new("notes").migration(CreateNotes))
            .seeder::<NoteSeeder>();
    }
}

async fn boot(temp: &TempDb) -> App {
    let mut options = temp.options();
    options.migrate_on_boot = true;
    Navree::app()
        .module(DatabaseModule::for_root(options))
        .module(NotesModule)
        .boot()
        .await
        .expect("boots")
}

async fn seed_log(app: &App) -> Vec<&'static str> {
    app.ctx()
        .get::<SeedLog>()
        .await
        .unwrap()
        .0
        .lock()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn seeders_run_via_db_seed() {
    let temp = TempDb::new();
    let app = boot(&temp).await;
    app.run_command("db:seed", vec![]).await.expect("seeds");
    let db = app.ctx().get::<Db>().await.unwrap();
    assert_eq!(count(&db, "notes").await, 3);
    assert_eq!(
        seed_log(&app).await,
        ["base", "notes"],
        "module dependency order"
    );
}

#[tokio::test]
async fn db_seed_runs_only_the_named_seeders() {
    let temp = TempDb::new();
    let app = boot(&temp).await;
    let ran = run_seeders(app.ctx(), &["base".to_owned()]).await.unwrap();
    assert_eq!(ran, ["base"]);
    assert_eq!(seed_log(&app).await, ["base"]);

    let error = app
        .run_command("db:seed", vec!["nope".into()])
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("nope"), "{error}");
    assert!(error.contains("base") && error.contains("notes"), "{error}");
}

#[tokio::test]
async fn migrate_fresh_with_seed_reseeds() {
    let temp = TempDb::new();
    let app = boot(&temp).await;
    let db = app.ctx().get::<Db>().await.unwrap();
    app.run_command("db:seed", vec![]).await.unwrap();
    app.run_command("migrate:fresh", vec!["--seed".into()])
        .await
        .unwrap();
    assert_eq!(
        count(&db, "notes").await,
        3,
        "dropped, migrated, seeded once"
    );
}
