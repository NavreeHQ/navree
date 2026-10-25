//! # keel-access
//!
//! `AccessModule`: roles, permissions and CASL-compatible abilities, with
//! record checks and SQL list filters.
//!
//! The [`engine`] is pure and usable on its own: build [`Rules`], check them
//! with [`Ability::can`], filter lists with [`Ability::sql_filter`] and ship
//! them to the browser with [`Ability::to_casl_json`]. The module half
//! (roles and permissions in the database, route authorization, extractors)
//! builds on it.
#![forbid(unsafe_code)]

pub mod engine;

pub use engine::{
    ALL, Ability, ActionMarker, CREATE, Cond, Conditions, Create, DELETE, DefaultColumns, Delete,
    FieldRef, IntoCondValue, MANAGE, Manage, READ, Read, Rule, RuleBuilder, RuleError, Rules,
    SqlColumns, Subject, UPDATE, Update, field, sql_value,
};
