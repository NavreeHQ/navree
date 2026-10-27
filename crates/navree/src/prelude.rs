//! The everyday names of a Navree app: `use navree::prelude::*;`.
//!
//! It brings the macros, the kernel types a module needs, the module types
//! an app root imports, the extractors and responses handlers use, the
//! access markers, the ORM's `Model` (trait and derive), `Record`,
//! `Collection`, `Builder` and `OrmModule` (with the model-level lifecycle
//! methods of `ModelLifecycle`), and every `ModuleBuilder` extension trait
//! (imported as `_`, so `m.migrations(..)`, `m.controller::<C>()`,
//! `m.job::<J>()`, `m.templates(..)`, `m.resource::<R>()` and friends
//! resolve without naming the traits). `navree::orm::prelude` has the rest
//! of the ORM's names.
//!
//! Every name is gated by the feature of the crate it comes from. With
//! `http`, `Result` here is `navree::Result` (error type `AppError` by
//! default), which shadows the standard prelude's `Result`; name the error
//! type when it is not an `AppError` (`Result<(), BoxError>`). With `db`,
//! navree-db's model factory trait is renamed `ModelFactory`, since
//! [`Factory`] is the kernel's provider factory. Test helpers
//! (`navree::testing`) are not part of the prelude.

pub use std::sync::Arc;

pub use crate::{controller, injectable, module};
pub use navree_config::{ConfigModule, ConfigSources, ModuleBuilderConfigExt as _};
pub use navree_contracts::ModuleBuilderJobsExt as _;
pub use navree_kernel::{
    AppBuilder, AppContext, BoxError, Factory, Injectable, Lazy, Module, ModuleBuilder, Navree,
    Resolver,
};

#[cfg(feature = "telemetry")]
pub use navree_telemetry::ModuleBuilderTelemetryExt as _;

#[cfg(feature = "http")]
pub use crate::Result;
#[cfg(feature = "http")]
pub use axum::extract::{Path, Query};
#[cfg(feature = "http")]
pub use navree_http::{
    AppError, Created, HttpModule, Json, ModuleBuilderHttpExt as _, NoContent, Scoped,
};

#[cfg(feature = "cache")]
pub use navree_cache::CacheModule;
#[cfg(feature = "crypto")]
pub use navree_crypto::CryptoModule;
#[cfg(feature = "events")]
pub use navree_events::{EventsModule, ModuleBuilderEventsExt as _};
#[cfg(feature = "http-client")]
pub use navree_http_client::HttpClientModule;
#[cfg(feature = "log")]
pub use navree_log::LoggerModule;
#[cfg(feature = "view")]
pub use navree_view::{ModuleBuilderViewExt as _, ViewModule};

#[cfg(feature = "api")]
pub use navree_api::{ApiModule, ModuleBuilderApiExt as _};
#[cfg(feature = "db")]
pub use navree_db::{
    DatabaseModule, Factory as ModelFactory, MigrationSet, ModuleBuilderDbExt as _, Page,
    PageQuery, Repo, Uow,
};
#[cfg(feature = "mail")]
pub use navree_mail::MailModule;
#[cfg(feature = "observe")]
pub use navree_observe::ObserveModule;
#[cfg(feature = "realtime")]
pub use navree_realtime::{ModuleBuilderRealtimeExt as _, RealtimeModule};
#[cfg(feature = "storage")]
pub use navree_storage::StorageModule;
#[cfg(feature = "time")]
pub use navree_time::{
    CurrentZone, DayRange, LocalDay, ModuleBuilderTimeExt as _, SendWindow, TimeModule, WithZone,
    Zone,
};

#[cfg(feature = "access")]
pub use navree_access::{
    ALL, Abilities, AbilityCtx, AccessModule, AccessOptions, Accessible, ActionMarker, Authorized,
    CREATE, Create, DELETE, Delete, MANAGE, Manage, READ, Read, Rules, Subject, UPDATE, Update,
    field,
};
#[cfg(feature = "audit")]
pub use navree_audit::AuditModule;
#[cfg(feature = "notify")]
pub use navree_notify::NotifyModule;
#[cfg(feature = "orm")]
pub use navree_orm::attributes::Attributes;
#[cfg(feature = "orm")]
pub use navree_orm::{
    Builder, Collection, Model, ModelLifecycle as _, ModuleBuilderOrmExt as _, OrmModule, Record,
    accessors, attrs,
};
#[cfg(feature = "queue")]
pub use navree_queue::{Batch, Operation, Operations, QueueModule};
#[cfg(feature = "schedule")]
pub use navree_schedule::{ModuleBuilderScheduleExt as _, ScheduleModule};
#[cfg(feature = "session")]
pub use navree_session::SessionModule;
#[cfg(feature = "validation")]
pub use navree_validation::{ModuleBuilderValidationExt as _, Valid, ValidAsync, ValidationModule};

#[cfg(feature = "auth")]
pub use navree_auth::{AuthModule, AuthService, CurrentUser, ModuleBuilderAuthExt as _};

#[cfg(feature = "ai")]
pub use navree_ai::{AiModule, ModuleBuilderAiExt as _};

#[cfg(feature = "mongo")]
pub use navree_mongo::{ModuleBuilderMongoExt as _, MongoDb, MongoModule};
#[cfg(feature = "redis")]
pub use navree_redis::RedisModule;

#[cfg(feature = "flags")]
pub use navree_flags::{CurrentFlags, FlagsModule, ModuleBuilderFlagsExt as _};
#[cfg(feature = "graphql")]
pub use navree_graphql::{GraphqlModule, ModuleBuilderGraphqlExt as _};
#[cfg(feature = "i18n")]
pub use navree_i18n::{I18nModule, Locale};

#[cfg(feature = "grpc")]
pub use navree_grpc::{GrpcModule, ModuleBuilderGrpcExt as _};

#[cfg(feature = "documents")]
pub use navree_documents::{DocumentsModule, ModuleBuilderDocumentsExt as _};

#[cfg(feature = "billing")]
pub use navree_billing::BillingModule;

#[cfg(feature = "mcp")]
pub use navree_mcp::{McpModule, ModuleBuilderMcpExt as _};

#[cfg(feature = "fleet")]
pub use navree_fleet::FleetModule;
#[cfg(feature = "lighthouse")]
pub use navree_lighthouse::{LighthouseModule, ModuleBuilderLighthouseExt as _};
#[cfg(feature = "logbook")]
pub use navree_logbook::LogbookModule;
#[cfg(feature = "workflows")]
pub use navree_workflows::{ModuleBuilderWorkflowsExt as _, WorkflowsModule};

#[cfg(feature = "workspace")]
pub use navree_workspace::{
    CurrentOrganization, CurrentPartition, CurrentWorkspace, InOrganization, InWorkspace,
    ModuleBuilderWorkspaceExt as _, OrgRepo, PartitionRepo, PartitionScope, PartitionScoped,
    WorkspaceModule, WorkspaceRepo, WorkspaceScoped, WorkspaceUow,
};

#[cfg(feature = "webhooks")]
pub use navree_webhooks::{ModuleBuilderWebhooksExt as _, WebhooksModule};

#[cfg(feature = "settings")]
pub use navree_settings::{ModuleBuilderSettingsExt as _, Setting, SettingsModule};
#[cfg(feature = "sheets")]
pub use navree_sheets::{ModuleBuilderSheetsExt as _, SheetsModule};

#[cfg(feature = "social")]
pub use navree_social::SocialModule;

#[cfg(feature = "sso")]
pub use navree_sso::SsoModule;

#[cfg(feature = "micro")]
pub use crate::message_handlers;
#[cfg(feature = "micro")]
pub use navree_micro::{MicroModule, ModuleBuilderMicroExt as _, Payload, ServiceClient};

#[cfg(feature = "oauth")]
pub use navree_oauth::{OAuthServerModule, OAuthToken};

#[cfg(feature = "helm")]
pub use navree_helm::{HelmModule, ModuleBuilderHelmExt as _};
#[cfg(feature = "search")]
pub use navree_search::SearchModule;
