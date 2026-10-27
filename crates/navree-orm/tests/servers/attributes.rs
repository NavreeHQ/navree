//! The attribute layer on real servers: every cast with the database's
//! native column types (NUMERIC / DECIMAL, BYTEA / BINARY(16), JSONB / JSON,
//! BIGINT), encryption and hashing through explicit services, conditions
//! and increments through casts, query-time casts of each database's
//! expression types, and vectors where the server has them.

use std::collections::BTreeSet;

use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use navree_crypto::{AppKey, Encrypter, Hasher, HashingOptions};
use navree_db::Db;
use navree_db::sea_orm::ConnectionTrait;
use navree_orm::attributes::{CastContext, ExtraCast};
use navree_orm::cast::{Cast, CastError, MultiColumnCast, is_password_hash};
use navree_orm::{Decimal, Model, Ulid, attrs};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::support::Server;

const POSTGRES_TABLES: &str = r#"
CREATE TABLE attribute_ledgers (
    id BIGSERIAL PRIMARY KEY,
    balance NUMERIC(12, 2) NOT NULL,
    opened_at TEXT NOT NULL,
    local_time TEXT NOT NULL,
    synced_at BIGINT NOT NULL,
    pinged_at BIGINT NULL,
    reference BYTEA NOT NULL,
    batch BYTEA NOT NULL,
    website TEXT NULL,
    statuses JSONB NOT NULL,
    total_cents BIGINT NOT NULL,
    total_currency TEXT NOT NULL,
    visits BIGINT NOT NULL,
    secret TEXT NOT NULL,
    history TEXT NOT NULL,
    password TEXT NOT NULL
)
"#;

const MYSQL_TABLES: &str = r#"
CREATE TABLE attribute_ledgers (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    balance DECIMAL(12, 2) NOT NULL,
    opened_at VARCHAR(32) NOT NULL,
    local_time VARCHAR(40) NOT NULL,
    synced_at BIGINT NOT NULL,
    pinged_at BIGINT NULL,
    reference BINARY(16) NOT NULL,
    batch BINARY(16) NOT NULL,
    website VARCHAR(255) NULL,
    statuses JSON NOT NULL,
    total_cents BIGINT NOT NULL,
    total_currency VARCHAR(3) NOT NULL,
    visits BIGINT NOT NULL,
    secret TEXT NOT NULL,
    history TEXT NOT NULL,
    password VARCHAR(255) NOT NULL
)
"#;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Draft,
    Open,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Money {
    pub cents: i64,
    pub currency: String,
}

pub struct MoneyColumns;

impl MultiColumnCast<Money> for MoneyColumns {
    type Stored = (i64, String);

    fn get((cents, currency): (i64, String)) -> Result<Money, CastError> {
        Ok(Money { cents, currency })
    }

    fn set(money: &Money) -> Result<(i64, String), CastError> {
        Ok((money.cents, money.currency.clone()))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Counter(pub i64);

pub struct Visits;

impl Cast<Counter> for Visits {
    type Stored = i64;

    fn get(stored: i64) -> Result<Counter, CastError> {
        Ok(Counter(stored))
    }

    fn set(counter: &Counter) -> Result<i64, CastError> {
        Ok(counter.0)
    }

    fn increment(value: &Counter, amount: &Counter) -> Result<Counter, CastError> {
        Ok(Counter(value.0 + amount.0))
    }

    fn decrement(value: &Counter, amount: &Counter) -> Result<Counter, CastError> {
        Ok(Counter(value.0 - amount.0))
    }
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "attribute_ledgers", unguarded, hidden(password))]
pub struct Ledger {
    pub id: i64,
    #[model(cast = "decimal(2)")]
    pub balance: Decimal,
    #[model(cast = "datetime_format(\"%d/%m/%Y %H:%M\")")]
    pub opened_at: DateTime<Utc>,
    #[model(cast = "datetime_format(\"%Y-%m-%dT%H:%M:%S%:z\")")]
    pub local_time: DateTime<FixedOffset>,
    #[model(cast = "unix_timestamp")]
    pub synced_at: DateTime<Utc>,
    #[model(cast = "unix_timestamp_ms")]
    pub pinged_at: Option<DateTime<Utc>>,
    #[model(cast = "binary_uuid")]
    pub reference: Uuid,
    #[model(cast = "binary_ulid")]
    pub batch: Ulid,
    pub website: Option<url::Url>,
    #[model(cast = "json")]
    pub statuses: BTreeSet<Status>,
    #[model(cast = MoneyColumns, columns(total_cents: i64, total_currency: String))]
    pub total: Money,
    #[model(cast = Visits)]
    pub visits: Counter,
    #[model(cast = "encrypted")]
    pub secret: String,
    #[model(cast = "encrypted_json")]
    pub history: Vec<String>,
    #[model(cast = "hashed")]
    pub password: String,
}

fn services(key: &AppKey) -> CastContext {
    CastContext::new()
        .with_encrypter(Encrypter::new(key.clone()))
        .with_hasher(
            Hasher::new(&HashingOptions {
                memory_kib: 1024,
                iterations: 1,
                parallelism: 1,
                max_concurrent_hashes: Some(2),
            })
            .unwrap(),
        )
}

fn ledger() -> Ledger {
    Ledger {
        balance: "1234.565".parse().unwrap(),
        opened_at: Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 59).unwrap(),
        local_time: FixedOffset::west_opt(4 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 27, 4, 30, 0)
            .unwrap(),
        synced_at: Utc.with_ymd_and_hms(2026, 10, 27, 9, 0, 0).unwrap(),
        pinged_at: Some(
            Utc.with_ymd_and_hms(2026, 10, 27, 9, 0, 1).unwrap()
                + chrono::Duration::milliseconds(250),
        ),
        reference: Uuid::now_v7(),
        batch: Ulid::new(),
        website: Some(url::Url::parse("https://navree.com/").unwrap()),
        statuses: [Status::Open, Status::Draft].into(),
        total: Money {
            cents: 999,
            currency: "EUR".into(),
        },
        visits: Counter(1),
        secret: "launch codes".into(),
        history: vec!["a".into(), "b".into()],
        password: "correct horse".into(),
        ..Ledger::default()
    }
}

/// Every cast against `server`'s native column types.
pub async fn casts(db: Db, server: Server) {
    let tables = match server {
        Server::Postgres => POSTGRES_TABLES,
        Server::MySql => MYSQL_TABLES,
    };
    db.conn().execute_unprepared(tables.trim()).await.unwrap();
    let key = AppKey::generate();
    services(&key)
        .scope(async {
            let created = Ledger::create(&db, ledger()).await.unwrap();
            assert_eq!(created.balance.to_string(), "1234.57");
            assert!(is_password_hash(&created.password));
            let stored = Ledger::find_or_fail(&db, created.id).await.unwrap();
            assert_eq!(*stored, *created, "{server:?}: the record holds what is stored");
            assert!(stored.is_clean());

            let row = navree_orm::table("attribute_ledgers").first(&db).await.unwrap().unwrap();
            assert_eq!(row.get::<Decimal>("balance").unwrap().to_string(), "1234.57");
            assert_eq!(row.get::<String>("opened_at").unwrap(), "27/10/2026 08:30");
            assert_eq!(row.get::<String>("local_time").unwrap(), "2026-10-27T04:30:00-04:00");
            assert_eq!(row.get::<Vec<u8>>("reference").unwrap(), created.reference.as_bytes());
            assert!(row.get::<String>("secret").unwrap().starts_with("v1."));
            assert_eq!(row.get::<serde_json::Value>("statuses").unwrap(), json!(["draft", "open"]));

            // Conditions encode through the casts.
            let found = Ledger::query()
                .where_(Ledger::REFERENCE.eq(created.reference))
                .where_(Ledger::BATCH.eq(created.batch))
                .where_(Ledger::BALANCE.eq("1234.574".parse::<Decimal>().unwrap()))
                .where_(Ledger::TOTAL_CURRENCY.eq("EUR"))
                .count(&db)
                .await
                .unwrap();
            assert_eq!(found, 1, "{server:?}");

            // Increments through a cast, atomic in the database.
            let mut record = stored;
            assert!(record.increment_cast(&db, Ledger::VISITS, Counter(4)).await.unwrap());
            assert_eq!(record.visits, Counter(5));
            assert_eq!(Ledger::find_or_fail(&db, record.id).await.unwrap().visits, Counter(5));

            // Mass assignment of plain values.
            record
                .fill(attrs! { "secret" => "new", "total_cents" => 5, "password" => "other", "statuses" => ["open"] })
                .unwrap();
            record.save(&db).await.unwrap();
            let reread = Ledger::find_or_fail(&db, record.id).await.unwrap();
            assert_eq!((reread.secret.as_str(), reread.total.cents), ("new", 5));
            assert_eq!(reread.statuses, [Status::Open].into());
            assert!(is_password_hash(&reread.password) && reread.password != created.password);

            // Query-time casts of each database's expression types.
            let typed = Ledger::query()
                .select_raw("total_cents > 1 AS positive", [] as [i64; 0])
                .select_raw("balance / 3 AS third", [] as [i64; 0])
                .with_casts([("positive", ExtraCast::Bool), ("third", ExtraCast::Decimal(Some(2)))])
                .first(&db)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(typed.extra::<bool>("positive").unwrap(), Some(true), "{server:?}");
            assert_eq!(
                typed.extra::<Decimal>("third").unwrap().map(|d| d.to_string()).as_deref(),
                Some("411.52"),
                "{server:?}"
            );
            let json = typed.to_value().unwrap();
            assert_eq!(json["positive"], true);
            assert!(json.get("password").is_none());
            assert_eq!(json["synced_at"], "2026-10-27T09:00:00.000000Z");
        })
        .await;

    // Rotating the key: the old payloads still decrypt, and the next save
    // re-encrypts them (the stored hash is kept as it is).
    let rotated = CastContext::new()
        .with_encrypter(Encrypter::new(AppKey::generate()).with_previous_keys([key]));
    rotated
        .scope(async {
            let mut record = Ledger::query().first(&db).await.unwrap().unwrap();
            assert_eq!(record.secret, "new");
            assert!(record.is_dirty_on(["secret", "history"]));
            record.save(&db).await.unwrap();
            assert!(
                Ledger::query()
                    .first(&db)
                    .await
                    .unwrap()
                    .unwrap()
                    .is_clean()
            );
        })
        .await;
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "attribute_embeddings", unguarded)]
pub struct Embedding {
    pub id: i64,
    #[model(cast = "vector")]
    pub vector: Vec<f32>,
    #[model(cast = "vector")]
    pub optional: Option<Vec<f32>>,
}

/// Vectors in the server's vector type: pgvector on PostgreSQL, `VECTOR`
/// on MySQL 9 and later (skipped with a notice where there is none).
pub async fn vectors(db: Db, server: Server) {
    let statements: &[&str] = match server {
        Server::Postgres => &[
            "CREATE EXTENSION IF NOT EXISTS vector",
            "CREATE TABLE attribute_embeddings (id BIGSERIAL PRIMARY KEY, vector vector(3) NOT NULL, optional vector(3) NULL)",
        ],
        Server::MySql => &[
            "CREATE TABLE attribute_embeddings (id BIGINT AUTO_INCREMENT PRIMARY KEY, vector VECTOR(3) NOT NULL, optional VECTOR(3) NULL)",
        ],
    };
    for statement in statements {
        if let Err(error) = db.conn().execute_unprepared(statement).await {
            eprintln!("skipped: {server:?} has no vector type ({error})");
            return;
        }
    }
    let mut created = Embedding::create(
        &db,
        Embedding {
            vector: vec![0.5, -1.0, 2.25],
            ..Embedding::default()
        },
    )
    .await
    .unwrap();
    let stored = Embedding::find_or_fail(&db, created.id).await.unwrap();
    assert_eq!(stored.vector, [0.5, -1.0, 2.25], "{server:?}");
    assert_eq!(stored.optional, None);
    assert!(stored.is_clean());

    created.optional = Some(vec![1.0, 0.0, -0.125]);
    created.save(&db).await.unwrap();
    let stored = Embedding::find_or_fail(&db, created.id).await.unwrap();
    assert_eq!(
        stored.optional.as_deref(),
        Some(&[1.0, 0.0, -0.125][..]),
        "{server:?}"
    );
}
