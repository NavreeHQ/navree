//! The relation attributes of `#[model(...)]` on the struct: `touches = [..]`,
//! `morph_name = ".."`, `with = [..]`, `with_count = [..]` and
//! `pivot(foreign_key = "..", related_key = "..")`.

use syn::meta::ParseNestedMeta;
use syn::{Expr, ExprArray, ExprLit, Lit, LitStr};

use crate::errors::key_name;

/// What the relation attributes say.
#[derive(Default)]
pub(crate) struct RelationAttrs {
    /// Relations touched when a record is saved or deleted.
    pub(crate) touches: Vec<LitStr>,
    /// The name stored in polymorphic type columns.
    pub(crate) morph_name: Option<LitStr>,
    /// Relations every query eager loads.
    pub(crate) with: Vec<LitStr>,
    /// Relations every query counts.
    pub(crate) with_count: Vec<LitStr>,
    /// The pivot columns of a pivot model: `(foreign_key, related_key)`.
    pub(crate) pivot: Option<(LitStr, LitStr)>,
}

/// Parses `meta` when it is one of [`KEYS`]; `Ok(false)` otherwise.
pub(crate) fn parse_key(
    attrs: &mut RelationAttrs,
    meta: &ParseNestedMeta<'_>,
) -> syn::Result<bool> {
    let path = &meta.path;
    if path.is_ident("touches") {
        attrs.touches.extend(names(meta, "touches", false)?);
    } else if path.is_ident("morph_name") {
        let name: LitStr = meta.value()?.parse()?;
        if name.value().trim().is_empty() {
            return Err(syn::Error::new(
                name.span(),
                "the morph name cannot be empty",
            ));
        }
        attrs.morph_name = Some(name);
    } else if path.is_ident("with") {
        attrs.with.extend(names(meta, "with", true)?);
    } else if path.is_ident("with_count") {
        attrs.with_count.extend(names(meta, "with_count", false)?);
    } else if path.is_ident("pivot") {
        let mut foreign_key = None;
        let mut related_key = None;
        meta.parse_nested_meta(|inner| {
            let slot = if inner.path.is_ident("foreign_key") {
                &mut foreign_key
            } else if inner.path.is_ident("related_key") {
                &mut related_key
            } else {
                return Err(inner.error(format!(
                    "unknown pivot option `{}`; expected `foreign_key` (the pivot column holding the parent's key) or `related_key` (the one holding the related record's key)",
                    key_name(&inner.path)
                )));
            };
            let column: LitStr = inner.value()?.parse()?;
            if column.value().trim().is_empty() {
                return Err(syn::Error::new(column.span(), "this column name cannot be empty"));
            }
            *slot = Some(column);
            Ok(())
        })?;
        match (foreign_key, related_key) {
            (Some(foreign_key), Some(related_key)) => {
                attrs.pivot = Some((foreign_key, related_key))
            }
            _ => {
                return Err(meta.error(
                    "a pivot model names both pivot columns: pivot(foreign_key = \"deal_id\", related_key = \"tag_id\")",
                ));
            }
        }
    } else {
        return Ok(false);
    }
    Ok(true)
}

/// `= ["a", "b"]`: relation names (`nested`: paths such as `"company.owner"`
/// too).
fn names(meta: &ParseNestedMeta<'_>, key: &str, nested: bool) -> syn::Result<Vec<LitStr>> {
    let array: ExprArray = meta.value()?.parse().map_err(|error| {
        syn::Error::new(
            error.span(),
            format!("`{key}` takes a list of relation names: {key} = [\"company\"]"),
        )
    })?;
    let mut names = Vec::with_capacity(array.elems.len());
    for element in array.elems {
        match element {
            Expr::Lit(ExprLit {
                lit: Lit::Str(name),
                ..
            }) => {
                let text = name.value();
                let is_step = |step: &str| {
                    !step.is_empty() && step.chars().all(|c| c.is_alphanumeric() || c == '_')
                };
                let valid = if nested {
                    text.split('.').all(is_step)
                } else {
                    is_step(&text)
                };
                if !valid {
                    let expected = if nested {
                        "letters, digits and `_`, nested relations joined by `.`"
                    } else {
                        "letters, digits and `_`"
                    };
                    return Err(syn::Error::new(
                        name.span(),
                        format!("`{text}` is not a relation name for `{key}` ({expected})"),
                    ));
                }
                names.push(name);
            }
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    format!("`{key}` takes relation names as string literals"),
                ));
            }
        }
    }
    Ok(names)
}
