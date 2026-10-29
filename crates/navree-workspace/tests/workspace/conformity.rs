//! The workspace conformity checks against a booted app and an app source
//! tree on disk: `workspace.entities`, `workspace.repo`, `workspace.tables`.

use std::path::Path;

use navree_contracts::{
    CONFORMITY_COMMAND, CheckReport, ConformityOptions, ConformityReport, Finding, Severity,
};
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{MigrationSet, ModuleBuilderDbExt};
use navree_kernel::{Module, ModuleBuilder};
use navree_testing::{TempDirGuard, TestApp};
use navree_workspace::{ModuleBuilderWorkspaceExt, WorkspaceScoped};
use uuid::Uuid;

use crate::database::Server;
use crate::support::{builder, uuid};

/// `countries`: shared by every workspace.
#[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, serde::Serialize)]
#[model(table = "countries", primary_key = "code")]
struct Country {
    code: String,
}

/// `deal_notes`: rows of one workspace, not an access subject.
#[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, serde::Serialize)]
#[model(table = "deal_notes")]
struct DealNote {
    id: Uuid,
    workspace_id: Uuid,
    body: String,
}

impl WorkspaceScoped for DealNote {}

/// `labels`: declared a workspace entity, but its table has no workspace
/// column.
#[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, serde::Serialize)]
#[model(table = "labels")]
struct Label {
    id: Uuid,
    name: String,
}

impl WorkspaceScoped for Label {}

/// `deal_notes` with an index on `workspace_id`, and `labels` without the
/// column.
struct CreateNotesAndLabels;

impl Migration for CreateNotesAndLabels {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("deal_notes", |t| {
                uuid(t, "id", dialect).primary();
                uuid(t, "workspace_id", dialect);
                t.string("body");
                t.index("workspace_id").name("deal_notes_workspace_id_index");
            })
            .await?;
        schema
            .create("labels", |t| {
                uuid(t, "id", dialect).primary();
                t.string("name");
            })
            .await
    }

    async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
        Ok(())
    }
}

/// Classifies the test models.
struct ClassifiedModule;

impl Module for ClassifiedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("classified")
                .after(["workspace"])
                .migration(SchemaMigration::new(
                    "m20261027_000001_create_deal_notes_and_labels",
                    CreateNotesAndLabels,
                )),
        )
        .global_entity::<Country>()
        .workspace_entity::<DealNote>()
        .workspace_entity::<Label>();
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
use navree::workspace::sea_compat::WorkspaceRepo;

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
                "register it in its module: m.sea_workspace_entity::<tags::Entity>() (its rows belong to one workspace) or m.sea_global_entity::<tags::Entity>() (shared by every workspace), or port it to a navree-orm model"
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
            "go through sea_compat::WorkspaceRepo::<entity::Entity> (select, find, insert, update, delete), or mark a deliberate cross-workspace access with // navree:allow(workspace-repo)"
        )
    );
}

#[tokio::test]
async fn workspace_tables_need_an_indexed_workspace_column() {
    let app = builder().module(ClassifiedModule).build().await;
    let dir = TempDirGuard::new();
    let report = report(&app, dir.path()).await;
    let tables = report.check("workspace.tables").unwrap();
    // deals (workspace_scoped in the fixtures), deal_notes and labels; on
    // MySQL, whose catalog the check does not read, none.
    let checked = if Server::current() == Server::MySql { 0 } else { 3 };
    assert_eq!(tables.outcome.checked, checked);
    let rls_off = |table: &str| {
        (
            Severity::Warning,
            table.to_owned(),
            "row-level security is off".to_owned(),
        )
    };
    let no_index = (
        Severity::Warning,
        "deals".to_owned(),
        "no index starts with `workspace_id`".to_owned(),
    );
    let no_column = (
        Severity::Error,
        "labels".to_owned(),
        "has no `workspace_id` column".to_owned(),
    );
    let expected = match Server::current() {
        // The catalog also says where row-level security is off.
        Server::Postgres => vec![rls_off("deal_notes"), no_index, rls_off("deals"), no_column],
        Server::MySql => vec![],
        Server::Sqlite => vec![no_index, no_column],
    };
    assert_eq!(subjects(tables), expected);
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
    // column; one table without an index (and, on PostgreSQL, two without
    // row-level security; on MySQL the tables are not checked).
    let expected = match Server::current() {
        Server::Postgres => "conformity: 4 errors, 3 warnings",
        Server::MySql => "conformity: 3 errors, 0 warnings",
        Server::Sqlite => "conformity: 4 errors, 1 warning",
    };
    assert_eq!(error.to_string(), expected);
}
