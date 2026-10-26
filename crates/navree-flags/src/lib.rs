//! # navree-flags
//!
//! `FlagsModule`: feature flags that keep shipping code apart from
//! releasing it.
//!
//! - **Flags are types.** `#[flag]` (navree-macros, re-exported as
//!   `navree::flags::flag`) implements [`Flag`] for a unit struct: its
//!   stored name (the type name in kebab case unless `name = ".."`), its
//!   `default` and whether the front end may read it (`expose`). Register
//!   each with [`FlagsModule::for_feature`]; a check of an unregistered
//!   flag is [`FlagsError::Unregistered`].
//! - **Checks.** The [`Flags`] service (`flags.active::<F>(&scope)`), the
//!   [`CurrentFlags`] extractor (the request's user and workspace,
//!   memoized per request) and `#[requires_flag(F)]` on routes (the
//!   [`RequireFlag`] guard: 404, or 403 with `status = 403`, while the flag
//!   is off; documented in OpenAPI as `x-navree-flag`).
//! - **Scopes and rollouts.** A [`FlagScope`] names a user, a workspace or
//!   both. The first rule that decides wins: the kill switch, the user's
//!   override, the workspace's override, the global state, the
//!   [`Rollout`] (a stable hash bucket of the flag's name and the user's or
//!   workspace's id, optionally limited to plans through a
//!   [`PlanResolver`]), then the default.
//! - **Storage.** `feature_flags` and `feature_flag_overrides`
//!   ([`DatabaseFlagStore`]) or in process ([`MemoryFlagStore`]), behind
//!   [`FlagStore`]; state is cached in the app's cache for
//!   `flags.cache_ttl_secs`. Writes join the current unit of work, clear
//!   the cache, emit [`FlagChanged`] and write a `flag.changed` audit
//!   entry.
//! - **Routes and commands.** `GET /api/flags` (exposed flags for the
//!   front end), `/api/admin/flags...` (permission `flags.manage`) and
//!   `flags:list`, `flags:on`, `flags:off`, `flags:forget`,
//!   `flags:rollout`, `flags:kill`, `flags:revive`, `flags:purge`.
//! - **Tests.** `flags.store = "memory"`, or a [`FlagsFake`] that answers
//!   what it is told and records every check.
//!
//! ```
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! use navree_flags::{Flag, FlagScope, FlagsFake};
//! use uuid::Uuid;
//!
//! /// The redesigned deal pipeline board.
//! struct NewPipeline; // #[flag] writes this impl
//! impl Flag for NewPipeline {
//!     const NAME: &'static str = "new-pipeline";
//! }
//!
//! let fake = FlagsFake::new().on::<NewPipeline>();
//! let flags = fake.flags();
//! let scope = FlagScope::user(Uuid::now_v7()).workspace(Uuid::now_v7());
//! assert!(flags.active::<NewPipeline>(&scope).await.unwrap());
//! # });
//! ```
#![forbid(unsafe_code)]

pub mod entity;

mod commands;
mod controller;
mod error;
mod events;
mod fake;
mod flag;
mod migration;
mod module;
mod options;
mod plans;
mod registry;
mod request;
mod resolve;
mod rollout;
mod scope;
mod service;
mod state;
mod store;

pub use controller::{
    FLAGS_MANAGE_PERMISSION, FlagDetailsResource, FlagListResource, FlagResource,
    FlagsAdminController, FlagsController, OverrideInput, OverrideTarget, UpdateFlagInput,
};
pub use error::FlagsError;
pub use events::{FlagChange, FlagChanged};
pub use fake::FlagsFake;
pub use flag::{Flag, FlagDefinition, MAX_NAME_LEN, check_name};
pub use migration::FLAGS_MIGRATIONS;
pub use module::{FlagsModule, ModuleBuilderFlagsExt};
pub use options::{FlagStoreKind, FlagsOptions, MissingScope};
pub use plans::PlanResolver;
pub use registry::FlagRegistry;
pub use request::{CurrentFlags, FLAG_EXTENSION, RequireFlag, requires_flag};
pub use rollout::{Rollout, RolloutUnit, bucket};
pub use scope::{FlagScope, ScopeType, UserScope};
pub use service::Flags;
pub use state::{FlagOverride, FlagState};
pub use store::{DatabaseFlagStore, FlagStore, MemoryFlagStore};
