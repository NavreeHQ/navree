//! The attribute layer's casts in SQL: conditions bind each cast's stored
//! form, and values whose stored form differs between databases (vectors)
//! take each database's form.

use std::borrow::Cow;

use chrono::{DateTime, TimeZone, Utc};
use navree_orm::attributes::CastContext;
use navree_orm::cast::{CastError, InboundCast};
use navree_orm::ir::{Expr as IrExpr, InsertIr};
use navree_orm::sql::compile_insert;
use navree_orm::{Decimal, Dialect, Model, Ulid};
use serde::Serialize;
use uuid::Uuid;

use super::{DIALECTS, per_dialect, query};

/// Codes stored upper-cased.
pub struct Upper;

impl InboundCast<String> for Upper {
    fn set(value: &String) -> Result<String, CastError> {
        Ok(value.to_uppercase())
    }
}

#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "ledgers")]
struct Ledger {
    id: i64,
    #[model(cast = "decimal(2)")]
    balance: Decimal,
    #[model(cast = "datetime_format(\"%d/%m/%Y\")")]
    opened_on: DateTime<Utc>,
    #[model(cast = "unix_timestamp")]
    synced_at: DateTime<Utc>,
    #[model(cast = "binary_uuid")]
    reference: Uuid,
    #[model(cast = "binary_ulid")]
    batch: Ulid,
    #[model(inbound = Upper)]
    code: String,
    #[model(cast = "vector")]
    embedding: Vec<f32>,
}

fn at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 0).unwrap()
}

#[test]
fn conditions_bind_the_stored_form_of_cast_fields() {
    let reference = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
    let batch = Ulid::from_parts(1_700_000_000_000, 42);
    insta::assert_snapshot!(query(
        Ledger::query()
            .where_(Ledger::BALANCE.eq("12.345".parse::<Decimal>().unwrap()))
            .where_(Ledger::OPENED_ON.gte(at()))
            .where_(Ledger::SYNCED_AT.lt(at()))
            .where_(Ledger::REFERENCE.eq(reference))
            .where_(Ledger::BATCH.eq(batch))
            .where_(Ledger::CODE.eq("abc"))
    ));
}

/// The INSERT of `ledger` for `dialect`, encoded as it is when a record
/// is saved on that database.
fn insert(ledger: &Ledger, dialect: Dialect) -> navree_orm::Result<navree_orm::Statement> {
    let values = CastContext::new()
        .with_dialect(dialect)
        .sync_scope(|| ledger.values())?;
    let (columns, row): (Vec<Cow<'static, str>>, Vec<IrExpr>) = values
        .into_iter()
        .filter(|(column, _)| *column == "embedding")
        .map(|(column, value)| (Cow::Borrowed(column), IrExpr::Value(value)))
        .unzip();
    compile_insert(
        &InsertIr {
            table: Cow::Borrowed("ledgers"),
            columns,
            rows: vec![row],
            returning: Vec::new(),
        },
        dialect,
    )
}

#[test]
fn vectors_take_each_databases_form() {
    let ledger = Ledger {
        embedding: vec![0.5, -1.0, 2.25],
        ..Ledger::default()
    };
    let statements = per_dialect(|dialect| insert(&ledger, dialect));
    assert_eq!(DIALECTS.len(), 3);
    // PostgreSQL binds pgvector's value with the `postgres` feature (which
    // links pgvector), and its text form without it.
    #[cfg(feature = "postgres")]
    insta::assert_snapshot!("vectors_with_pgvector", statements);
    #[cfg(not(feature = "postgres"))]
    insta::assert_snapshot!("vectors_without_pgvector", statements);
}
