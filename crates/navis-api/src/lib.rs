//! # navis-api
//!
//! `ApiModule`: the app's OpenAPI 3.1 document, generated at boot from the
//! route table, a docs UI, and API resource helpers.
//!
//! - [`ApiModule`] builds the document from `navis_http::route_table` when the
//!   app boots and serves it at `GET {api_prefix}/openapi.json` (default
//!   `/api/openapi.json`), with the [Scalar](https://scalar.com) docs UI at
//!   `GET {api_prefix}/docs` (both public). The command
//!   `openapi:export [path]` writes it to a file (default `openapi.json`), for
//!   typed client generators such as openapi-typescript.
//! - Routes describe themselves with [`ApiMeta`](navis_http::ApiMeta)
//!   (`#[api(...)]` in controllers): request and response schemas come from
//!   [`navis_http::schema_fn`], and the schemas they reference become
//!   component schemas. Error statuses render as `application/json`
//!   responses with the [`HttpError`] schema (the NestJS error format:
//!   `statusCode`, `message`, `error`, and `errors` for validation errors);
//!   non-public routes carry the `cookie` (session)
//!   and `bearer` (JWT access token) security schemes. See [`openapi_document`] for the
//!   full mapping.
//! - Documentation mistakes stop the boot: two different schemas with one
//!   name ([`ApiError::SchemaConflict`]) or a reference to a schema nothing
//!   registered ([`ApiError::MissingSchemas`]; register generic type
//!   arguments with [`ModuleBuilderApiExt::api_schema`]).
//! - [`ApiResource`], [`resource_json`] and [`mask_fields`] shape JSON
//!   resources for front ends that check permissions with CASL: every
//!   resource carries its subject type in `__type`, and fields the user may
//!   not read are dropped.
//!
//! ```no_run
//! use navis_api::{ApiModule, ApiOptions};
//! use navis_config::{ConfigModule, ConfigSources};
//! use navis_http::HttpModule;
//! use navis_kernel::Navis;
//!
//! # fn main() -> Result<(), navis_kernel::BoxError> {
//! Navis::app()
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

mod docs;
mod document;
mod ext;
mod module;
mod options;
mod resource;

pub use document::{
    ApiError, DEFAULT_SESSION_COOKIE, HttpError, HttpErrorMessage, openapi_document,
};
pub use ext::ModuleBuilderApiExt;
pub use module::{ApiDocs, ApiModule};
pub use options::ApiOptions;
pub use resource::{ApiResource, TYPE_FIELD, mask_fields, resource_json};
