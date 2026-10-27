//! The MongoDB server of the integration tests: `NAVREE_TEST_MONGODB_URL`
//! (a replica set, so transactions work: e.g.
//! `mongodb://127.0.0.1:57017/navree_test?replicaSet=rs0`). Each test gets a
//! database of its own, dropped when it ends. Without the variable, the
//! tests that need a server print a notice and pass, unless
//! `NAVREE_TEST_REQUIRE_SERVICES=1` makes them fail.

#![allow(dead_code)]

use navree_mongo::{MongoDb, MongoOptions};

/// An address nothing listens on: commands fail, connecting does not.
pub const UNREACHABLE: &str = "mongodb://127.0.0.1:1/navree_unreachable";

/// The server's URL, or `None` (after a notice) when none is configured.
pub fn server_url(test: &str) -> Option<String> {
    let url = std::env::var("NAVREE_TEST_MONGODB_URL")
        .ok()
        .filter(|url| !url.trim().is_empty());
    if url.is_none() {
        if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") {
            panic!("{test}: NAVREE_TEST_MONGODB_URL is not set and NAVREE_TEST_REQUIRE_SERVICES=1");
        }
        eprintln!("skipped {test}: set NAVREE_TEST_MONGODB_URL to run it against MongoDB");
    }
    url
}

/// A database of the test server for one test.
pub struct TestMongo {
    pub url: String,
    pub name: String,
    pub db: MongoDb,
}

impl TestMongo {
    /// A fresh database, or `None` when no server is configured.
    pub async fn connect(test: &str) -> Option<TestMongo> {
        let url = server_url(test)?;
        let name = format!(
            "navree_test_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..16]
        );
        let db = MongoDb::connect(&TestMongo::options_for(&url, &name))
            .await
            .expect("NAVREE_TEST_MONGODB_URL is a MongoDB URL");
        db.ping()
            .await
            .expect("the server at NAVREE_TEST_MONGODB_URL answers");
        Some(TestMongo { url, name, db })
    }

    fn options_for(url: &str, name: &str) -> MongoOptions {
        MongoOptions {
            url: url.to_owned(),
            database: Some(name.to_owned()),
            connect_timeout_ms: 5_000,
            server_selection_timeout_ms: 5_000,
            ..MongoOptions::default()
        }
    }

    /// Options for an app on this test's database.
    pub fn options(&self) -> MongoOptions {
        TestMongo::options_for(&self.url, &self.name)
    }

    /// Drops the test's database.
    pub async fn cleanup(self) {
        self.db
            .database()
            .drop()
            .await
            .expect("the test database drops");
    }
}
