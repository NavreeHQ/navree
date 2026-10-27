//! `#[derive(Model)]`: reads the struct and its attributes, checks them,
//! and writes the `Model` impl and the column constants.

use proc_macro2::{Span, TokenStream};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, GenericArgument, Ident, LitInt, LitStr, PathArguments, Type};

use crate::errors::Errors;
use crate::paths;

mod container;
mod expand;
mod field;
mod naming;
mod relations;

pub(crate) use container::KeyType;
pub(crate) use field::Cast;

/// A column of the model: a field that is stored.
pub(crate) struct Column {
    /// The field.
    pub(crate) ident: Ident,
    /// The field's type.
    pub(crate) ty: Type,
    /// The column name.
    pub(crate) name: String,
    /// `T` when the field is an `Option<T>`.
    pub(crate) inner: Option<Type>,
    /// The cast the field is stored through.
    pub(crate) cast: Option<Cast>,
    /// A row without the column reads as the field's default.
    pub(crate) default: bool,
}

/// What the derive knows about the model.
pub(crate) struct Model {
    pub(crate) ident: Ident,
    pub(crate) vis: syn::Visibility,
    pub(crate) table: String,
    /// The key column's index in `columns`.
    pub(crate) key: usize,
    pub(crate) key_type: KeyType,
    pub(crate) columns: Vec<Column>,
    /// Fields that are not columns (`#[model(skip)]`).
    pub(crate) skipped: Vec<Ident>,
    pub(crate) created_at: Option<String>,
    pub(crate) updated_at: Option<String>,
    pub(crate) deleted_at: Option<String>,
    pub(crate) connection: Option<LitStr>,
    pub(crate) per_page: Option<LitInt>,
    pub(crate) global_scopes: Vec<syn::Expr>,
    /// The name polymorphic type columns store for the model.
    pub(crate) morph_name: String,
    /// The default name of foreign keys to the model.
    pub(crate) foreign_key: String,
    /// Relations touched when a record is saved or deleted.
    pub(crate) touches: Vec<LitStr>,
    /// Relations every query eager loads.
    pub(crate) with: Vec<LitStr>,
    /// Relations every query counts.
    pub(crate) with_count: Vec<LitStr>,
    /// The pivot columns of a pivot model.
    pub(crate) pivot: Option<(String, String)>,
}

impl Model {
    /// The column named `name`.
    pub(crate) fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|column| column.name == name)
    }

    /// The timestamp columns (creation, update, soft delete).
    pub(crate) fn timestamp_columns(&self) -> impl Iterator<Item = &Column> {
        [&self.created_at, &self.updated_at, &self.deleted_at]
            .into_iter()
            .flatten()
            .filter_map(|name| self.column(name))
    }
}

/// Expands `#[derive(Model)]`.
pub(crate) fn derive(input: DeriveInput) -> syn::Result<TokenStream> {
    let krate = paths::orm(input.ident.span())?;
    let model = analyse(&input)?;
    Ok(expand::expand(&model, &krate))
}

/// The field's name without a raw-identifier prefix (`r#type` → `type`).
pub(crate) fn plain_name(ident: &Ident) -> String {
    let name = ident.to_string();
    name.strip_prefix("r#").map_or(name.clone(), str::to_owned)
}

/// `T` when `ty` is `Option<T>`.
fn option_inner(ty: &Type) -> Option<Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let last = path.path.segments.last()?;
    if last.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &last.arguments else {
        return None;
    };
    match (arguments.args.len(), arguments.args.first()) {
        (1, Some(GenericArgument::Type(inner))) => Some(inner.clone()),
        _ => None,
    }
}

/// The key type a key field's type implies: integers are assigned by the
/// database, a `Uuid` gets a UUID v7, a `Ulid` a ULID, anything else is set
/// by the application.
fn inferred_key_type(ty: &Type) -> KeyType {
    let Type::Path(path) = ty else {
        return KeyType::Manual;
    };
    match path
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .as_deref()
    {
        Some("i16" | "i32" | "i64" | "u32" | "u64") => KeyType::Increments,
        Some("Uuid") => KeyType::UuidV7,
        Some("Ulid") => KeyType::Ulid,
        _ => KeyType::Manual,
    }
}

fn analyse(input: &DeriveInput) -> syn::Result<Model> {
    let ident = input.ident.clone();
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            ident.span(),
            "#[derive(Model)] needs a struct with named fields",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new(
            ident.span(),
            "#[derive(Model)] needs a struct with named fields",
        ));
    };
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "#[derive(Model)] does not take generic parameters: a model is one table",
        ));
    }

    let mut errors = Errors::default();
    let attrs = container::parse(&input.attrs, &mut errors);
    let mut columns: Vec<Column> = Vec::new();
    let mut skipped = Vec::new();
    for field in &fields.named {
        let Some(field_ident) = field.ident.clone() else {
            continue;
        };
        let field_attrs = field::parse(&field.attrs, &mut errors);
        if let Some(span) = field_attrs.skip {
            if field_attrs.column.is_some() || field_attrs.cast.is_some() || field_attrs.default {
                errors.push(syn::Error::new(
                    span,
                    "a skipped field is not a column: drop `column`, `cast` and `default`",
                ));
            }
            skipped.push(field_ident);
            continue;
        }
        let name = field_attrs
            .column
            .as_ref()
            .map_or_else(|| plain_name(&field_ident), LitStr::value);
        if let Some(existing) = columns.iter().find(|column| column.name == name) {
            errors.push(syn::Error::new(
                field_ident.span(),
                format!(
                    "column `{name}` is already the column of field `{}`",
                    existing.ident
                ),
            ));
        }
        columns.push(Column {
            ident: field_ident,
            ty: field.ty.clone(),
            name,
            inner: option_inner(&field.ty),
            cast: field_attrs.cast.map(|(cast, _)| cast),
            default: field_attrs.default,
        });
    }

    // Malformed attributes first: the checks below would only cascade.
    errors.finish()?;
    let mut errors = Errors::default();

    // A pivot model without an `id` column is keyed by its related pivot
    // column (rows are identified by both pivot columns).
    let pivot = attrs
        .relations
        .pivot
        .as_ref()
        .map(|(foreign, related)| (foreign.value(), related.value()));
    let keyed_by_pivot = attrs.primary_key.is_none()
        && pivot.is_some()
        && !columns.iter().any(|column| column.name == "id");
    let key_name = match (&attrs.primary_key, &pivot) {
        (Some(key), _) => key.value(),
        (None, Some((_, related))) if keyed_by_pivot => related.clone(),
        _ => "id".to_owned(),
    };
    if let Some((foreign_key, related_key)) = &attrs.relations.pivot {
        for column in [foreign_key, related_key] {
            if !columns
                .iter()
                .any(|candidate| candidate.name == column.value())
            {
                errors.push(syn::Error::new(
                    column.span(),
                    format!(
                        "the pivot column `{}` is not a column of {ident}: add a field for it",
                        column.value()
                    ),
                ));
            }
        }
        // A missing related pivot column is also the missing key of a
        // model keyed by it: the error above says it once.
        if keyed_by_pivot && !columns.iter().any(|column| column.name == key_name) {
            errors.finish()?;
            return Err(syn::Error::new(
                related_key.span(),
                format!("the pivot column `{key_name}` is not a column of {ident}"),
            ));
        }
    }
    let Some(key) = columns.iter().position(|column| column.name == key_name) else {
        return Err(errors.with(syn::Error::new(
            ident.span(),
            format!(
                "{ident} has no field for its primary key `{key_name}`: add one, or name the key column with #[model(primary_key = \"...\")]"
            ),
        )));
    };
    let key_column = &columns[key];
    if key_column.cast.is_some() {
        errors.push(syn::Error::new(
            key_column.ident.span(),
            "the key field cannot have a cast: use an integer, `Uuid`, `Ulid` or `String` field",
        ));
    }
    if key_column.inner.is_some() {
        errors.push(syn::Error::new(
            key_column.ident.span(),
            "the key field cannot be an `Option`: an unset key is `0`, the nil UUID or ULID, or an empty string",
        ));
    }
    let key_type = attrs.key_type.unwrap_or_else(|| {
        if keyed_by_pivot {
            KeyType::Manual
        } else {
            inferred_key_type(&key_column.ty)
        }
    });

    let timestamp_span = attrs.timestamps.span.unwrap_or_else(Span::call_site);
    let mut check_timestamp = |column: &Option<String>, what: &str, option: &str, span: Span| {
        let Some(name) = column else {
            return;
        };
        match columns.iter().find(|candidate| &candidate.name == name) {
            None => errors.push(syn::Error::new(
                span,
                format!("{what} needs a field for the `{name}` column (or {option})"),
            )),
            Some(found) if found.cast.is_some() => errors.push(syn::Error::new(
                found.ident.span(),
                "a timestamp field cannot have a cast: use `DateTime<Utc>`, `DateTime<FixedOffset>`, `NaiveDateTime` or `i64` (or an `Option` of one)",
            )),
            Some(_) => {}
        }
    };
    check_timestamp(
        &attrs.timestamps.created_at,
        "#[model(timestamps)]",
        "name the column with timestamps(created_at = \"...\"), or turn it off with created_at = false",
        timestamp_span,
    );
    check_timestamp(
        &attrs.timestamps.updated_at,
        "#[model(timestamps)]",
        "name the column with timestamps(updated_at = \"...\"), or turn it off with updated_at = false",
        timestamp_span,
    );
    let deleted_at = attrs.soft_deletes.as_ref().map(|(name, _)| name.clone());
    if let Some((name, span)) = &attrs.soft_deletes {
        check_timestamp(
            &deleted_at,
            "#[model(soft_deletes)]",
            "name the column with soft_deletes = \"...\"",
            *span,
        );
        if let Some(column) = columns.iter().find(|candidate| &candidate.name == name)
            && column.inner.is_none()
        {
            errors.push(syn::Error::new(
                column.ident.span(),
                format!(
                    "the soft-delete column `{name}` must be nullable: make the field an `Option<DateTime<Utc>>` (NULL means not deleted)"
                ),
            ));
        }
    }
    errors.finish()?;

    let table = attrs
        .table
        .as_ref()
        .map_or_else(|| naming::table_name(&ident.to_string()), LitStr::value);
    let snake = heck::ToSnakeCase::to_snake_case(ident.to_string().as_str());
    let morph_name = attrs
        .relations
        .morph_name
        .as_ref()
        .map_or_else(|| snake.clone(), LitStr::value);
    let foreign_key = format!("{snake}_{key_name}");
    Ok(Model {
        ident,
        vis: input.vis.clone(),
        table,
        key,
        key_type,
        columns,
        skipped,
        created_at: attrs
            .timestamps
            .on
            .then(|| attrs.timestamps.created_at.clone())
            .flatten(),
        updated_at: attrs
            .timestamps
            .on
            .then(|| attrs.timestamps.updated_at.clone())
            .flatten(),
        deleted_at,
        connection: attrs.connection,
        per_page: attrs.per_page,
        global_scopes: attrs.global_scopes,
        morph_name,
        foreign_key,
        touches: attrs.relations.touches,
        with: attrs.relations.with,
        with_count: attrs.relations.with_count,
        pivot,
    })
}
