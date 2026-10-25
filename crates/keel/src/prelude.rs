//! The everyday names of a Keel app: `use keel::prelude::*;`.
//!
//! It brings the macros, the kernel types a module needs, the module types
//! an app root imports, the extractors and responses handlers use, the
//! access markers, and every `ModuleBuilder` extension trait (imported as
//! `_`, so `m.migrations(..)`, `m.controller::<C>()`, `m.job::<J>()`,
//! `m.templates(..)` and friends resolve without naming the traits).
//!
//! Every name is gated by the feature of the crate it comes from. With
//! `http`, `Result` here is `keel::Result` (error type `AppError` by
//! default), which shadows the standard prelude's `Result`; name the error
//! type when it is not an `AppError` (`Result<(), BoxError>`). With `db`,
//! keel-db's model factory trait is renamed `ModelFactory`, since
//! [`Factory`] is the kernel's provider factory. Test helpers
//! (`keel::testing`) are not part of the prelude.

pub use std::sync::Arc;

pub use crate::{controller, injectable, module};
pub use keel_config::{ConfigModule, ConfigSources, ModuleBuilderConfigExt as _};
pub use keel_contracts::ModuleBuilderJobsExt as _;
pub use keel_kernel::{
    AppBuilder, AppContext, BoxError, Factory, Injectable, Keel, Lazy, Module, ModuleBuilder,
    Resolver,
};

#[cfg(feature = "http")]
pub use crate::Result;
#[cfg(feature = "http")]
pub use axum::extract::{Path, Query};
#[cfg(feature = "http")]
pub use keel_http::{
    AppError, Created, HttpModule, Json, ModuleBuilderHttpExt as _, NoContent, Scoped,
};

#[cfg(feature = "cache")]
pub use keel_cache::CacheModule;
#[cfg(feature = "crypto")]
pub use keel_crypto::CryptoModule;
#[cfg(feature = "events")]
pub use keel_events::{EventsModule, ModuleBuilderEventsExt as _};
#[cfg(feature = "http-client")]
pub use keel_http_client::HttpClientModule;
#[cfg(feature = "log")]
pub use keel_log::LoggerModule;
#[cfg(feature = "view")]
pub use keel_view::{ModuleBuilderViewExt as _, ViewModule};

#[cfg(feature = "api")]
pub use keel_api::{ApiModule, ModuleBuilderApiExt as _};
#[cfg(feature = "db")]
pub use keel_db::{
    DatabaseModule, Factory as ModelFactory, MigrationSet, ModuleBuilderDbExt as _, Page,
    PageQuery, Repo, Uow,
};
#[cfg(feature = "mail")]
pub use keel_mail::MailModule;
#[cfg(feature = "observe")]
pub use keel_observe::ObserveModule;
#[cfg(feature = "realtime")]
pub use keel_realtime::{ModuleBuilderRealtimeExt as _, RealtimeModule};
#[cfg(feature = "storage")]
pub use keel_storage::StorageModule;

#[cfg(feature = "access")]
pub use keel_access::{
    ALL, Abilities, AbilityCtx, AccessModule, AccessOptions, Accessible, ActionMarker, Authorized,
    CREATE, Create, DELETE, Delete, MANAGE, Manage, READ, Read, Rules, Subject, UPDATE, Update,
    field,
};
#[cfg(feature = "audit")]
pub use keel_audit::AuditModule;
#[cfg(feature = "notify")]
pub use keel_notify::NotifyModule;
#[cfg(feature = "queue")]
pub use keel_queue::QueueModule;
#[cfg(feature = "schedule")]
pub use keel_schedule::{ModuleBuilderScheduleExt as _, ScheduleModule};
#[cfg(feature = "session")]
pub use keel_session::SessionModule;
#[cfg(feature = "validation")]
pub use keel_validation::{Valid, ValidAsync, ValidationModule};

#[cfg(feature = "auth")]
pub use keel_auth::{AuthModule, AuthService, CurrentUser, ModuleBuilderAuthExt as _};
