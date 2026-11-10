//! # navree-grpc
//!
//! `GrpcModule`: tonic services served from Navree modules, with the same
//! dependency injection, guards, access rules, extractors, errors and unit
//! of work as HTTP routes; gRPC-Web and Connect for browsers and `curl`;
//! health and reflection; and named channels to the services an app calls.
//!
//! Items hidden from this documentation are not public API: Navree's own
//! macros and crates use them, and they can change in any release.
//!
//! - **Services.** Implement tonic's generated trait on an `#[injectable]`
//!   struct and mark the impl block [`#[grpc_service(server = DealsServer)]`](grpc_service).
//!   Methods take tonic's [`Request<T>`] (or [`Valid<Request<T>>`](Valid)
//!   to run the message's validation rules), then any Navree extractor (`Uow`,
//!   `CurrentPrincipal`, `Authorized<E, A>`, ...), and return
//!   `Result<Response<T>>` with an `AppError`, or a [`Status`]. Each method
//!   declares its access (`#[public]`, `#[authenticated]`, `#[roles]`,
//!   `#[permissions]`, `#[can]`) on itself or on the impl block, or the
//!   service does not compile. Register the struct with
//!   [`ModuleBuilderGrpcExt::grpc_service`] and its descriptor set with
//!   [`ModuleBuilderGrpcExt::grpc_descriptors`].
//! - **The call pipeline.** Request id and tracing, the interceptor, the
//!   app's `Authenticator`, the access check, guards and the route
//!   authorizer (`#[can(.., from = "id")]` loads the record named by the
//!   message's `id` field), the deadline (`grpc-timeout`, up to
//!   `grpc.timeout_secs`), then the method in a unit of work that commits
//!   when it returns `Ok`. Errors become the status of the same meaning
//!   ([`status_from_error`]); `?` on a `Status` in an HTTP handler gives the
//!   app error of the same meaning ([`error_from_status`]).
//! - **Serving.** By default on the HTTP port: requests whose content type
//!   is gRPC, gRPC-Web or Connect go to the services, the rest to the
//!   routes. `grpc.addr` gives gRPC a listener of its own. Health
//!   (`grpc.health.v1.Health`, from the readiness checks) and server
//!   reflection (in debug builds) are on by default.
//! - **Clients.** [`GrpcChannels`] holds a channel per `grpc.clients.<name>`;
//!   calls carry the request id, the trace context, the `authorization`,
//!   the workspace and the remaining deadline of the call being handled.
//! - **Testing.** With the `testing` feature, [`testing::TestChannel`]
//!   connects tonic clients to a `TestApp` in-process.
//!
//! Code generation needs no system `protoc`: compile the `.proto` files
//! with [protox](https://docs.rs/protox) and generate with
//! `tonic_prost_build::configure().compile_fds(..)` in `build.rs`.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// The `#[grpc_service]` expansion names `::navree_grpc` when this crate is
// used without the facade, including in its own tests and docs.
extern crate self as navree_grpc;

mod call;
mod channels;
mod connect;
mod context;
mod edge;
mod frame;
mod health;
mod module;
mod options;
mod routes;
mod server;
mod service;
mod status;
#[cfg(feature = "testing")]
pub mod testing;

pub use call::{CallError, RecordKey};
pub use channels::{GrpcChannel, GrpcChannels};
pub use module::GrpcModule;
pub use options::{ClientOptions, ClientTlsOptions, GrpcOptions};
pub use routes::Routes;
pub use server::{GrpcErrors, GrpcServer};
pub use service::{GrpcMethod, GrpcService, ModuleBuilderGrpcExt, ServiceLimits};
pub use status::{error_from_status, status_from_error};

/// The attribute that serves a tonic service impl with Navree's call
/// pipeline (see the crate docs).
pub use navree_macros::grpc_service;
/// `Valid<Request<T>>`: the message's validation rules run before the method.
pub use navree_validation::Valid;
/// tonic's request, response, status and streaming types, and the
/// `async_trait` attribute its generated traits use.
pub use tonic::{Code, Request, Response, Status, Streaming, async_trait, metadata};

/// What `#[grpc_service]` expansions call; not a public API.
#[doc(hidden)]
pub mod __private {
    pub use std::sync::Arc;

    pub use http::request::Parts;
    pub use navree_http::Guard;
    pub use tonic::server::NamedService;
    pub use tonic::service::interceptor::InterceptedService;

    pub use crate::routes::Routes;

    pub use crate::call::{
        CallError, RecordKey, RecordKeyFn, call, extract, finish, lift_body_limit, returns,
        validate,
    };
}
