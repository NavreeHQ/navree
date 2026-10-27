//! # navree-orm-macros
//!
//! The ORM's macros, re-exported by navree-orm (and through it by the
//! `navree` facade): depend on `navree-orm` or `navree`, not on this crate.
//!
//! - `#[derive(Model)]` implements `navree_orm::Model` for a struct with
//!   named fields and gives it one typed column constant per column.
//! - `#[scopes]` on an `impl` block turns its `#[scope]` functions into
//!   methods of the model's query builders.
//! - `#[relations]` on an `impl` block names each of its relation methods
//!   after itself and registers them, so relation paths
//!   (`"company.owner"`) find them.
//!
//! The expansions name the runtime crate as `::navree_orm` when the crate
//! being compiled depends on navree-orm, else as `::navree::orm` through
//! the facade (honouring a renamed dependency).
#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod errors;
mod model;
mod paths;
mod relations;
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
/// Relation attributes: `touches = ["company"]` (relations whose records
/// are touched when a record is saved or deleted), `morph_name = "deal"`
/// (stored in polymorphic type columns; default: the snake_case name),
/// `with = ["company.owner"]` and `with_count = ["notes"]` (loaded and
/// counted by every query), `pivot(foreign_key = "deal_id", related_key =
/// "tag_id")` (a pivot model; without an `id` field it is keyed by the
/// related pivot column and identified by both).
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

/// Declares a model's relations: each method of the impl block, `fn
/// name(&self) -> RelationType`, is a relation, named after the method (so
/// `deal.company()` and `with("company")` mean the same relation).
///
/// ```ignore
/// #[relations]
/// impl Deal {
///     pub fn company(&self) -> BelongsTo<Deal, Company> {
///         self.belongs_to(Deal::COMPANY_ID)
///     }
///
///     pub fn activities(&self) -> HasMany<Deal, Activity> {
///         self.has_many(Activity::DEAL_ID).latest()
///     }
/// }
///
/// let deals = Deal::query().with("company").with_count("activities").get(&db).await?;
/// let company = deal.company().first(&db).await?;
/// ```
///
/// The relation definitions (`belongs_to`, `has_many`, ...) are in scope in
/// the methods. A model with relations implements `Default` (definitions
/// are read from a default instance when a relation is used by name), and
/// has one `#[relations]` block; relations added by other modules go
/// through `RelationRegistry`.
#[proc_macro_attribute]
pub fn relations(args: TokenStream, item: TokenStream) -> TokenStream {
    let item: proc_macro2::TokenStream = item.into();
    relations::expand(args.into(), item.clone())
        .unwrap_or_else(|error| {
            // The impl block stays, so the errors are the only ones.
            let mut out = error.into_compile_error();
            out.extend(item);
            out
        })
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
    let item: proc_macro2::TokenStream = item.into();
    scopes::expand(args.into(), item.clone())
        .unwrap_or_else(|error| {
            // The impl block stays, so the errors are the only ones.
            let mut out = error.into_compile_error();
            out.extend(scopes::without_markers(item));
            out
        })
        .into()
}
