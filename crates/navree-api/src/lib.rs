//! # navree-api
//!
//! `ApiModule`: the app's OpenAPI 3.1 document, generated at boot from the
//! route table, an API reference page, and API resource helpers.
//!
//! Items hidden from this documentation are not public API: Navree's own
//! macros and crates use them, and they can change in any release.
//!
//! - [`ApiModule`] builds the document from `navree_http::route_table` when the
//!   app boots and serves it at `GET {api_prefix}/openapi.json` (default
//!   `/api/openapi.json`), with the API reference page at
//!   `GET {api_prefix}/docs` (both public). The page is rendered from the
//!   document, and its stylesheet and script are embedded in the binary and
//!   served by the app at `{api_prefix}/docs/assets/`, so it loads nothing from
//!   a third party and its policy allows the app's own origin only. The
//!   command `openapi:export [path]` writes the document to a file (default
//!   `openapi.json`), for typed client generators such as `navree openapi:client`.
//! - Routes describe themselves with [`ApiMeta`](navree_http::ApiMeta)
//!   (`#[api(...)]` in controllers): request and response schemas come from
//!   [`navree_http::schema_fn`], and the schemas they reference become
//!   component schemas. Error statuses render as `application/json`
//!   responses with the [`HttpError`] schema (Navree's JSON error shape:
//!   `statusCode`, `message`, `error`, the machine-readable `code`, and
//!   `errors` for validation errors);
//!   non-public routes carry the `cookie` (session)
//!   and `bearer` (JWT access token) security schemes, or the ones the app
//!   says it uses ([`ModuleBuilderApiExt::api_security`]). See [`openapi_document`] for the
//!   full mapping.
//! - What `#[api]` cannot say is added with a [`RouteDoc`]
//!   ([`ModuleBuilderApiExt::api_route`]): the list query a route reads
//!   (`filter`, `sort`, `search`, paging, from its `ListSpec`), a response that is
//!   not JSON, another success status, or the security a route needs. The module
//!   documents the routes of navree's own modules this way: health, metrics, the
//!   real-time endpoints and the abilities.
//! - Documentation mistakes stop the boot: two different schemas with one
//!   name ([`ApiError::SchemaConflict`]) or a reference to a schema nothing
//!   registered ([`ApiError::MissingSchemas`]; register generic type
//!   arguments with [`ModuleBuilderApiExt::api_schema`]).
//! - `navree check` runs the conformity check `api.docs`: every route is
//!   documented with `#[api]` (or allowed by `api.undocumented`). With
//!   `api.require_docs = true`, an undocumented route stops the boot.
//! - [`ApiResource`], [`resource_json`] and [`mask_fields`] shape JSON
//!   resources for front ends that check permissions with CASL: every
//!   resource carries its subject type in `__type`, and fields the user may
//!   not read are dropped.
//!
//! ```no_run
//! use navree_api::{ApiModule, ApiOptions};
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_http::HttpModule;
//! use navree_kernel::Navree;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(HttpModule::from_config())
//!     .module(ApiModule::for_root(ApiOptions {
//!         title: "CRM API".to_owned(),
//!         ..ApiOptions::default()
//!     }))
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod conformity;
mod docs;
mod document;
mod ext;
mod framework_docs;
#[doc(hidden)]
pub mod markdown;
mod module;
mod options;
mod reference;
mod resource;
mod route_doc;
mod shapes;

pub use document::{
    ApiError, DEFAULT_SESSION_COOKIE, HttpError, HttpErrorMessage, openapi_document,
    openapi_document_with,
};
pub use ext::ModuleBuilderApiExt;
pub use module::{ApiDocs, ApiModule};
pub use options::ApiOptions;
pub use resource::{ApiResource, TYPE_FIELD, mask_fields, resource_json};
pub use route_doc::RouteDoc;
pub use shapes::{AbilityRule, AccessRole, AccessUserRoles, PageMeta};
