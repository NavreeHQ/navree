//! The everyday names of a Navis app: `use navis::prelude::*;`.
//!
//! It brings the macros, the kernel types a module needs, the module types
//! an app root imports, the extractors and responses handlers use, the
//! access markers, and every `ModuleBuilder` extension trait (imported as
//! `_`, so `m.migrations(..)`, `m.controller::<C>()`, `m.job::<J>()`,
//! `m.templates(..)` and friends resolve without naming the traits).
//!
//! Every name is gated by the feature of the crate it comes from. With
//! `http`, `Result` here is `navis::Result` (error type `AppError` by
//! default), which shadows the standard prelude's `Result`; name the error
//! type when it is not an `AppError` (`Result<(), BoxError>`). With `db`,
//! navis-db's model factory trait is renamed `ModelFactory`, since
//! [`Factory`] is the kernel's provider factory. Test helpers
//! (`navis::testing`) are not part of the prelude.

pub use std::sync::Arc;

pub use crate::{controller, injectable, module};
pub use navis_config::{ConfigModule, ConfigSources, ModuleBuilderConfigExt as _};
pub use navis_contracts::ModuleBuilderJobsExt as _;
pub use navis_kernel::{
    AppBuilder, AppContext, BoxError, Factory, Injectable, Lazy, Module, ModuleBuilder, Navis,
    Resolver,
};

#[cfg(feature = "http")]
pub use crate::Result;
#[cfg(feature = "http")]
pub use axum::extract::{Path, Query};
#[cfg(feature = "http")]
pub use navis_http::{
    AppError, Created, HttpModule, Json, ModuleBuilderHttpExt as _, NoContent, Scoped,
};

#[cfg(feature = "cache")]
pub use navis_cache::CacheModule;
#[cfg(feature = "crypto")]
pub use navis_crypto::CryptoModule;
#[cfg(feature = "events")]
pub use navis_events::{EventsModule, ModuleBuilderEventsExt as _};
#[cfg(feature = "http-client")]
pub use navis_http_client::HttpClientModule;
#[cfg(feature = "log")]
pub use navis_log::LoggerModule;
#[cfg(feature = "view")]
pub use navis_view::{ModuleBuilderViewExt as _, ViewModule};

#[cfg(feature = "api")]
pub use navis_api::{ApiModule, ModuleBuilderApiExt as _};
#[cfg(feature = "db")]
pub use navis_db::{
    DatabaseModule, Factory as ModelFactory, MigrationSet, ModuleBuilderDbExt as _, Page,
    PageQuery, Repo, Uow,
};
#[cfg(feature = "mail")]
pub use navis_mail::MailModule;
#[cfg(feature = "observe")]
pub use navis_observe::ObserveModule;
#[cfg(feature = "realtime")]
pub use navis_realtime::{ModuleBuilderRealtimeExt as _, RealtimeModule};
#[cfg(feature = "storage")]
pub use navis_storage::StorageModule;

#[cfg(feature = "access")]
pub use navis_access::{
    ALL, Abilities, AbilityCtx, AccessModule, AccessOptions, Accessible, ActionMarker, Authorized,
    CREATE, Create, DELETE, Delete, MANAGE, Manage, READ, Read, Rules, Subject, UPDATE, Update,
    field,
};
#[cfg(feature = "audit")]
pub use navis_audit::AuditModule;
#[cfg(feature = "notify")]
pub use navis_notify::NotifyModule;
#[cfg(feature = "queue")]
pub use navis_queue::QueueModule;
#[cfg(feature = "schedule")]
pub use navis_schedule::{ModuleBuilderScheduleExt as _, ScheduleModule};
#[cfg(feature = "session")]
pub use navis_session::SessionModule;
#[cfg(feature = "validation")]
pub use navis_validation::{Valid, ValidAsync, ValidationModule};

#[cfg(feature = "auth")]
pub use navis_auth::{AuthModule, AuthService, CurrentUser, ModuleBuilderAuthExt as _};
