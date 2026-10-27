//! `model:show`: what the app knows about a model, from its metadata and
//! its registrations: table, key, columns with their types and casts,
//! timestamps, soft deletes, workspace, factory and the relationships it
//! declares, access subject and policy, global scopes and hooks.
//!
//! Models are registered with
//! [`ModuleBuilderOrmExt::model`](crate::ModuleBuilderOrmExt::model)
//! (`navree make:model` adds the line to the module's `wire`), which also
//! imports [`OrmModule`](crate::OrmModule), the module registering the
//! command:
//!
//! ```text
//! navree model:show              # the registered models
//! navree model:show Deal         # one model (by name or table)
//! navree model:show Deal --json  # as JSON
//! ```
//!
//! ```
//! # use navree_orm::__doc_factories::*;
//! use navree_orm::show::ModelDescription;
//!
//! let deal = ModelDescription::of::<Deal>(Vec::new());
//! assert_eq!((deal.model, deal.table, deal.route_key), ("Deal", "deals", "slug"));
//! assert_eq!(deal.factory, Some("DealFactory"));
//! assert!(deal.render().contains("company_id"));
//! ```

use std::borrow::Cow;
use std::fmt::Write as _;

use navree_kernel::{AppContext, BoxError, CommandSpec};
use serde::Serialize;

use crate::factory::RelationshipInfo;
use crate::model::{KeyKind, Model};
use crate::module::registered_models;

/// The `model:show` command ([`OrmModule`](crate::OrmModule) registers
/// it).
pub(crate) fn command() -> CommandSpec {
    CommandSpec::new(
        "model:show",
        "Show a model: table, key, columns and casts, relationships, scopes, hooks, policy (model:show [Model] [--json])",
        |ctx, args| async move { run(&ctx, &args).await },
    )
}

/// A model as `model:show` shows it (`--json` prints this).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ModelDescription {
    /// The model's name (`Deal`).
    pub model: &'static str,
    /// The table.
    pub table: &'static str,
    /// The named connection, if the model declares one.
    pub connection: Option<&'static str>,
    /// The key column.
    pub key: &'static str,
    /// How the key gets its value: `increments`, `uuid_v7`, `uuid_v4`,
    /// `ulid` or `manual`.
    pub key_type: &'static str,
    /// The column route model binding looks records up by.
    pub route_key: &'static str,
    /// The creation timestamp column.
    pub created_at: Option<&'static str>,
    /// The update timestamp column.
    pub updated_at: Option<&'static str>,
    /// The soft-delete column.
    pub deleted_at: Option<&'static str>,
    /// The workspace column of a workspace-scoped model.
    pub workspace: Option<&'static str>,
    /// The default page size.
    pub per_page: u64,
    /// The factory, as written in `#[model(factory = ...)]`.
    pub factory: Option<&'static str>,
    /// The navree-access subject the records are.
    pub subject: Option<&'static str>,
    /// The attached policy, as written in `#[model(policy = ...)]`.
    pub policy: Option<&'static str>,
    /// The columns, in field order.
    pub columns: Vec<ColumnDescription>,
    /// The relationships the factory declares.
    pub relationships: Vec<RelationshipInfo>,
    /// The global scopes every query applies, by name: the declared ones,
    /// the ones modules registered at boot, soft deletes and the
    /// workspace's.
    pub global_scopes: Vec<Cow<'static, str>>,
    /// The model's own hooks (the workspace's included), by type.
    pub hooks: Vec<&'static str>,
    /// The hooks the app registers for the model, by type.
    pub app_hooks: Vec<&'static str>,
}

/// A column of a [`ModelDescription`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ColumnDescription {
    /// The column.
    pub name: &'static str,
    /// The struct field.
    pub field: &'static str,
    /// The field's Rust type, as written.
    pub rust_type: &'static str,
    /// Whether the column holds NULLs.
    pub nullable: bool,
    /// The cast converting the column, if any.
    pub cast: Option<&'static str>,
}

fn key_type(kind: KeyKind) -> &'static str {
    match kind {
        KeyKind::Increments => "increments",
        KeyKind::UuidV7 => "uuid_v7",
        KeyKind::UuidV4 => "uuid_v4",
        KeyKind::Ulid => "ulid",
        KeyKind::ObjectId => "object_id",
        KeyKind::Manual => "manual",
    }
}

impl ModelDescription {
    /// Model `M` from its metadata, with `app_hooks`, the names of the
    /// hooks the app registers for it.
    pub fn of<M: Model>(app_hooks: Vec<&'static str>) -> Self {
        let meta = M::meta();
        let global_scopes = crate::global_scopes::scopes_of::<M>()
            .iter()
            .map(|scope| scope.name())
            .collect();
        let mut hooks: Vec<&'static str> = M::hooks().iter().map(|hook| hook.name()).collect();
        if meta.is_workspace_scoped() {
            hooks.insert(0, std::any::type_name::<crate::workspace::WorkspaceHook>());
        }
        ModelDescription {
            model: meta.name(),
            table: meta.table(),
            connection: meta.connection_name(),
            key: meta.key_column(),
            key_type: key_type(meta.key_kind()),
            route_key: meta.route_key_column(),
            created_at: meta.created_at(),
            updated_at: meta.updated_at(),
            deleted_at: meta.deleted_at(),
            workspace: meta.workspace_column(),
            per_page: meta.default_per_page(),
            factory: meta.factory_name(),
            subject: meta.subject_name(),
            policy: meta.policy_name(),
            columns: meta
                .all_columns()
                .iter()
                .map(|column| ColumnDescription {
                    name: column.name(),
                    field: column.field(),
                    rust_type: column.rust_type(),
                    nullable: column.is_nullable(),
                    cast: column.cast_name(),
                })
                .collect(),
            relationships: meta.factory_relationships(),
            global_scopes,
            hooks,
            app_hooks,
        }
    }

    /// The description as `model:show` prints it.
    pub fn render(&self) -> String {
        let none = "-";
        let or_none = |value: Option<&str>| value.unwrap_or(none).to_owned();
        let timestamps = match (self.created_at, self.updated_at) {
            (None, None) => none.to_owned(),
            (created, updated) => [created, updated]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", "),
        };
        let mut out = format!("{} ({})\n", self.model, self.table);
        let settings = [
            ("key", format!("{} ({})", self.key, self.key_type)),
            ("route key", self.route_key.to_owned()),
            ("timestamps", timestamps),
            ("soft deletes", or_none(self.deleted_at)),
            ("workspace", or_none(self.workspace)),
            (
                "connection",
                self.connection.unwrap_or("default").to_owned(),
            ),
            ("per page", self.per_page.to_string()),
            ("factory", or_none(self.factory)),
            ("subject", or_none(self.subject)),
            ("policy", or_none(self.policy)),
        ];
        table(
            &mut out,
            None,
            settings.iter().map(|(k, v)| [(*k).to_owned(), v.clone()]),
        );

        let columns = self.columns.iter().map(|column| {
            let mut notes = Vec::new();
            if column.nullable {
                notes.push("nullable".to_owned());
            }
            if let Some(cast) = column.cast {
                notes.push(format!("cast {cast}"));
            }
            [
                column.name.to_owned(),
                column.rust_type.to_owned(),
                notes.join(", "),
            ]
        });
        table(&mut out, Some("columns"), columns);

        if !self.relationships.is_empty() {
            let relationships = self.relationships.iter().map(|relationship| {
                [
                    relationship.name.to_owned(),
                    format!("{} {}", relationship.kind, relationship.related),
                    format!("via {}", relationship.via),
                ]
            });
            table(
                &mut out,
                Some("relationships (declared by the factory)"),
                relationships,
            );
        }
        fn list<S: AsRef<str>>(out: &mut String, title: &str, items: &[S]) {
            if !items.is_empty() {
                table(
                    out,
                    Some(title),
                    items.iter().map(|item| [item.as_ref().to_owned()]),
                );
            }
        }
        list(&mut out, "global scopes", &self.global_scopes);
        list(&mut out, "hooks", &self.hooks);
        list(&mut out, "hooks registered by the app", &self.app_hooks);
        out
    }
}

/// Appends `rows` to `out`, under `title`, columns aligned.
fn table<const N: usize>(
    out: &mut String,
    title: Option<&str>,
    rows: impl Iterator<Item = [String; N]>,
) {
    let rows: Vec<[String; N]> = rows.collect();
    let mut widths = [0; N];
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    if let Some(title) = title {
        let _ = writeln!(out, "\n{title}");
    }
    for row in &rows {
        let mut line = String::from("  ");
        for (index, (cell, width)) in row.iter().zip(widths).enumerate() {
            if index + 1 == N {
                line.push_str(cell);
            } else {
                let _ = write!(line, "{cell:<width$}  ");
            }
        }
        let _ = writeln!(out, "{}", line.trim_end());
    }
}

/// The registered model named `name` (in any case) or stored in table
/// `name` ([`registered_models`](crate::registered_models)), described
/// with the hooks the app registers for it.
pub async fn describe_model(ctx: &AppContext, name: &str) -> Result<ModelDescription, BoxError> {
    let models = registered_models(ctx);
    let model = models
        .iter()
        .find(|model| model.name().eq_ignore_ascii_case(name) || model.meta().table() == name)
        .ok_or_else(|| {
            let known: Vec<&str> = models.iter().map(|model| model.name()).collect();
            if known.is_empty() {
                format!("no model `{name}`. {}", no_models())
            } else {
                format!(
                    "no model `{name}`; the registered models are {}",
                    known.join(", ")
                )
            }
        })?;
    model.describe(ctx).await
}

/// `model:show [Model] [--json]`.
async fn run(ctx: &AppContext, args: &[String]) -> Result<(), BoxError> {
    let usage = "model:show [Model] [--json]";
    if let Some(flag) = args
        .iter()
        .find(|arg| arg.starts_with("--") && *arg != "--json")
    {
        return Err(format!("model:show: unknown option `{flag}` ({usage})").into());
    }
    let json = args.iter().any(|arg| arg == "--json");
    let names: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|arg| !arg.starts_with("--"))
        .collect();
    match names.as_slice() {
        [] => {
            let models: Vec<_> = registered_models(ctx)
                .iter()
                .map(|model| model.meta())
                .collect();
            if json {
                let list: Vec<serde_json::Value> = models
                    .iter()
                    .map(|meta| serde_json::json!({ "model": meta.name(), "table": meta.table() }))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else if models.is_empty() {
                println!("{}", no_models());
            } else {
                let mut out = String::new();
                let rows = models
                    .iter()
                    .map(|meta| [meta.name().to_owned(), meta.table().to_owned()]);
                table(&mut out, None, rows);
                print!("{out}");
            }
            Ok(())
        }
        [name] => {
            let description = describe_model(ctx, name)
                .await
                .map_err(|error| format!("model:show: {error}"))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&description)?);
            } else {
                print!("{}", description.render());
            }
            Ok(())
        }
        _ => Err(format!("model:show takes one model ({usage})").into()),
    }
}

fn no_models() -> &'static str {
    "No models are registered: register one in its module's `wire` with \
     `navree::orm::ModuleBuilderOrmExt::model::<Model>(m)`."
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::__doc_factories::{Company, Deal};

    #[test]
    fn descriptions_come_from_the_metadata() {
        let deal = ModelDescription::of::<Deal>(vec!["app::LogDeals"]);
        assert_eq!(deal.key, "id");
        assert_eq!(deal.key_type, "uuid_v7");
        assert_eq!(
            (deal.created_at, deal.updated_at, deal.deleted_at),
            (Some("created_at"), Some("updated_at"), Some("deleted_at"))
        );
        let stage = deal.columns.iter().find(|c| c.name == "stage").unwrap();
        assert_eq!(
            (stage.rust_type, stage.cast),
            ("Stage", Some("string_enum"))
        );
        let company_id = deal
            .columns
            .iter()
            .find(|c| c.name == "company_id")
            .unwrap();
        assert!(company_id.nullable);
        assert_eq!(deal.global_scopes, ["SoftDeletes"]);
        assert_eq!(deal.app_hooks, ["app::LogDeals"]);
        let names: Vec<&str> = deal.relationships.iter().map(|r| r.name).collect();
        assert_eq!(names, ["company", "activities", "tags"]);
        assert_eq!(deal.relationships[0].via, "company_id");
        assert_eq!(deal.relationships[2].via, "deal_tag");

        let company = ModelDescription::of::<Company>(Vec::new());
        assert_eq!((company.key_type, company.route_key), ("increments", "id"));
        assert!(company.global_scopes.is_empty());
        assert!(company.relationships.is_empty());
    }

    #[test]
    fn the_text_lists_everything_aligned() {
        let text = ModelDescription::of::<Deal>(Vec::new()).render();
        assert!(text.starts_with("Deal (deals)\n"), "{text}");
        assert!(text.contains("  key           id (uuid_v7)\n"), "{text}");
        assert!(text.contains("  route key     slug\n"), "{text}");
        assert!(text.contains("  soft deletes  deleted_at\n"), "{text}");
        assert!(text.contains("  workspace     -\n"), "{text}");
        assert!(text.contains("  connection    default\n"), "{text}");
        assert!(text.contains("  factory       DealFactory\n"), "{text}");
        assert!(text.contains("\ncolumns\n"), "{text}");
        assert!(
            text.contains("  company_id    Option<i64>            nullable\n"),
            "{text}"
        );
        assert!(
            text.contains("  stage         Stage                  cast string_enum\n"),
            "{text}"
        );
        assert!(text.contains("  title         String\n"), "{text}");
        assert!(
            text.contains("\nrelationships (declared by the factory)\n"),
            "{text}"
        );
        assert!(
            text.contains("  tags        belongs_to_many Tag  via deal_tag\n"),
            "{text}"
        );
        assert!(text.contains("\nglobal scopes\n  SoftDeletes\n"), "{text}");
        assert!(!text.contains("hooks"), "{text}");
    }
}
