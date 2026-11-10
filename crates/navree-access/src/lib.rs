//! # navree-access
//!
//! `AccessModule`: roles, permissions and abilities (CASL JSON), with
//! record checks and SQL list filters.
//!
//! The [`engine`] is pure and usable on its own: build [`Rules`], check them
//! with [`Ability::can`], filter lists with [`Ability::sql_filter`] and ship
//! them to the browser with [`Ability::to_casl_json`].
//!
//! The module half stores roles and permissions in the database and
//! enforces the rules on every request:
//!
//! - [`AccessModule::for_root`] with [`AccessOptions`] (an abilities
//!   function and an optional super-admin role) registers the tables, the
//!   [`AccessService`] (manage roles and permissions), the
//!   [`AbilityFactory`] (each principal's compiled, cached abilities) and
//!   the app's route authorizer ([`AccessAuthorizer`]) for `roles`,
//!   `permissions` and `can` route declarations.
//! - [`AccessModule::for_feature`] registers a subject whose records routes
//!   load ([`SubjectRecords`]: an ORM model with `#[model(subject =
//!   "...")]`, or a navree-db record), so `can(Update, Post, from = "id")`
//!   routes load the record in the request's unit of work and hand it to
//!   the handler as [`Authorized<S, A>`](Authorized); a [`Policy`]
//!   attached to the subject joins every principal's rules.
//! - [`Accessible<E, A>`](Accessible) filters list queries with the same
//!   rules (one SQL `CASE` chain, last match wins), so a record check and a
//!   list filter never disagree; [`Abilities`] gives the handler the rules.
//!
//! The module's tables are navree-db records ([`entity`]), created by a
//! navree-schema migration; list filters are navree-db values and portable
//! statements ([`SqlFilter::to_statement`]).
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod engine;
pub mod entity;

mod authorizer;
mod commands;
mod context;
mod controller;
mod disconnect;
mod error;
mod extract;
mod factory;
mod migration;
mod module;
mod options;
mod permission_check;
mod records;
mod service;
mod store;
mod subject;
mod workspace_gate;

pub use authorizer::{AccessAuthorizer, AuthorizedRecord, FieldCheck};
pub use context::{AbilityCtx, AccessUser};
pub use engine::{
    ALL, Ability, ActionMarker, CREATE, Cond, Conditions, Create, DELETE, DefaultColumns, Delete,
    FieldRef, IntoCondValue, MANAGE, Manage, READ, Read, Rule, RuleBuilder, RuleError, Rules,
    SqlColumn, SqlColumns, SqlFilter, SqlOp, Subject, TypedColumns, UPDATE, Update, field,
    sql_value,
};
pub use error::AccessError;
pub use extract::{Abilities, Accessible, Authorized, CurrentAccess};
pub use factory::{AbilityFactory, PrincipalAccess};
pub use module::AccessModule;
pub use options::{AbilitiesFn, AccessOptions, default_abilities};
pub use records::{Policy, SubjectRecords};
pub use service::{AccessService, Explanation, RoleInfo};
pub use subject::row_json;
pub use workspace_gate::{PinnedWorkspace, WorkspaceEntry, WorkspaceGate, WorkspaceWrites};
