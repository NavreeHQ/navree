//! The models of the real-server tests and their tables per server.

#![allow(dead_code)]

use chrono::{DateTime, NaiveDate, Utc};
use navree_orm::{Decimal, Model, Ulid};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// PostgreSQL / TimescaleDB tables (statements separated by `;` and a
/// newline).
pub const POSTGRES_SCHEMA: &str = r#"
CREATE TABLE companies (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    country TEXT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    employees INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE TABLE deals (
    id UUID PRIMARY KEY,
    company_id BIGINT NULL REFERENCES companies (id),
    title TEXT NOT NULL,
    amount_cents BIGINT NOT NULL,
    stage TEXT NOT NULL,
    priority SMALLINT NOT NULL,
    meta JSONB NOT NULL,
    tags JSONB NOT NULL,
    probability DOUBLE PRECISION NOT NULL,
    value NUMERIC(14, 2) NOT NULL,
    closes_on DATE NULL,
    closed_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ NULL
);
CREATE TABLE tags (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    color TEXT NOT NULL
);
CREATE TABLE events (
    id CHAR(26) PRIMARY KEY,
    name TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    payload JSONB NULL
);
CREATE TABLE notes (
    id BIGSERIAL PRIMARY KEY,
    body TEXT NOT NULL,
    written_by TEXT NOT NULL DEFAULT ''
);
CREATE FUNCTION stamp_actor() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN NEW.written_by := COALESCE(current_setting('navree.actor', true), ''); RETURN NEW; END $$;
CREATE TRIGGER notes_actor BEFORE INSERT ON notes FOR EACH ROW EXECUTE FUNCTION stamp_actor();
CREATE TABLE readings (
    taken_at TIMESTAMPTZ NOT NULL,
    sensor TEXT NOT NULL,
    celsius DOUBLE PRECISION NOT NULL
);
SELECT create_hypertable('readings', 'taken_at', chunk_time_interval => INTERVAL '1 day')
"#;

/// MySQL tables.
pub const MYSQL_SCHEMA: &str = r#"
CREATE TABLE companies (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    country VARCHAR(16) NULL,
    active TINYINT(1) NOT NULL DEFAULT 1,
    employees INT NOT NULL DEFAULT 0,
    created_at DATETIME(6) NOT NULL,
    updated_at DATETIME(6) NOT NULL
) ENGINE = InnoDB;
CREATE TABLE deals (
    id BINARY(16) PRIMARY KEY,
    company_id BIGINT NULL,
    title VARCHAR(255) NOT NULL,
    amount_cents BIGINT NOT NULL,
    stage VARCHAR(32) NOT NULL,
    priority SMALLINT NOT NULL,
    meta JSON NOT NULL,
    tags JSON NOT NULL,
    probability DOUBLE NOT NULL,
    value DECIMAL(14, 2) NOT NULL,
    closes_on DATE NULL,
    closed_at DATETIME(6) NULL,
    created_at DATETIME(6) NOT NULL,
    updated_at DATETIME(6) NOT NULL,
    deleted_at DATETIME(6) NULL,
    FULLTEXT KEY deals_title_full_text (title),
    CONSTRAINT deals_company FOREIGN KEY (company_id) REFERENCES companies (id)
) ENGINE = InnoDB;
CREATE TABLE tags (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(64) NOT NULL UNIQUE,
    color VARCHAR(32) NOT NULL
) ENGINE = InnoDB;
CREATE TABLE events (
    id CHAR(26) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    occurred_at DATETIME(6) NOT NULL,
    payload JSON NULL
) ENGINE = InnoDB;
CREATE TABLE notes (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    body TEXT NOT NULL,
    written_by VARCHAR(64) NOT NULL DEFAULT ''
) ENGINE = InnoDB;
CREATE TABLE readings (
    taken_at DATETIME(6) NOT NULL,
    sensor VARCHAR(32) NOT NULL,
    celsius DOUBLE NOT NULL
) ENGINE = InnoDB
"#;

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(timestamps)]
pub struct Company {
    pub id: i64,
    pub name: String,
    pub country: Option<String>,
    pub active: bool,
    pub employees: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[default]
    Lead,
    Qualified,
    ClosedWon,
    ClosedLost,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Priority {
    #[default]
    Low = 1,
    High = 3,
}

impl From<Priority> for i64 {
    fn from(priority: Priority) -> i64 {
        priority as i64
    }
}

impl TryFrom<i64> for Priority {
    type Error = String;

    fn try_from(value: i64) -> Result<Self, String> {
        match value {
            1 => Ok(Priority::Low),
            3 => Ok(Priority::High),
            other => Err(format!("{other} is not a priority")),
        }
    }
}

impl Serialize for Priority {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(i64::from(*self))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DealMeta {
    pub source: String,
    pub score: u32,
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "deals", timestamps, soft_deletes)]
pub struct Deal {
    pub id: Uuid,
    pub company_id: Option<i64>,
    pub title: String,
    pub amount_cents: i64,
    #[model(cast = "string_enum")]
    pub stage: Stage,
    #[model(cast = "int_enum")]
    pub priority: Priority,
    #[model(cast = "json")]
    pub meta: DealMeta,
    #[model(cast = "json")]
    pub tags: Vec<String>,
    pub probability: f64,
    pub value: Decimal,
    pub closes_on: Option<NaiveDate>,
    pub closed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Deal {
    pub fn new(title: &str, amount_cents: i64) -> Deal {
        Deal {
            title: title.to_owned(),
            amount_cents,
            ..Deal::default()
        }
    }
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color: String,
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Event {
    pub id: Ulid,
    pub name: String,
    pub occurred_at: DateTime<Utc>,
    pub payload: Option<serde_json::Value>,
}

/// A trigger fills `written_by` with the unit of work's actor on
/// PostgreSQL (`navree.actor`), as audit triggers do.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Note {
    pub id: i64,
    pub body: String,
    pub written_by: String,
}
