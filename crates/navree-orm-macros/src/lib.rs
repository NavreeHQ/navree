//! # navree-orm-macros
//!
//! The ORM's macros, re-exported by navree-orm (and through it by the
//! `navree` facade): depend on `navree-orm` or `navree`, not on this crate.
//!
//! - `#[derive(Model)]` implements `navree_orm::Model` for a struct with
//!   named fields and gives it one typed column constant per column.
//! - `#[scopes]` on an `impl` block turns its `#[scope]` functions into
//!   methods of the model's query builders.
//!
//! The expansions name the runtime crate as `::navree_orm` when the crate
//! being compiled depends on navree-orm, else as `::navree::orm` through
//! the facade (honouring a renamed dependency).
#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod errors;
mod model;
mod paths;
mod scopes;

/// Implements `Model` for a struct with named fields.
///
/// ```ignore
/// #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// #[model(table = "deals", timestamps, soft_deletes, per_page = 25)]
/// pub struct Deal {
///     pub id: Uuid,                      // the key: a UUID v7 is generated when it is nil
///     pub title: String,
///     #[model(cast = "string_enum")]
///     pub stage: Stage,                  // stored as its serde name
///     #[model(cast = "json")]
///     pub meta: Option<DealMeta>,        // stored as JSON, NULL as None
///     pub created_at: DateTime<Utc>,
///     pub updated_at: DateTime<Utc>,
///     pub deleted_at: Option<DateTime<Utc>>,
/// }
///
/// Deal::query().where_(Deal::STAGE.eq(Stage::Won)).get(&db).await?;
/// ```
///
/// Container attributes: `table = "..."` (default: the snake_case plural of
/// the struct's name), `primary_key = "..."` (default `id`), `key_type =
/// "int" | "uuid" | "uuid_v7" | "uuid_v4" | "ulid" | "manual"` (default: by
/// the key's type), `timestamps` or `timestamps(created_at = "...",
/// updated_at = false)`, `soft_deletes` or `soft_deletes = "..."`,
/// `connection = "..."`, `per_page = N`, `global_scopes(Scope, ...)`.
///
/// Field attributes: `column = "..."`, `cast = "json" | "string_enum" |
/// "int_enum" | path::to::Cast`, `default` (a row without the column reads
/// as the field's default), `skip` (not a column).
#[proc_macro_derive(Model, attributes(model))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    model::derive(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Turns the `#[scope]` functions of an `impl` block into methods of the
/// model's query builders, through a generated trait (`{Model}Scopes`,
/// public unless `vis = "..."` says otherwise; name it with `name =
/// "..."`).
///
/// ```ignore
/// #[scopes]
/// impl Deal {
///     /// Deals not closed yet.
///     #[scope]
///     fn open(query: Builder<Self>) -> Builder<Self> {
///         query.where_null(Deal::CLOSED_AT)
///     }
///
///     #[scope]
///     fn owned_by(query: Builder<Self>, owner: Uuid) -> Builder<Self> {
///         query.where_(Deal::OWNER_ID.eq(owner))
///     }
/// }
///
/// use crate::deals::DealScopes;
/// Deal::query().open().owned_by(me).get(&db).await?;
/// ```
#[proc_macro_attribute]
pub fn scopes(args: TokenStream, item: TokenStream) -> TokenStream {
    scopes::expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
