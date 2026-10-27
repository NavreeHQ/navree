//! Seeders (`db:seed`, `migrate:fresh --seed`) and model factories.

use std::sync::{Arc, Mutex};

use navree_db::{
    DatabaseModule, Db, Factory, MigrationSet, ModuleBuilderDbExt, Seeder, SeederCalls, create,
    create_many, run_seeders,
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

/// The root seeder: calls the others.
struct RootSeeder(Arc<SeedLog>);

impl Injectable for RootSeeder {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<SeedLog>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(Self(r.get::<SeedLog>().await?)) })
    }
}

impl Seeder for RootSeeder {
    fn name(&self) -> &'static str {
        "root"
    }

    fn is_root(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, ctx: &'a AppContext, _db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push("root");
            ctx.call::<BaseSeeder>().await?;
            ctx.call_once::<BaseSeeder>().await?;
            ctx.call_silent::<QuietSeeder>().await?;
            ctx.call_seeder(&CountSeeder {
                log: self.0.clone(),
                count: 2,
            })
            .await?;
            Ok(())
        })
    }
}

/// Not registered (built for the call); runs without model events.
struct QuietSeeder(Arc<SeedLog>);

impl Injectable for QuietSeeder {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<SeedLog>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(Self(r.get::<SeedLog>().await?)) })
    }
}

impl Seeder for QuietSeeder {
    fn name(&self) -> &'static str {
        "quiet"
    }

    fn without_model_events(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, _ctx: &'a AppContext, _db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            assert!(navree_db::seeding_without_model_events());
            self.0.0.lock().unwrap().push("quiet");
            Ok(())
        })
    }
}

/// A seeder with a parameter, called as a value.
struct CountSeeder {
    log: Arc<SeedLog>,
    count: usize,
}

impl Seeder for CountSeeder {
    fn name(&self) -> &'static str {
        "count"
    }

    fn run<'a>(&'a self, _ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            assert!(!navree_db::seeding_without_model_events());
            self.log.0.lock().unwrap().push("count");
            create_many(&NoteFactory, db.conn(), self.count).await?;
            Ok(())
        })
    }
}

struct RootModule;

impl Module for RootModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(NotesModule).seeder::<RootSeeder>();
    }
}

#[tokio::test]
async fn a_root_seeder_runs_alone_and_calls_the_others() {
    let temp = TempDb::new();
    let mut options = temp.options();
    options.migrate_on_boot = true;
    let app = Navree::app()
        .module(DatabaseModule::for_root(options))
        .module(RootModule)
        .boot()
        .await
        .expect("boots");
    app.run_command("db:seed", vec![]).await.expect("seeds");
    assert_eq!(
        seed_log(&app).await,
        ["root", "base", "quiet", "count"],
        "only the root ran by itself; base once; the notes seeder not at all"
    );
    let db = app.ctx().get::<Db>().await.unwrap();
    assert_eq!(count(&db, "notes").await, 2);
    // Named seeders still run on their own.
    let ran = run_seeders(app.ctx(), &["base".to_owned()]).await.unwrap();
    assert_eq!(ran, ["base"]);
}

/// A second database, named `archive`, with the notes table.
struct Archive(String);

impl Module for Archive {
    fn register(&self, m: &mut ModuleBuilder) {
        let url = self.0.clone();
        m.factory(
            navree_kernel::Factory::<Db>::new(move |_| {
                let url = url.clone();
                async move {
                    let db = Db::connect(&navree_db::DbOptions {
                        url,
                        ..navree_db::DbOptions::default()
                    })
                    .await?;
                    let manager = sea_orm_migration::SchemaManager::new(db.conn());
                    sea_orm_migration::MigrationTrait::up(&CreateNotes, &manager).await?;
                    Ok(Arc::new(db))
                }
            })
            .named("archive"),
        );
    }
}

#[tokio::test]
async fn db_seed_seeds_a_named_database_and_guards_production() {
    let temp = TempDb::new();
    let archive = TempDb::new();
    let mut options = temp.options();
    options.migrate_on_boot = true;
    let app = Navree::app()
        .module(navree_config::ConfigModule::for_root(
            navree_config::ConfigSources::new().set("env", "production"),
        ))
        .module(DatabaseModule::for_root(options))
        .module(NotesModule)
        .module(Archive(archive.options().url))
        .boot()
        .await
        .expect("boots");
    let refused = app
        .run_command("db:seed", vec!["notes".into()])
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("pass --force"), "{refused}");
    app.run_command(
        "db:seed",
        vec!["notes".into(), "--database".into(), "archive".into(), "--force".into()],
    )
    .await
    .expect("seeds the archive");
    let db = app.ctx().get::<Db>().await.unwrap();
    assert_eq!(count(&db, "notes").await, 0);
    let named = app.ctx().get_named::<Db>("archive").await.unwrap();
    assert_eq!(count(&named, "notes").await, 3);
    let unknown = app
        .run_command("db:seed", vec!["--database=nowhere".into(), "--force".into()])
        .await
        .unwrap_err()
        .to_string();
    assert!(unknown.contains("no database connection named `nowhere`"), "{unknown}");
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
