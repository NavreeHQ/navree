//! `#[derive(Model)]`: reads the struct and its attributes, checks them,
//! and writes the `Model` impl and the column constants.

use proc_macro2::{Span, TokenStream};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, GenericArgument, Ident, LitInt, LitStr, PathArguments, Type};

use crate::errors::Errors;
use crate::paths;

pub(crate) mod attributes;
mod container;
mod expand;
mod field;
mod naming;

pub(crate) use container::KeyType;
pub(crate) use field::Cast;

/// A column of the model: a field that is stored (or, for a field stored
/// across several columns, one of its columns).
pub(crate) struct Column {
    /// The field.
    pub(crate) ident: Ident,
    /// The field's type (for one column of a multi-column field: the
    /// column's declared type).
    pub(crate) ty: Type,
    /// The column name.
    pub(crate) name: String,
    /// `T` when the field is an `Option<T>`.
    pub(crate) inner: Option<Type>,
    /// The cast the field is stored through.
    pub(crate) cast: Option<Cast>,
    /// A row without the column reads as the field's default.
    pub(crate) default: bool,
    /// The attribute layer's view of the field.
    pub(crate) field: attributes::FieldInfo,
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
    /// The attribute layer's view of the model.
    pub(crate) attributes: attributes::ModelAttrs,
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
    expand::expand(&model, &krate)
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

/// The columns of a field stored across several columns (`columns(..)`).
fn multi_columns(
    field_ident: &Ident,
    field_ty: &Type,
    cast: Option<(Cast, Span)>,
    columns: (Vec<(Ident, Type)>, Span),
    info: &attributes::FieldInfo,
    errors: &mut Errors,
) -> Vec<Column> {
    let (declared, span) = columns;
    let cast = match cast {
        Some((Cast::Path(path), _)) => path,
        Some((_, cast_span)) => {
            errors.push(syn::Error::new(
                cast_span,
                "a field stored across several columns needs the path of a type implementing `MultiColumnCast`: cast = path::To::Cast",
            ));
            return Vec::new();
        }
        None => {
            errors.push(syn::Error::new(
                span,
                "columns(..) needs the cast that splits the field: add cast = path::To::Cast (a `MultiColumnCast`)",
            ));
            return Vec::new();
        }
    };
    let names: Vec<String> = declared.iter().map(|(name, _)| plain_name(name)).collect();
    declared
        .into_iter()
        .enumerate()
        .map(|(index, (name, ty))| Column {
            ident: field_ident.clone(),
            ty,
            name: plain_name(&name),
            inner: None,
            cast: None,
            default: false,
            field: attributes::FieldInfo {
                multi: Some(attributes::MultiPart {
                    field_ty: field_ty.clone(),
                    field_inner: option_inner(field_ty),
                    cast: cast.clone(),
                    index,
                    names: names.clone(),
                }),
                ..info.clone()
            },
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
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
    let serde_container = attributes::serde_container(&input.attrs);
    let mut columns: Vec<Column> = Vec::new();
    let mut skipped = Vec::new();
    let mut skipped_hidden = Vec::new();
    let mut field_names = Vec::new();
    let mut flags: Vec<(String, attributes::FieldAttrs)> = Vec::new();
    for field in &fields.named {
        let Some(field_ident) = field.ident.clone() else {
            continue;
        };
        field_names.push(plain_name(&field_ident));
        let field_attrs = field::parse(&field.attrs, &mut errors);
        let serde_field = attributes::serde_field(&field.attrs);
        let layer = field_attrs.attributes;
        if let Some(span) = field_attrs.skip {
            if field_attrs.column.is_some() || field_attrs.cast.is_some() || field_attrs.default {
                errors.push(syn::Error::new(
                    span,
                    "a skipped field is not a column: drop `column`, `cast` and `default`",
                ));
            }
            if layer.fillable.is_some()
                || layer.guarded.is_some()
                || layer.set_with.is_some()
                || layer.columns.is_some()
                || layer.schema.is_some()
            {
                errors.push(syn::Error::new(
                    span,
                    "a skipped field is not a column: it can only be `hidden`",
                ));
            }
            if layer.hidden.is_some()
                && let Some(key) =
                    attributes::serialized_key(&field_ident, &serde_container, &serde_field)
            {
                skipped_hidden.push(key);
            }
            skipped.push(field_ident);
            continue;
        }
        let info = attributes::FieldInfo {
            serialized: attributes::serialized_key(&field_ident, &serde_container, &serde_field),
            serde_custom: serde_field_custom(&serde_field),
            serde_optional: serde_field.is_optional(),
            set_with: layer.set_with.clone(),
            schema: layer.schema.clone(),
            multi: None,
        };
        let new_columns = if let Some(declared) = layer.columns.clone() {
            if let Some(column) = &field_attrs.column {
                errors.push(syn::Error::new(
                    column.span(),
                    "a field stored across several columns names them in columns(..), not with `column`",
                ));
            }
            if field_attrs.default {
                errors.push(syn::Error::new(
                    field_ident.span(),
                    "a field stored across several columns cannot be `default`",
                ));
            }
            multi_columns(
                &field_ident,
                &field.ty,
                field_attrs.cast,
                declared,
                &info,
                &mut errors,
            )
        } else {
            let name = field_attrs
                .column
                .as_ref()
                .map_or_else(|| plain_name(&field_ident), LitStr::value);
            vec![Column {
                ident: field_ident.clone(),
                ty: field.ty.clone(),
                name,
                inner: option_inner(&field.ty),
                cast: field_attrs.cast.map(|(cast, _)| cast),
                default: field_attrs.default,
                field: info,
            }]
        };
        for column in new_columns {
            if let Some(existing) = columns.iter().find(|known| known.name == column.name) {
                errors.push(syn::Error::new(
                    field_ident.span(),
                    format!(
                        "column `{}` is already the column of field `{}`",
                        column.name, existing.ident
                    ),
                ));
            }
            flags.push((column.name.clone(), layer.flags()));
            columns.push(column);
        }
    }

    // Malformed attributes first: the checks below would only cascade.
    errors.finish()?;
    let mut errors = Errors::default();

    let key_name = attrs
        .primary_key
        .as_ref()
        .map_or_else(|| "id".to_owned(), LitStr::value);
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
    if key_column.field.multi.is_some() {
        errors.push(syn::Error::new(
            key_column.ident.span(),
            "the key cannot be stored across several columns: use an integer, `Uuid`, `Ulid` or `String` field",
        ));
    }
    if key_column.inner.is_some() {
        errors.push(syn::Error::new(
            key_column.ident.span(),
            "the key field cannot be an `Option`: an unset key is `0`, the nil UUID or ULID, or an empty string",
        ));
    }
    let key_type = attrs
        .key_type
        .unwrap_or_else(|| inferred_key_type(&key_column.ty));

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
            Some(found) if found.cast.is_some() || found.field.multi.is_some() => errors.push(syn::Error::new(
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
    let model_attrs = attributes::analyse(
        attrs.attributes,
        &columns,
        &flags,
        skipped_hidden,
        &field_names,
        &mut errors,
    );
    errors.finish()?;

    let table = attrs
        .table
        .as_ref()
        .map_or_else(|| naming::table_name(&ident.to_string()), LitStr::value);
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
        attributes: model_attrs,
    })
}

/// Whether serde writes the field its own way.
fn serde_field_custom(serde: &attributes::SerdeField) -> bool {
    serde.is_custom()
}
