//! The workspace conformity checks against a booted app and an app source
//! tree on disk: `workspace.entities`, `workspace.repo`, `workspace.tables`.

use std::path::Path;

use navree_contracts::{
    CONFORMITY_COMMAND, CheckReport, ConformityOptions, ConformityReport, Finding, Severity,
};
use navree_db::{MigrationSet, ModuleBuilderDbExt};
use navree_kernel::{Module, ModuleBuilder};
use navree_testing::{TempDirGuard, TestApp};
use navree_workspace::{ModuleBuilderWorkspaceExt, WorkspaceScoped};
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

use crate::support::builder;

/// `countries`: shared by every workspace.
mod country {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "countries")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub code: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `deal_notes`: rows of one workspace, not an access subject.
mod deal_note {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "deal_notes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub workspace_id: Uuid,
        pub body: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl WorkspaceScoped for deal_note::Entity {}

/// `labels`: declared a workspace entity, but its table has no workspace
/// column.
mod label {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "labels")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub name: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl WorkspaceScoped for label::Entity {}

/// `deal_notes` with an index on `workspace_id`, and `labels` without the
/// column.
struct CreateNotesAndLabels;

impl MigrationName for CreateNotesAndLabels {
    fn name(&self) -> &str {
        "m20261027_000001_create_deal_notes_and_labels"
    }
}

#[async_trait]
impl MigrationTrait for CreateNotesAndLabels {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("deal_notes")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("workspace_id").uuid().not_null())
                    .col(ColumnDef::new("body").string().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("deal_notes_workspace_id_index")
                    .table("deal_notes")
                    .col("workspace_id")
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("labels")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("name").string().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

/// Classifies the test entities.
struct ClassifiedModule;

impl Module for ClassifiedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("classified")
                .after(["workspace"])
                .migration(CreateNotesAndLabels),
        )
        .global_entity::<country::Entity>()
        .workspace_entity::<deal_note::Entity>()
        .workspace_entity::<label::Entity>();
    }
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn entity(table: &str) -> String {
    format!(
        "use sea_orm::entity::prelude::*;\n\n#[derive(Clone, Debug, DeriveEntityModel)]\n#[sea_orm(table_name = \"{table}\")]\npub struct Model {{\n    #[sea_orm(primary_key)]\n    pub id: Uuid,\n}}\n"
    )
}

/// An app source tree: deals, deal notes, countries (classified) and tags
/// (not classified), with a service reaching some of them directly.
fn shop(root: &Path) {
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
    );
    write(
        root,
        "src/lib.rs",
        "pub mod countries;\npub mod deals;\npub mod tags;\n",
    );
    write(root, "src/countries.rs", &entity("countries"));
    write(root, "src/tags.rs", &entity("tags"));
    write(
        root,
        "src/deals/mod.rs",
        "pub mod entity;\npub mod note;\npub mod service;\n",
    );
    write(root, "src/deals/entity.rs", &entity("deals"));
    write(root, "src/deals/note.rs", &entity("deal_notes"));
    write(
        root,
        "src/deals/service.rs",
        r#"use navree::db::Repo;
use navree::workspace::WorkspaceRepo;

use super::{entity, note};
use crate::countries;

pub async fn index(ws: Uuid) {
    let leak = entity::Entity::find();
    let scoped = WorkspaceRepo::<entity::Entity>::select(ws);
    let note = Repo::<note::Entity>::insert(&uow, am).await;
    let countries = countries::Entity::find();
    // An admin report across workspaces. navree:allow(workspace-repo)
    let all = entity::Entity::find();
}
"#,
    );
    // Test code is not read.
    write(
        root,
        "tests/it.rs",
        "fn t() { shop::deals::entity::Entity::find(); }\n",
    );
}

async fn report(app: &TestApp, root: &Path) -> ConformityReport {
    ConformityReport::run(
        app.ctx(),
        &ConformityOptions::new(root).only(["workspace".to_owned()]),
    )
    .await
}

fn subjects(check: &CheckReport) -> Vec<(Severity, String, String)> {
    check
        .outcome
        .findings
        .iter()
        .map(|f| (f.severity, f.subject.clone(), f.message.clone()))
        .collect()
}

#[tokio::test]
async fn every_entity_must_be_classified() {
    let app = builder().module(ClassifiedModule).build().await;
    let dir = TempDirGuard::new();
    shop(dir.path());
    let report = report(&app, dir.path()).await;
    let entities = report.check("workspace.entities").unwrap();
    assert_eq!(entities.outcome.checked, 4);
    assert_eq!(
        entities.outcome.findings,
        vec![
            Finding::error(
                "src/tags.rs:4",
                "entity `tags` is not classified as a workspace or a global entity"
            )
            .hint(
                "register it in its module: m.workspace_entity::<tags::Entity>() (its rows belong to one workspace) or m.global_entity::<tags::Entity>() (shared by every workspace)"
            )
        ]
    );
}

#[tokio::test]
async fn workspace_entities_must_be_reached_through_workspace_repo() {
    let app = builder().module(ClassifiedModule).build().await;
    let dir = TempDirGuard::new();
    shop(dir.path());
    let report = report(&app, dir.path()).await;
    let repo = report.check("workspace.repo").unwrap();
    assert_eq!(repo.outcome.checked, 7, "{repo:?}");
    assert_eq!(
        subjects(repo),
        [
            (
                Severity::Error,
                "src/deals/service.rs:8".to_owned(),
                "reaches workspace entity `deals` directly with `entity::Entity::find`".to_owned()
            ),
            (
                Severity::Error,
                "src/deals/service.rs:10".to_owned(),
                "reaches workspace entity `deal_notes` directly with `Repo::<note::Entity>`"
                    .to_owned()
            ),
        ]
    );
    assert_eq!(
        repo.outcome.findings[0].hint.as_deref(),
        Some(
            "go through WorkspaceRepo::<entity::Entity> (select, find, insert, update, delete), or mark a deliberate cross-workspace access with // navree:allow(workspace-repo)"
        )
    );
}

#[tokio::test]
async fn workspace_tables_need_an_indexed_workspace_column() {
    let app = builder().module(ClassifiedModule).build().await;
    let dir = TempDirGuard::new();
    let report = report(&app, dir.path()).await;
    let tables = report.check("workspace.tables").unwrap();
    // deals (workspace_scoped in the fixtures), deal_notes and labels.
    assert_eq!(tables.outcome.checked, 3);
    assert_eq!(
        subjects(tables),
        [
            (
                Severity::Warning,
                "deals".to_owned(),
                "no index starts with `workspace_id`".to_owned()
            ),
            (
                Severity::Error,
                "labels".to_owned(),
                "has no `workspace_id` column".to_owned()
            ),
        ]
    );
    // Without sources, the source checks are skipped.
    let entities = report.check("workspace.entities").unwrap();
    assert!(
        entities
            .outcome
            .skipped
            .as_deref()
            .unwrap()
            .starts_with("no Rust sources under "),
        "{entities:?}"
    );
}

#[tokio::test]
async fn the_checks_are_skipped_under_database_isolation() {
    let dir = TempDirGuard::new();
    let template = format!(
        "sqlite://{}?mode=rwc",
        dir.path().join("workspace-{id}.sqlite").display()
    );
    let app = builder()
        .config("workspace.isolation", "database")
        .config("workspace.database_url", template)
        .build()
        .await;
    shop(dir.path());
    let report = report(&app, dir.path()).await;
    assert_eq!(report.checks.len(), 3);
    for check in &report.checks {
        assert_eq!(
            check.outcome.skipped.as_deref(),
            Some("workspace.isolation is `database`: each workspace's tables are apart already"),
            "{check:?}"
        );
    }
}

#[tokio::test]
async fn navree_check_runs_them_through_the_conformity_command() {
    let app = builder().module(ClassifiedModule).build().await;
    let dir = TempDirGuard::new();
    shop(dir.path());
    let root = dir.path().display().to_string();
    let error = app
        .run_command(
            CONFORMITY_COMMAND,
            &["--root", &root, "--only", "workspace"],
        )
        .await
        .unwrap_err();
    // One unclassified entity, two direct accesses, one table without the
    // column; one table without an index.
    assert_eq!(error.to_string(), "conformity: 4 errors, 1 warning");
}
