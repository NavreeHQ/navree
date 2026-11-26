//! `sqlx` `Type`, `Encode` and `Decode` implementations for PostgreSQL,
//! MySQL and SQLite, producing the exact SQL type names and wire bytes
//! Navree's databases store.

#[cfg(feature = "sqlx-mysql")]
pub(crate) mod mysql;
#[cfg(feature = "sqlx-postgres")]
pub(crate) mod postgres;
#[cfg(feature = "sqlx-sqlite")]
pub(crate) mod sqlite;
