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
                t.index("workspace_id")
                    .name("deal_notes_workspace_id_index");
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

/// The source of model `name`, with container attribute `attributes`
/// (none when empty).
fn model(name: &str, attributes: &str) -> String {
    let attribute = if attributes.is_empty() {
        String::new()
    } else {
        format!("#[model({attributes})]\n")
    };
    format!(
        "use navree::orm::Model;\n\n#[derive(Clone, Debug, Default, Model, serde::Serialize)]\n{attribute}pub struct {name} {{\n    #[model(column = \"id\")]\n    pub id: Uuid,\n}}\n"
    )
}

/// An app source tree: deals, deal notes, countries (registered), projects
/// (`#[model(workspace)]`) and tags (not classified), with a service
/// reaching some of them directly.
fn shop(root: &Path) {
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
    );
    write(
        root,
        "src/lib.rs",
        "pub mod countries;\npub mod deals;\npub mod projects;\npub mod tags;\n",
    );
    write(
        root,
        "src/countries.rs",
        &model("Country", "table = \"countries\", primary_key = \"code\""),
    );
    write(root, "src/tags.rs", &model("Tag", ""));
    write(root, "src/projects.rs", &model("Project", "workspace"));
    write(
        root,
        "src/deals/mod.rs",
        "pub mod deal;\npub mod note;\npub mod service;\n",
    );
    write(
        root,
        "src/deals/deal.rs",
        &model("Deal", "table = \"deals\", subject = \"Deal\""),
    );
    write(
        root,
        "src/deals/note.rs",
        &model("DealNote", "table = \"deal_notes\""),
    );
    write(
        root,
        "src/deals/service.rs",
        r#"use navree::orm::Model;
use navree::workspace::WorkspaceRepo;

use super::{deal::Deal, note::DealNote};
use crate::countries::Country;
use crate::projects::Project;

pub async fn index(uow: Uow, ws: Uuid) {
    let leak = Deal::query().get(&uow).await;
    let scoped = WorkspaceRepo::<Deal>::query(ws).get(&uow).await;
    let note = DealNote::create(&uow, note).await;
    let countries = Country::all(&uow).await;
    let projects = Project::query().get(&uow).await;
    let every = Project::query().without_workspace_scope().count(&uow).await;
    // An admin report across workspaces. navree:allow(workspace-repo)
    let all = Deal::all(&uow).await;
}
"#,
    );
    // Test code is not read.
    write(
        root,
        "tests/it.rs",
        "fn t() { shop::deals::deal::Deal::query(); }\n",
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
async fn every_model_must_be_classified() {
    let app = builder().module(ClassifiedModule).build().await;
    let dir = TempDirGuard::new();
    shop(dir.path());
    let report = report(&app, dir.path()).await;
    let entities = report.check("workspace.entities").unwrap();
    // countries, deals, deal_notes, projects (classified by
    // #[model(workspace)] alone) and tags.
    assert_eq!(entities.outcome.checked, 5);
    assert_eq!(
        entities.outcome.findings,
        vec![
            Finding::error(
                "src/tags.rs:4",
                "model `Tag` (table `tags`) is not classified as a workspace or a global entity"
            )
            .hint(
                "classify it in its module: #[model(workspace)] or m.workspace_entity::<Tag>() with impl WorkspaceScoped for Tag (its rows belong to one workspace), m.global_entity::<Tag>() (shared by every workspace) or m.library::<Tag>() (a shared library)"
            )
        ]
    );
}

#[tokio::test]
async fn workspace_models_must_be_reached_through_a_workspace_scope() {
    let app = builder().module(ClassifiedModule).build().await;
    let dir = TempDirGuard::new();
    shop(dir.path());
    let report = report(&app, dir.path()).await;
    let repo = report.check("workspace.repo").unwrap();
    assert_eq!(repo.outcome.checked, 8, "{repo:?}");
    assert_eq!(
        subjects(repo),
        [
            (
                Severity::Error,
                "src/deals/service.rs:9".to_owned(),
                "reaches workspace entity `deals` directly with `Deal::query`".to_owned()
            ),
            (
                Severity::Error,
                "src/deals/service.rs:11".to_owned(),
                "reaches workspace entity `deal_notes` directly with `DealNote::create`".to_owned()
            ),
            (
                Severity::Error,
                "src/deals/service.rs:14".to_owned(),
                "leaves the workspace scope of `projects` with `Project::query().without_workspace_scope()`"
                    .to_owned()
            ),
        ]
    );
    assert_eq!(
        repo.outcome.findings[0].hint.as_deref(),
        Some(
            "go through WorkspaceRepo::<Deal> (query, find, find_or_fail, insert, update, delete), declare the model #[model(workspace)] so it scopes its own queries, or mark a deliberate cross-workspace access with // navree:allow(workspace-repo)"
        )
    );
    assert_eq!(
        repo.outcome.findings[2].hint.as_deref(),
        Some(
            "name the workspace with .for_workspace(id) or WorkspaceRepo::<Project>, or mark a deliberate cross-workspace access with // navree:allow(workspace-repo)"
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
    let checked = if Server::current() == Server::MySql {
        0
    } else {
        3
    };
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
    // One unclassified model, three direct accesses, one table without the
    // column; one table without an index (and, on PostgreSQL, two without
    // row-level security; on MySQL the tables are not checked).
    let expected = match Server::current() {
        Server::Postgres => "conformity: 5 errors, 3 warnings",
        Server::MySql => "conformity: 4 errors, 0 warnings",
        Server::Sqlite => "conformity: 5 errors, 1 warning",
    };
    assert_eq!(error.to_string(), expected);
}
