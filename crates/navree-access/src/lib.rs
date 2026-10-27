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
//! - [`AccessModule::for_feature`] registers an entity as a subject, so
//!   `can(Update, Post, from = "id")` routes load the record in the
//!   request's unit of work and hand it to the handler as
//!   [`Authorized<E, A>`](Authorized).
//! - [`Accessible<E, A>`](Accessible) filters list queries with the same
//!   rules (one SQL `CASE` chain, last match wins), so a record check and a
//!   list filter never disagree; [`Abilities`] gives the handler the rules.
#![forbid(unsafe_code)]

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
mod service;
mod store;
mod subject;

pub use authorizer::{AccessAuthorizer, AuthorizedRecord, FieldCheck};
pub use context::{AbilityCtx, AccessUser};
pub use engine::{
    ALL, Ability, ActionMarker, CREATE, Cond, Conditions, Create, DELETE, DefaultColumns, Delete,
    FieldRef, IntoCondValue, MANAGE, Manage, READ, Read, Rule, RuleBuilder, RuleError, Rules,
    SqlColumns, Subject, UPDATE, Update, field, sql_value,
};
pub use error::AccessError;
pub use extract::{Abilities, Accessible, Authorized, CurrentAccess};
pub use factory::{AbilityFactory, PrincipalAccess};
pub use module::AccessModule;
pub use options::{AbilitiesFn, AccessOptions, default_abilities};
pub use service::{AccessService, Explanation, RoleInfo};
pub use subject::{EntityColumns, record_json};
