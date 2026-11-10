//! The `database` health check on MySQL: up while the server answers, down
//! (with a reason that names no secret) when the pool cannot reach its
//! database, and up again once it can.

use navree_contracts::{HealthCheck, HealthStatus};
use navree_db::{DatabaseModule, Db, Executor};
use navree_kernel::Navree;

use crate::support::{Scratch, string};

#[tokio::test]
async fn the_database_health_check_follows_the_server() {
    let Some(scratch) = Scratch::new("the_database_health_check_follows_the_server").await else {
        return;
    };
    let mut options = scratch.options();
    options.max_connections = 1;
    options.connect_timeout_secs = 2;
    let app = Navree::app()
        .module(DatabaseModule::for_root(options))
        .boot()
        .await
        .expect("boots");
    let checks = app.ctx().get_all::<dyn HealthCheck>().await.unwrap();
    let database = checks
        .iter()
        .find(|check| check.name() == "database")
        .expect("DatabaseModule registers the database check");
    assert_eq!(database.check().await, HealthStatus::Up);

    // The server ends the pool's only connection and the database goes:
    // the pool's next connection cannot open.
    let db = app.ctx().get::<Db>().await.unwrap();
    let connection = string(db.pool(), "SELECT CAST(CONNECTION_ID() AS CHAR)").await;
    let admin = scratch.admin().await;
    admin
        .execute_script(&format!("KILL {connection}"))
        .await
        .expect("kills the pool's connection");
    admin
        .execute_script(&format!("DROP DATABASE `{}`", scratch.name))
        .await
        .expect("drops the database");
    assert_eq!(
        database.check().await,
        HealthStatus::Down("the database did not answer a ping".to_owned())
    );

    admin
        .execute_script(&format!(
            "CREATE DATABASE `{}` CHARACTER SET utf8mb4",
            scratch.name
        ))
        .await
        .expect("creates the database again");
    assert_eq!(
        database.check().await,
        HealthStatus::Up,
        "the pool reconnects"
    );
    admin.close().await;
    app.shutdown().await.unwrap();
}
