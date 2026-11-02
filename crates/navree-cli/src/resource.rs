//! `navree make:resource <module> <Model>`: a Helm resource with a field
//! for every column of a navree-orm model a user may see, registered with the
//! module. A column the model hides, or one named like a secret, gets no field.

use std::path::Path;

use anyhow::Context;
use heck::ToSnakeCase;
use serde::Serialize;

use crate::edit::{declare_module, insert_above_marker};
use crate::make::MakeReport;
use crate::names::Name;
use crate::scaffold::{FileSet, join};
use crate::templates::render;

/// One column of the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// The column name (the field's, or `#[model(column = "..")]`).
    pub name: String,
    /// The type's last path segment (`String`, `Uuid`, `DateTime`, ...),
    /// inside `Option<..>`.
    pub ty: String,
    /// Whether the type is an `Option`.
    pub optional: bool,
    /// Whether the column is the primary key.
    pub primary_key: bool,
    /// Whether serialized records leave it out (`#[model(hidden)]`, a model's
    /// `hidden(..)`, or not in its `visible(..)`).
    pub hidden: bool,
}

impl Column {
    /// Whether a generated resource leaves the column out.
    fn left_out(&self) -> bool {
        !self.primary_key && (self.hidden || looks_secret(&self.name, &self.ty))
    }
}

/// The words of a column name that mean it holds a secret.
const SECRET_WORDS: [&str; 7] = [
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "credentials",
    "otp",
];

/// Whether a column named `name` of type `ty` looks like a secret: a word of its
/// name says so (`password_hash`, `api_token`, not `secretary`), or it ends in
/// `key` after another word that says what kind (`api_key`, `signing_key`). A
/// number is a count of them (`token_count`).
fn looks_secret(name: &str, ty: &str) -> bool {
    if is_number(ty) || ty == "bool" {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    let words: Vec<&str> = lower.split('_').collect();
    let word = |candidate: &str| words.contains(&candidate);
    SECRET_WORDS.iter().any(|secret| word(secret))
        || lower == "apikey"
        || lower.contains("recovery_code")
        || matches!(
            words.as_slice(),
            [.., "api" | "private" | "signing" | "secret" | "encryption" | "access", "key"]
        )
}

/// The access subject a model declares with `#[model(subject ..)]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectInfo {
    /// The subject's name (`Deal`): `subject = ".."`, else the struct's.
    pub name: String,
    /// The permission prefix: `permission_prefix = ".."`, else the table.
    pub permission_prefix: String,
}

/// What `make:resource` reads from a model file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// The struct's name (`Deal`).
    pub name: String,
    /// `#[model(table = "..")]`, else the snake_case plural of the name.
    pub table: String,
    /// The struct's fields that are columns (not `#[model(skip)]`).
    pub columns: Vec<Column>,
    /// The access subject, when the model is one.
    pub subject: Option<SubjectInfo>,
}

/// Whether `attrs` derive `Model` (`#[derive(Model)]` or a path ending in
/// `Model`, such as `navree::orm::Model`).
fn derives_model(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("derive"))
        .any(|a| {
            let mut found = false;
            let _ = a.parse_nested_meta(|meta| {
                if meta
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "Model")
                {
                    found = true;
                }
                Ok(())
            });
            found
        })
}

/// The tokens of every `#[model(..)]` attribute in `attrs`, as text.
fn model_tokens(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("model"))
        .map(list_tokens)
        .collect()
}

/// Whether the attribute tokens hold the bare word or key `word`
/// (`subject`, `subject = ".."`, `skip`).
fn has_word(tokens: &str, word: &str) -> bool {
    tokens
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|t| t == word)
}

/// Parses a model file: its `#[derive(Model)]` struct (the one named
/// `model`, when given and there are several), its table, columns, key and
/// access subject.
pub fn parse_model(source: &str, model: Option<&Name>) -> anyhow::Result<ModelInfo> {
    let file = syn::parse_file(source).context("the model file is not valid Rust")?;
    let structs: Vec<&syn::ItemStruct> = file
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Struct(s) if derives_model(&s.attrs) => Some(s),
            _ => None,
        })
        .collect();
    let item = match model {
        Some(name) => structs
            .iter()
            .find(|s| s.ident.to_string().to_snake_case() == name.snake())
            .or(structs.first()),
        None => structs.first(),
    }
    .context("no `#[derive(Model)]` struct (a navree-orm model) in the file")?;
    let name = item.ident.to_string();
    let tokens = model_tokens(&item.attrs).join(", ");
    let table = quoted_value(&tokens, "table").unwrap_or_else(|| {
        Name::parse("model", &name.to_snake_case())
            .map(|n| n.plural().snake().to_owned())
            .unwrap_or_else(|_| name.to_snake_case())
    });
    let key = quoted_value(&tokens, "primary_key").unwrap_or_else(|| "id".to_owned());
    let subject = has_word(&tokens, "subject").then(|| SubjectInfo {
        name: quoted_value(&tokens, "subject").unwrap_or_else(|| name.clone()),
        permission_prefix: quoted_value(&tokens, "permission_prefix")
            .unwrap_or_else(|| table.clone()),
    });
    let syn::Fields::Named(fields) = &item.fields else {
        anyhow::bail!("`{name}` has no named fields");
    };
    let hidden_names = name_list(&tokens, "hidden");
    let visible_names = name_list(&tokens, "visible");
    let columns = fields
        .named
        .iter()
        .filter_map(|field| {
            let field_tokens = model_tokens(&field.attrs).join(", ");
            if has_word(&field_tokens, "skip") {
                return None;
            }
            let ident = field.ident.as_ref()?.to_string();
            let ident = ident.strip_prefix("r#").unwrap_or(&ident).to_owned();
            let name = quoted_value(&field_tokens, "column").unwrap_or_else(|| ident.clone());
            let (optional, ty) = type_name(&field.ty);
            let named = |list: &[String]| list.contains(&ident) || list.contains(&name);
            let hidden = has_word(&field_tokens, "hidden")
                || named(&hidden_names)
                || (!visible_names.is_empty() && !named(&visible_names));
            Some(Column {
                primary_key: name == key,
                name,
                ty,
                optional,
                hidden,
            })
        })
        .collect();
    Ok(ModelInfo {
        name,
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

/// The names in `word(a, "b")` in attribute tokens.
fn name_list(tokens: &str, word: &str) -> Vec<String> {
    let mut from = 0;
    while let Some(found) = tokens[from..].find(word) {
        let at = from + found;
        from = at + word.len();
        let before = tokens[..at].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let Some(rest) = tokens[from..].trim_start().strip_prefix('(') else {
            continue;
        };
        return rest
            .split(')')
            .next()
            .unwrap_or_default()
            .split(',')
            .map(|name| name.trim().trim_matches('"').to_owned())
            .filter(|name| !name.is_empty())
            .collect();
    }
    Vec::new()
}

/// The string after `key =` in attribute tokens (the key as a whole word).
fn quoted_value(tokens: &str, key: &str) -> Option<String> {
    let mut from = 0;
    while let Some(found) = tokens[from..].find(key) {
        let at = from + found;
        from = at + key.len();
        let before = tokens[..at].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let Some(rest) = tokens[from..].trim_start().strip_prefix('=') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('"') else {
            continue;
        };
        return Some(rest.split('"').next()?.to_owned());
    }
    None
}

/// `(optional, last segment)` of a type: `Option<DateTime<Utc>>` is
/// `(true, "DateTime")`.
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
        "NaiveDate" | "Date" => format!("Field::date(\"{name}\").sortable()"),
        "DateTime" | "NaiveDateTime" | "DateTimeUtc" => {
            format!("Field::datetime(\"{name}\"){readonly}.sortable()")
        }
        "Uuid" | "Ulid" if name.ends_with("_id") => {
            let related = Name::parse("model", name.trim_end_matches("_id"))
                .map(|n| format!("{}Resource", n.pascal()))
                .unwrap_or_else(|_| "TheirResource".to_owned());
            return Some(FieldCode {
                code: format!("Field::text(\"{name}\").hide_on_index()"),
                comment: Some(format!(
                    "Shows the related title with a picker: Field::belongs_to::<{related}>(\"{name}\")"
                )),
            });
        }
        "Uuid" | "Ulid" => format!("Field::text(\"{name}\")"),
        "Value" | "JsonValue" | "Json" => {
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
    model: String,
    model_path: String,
    label: String,
    singular: String,
    slug: String,
    permission_prefix: String,
    subject: String,
    title: String,
    search: String,
    fields: Vec<FieldCode>,
    left_out: String,
}

/// The resource file for model `name` (its resource is `<Name>Resource`),
/// read from `info`; `model_path` is the Rust path of the model's module
/// from the resource file (`super::super::models::deal`).
pub fn resource_file(name: &Name, info: &ModelInfo, model_path: &str) -> anyhow::Result<String> {
    let table = Name::parse("table", &info.table).ok();
    let plural = table.clone().unwrap_or_else(|| name.plural());
    let shown: Vec<Column> = info
        .columns
        .iter()
        .filter(|c| !c.left_out())
        .cloned()
        .collect();
    let left_out: Vec<&str> = info
        .columns
        .iter()
        .filter(|c| c.left_out())
        .map(|c| c.name.as_str())
        .collect();
    let title = TITLE_COLUMNS
        .iter()
        .find(|t| shown.iter().any(|c| c.name == **t && is_text(&c.ty)))
        .map(|t| (*t).to_owned())
        .or_else(|| {
            shown
                .iter()
                .find(|c| c.primary_key)
                .map(|c| c.name.clone())
        })
        .unwrap_or_else(|| "id".to_owned());
    let searchable = shown.iter().any(|c| c.name == title && is_text(&c.ty));
    let mut search = Vec::new();
    if searchable {
        search.push(format!("\"{title}\""));
    }
    if title != "email" && shown.iter().any(|c| c.name == "email" && is_text(&c.ty)) {
        search.push("\"email\"".to_owned());
    }
    let ctx = ResourceContext {
        pascal: name.pascal(),
        model: info.name.clone(),
        model_path: model_path.to_owned(),
        label: plural.title(),
        singular: name.title(),
        slug: info.table.replace('_', "-"),
        permission_prefix: info
            .subject
            .as_ref()
            .map_or_else(|| info.table.clone(), |s| s.permission_prefix.clone()),
        subject: info
            .subject
            .as_ref()
            .map_or_else(|| info.name.clone(), |s| s.name.clone()),
        title,
        search: search.join(", "),
        fields: shown.iter().filter_map(|c| field_for(c, &shown)).collect(),
        left_out: left_out.join(", "),
    };
    Ok(render("make/resource.rs", &ctx)?)
}

/// Generates a Helm resource for model `model` (`Deal` or `deal`) of
/// module `module` under `root/modules_dir`, read from the module's
/// `models/<model>.rs` (`navree make:model`): `resources/<model>.rs`, the
/// `resources` module, and the registration at the module's
/// `// navree:wire` marker. Nothing is written when a file exists.
pub fn make_resource(
    root: &Path,
    modules_dir: &str,
    module: &Name,
    model: &str,
) -> anyhow::Result<MakeReport> {
    let model = Name::parse("model", &model.trim().to_snake_case())?;
    let module_dir = join(root, modules_dir).join(module.snake());
    let module_rs = module_dir.join("mod.rs");
    if !module_rs.is_file() {
        anyhow::bail!(
            "module `{m}` not found at {}; create it with `navree make:module {m} --crud`",
            module_rs.display(),
            m = module.snake()
        );
    }
    let model_file = module_dir
        .join("models")
        .join(format!("{}.rs", model.snake()));
    if !model_file.is_file() {
        anyhow::bail!(
            "model `{m}` not found at {}; Helm resources take navree-orm models: create it with `navree make:model {} {m} --policy`",
            model_file.display(),
            module.snake(),
            m = model.snake()
        );
    }
    let source = std::fs::read_to_string(&model_file)
        .with_context(|| format!("cannot read {}", model_file.display()))?;
    let info = parse_model(&source, Some(&model))
        .with_context(|| format!("cannot read the model in {}", model_file.display()))?;
    let model_path = format!("super::super::models::{}", model.snake());

    let resources_mod = module_dir.join("resources").join("mod.rs");
    let new_dir = !resources_mod.is_file();
    let mut files = FileSet::new();
    files.add(
        format!("resources/{}.rs", model.snake()),
        resource_file(&model, &info, &model_path)?,
    );
    if new_dir {
        files.add(
            "resources/mod.rs",
            format!(
                "//! Helm resources of the {} module.\n\npub mod {};\n",
                module.snake().replace('_', " "),
                model.snake()
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
        let decl = format!("pub mod {};", model.snake());
        report.edit_file(
            &resources_mod,
            |src| Some(declare_module(src, &decl)),
            || format!("declare `{decl}` in {}", resources_mod.display()),
        )?;
    }
    let line = format!(
        "navree::helm::ModuleBuilderHelmExt::resource::<resources::{}::{}Resource>(m);",
        model.snake(),
        model.pascal()
    );
    report.edit_file(
        &module_rs,
        |src| insert_above_marker(src, "navree:wire", &line),
        || format!("register it in the module's `wire` method: `{line}`"),
    )?;
    if info.subject.is_none() {
        report.hints.push(format!(
            "the model needs an access subject: add `subject` to its `#[model(..)]` attribute (`#[model(table = \"{}\", subject)]`: the subject `{}`, permissions `{}.<action>`) in {}",
            info.table,
            info.name,
            info.table,
            model_file.display()
        ));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEAL: &str = r#"
use chrono::{DateTime, NaiveDate, Utc};
use navree::orm::Model;
use serde::Serialize;
use uuid::Uuid;

#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "deals", timestamps, subject = "Deal", permission_prefix = "deals")]
pub struct Deal {
    pub id: Uuid,
    pub title: String,
    pub amount_cents: i64,
    pub currency: String,
    #[model(cast = "string_enum")]
    pub stage: Stage,
    pub company_id: Uuid,
    pub owner_id: Option<Uuid>,
    pub notes: Option<String>,
    pub won: bool,
    pub expected_close_date: Option<NaiveDate>,
    #[model(skip)]
    pub score: u8,
    pub created_at: DateTime<Utc>,
}
"#;

    #[test]
    fn models_are_read_with_syn() {
        let info = parse_model(DEAL, None).unwrap();
        assert_eq!(info.name, "Deal");
        assert_eq!(info.table, "deals");
        assert_eq!(
            info.subject,
            Some(SubjectInfo {
                name: "Deal".into(),
                permission_prefix: "deals".into()
            })
        );
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
        assert!(parse_model("struct Other;", None).is_err());
        assert!(parse_model("not rust {", None).is_err());
    }

    #[test]
    fn tables_keys_and_subjects_have_defaults() {
        let source = r#"
#[derive(navree::orm::Model, Clone, Debug, Default, Serialize)]
#[model(primary_key = "code", subject)]
pub struct Country {
    pub code: String,
    #[model(column = "display_name")]
    pub name: String,
}
"#;
        let info = parse_model(source, None).unwrap();
        assert_eq!(info.table, "countries");
        assert_eq!(
            info.subject,
            Some(SubjectInfo {
                name: "Country".into(),
                permission_prefix: "countries".into()
            })
        );
        assert!(info.columns[0].primary_key);
        assert_eq!(info.columns[1].name, "display_name");
        let plain = parse_model(
            "#[derive(Model)]\n#[model(table = \"subjects\")]\npub struct Topic { pub id: i64 }",
            None,
        )
        .unwrap();
        assert_eq!(plain.subject, None);
    }

    #[test]
    fn columns_become_fields() {
        let info = parse_model(DEAL, None).unwrap();
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
