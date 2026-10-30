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
//! - `#[test]` (`#[navree::test]`) runs an `async fn` test, on a database
//!   of its own when it takes one.
//!
//! The expansions name the runtime crate as `::navree_orm` when the crate
//! being compiled depends on navree-orm, else as `::navree::orm` through
//! the facade (honouring a renamed dependency).
#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod accessors;
mod errors;
mod model;
mod paths;
mod relations;
mod scopes;
mod test;

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
/// the struct's name; `collection = "..."` says the same for a document
/// store), `primary_key = "..."` (default `id`), `key_type = "int" |
/// "uuid" | "uuid_v7" | "uuid_v4" | "ulid" | "object_id" | "manual"`
/// (default: by the key's type), `timestamps` or `timestamps(created_at =
/// "...", updated_at = false)`, `soft_deletes` or `soft_deletes = "..."`,
/// `connection = "..."`, `driver = "..."` (the document driver the model is
/// stored with), `per_page = N`, `global_scopes(Scope, ...)`.
/// Relation attributes: `touches = ["company"]` (relations whose records
/// are touched when a record is saved or deleted), `morph_name = "deal"`
/// (stored in polymorphic type columns; default: the snake_case name),
/// `with = ["company.owner"]` and `with_count = ["notes"]` (loaded and
/// counted by every query), `pivot(foreign_key = "deal_id", related_key =
/// "tag_id")` (a pivot model; without an `id` field it is keyed by the
/// related pivot column and identified by both).
/// Mass assignment: `fillable(field, "json->path", ..)`, `guarded(..)`,
/// `unguarded`; serialization: `hidden(field, "name", ..)`,
/// `visible(..)`, `appends(accessor, ..)`, `serialize_dates_with = path`,
/// `resource = Type`; encryption: `encrypter = "name"`.
/// Factories, binding, workspaces and access:
/// `factory = DealFactory` (`Deal::factory()`, see `navree_orm::factory`),
/// `route_key = "slug"` (the column route model binding looks records up
/// by), `workspace` or `workspace = "..."` (scoped to the current
/// workspace through that column, default `workspace_id`), and with
/// navree-orm's `access` feature `subject` or `subject = "Name"` (records
/// are navree-access subjects; default: the struct's name),
/// `permission_prefix = "..."` (default: the table) and `policy =
/// DealPolicy` (a `navree_access::Policy` attached to the model; implies
/// `subject`).
///
/// A model with `driver = "mongodb"` keeps its key in the document's `_id`:
/// unless `primary_key` says otherwise, the key column is `_id`, and a field
/// `id` without a `column` of its own is stored there.
///
/// ```ignore
/// #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// #[model(collection = "events", driver = "mongodb", timestamps)]
/// pub struct Event {
///     pub id: ObjectId,                  // stored as `_id`; generated when all zeros
///     pub kind: String,
///     pub created_at: DateTime<Utc>,
///     pub updated_at: DateTime<Utc>,
/// }
/// ```
///
/// Field attributes: `column = "..."`, `cast = "json" | "string_enum" |
/// "int_enum" | "decimal(N)" | "datetime_format(\"..\")" |
/// "unix_timestamp" | "unix_timestamp_ms" | "encrypted" |
/// "encrypted_json" | "hashed" | "binary_uuid" | "binary_ulid" | "vector" |
/// path::to::Cast`, `cast` (the type's own `Castable` cast), `cast(Path,
/// args..)`, `inbound = path::to::InboundCast`, `columns(name: Type, ..)`
/// (a field stored across several columns), `default` (a row without the
/// column reads as the field's default), `skip` (not a column), `hidden`,
/// `fillable`, `guarded`, `set_with = path` (a mutator `fn(&mut Model,
/// T)`, also generating `set_<field>`), `schema = Type` (its OpenAPI
/// schema).
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

/// Registers the `#[accessor]` methods of an impl block as the model's
/// accessors: computed values serialized records append, by default
/// (`#[model(appends(name))]`) or per record (`record.append(["name"])`).
///
/// ```ignore
/// #[accessors]
/// impl Deal {
///     /// The amount weighted by the stage's probability.
///     #[accessor]
///     fn weighted_amount(&self) -> Decimal {
///         self.amount * self.stage.probability()
///     }
///
///     #[accessor(name = "label", schema = String)]
///     fn display_label(&self) -> String {
///         format!("{} ({})", self.title, self.stage)
///     }
/// }
/// ```
///
/// An accessor is `fn name(&self) -> T` with `T: Serialize`; it stays a
/// plain method. `#[accessor(name = "...")]` renames its key,
/// `#[accessor(schema = Type)]` gives its OpenAPI schema. One
/// `#[accessors]` block per model.
#[proc_macro_attribute]
pub fn accessors(args: TokenStream, item: TokenStream) -> TokenStream {
    let item: proc_macro2::TokenStream = item.into();
    accessors::expand(args.into(), item.clone())
        .unwrap_or_else(|error| {
            let mut out = error.into_compile_error();
            out.extend(accessors::without_markers(item));
            out
        })
        .into()
}

/// Marks an accessor inside an `#[accessors]` impl block (on its own it
/// only reports where it belongs).
#[proc_macro_attribute]
pub fn accessor(args: TokenStream, item: TokenStream) -> TokenStream {
    let _ = args;
    let item: proc_macro2::TokenStream = item.into();
    let mut out = syn::Error::new(
        proc_macro2::Span::call_site(),
        "#[accessor] marks a method of an #[accessors] impl block: put #[accessors] on the block",
    )
    .into_compile_error();
    out.extend(item);
    out.into()
}

/// An `async fn` test, run on a runtime of its own (no `#[tokio::test]`
/// needed), given a database of its own when it takes one (navree-orm's
/// `testing` feature; see `navree_orm::testing::database`):
///
/// ```ignore
/// static DATABASE: Template = Template::new("crm", |template| Box::pin(migrate(template)));
///
/// #[navree::test(template = DATABASE)]          // a copy of the template, on SQLite
/// async fn wins_deals(db: TestDb) { /* ... */ }
///
/// #[navree::test(template = DATABASE, database = "postgres")]   // skipped without a server
/// async fn on_postgres(db: TestDb) -> Result<(), BoxError> { /* ... */ Ok(()) }
///
/// #[navree::test(database = "memory")]          // an empty in-memory SQLite database
/// async fn in_memory(db: TestDb) { /* ... */ }
///
/// #[navree::test]                               // no database
/// async fn plain() { /* ... */ }
/// ```
///
/// Options: `template = PATH` (a `static` `Template`), `database = "sqlite"
/// | "memory" | "postgres" | "mysql"` (default `"sqlite"`). Without a
/// template the database is empty. On a server that is not configured
/// (`NAVREE_TEST_POSTGRES_URL`, `NAVREE_TEST_MYSQL_URL`, or
/// `target/test-databases.env`) the test returns early, after a notice.
/// The test build needs the database's driver: navree-db's `sqlite` feature
/// (the facade's `sqlite`) for `"sqlite"` and `"memory"`, `postgres` or
/// `mysql` for the servers.
#[proc_macro_attribute]
pub fn test(args: TokenStream, item: TokenStream) -> TokenStream {
    test::expand(args.into(), item.into())
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
