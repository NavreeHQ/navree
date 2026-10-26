//! `navree make:resource <module> <Entity>`: a Helm resource with a field
//! for every column of a SeaORM entity, registered with the module.

use std::path::{Path, PathBuf};

use anyhow::Context;
use heck::ToSnakeCase;
use serde::Serialize;

use crate::edit::{declare_module, insert_above_marker};
use crate::make::MakeReport;
use crate::names::Name;
use crate::scaffold::{FileSet, join};
use crate::templates::render;

/// One column of the entity's `Model`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// The field (and column) name.
    pub name: String,
    /// The type's last path segment (`String`, `Uuid`, `DateTimeUtc`, ...),
    /// inside `Option<..>`.
    pub ty: String,
    /// Whether the type is an `Option`.
    pub optional: bool,
    /// Whether the column is the primary key.
    pub primary_key: bool,
}

/// What `make:resource` reads from an entity file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityInfo {
    /// `#[sea_orm(table_name = "..")]`.
    pub table: String,
    /// The `Model` struct's fields.
    pub columns: Vec<Column>,
    /// The `SUBJECT` of `impl Subject for Entity`, when there is one.
    pub subject: Option<String>,
}

/// Parses an entity file: its `Model` struct, its table name and its
/// access subject.
pub fn parse_entity(source: &str) -> anyhow::Result<EntityInfo> {
    let file = syn::parse_file(source).context("the entity file is not valid Rust")?;
    let mut info = None;
    let mut subject = None;
    for item in &file.items {
        match item {
            syn::Item::Struct(model) if model.ident == "Model" => {
                let table = model
                    .attrs
                    .iter()
                    .filter(|a| a.path().is_ident("sea_orm"))
                    .find_map(|a| quoted_value(&list_tokens(a), "table_name"))
                    .context("`Model` has no #[sea_orm(table_name = \"..\")] attribute")?;
                let syn::Fields::Named(fields) = &model.fields else {
                    anyhow::bail!("`Model` has no named fields");
                };
                let columns = fields
                    .named
                    .iter()
                    .filter_map(|field| {
                        let name = field.ident.as_ref()?.to_string();
                        let name = name.strip_prefix("r#").unwrap_or(&name).to_owned();
                        let (optional, ty) = type_name(&field.ty);
                        let primary_key = field.attrs.iter().any(|a| {
                            a.path().is_ident("sea_orm") && list_tokens(a).contains("primary_key")
                        });
                        Some(Column {
                            name,
                            ty,
                            optional,
                            primary_key,
                        })
                    })
                    .collect();
                info = Some((table, columns));
            }
            syn::Item::Impl(block) => {
                let is_subject = block
                    .trait_
                    .as_ref()
                    .and_then(|(path, _)| path.segments.last())
                    .is_some_and(|s| s.ident == "Subject");
                if is_subject {
                    subject = block.items.iter().find_map(|item| match item {
                        syn::ImplItem::Const(c) if c.ident == "SUBJECT" => match &c.expr {
                            syn::Expr::Lit(syn::ExprLit {
                                lit: syn::Lit::Str(s),
                                ..
                            }) => Some(s.value()),
                            _ => None,
                        },
                        _ => None,
                    });
                }
            }
            _ => {}
        }
    }
    let (table, columns) = info.context("no `pub struct Model` (a SeaORM entity) in the file")?;
    Ok(EntityInfo {
        table,
        columns,
        subject,
    })
}

/// The tokens of `#[name(tokens)]`, as text.
fn list_tokens(attr: &syn::Attribute) -> String {
    match &attr.meta {
        syn::Meta::List(list) => list.tokens.to_string(),
        _ => String::new(),
    }
}

/// The string after `key =` in attribute tokens.
fn quoted_value(tokens: &str, key: &str) -> Option<String> {
    let at = tokens.find(key)?;
    let rest = tokens[at + key.len()..].trim_start().strip_prefix('=')?;
    let rest = rest.trim_start().strip_prefix('"')?;
    Some(rest.split('"').next()?.to_owned())
}

/// `(optional, last segment)` of a type: `Option<DateTimeUtc>` is
/// `(true, "DateTimeUtc")`.
fn type_name(ty: &syn::Type) -> (bool, String) {
    let syn::Type::Path(path) = ty else {
        return (false, String::new());
    };
    let Some(last) = path.path.segments.last() else {
        return (false, String::new());
    };
    if last.ident == "Option"
        && let syn::PathArguments::AngleBracketed(args) = &last.arguments
        && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
    {
        return (true, type_name(inner).1);
    }
    (false, last.ident.to_string())
}

/// One generated field.
#[derive(Debug, Clone, Serialize)]
struct FieldCode {
    code: String,
    comment: Option<String>,
}

const TITLE_COLUMNS: [&str; 7] = [
    "name",
    "title",
    "full_name",
    "subject",
    "label",
    "email",
    "slug",
];
const LONG_TEXT: [&str; 7] = [
    "body",
    "content",
    "description",
    "notes",
    "bio",
    "summary",
    "text",
];
const TIMESTAMPS: [&str; 3] = ["created_at", "updated_at", "deleted_at"];

fn is_text(ty: &str) -> bool {
    ty == "String"
}

fn is_number(ty: &str) -> bool {
    matches!(
        ty,
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "f32" | "f64" | "Decimal"
    )
}

/// The field for `column`.
fn field_for(column: &Column, columns: &[Column]) -> Option<FieldCode> {
    let name = column.name.as_str();
    let ty = column.ty.as_str();
    let field = |code: String| {
        Some(FieldCode {
            code,
            comment: None,
        })
    };
    if column.primary_key {
        return field("Field::id()".to_owned());
    }
    let readonly = if TIMESTAMPS.contains(&name) {
        ".readonly()"
    } else {
        ""
    };
    let has = |n: &str| columns.iter().any(|c| c.name == n);
    let code = match ty {
        _ if is_text(ty) && name.contains("email") => format!("Field::email(\"{name}\")"),
        _ if is_text(ty) && (name.contains("url") || name == "website" || name == "homepage") => {
            format!("Field::url(\"{name}\")")
        }
        _ if is_text(ty) && LONG_TEXT.contains(&name) => {
            format!("Field::textarea(\"{name}\").hide_on_index()")
        }
        _ if is_text(ty) => {
            let required = if column.optional { "" } else { ".required()" };
            format!("Field::text(\"{name}\"){required}.sortable()")
        }
        "bool" => format!("Field::boolean(\"{name}\")"),
        _ if is_number(ty) && name.ends_with("_cents") && has("currency") => {
            format!("Field::money(\"{name}\", \"currency\").sortable()")
        }
        _ if is_number(ty) => format!("Field::number(\"{name}\").sortable()"),
        "Date" | "NaiveDate" => format!("Field::date(\"{name}\").sortable()"),
        "DateTimeUtc"
        | "DateTime"
        | "DateTimeWithTimeZone"
        | "DateTimeLocal"
        | "NaiveDateTime"
        | "TimeDateTime"
        | "TimeDateTimeWithTimeZone" => {
            format!("Field::datetime(\"{name}\"){readonly}.sortable()")
        }
        "Uuid" if name.ends_with("_id") => {
            let related = Name::parse("entity", name.trim_end_matches("_id"))
                .map(|n| format!("{}Resource", n.pascal()))
                .unwrap_or_else(|_| "TheirResource".to_owned());
            return Some(FieldCode {
                code: format!("Field::text(\"{name}\").hide_on_index()"),
                comment: Some(format!(
                    "Shows the related title with a picker: Field::belongs_to::<{related}>(\"{name}\")"
                )),
            });
        }
        "Uuid" => format!("Field::text(\"{name}\")"),
        "Json" | "Value" | "JsonValue" => {
            return Some(FieldCode {
                code: format!("Field::textarea(\"{name}\").readonly().hide_on_index()"),
                comment: Some("A JSON column, shown as text.".to_owned()),
            });
        }
        "Vec" => return None,
        _ => {
            return Some(FieldCode {
                code: format!("Field::text(\"{name}\").sortable()"),
                comment: Some(format!(
                    "`{ty}`: for an enum, list its values with Field::select(\"{name}\").options([..])"
                )),
            });
        }
    };
    field(code)
}

/// What the resource template is rendered with.
#[derive(Debug, Clone, Serialize)]
struct ResourceContext {
    pascal: String,
    entity: String,
    entity_path: String,
    label: String,
    singular: String,
    slug: String,
    table: String,
    subject: String,
    title: String,
    search: String,
    fields: Vec<FieldCode>,
}

/// The resource file for entity `entity` (snake_case), read from `info`;
/// `entity_path` is the Rust path of the entity module from the resource
/// file (`super::super::entity`).
pub fn resource_file(
    entity: &Name,
    info: &EntityInfo,
    entity_path: &str,
) -> anyhow::Result<String> {
    let table = Name::parse("table", &info.table).ok();
    let plural = table.clone().unwrap_or_else(|| entity.plural());
    let title = TITLE_COLUMNS
        .iter()
        .find(|t| info.columns.iter().any(|c| c.name == **t && is_text(&c.ty)))
        .map(|t| (*t).to_owned())
        .or_else(|| {
            info.columns
                .iter()
                .find(|c| c.primary_key)
                .map(|c| c.name.clone())
        })
        .unwrap_or_else(|| "id".to_owned());
    let searchable = info
        .columns
        .iter()
        .any(|c| c.name == title && is_text(&c.ty));
    let mut search = Vec::new();
    if searchable {
        search.push(format!("\"{title}\""));
    }
    if title != "email"
        && info
            .columns
            .iter()
            .any(|c| c.name == "email" && is_text(&c.ty))
    {
        search.push("\"email\"".to_owned());
    }
    let ctx = ResourceContext {
        pascal: entity.pascal(),
        entity: entity.snake().to_owned(),
        entity_path: entity_path.to_owned(),
        label: plural.title(),
        singular: entity.title(),
        slug: info.table.replace('_', "-"),
        table: info.table.clone(),
        subject: info.subject.clone().unwrap_or_else(|| entity.pascal()),
        title,
        search: search.join(", "),
        fields: info
            .columns
            .iter()
            .filter_map(|c| field_for(c, &info.columns))
            .collect(),
    };
    Ok(render("make/resource.rs", &ctx)?)
}

/// Generates a Helm resource for entity `entity` (`Deal` or `deal`) of
/// module `module` under `root/modules_dir`: `resources/<entity>.rs`, the
/// `resources` module, and the registration at the module's
/// `// navree:wire` marker. Nothing is written when a file exists.
pub fn make_resource(
    root: &Path,
    modules_dir: &str,
    module: &Name,
    entity: &str,
) -> anyhow::Result<MakeReport> {
    let entity = Name::parse("entity", &entity.trim().to_snake_case())?;
    let module_dir = join(root, modules_dir).join(module.snake());
    let module_rs = module_dir.join("mod.rs");
    if !module_rs.is_file() {
        anyhow::bail!(
            "module `{m}` not found at {}; create it with `navree make:module {m} --crud`",
            module_rs.display(),
            m = module.snake()
        );
    }
    let candidates: [(PathBuf, String); 2] = [
        (
            module_dir
                .join("entities")
                .join(format!("{}.rs", entity.snake())),
            format!("super::super::entities::{}", entity.snake()),
        ),
        (
            module_dir.join("entity.rs"),
            "super::super::entity".to_owned(),
        ),
    ];
    // `entities/<entity>.rs` (make:entity), else the module's `entity.rs`
    // (make:module) when it is this entity: its table is the entity's
    // plural, or the entity is the module's singular.
    let mut found = None;
    for (index, (path, rust_path)) in candidates.iter().enumerate() {
        if !path.is_file() {
            continue;
        }
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let info = parse_entity(&source)
            .with_context(|| format!("cannot read the entity in {}", path.display()))?;
        let matches = index == 0
            || info.table == entity.plural().snake()
            || entity.snake() == module.singular().snake();
        if matches {
            found = Some((path.clone(), rust_path.clone(), info));
            break;
        }
    }
    let Some((entity_file, entity_path, info)) = found else {
        anyhow::bail!(
            "entity `{e}` not found at {} (or as the module's entity in {}); create it with `navree make:entity {} {e}`",
            candidates[0].0.display(),
            candidates[1].0.display(),
            module.snake(),
            e = entity.snake()
        );
    };
    let entity_path = entity_path.as_str();

    let resources_mod = module_dir.join("resources").join("mod.rs");
    let new_dir = !resources_mod.is_file();
    let mut files = FileSet::new();
    files.add(
        format!("resources/{}.rs", entity.snake()),
        resource_file(&entity, &info, entity_path)?,
    );
    if new_dir {
        files.add(
            "resources/mod.rs",
            format!(
                "//! Helm resources of the {} module.\n\npub mod {};\n",
                module.snake().replace('_', " "),
                entity.snake()
            ),
        );
    }
    let mut report = MakeReport {
        created: files.write(&module_dir)?,
        ..MakeReport::default()
    };
    if new_dir {
        report.edit_file(
            &module_rs,
            |src| Some(declare_module(src, "pub mod resources;")),
            || format!("declare `pub mod resources;` in {}", module_rs.display()),
        )?;
    } else {
        let decl = format!("pub mod {};", entity.snake());
        report.edit_file(
            &resources_mod,
            |src| Some(declare_module(src, &decl)),
            || format!("declare `{decl}` in {}", resources_mod.display()),
        )?;
    }
    let line = format!(
        "navree::helm::ModuleBuilderHelmExt::resource::<resources::{}::{}Resource>(m);",
        entity.snake(),
        entity.pascal()
    );
    report.edit_file(
        &module_rs,
        |src| insert_above_marker(src, "navree:wire", &line),
        || format!("register it in the module's `wire` method: `{line}`"),
    )?;
    if info.subject.is_none() {
        report.hints.push(format!(
            "the entity needs an access subject: add `impl navree::access::Subject for Entity {{ const SUBJECT: &'static str = \"{}\"; const PERMISSION_PREFIX: &'static str = \"{}\"; }}` to {}",
            entity.pascal(),
            info.table,
            entity_file.display()
        ));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEAL: &str = r#"
use navree::access::Subject;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "deals")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub title: String,
    pub amount_cents: i64,
    pub currency: String,
    pub stage: Stage,
    pub company_id: Uuid,
    pub owner_id: Option<Uuid>,
    pub notes: Option<String>,
    pub won: bool,
    pub expected_close_date: Option<Date>,
    pub created_at: DateTimeUtc,
}

impl Subject for Entity {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}
"#;

    #[test]
    fn entities_are_read_with_syn() {
        let info = parse_entity(DEAL).unwrap();
        assert_eq!(info.table, "deals");
        assert_eq!(info.subject.as_deref(), Some("Deal"));
        assert_eq!(info.columns.len(), 11);
        assert_eq!(
            info.columns[0],
            Column {
                name: "id".into(),
                ty: "Uuid".into(),
                optional: false,
                primary_key: true
            }
        );
        assert_eq!(
            info.columns[6],
            Column {
                name: "owner_id".into(),
                ty: "Uuid".into(),
                optional: true,
                primary_key: false
            }
        );
        assert!(parse_entity("struct Other;").is_err());
        assert!(parse_entity("not rust {").is_err());
    }

    #[test]
    fn columns_become_fields() {
        let info = parse_entity(DEAL).unwrap();
        let code: Vec<String> = info
            .columns
            .iter()
            .filter_map(|c| field_for(c, &info.columns))
            .map(|f| f.code)
            .collect();
        assert_eq!(
            code,
            [
                "Field::id()",
                "Field::text(\"title\").required().sortable()",
                "Field::money(\"amount_cents\", \"currency\").sortable()",
                "Field::text(\"currency\").required().sortable()",
                "Field::text(\"stage\").sortable()",
                "Field::text(\"company_id\").hide_on_index()",
                "Field::text(\"owner_id\").hide_on_index()",
                "Field::textarea(\"notes\").hide_on_index()",
                "Field::boolean(\"won\")",
                "Field::date(\"expected_close_date\").sortable()",
                "Field::datetime(\"created_at\").readonly().sortable()",
            ]
        );
    }
}
