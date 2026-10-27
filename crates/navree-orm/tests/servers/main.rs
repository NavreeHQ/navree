//! navree-orm against real servers: PostgreSQL with TimescaleDB
//! (`NAVREE_TEST_POSTGRES_URL`) and MySQL (`NAVREE_TEST_MYSQL_URL`). Each
//! test skips with a notice when its variable is unset, and runs in a
//! schema (PostgreSQL) or database (MySQL) of its own.
//!
//! `cargo test -p navree-orm --features postgres,mysql --test servers`

mod attributes;
mod lifecycle;
mod models;
mod relations;
mod scenarios;
mod support;

use support::{Server, run};

#[tokio::test]
async fn postgres_attribute_casts() {
    run(Server::Postgres, |db| {
        attributes::casts(db, Server::Postgres)
    })
    .await;
}

#[tokio::test]
async fn postgres_vectors() {
    run(Server::Postgres, |db| {
        attributes::vectors(db, Server::Postgres)
    })
    .await;
}

#[tokio::test]
async fn mysql_attribute_casts() {
    run(Server::MySql, |db| attributes::casts(db, Server::MySql)).await;
}

#[tokio::test]
async fn mysql_vectors() {
    run(Server::MySql, |db| attributes::vectors(db, Server::MySql)).await;
}

#[tokio::test]
async fn postgres_records_round_trip() {
    run(Server::Postgres, scenarios::records_round_trip).await;
}

#[tokio::test]
async fn postgres_queries() {
    run(Server::Postgres, |db| {
        scenarios::queries(db, Server::Postgres)
    })
    .await;
}

#[tokio::test]
async fn postgres_writes() {
    run(Server::Postgres, scenarios::writes).await;
}

#[tokio::test]
async fn postgres_pagination() {
    run(Server::Postgres, scenarios::pagination).await;
}

#[tokio::test]
async fn postgres_unit_of_work() {
    run(Server::Postgres, |db| {
        scenarios::unit_of_work(db, Server::Postgres)
    })
    .await;
}

#[tokio::test]
async fn timescaledb_time_buckets() {
    run(Server::Postgres, |db| {
        scenarios::time_buckets(db, Server::Postgres)
    })
    .await;
}

#[tokio::test]
async fn postgres_lifecycle() {
    run(Server::Postgres, |db| {
        lifecycle::scenario(db, Server::Postgres)
    })
    .await;
}

#[tokio::test]
async fn postgres_relations() {
    run(Server::Postgres, |db| {
        relations::relations(db, Server::Postgres)
    })
    .await;
}

#[tokio::test]
async fn mysql_lifecycle() {
    run(Server::MySql, |db| lifecycle::scenario(db, Server::MySql)).await;
}

#[tokio::test]
async fn mysql_relations() {
    run(Server::MySql, |db| relations::relations(db, Server::MySql)).await;
}

#[tokio::test]
async fn mysql_records_round_trip() {
    run(Server::MySql, scenarios::records_round_trip).await;
}

#[tokio::test]
async fn mysql_queries() {
    run(Server::MySql, |db| scenarios::queries(db, Server::MySql)).await;
}

#[tokio::test]
async fn mysql_writes() {
    run(Server::MySql, scenarios::writes).await;
}

#[tokio::test]
async fn mysql_pagination() {
    run(Server::MySql, scenarios::pagination).await;
}

#[tokio::test]
async fn mysql_unit_of_work() {
    run(Server::MySql, |db| {
        scenarios::unit_of_work(db, Server::MySql)
    })
    .await;
}

#[tokio::test]
async fn mysql_time_buckets() {
    run(Server::MySql, |db| {
        scenarios::time_buckets(db, Server::MySql)
    })
    .await;
}
