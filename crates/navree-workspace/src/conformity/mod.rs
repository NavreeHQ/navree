//! The workspace conformity checks `navree check` runs, and the entity
//! classification they read.
//!
//! - `workspace.entities`: every navree-orm model the app's source defines
//!   (a struct deriving `Model`) is classified exactly once, by its table: a
//!   workspace entity (`m.workspace_scoped::<M>()`,
//!   `m.workspace_entity::<M>()`, or `#[model(workspace)]` on the model,
//!   which scopes its own queries and so classifies it on its own), a global
//!   one (`m.global_entity::<M>()`) or a library (`m.library::<M>()`).
//! - `workspace.repo`: the workspace models are reached only through a
//!   workspace scope (a source scan; `// navree:allow(workspace-repo)`
//!   allows one access). A registered workspace model that does not scope
//!   itself is reached through `WorkspaceRepo`, never through the `Model`
//!   entry points (`Deal::query()`, `Deal::find(..)`, `Deal::create(..)`,
//!   ..); a `#[model(workspace)]` model is reached any way but with its
//!   scope left out (`.without_workspace_scope()`,
//!   `.without_global_scopes()`).
//! - `workspace.tables`: each registered workspace table (`workspace_scoped`,
//!   `workspace_entity`) has its workspace column, an index that starts
//!   with it and, on PostgreSQL, row-level security.
//!
//! All three apply to row isolation; under schema and database isolation
//! each workspace's tables are apart already, and they are skipped.

mod source;
mod tables;

use std::collections::{BTreeMap, BTreeSet};

use navree_contracts::{CheckContext, CheckOutcome, ConformityCheck, Finding};
use navree_db::Db;

use crate::{Isolation, WorkspaceConfig, WorkspaceScoped};

pub(crate) use source::ALLOW_MARKER;
use source::{AccessKind, ModelDef, Project};

/// How an entity is classified, contributed by the module that owns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntityClass {
    /// The entity's table.
    pub(crate) table: String,
    /// Workspace or global.
    pub(crate) kind: EntityKind,
}

/// Workspace or global.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntityKind {
    /// Rows belong to one workspace, by this column.
    Workspace {
        /// [`WorkspaceScoped::WORKSPACE_COLUMN`].
        column: &'static str,
    },
    /// Shared by every workspace.
    Global,
    /// A library: global, organization and workspace rows, by its scope
    /// column (`m.library::<M>()`).
    Library,
}

impl EntityClass {
    /// Model `M` as a workspace entity.
    pub(crate) fn workspace<M: WorkspaceScoped>() -> Self {
        Self::workspace_table(M::meta().table(), M::WORKSPACE_COLUMN)
    }

    /// Table `table` as a workspace entity's, by `column`.
    pub(crate) fn workspace_table(table: &str, column: &'static str) -> Self {
        Self {
            table: table.to_owned(),
            kind: EntityKind::Workspace { column },
        }
    }

    /// Table `table` as a global entity's.
    pub(crate) fn global(table: &str) -> Self {
        Self {
            table: table.to_owned(),
            kind: EntityKind::Global,
        }
    }

    /// Table `table` as a library.
    pub(crate) fn library(table: &str) -> Self {
        Self {
            table: table.to_owned(),
            kind: EntityKind::Library,
        }
    }
}

/// The classifications of the app: workspace tables (with their column),
/// global tables and libraries.
#[derive(Debug, Default)]
struct Registry {
    workspace: BTreeMap<String, &'static str>,
    global: BTreeSet<String>,
    library: BTreeSet<String>,
}

impl Registry {
    fn of(cx: &CheckContext) -> Registry {
        let mut registry = Registry::default();
        for class in cx.app().contributions::<EntityClass>() {
            match class.value.kind {
                EntityKind::Workspace { column } => {
                    registry.workspace.insert(class.value.table.clone(), column);
                }
                EntityKind::Global => {
                    registry.global.insert(class.value.table.clone());
                }
                EntityKind::Library => {
                    registry.library.insert(class.value.table.clone());
                }
            }
        }
        registry
    }
}

/// `Some(reason)` when the checks do not apply to the app's isolation.
async fn not_row_isolation(cx: &CheckContext) -> Result<Option<String>, navree_kernel::BoxError> {
    let config = cx.app().get::<WorkspaceConfig>().await?;
    Ok(match config.isolation {
        Isolation::Row => None,
        other => Some(format!(
            "workspace.isolation is `{}`: each workspace's tables are apart already",
            other.as_str()
        )),
    })
}

/// The project under the check's root, or the reason it has none.
fn load(cx: &CheckContext) -> Result<Project, String> {
    let project = Project::load(cx.root());
    if project.file_count() == 0 {
        Err(format!("no Rust sources under {}", cx.root().display()))
    } else {
        Ok(project)
    }
}

/// The three checks, in the order they run.
pub(crate) fn checks() -> [ConformityCheck; 3] {
    [entities_check(), repo_check(), tables_check()]
}

fn entities_check() -> ConformityCheck {
    ConformityCheck::new(
        "workspace.entities",
        "Every entity is classified as a workspace or a global entity",
        |cx| async move {
            if let Some(reason) = not_row_isolation(&cx).await? {
                return Ok(CheckOutcome::skipped(reason));
            }
            let project = match load(&cx) {
                Ok(project) => project,
                Err(reason) => return Ok(CheckOutcome::skipped(reason)),
            };
            let registry = Registry::of(&cx);
            let models = project.models();
            let findings = models
                .iter()
                .filter_map(|model| classification_finding(model, &registry));
            Ok(CheckOutcome::checked(models.len()).findings(findings.collect::<Vec<_>>()))
        },
    )
}

/// The finding for `model` when it is not classified exactly once.
/// `#[model(workspace)]` classifies a model as a workspace entity on its
/// own: it scopes its own queries to the current workspace.
fn classification_finding(model: &ModelDef, registry: &Registry) -> Option<Finding> {
    let classes: Vec<&str> = [
        (
            model.scopes_itself() || registry.workspace.contains_key(&model.table),
            "a workspace",
        ),
        (registry.global.contains(&model.table), "a global"),
        (registry.library.contains(&model.table), "a library"),
    ]
    .into_iter()
    .filter_map(|(classified, class)| classified.then_some(class))
    .collect();
    let name = &model.name;
    match classes.as_slice() {
        [_] => None,
        [.., last] if classes.len() > 1 => {
            let others = &classes[..classes.len() - 1];
            let listed = match others {
                [only] => format!("both {only} and {last}"),
                _ => format!("{} and {last}", others.join(", ")),
            };
            Some(
                Finding::error(
                    &model.location,
                    format!(
                        "model `{name}` (table `{}`) is classified as {listed} entity",
                        model.table
                    ),
                )
                .hint(format!(
                    "keep one classification for its table: #[model(workspace)] or m.workspace_entity::<{name}>() (a workspace entity), m.global_entity::<{name}>() or m.library::<{name}>()"
                )),
            )
        }
        _ => Some(
            Finding::error(
                &model.location,
                format!(
                    "model `{name}` (table `{}`) is not classified as a workspace or a global entity",
                    model.table
                ),
            )
            .hint(format!(
                "classify it in its module: #[model(workspace)] or m.workspace_entity::<{name}>() with impl WorkspaceScoped for {name} (its rows belong to one workspace), m.global_entity::<{name}>() (shared by every workspace) or m.library::<{name}>() (a shared library)"
            )),
        ),
    }
}

fn repo_check() -> ConformityCheck {
    ConformityCheck::new(
        "workspace.repo",
        "Workspace entities are reached only through WorkspaceRepo",
        |cx| async move {
            if let Some(reason) = not_row_isolation(&cx).await? {
                return Ok(CheckOutcome::skipped(reason));
            }
            let project = match load(&cx) {
                Ok(project) => project,
                Err(reason) => return Ok(CheckOutcome::skipped(reason)),
            };
            let registry = Registry::of(&cx);
            let workspace_models: Vec<ModelDef> = project
                .models()
                .into_iter()
                .filter(|m| m.scopes_itself() || registry.workspace.contains_key(&m.table))
                .collect();
            let findings: Vec<Finding> = project
                .direct_accesses(&workspace_models)
                .into_iter()
                .map(|access| {
                    let name = &access.model;
                    match access.kind {
                        AccessKind::Unscoped => Finding::error(
                            access.location,
                            format!(
                                "reaches workspace entity `{}` directly with `{}`",
                                access.table, access.expression
                            ),
                        )
                        .hint(format!(
                            "go through WorkspaceRepo::<{name}> (query, find, find_or_fail, insert, update, delete), declare the model #[model(workspace)] so it scopes its own queries, or mark a deliberate cross-workspace access with // {ALLOW_MARKER}"
                        )),
                        AccessKind::LeavesScope => Finding::error(
                            access.location,
                            format!(
                                "leaves the workspace scope of `{}` with `{}`",
                                access.table, access.expression
                            ),
                        )
                        .hint(format!(
                            "name the workspace with .for_workspace(id) or WorkspaceRepo::<{name}>, or mark a deliberate cross-workspace access with // {ALLOW_MARKER}"
                        )),
                    }
                })
                .collect();
            Ok(CheckOutcome::checked(project.file_count()).findings(findings))
        },
    )
}

fn tables_check() -> ConformityCheck {
    ConformityCheck::new(
        "workspace.tables",
        "Workspace tables have an indexed workspace column",
        |cx| async move {
            if let Some(reason) = not_row_isolation(&cx).await? {
                return Ok(CheckOutcome::skipped(reason));
            }
            let Some(db) = cx.app().try_get::<Db>().await else {
                return Ok(CheckOutcome::skipped("the app has no database"));
            };
            let registry = Registry::of(&cx);
            let mut outcome = CheckOutcome::checked(registry.workspace.len());
            for (table, column) in &registry.workspace {
                let Some(facts) = tables::table_facts(&db, table).await? else {
                    return Ok(CheckOutcome::skipped(format!(
                        "tables are read on SQLite and PostgreSQL, not on {:?}",
                        db.backend()
                    )));
                };
                outcome = outcome.findings(table_findings(table, column, &facts));
            }
            Ok(outcome)
        },
    )
}

fn table_findings(table: &str, column: &str, facts: &tables::TableFacts) -> Vec<Finding> {
    if !facts.exists {
        return vec![
            Finding::warning(table, "the table does not exist")
                .hint("run the migrations (navree migrate), then check again"),
        ];
    }
    if !facts.columns.iter().any(|c| c == column) {
        return vec![
            Finding::error(table, format!("has no `{column}` column"))
                .hint(
                    "add the column in a migration, or set WorkspaceScoped::WORKSPACE_COLUMN to the column that holds the workspace id",
                ),
        ];
    }
    let mut findings = Vec::new();
    if !facts.index_leads.iter().any(|c| c == column) {
        findings.push(
            Finding::warning(table, format!("no index starts with `{column}`"))
                .hint(format!(
                    "every workspace query filters by {column}: create an index on ({column}) or one that starts with it"
                )),
        );
    }
    if let Some((enabled, forced)) = facts.rls
        && !(enabled && forced)
    {
        findings.push(
            Finding::warning(table, "row-level security is off")
                .hint(format!(
                    "run RlsPolicy::new(\"{table}\").create_sql() in its migration, so PostgreSQL enforces the workspace scope too"
                )),
        );
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::tables::TableFacts;
    use super::*;

    fn facts(columns: &[&str], leads: &[&str], rls: Option<(bool, bool)>) -> TableFacts {
        TableFacts {
            exists: true,
            columns: columns.iter().map(|s| (*s).to_owned()).collect(),
            index_leads: leads.iter().map(|s| (*s).to_owned()).collect(),
            rls,
        }
    }

    fn template(workspace: Option<&str>) -> ModelDef {
        ModelDef {
            name: "Template".to_owned(),
            table: "templates".to_owned(),
            workspace: workspace.map(str::to_owned),
            path: vec!["app".into(), "template".into(), "Template".into()],
            location: "src/template.rs:3".to_owned(),
        }
    }

    #[test]
    fn libraries_are_classified_once() {
        let model = template(None);
        let mut registry = Registry::default();
        let unclassified = classification_finding(&model, &registry).unwrap();
        assert_eq!(
            unclassified.message,
            "model `Template` (table `templates`) is not classified as a workspace or a global entity"
        );
        let hint = unclassified.hint.unwrap();
        assert!(hint.contains("m.workspace_entity::<Template>()"), "{hint}");
        assert!(hint.contains("m.global_entity::<Template>()"), "{hint}");
        assert!(!hint.contains("sea_"), "{hint}");
        registry.library.insert("templates".to_owned());
        assert!(classification_finding(&model, &registry).is_none());
        registry.global.insert("templates".to_owned());
        let both = classification_finding(&model, &registry).unwrap();
        assert_eq!(
            both.message,
            "model `Template` (table `templates`) is classified as both a global and a library entity"
        );
        assert!(both.hint.unwrap().contains("m.library::<Template>()"));
        registry
            .workspace
            .insert("templates".to_owned(), "workspace_id");
        assert_eq!(
            classification_finding(&model, &registry).unwrap().message,
            "model `Template` (table `templates`) is classified as a workspace, a global and a library entity"
        );
    }

    #[test]
    fn self_scoping_models_are_workspace_entities_on_their_own() {
        let model = template(Some("workspace_id"));
        let mut registry = Registry::default();
        assert!(classification_finding(&model, &registry).is_none());
        // Registered as a workspace entity too: still one class.
        registry
            .workspace
            .insert("templates".to_owned(), "workspace_id");
        assert!(classification_finding(&model, &registry).is_none());
        registry.workspace.clear();
        registry.global.insert("templates".to_owned());
        let both = classification_finding(&model, &registry).unwrap();
        assert_eq!(
            both.message,
            "model `Template` (table `templates`) is classified as both a workspace and a global entity"
        );
        assert!(both.hint.unwrap().contains("#[model(workspace)]"));
    }

    #[test]
    fn table_findings_name_what_is_missing() {
        assert!(
            table_findings(
                "deals",
                "workspace_id",
                &facts(&["id", "workspace_id"], &["id", "workspace_id"], None)
            )
            .is_empty()
        );
        let missing = table_findings("deals", "workspace_id", &TableFacts::default());
        assert_eq!(missing[0].message, "the table does not exist");
        let no_column = table_findings(
            "deals",
            "team_id",
            &facts(&["id", "workspace_id"], &[], None),
        );
        assert_eq!(no_column.len(), 1);
        assert_eq!(no_column[0].message, "has no `team_id` column");
        assert_eq!(no_column[0].severity, navree_contracts::Severity::Error);
        let unindexed = table_findings(
            "deals",
            "workspace_id",
            &facts(&["id", "workspace_id"], &["id"], Some((true, false))),
        );
        let messages: Vec<_> = unindexed.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "no index starts with `workspace_id`",
                "row-level security is off"
            ]
        );
        assert!(
            unindexed
                .iter()
                .all(|f| f.severity == navree_contracts::Severity::Warning)
        );
    }
}
