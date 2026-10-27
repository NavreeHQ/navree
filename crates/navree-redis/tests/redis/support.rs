//! The Redis server of the integration tests: `NAVREE_TEST_REDIS_URL`
//! (e.g. `redis://127.0.0.1:56379` with
//! `docker run --rm -d -p 56379:6379 redis:8-alpine`). Without it, the
//! tests that need a server print a notice and pass, unless
//! `NAVREE_TEST_REQUIRE_SERVICES=1` makes them fail.

#![allow(dead_code)]

use navree_redis::{Redis, RedisOptions, redis};

/// An address nothing listens on: commands fail, boots do not.
pub const UNREACHABLE: &str = "redis://127.0.0.1:1";

/// Signs nobody in (the realtime module's auth route needs an
/// authenticator to boot; the tests sign subscriptions themselves).
pub struct NoAuthenticator;

impl navree_http::Authenticator for NoAuthenticator {
    fn authenticate<'a>(
        &'a self,
        _parts: &'a mut axum::http::request::Parts,
    ) -> navree_kernel::BoxFuture<
        'a,
        Result<Option<navree_contracts::Principal>, navree_http::AppError>,
    > {
        Box::pin(async { Ok(None) })
    }
}

/// The test server, with a prefix of its own so tests never see each
/// other's keys.
pub struct TestRedis {
    pub url: String,
    pub prefix: String,
    pub redis: Redis,
}

impl TestRedis {
    /// The server, or `None` (after a notice) when none is configured.
    pub async fn connect(test: &str) -> Option<TestRedis> {
        let Some(url) = std::env::var("NAVREE_TEST_REDIS_URL")
            .ok()
            .filter(|url| !url.trim().is_empty())
        else {
            if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") {
                panic!(
                    "{test}: NAVREE_TEST_REDIS_URL is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
                );
            }
            eprintln!("skipped {test}: set NAVREE_TEST_REDIS_URL to run it against Redis");
            return None;
        };
        let prefix = format!("navree-test:{}:", uuid::Uuid::new_v4().simple());
        let options = RedisOptions {
            url: url.clone(),
            prefix: prefix.clone(),
            ..RedisOptions::default()
        };
        let redis = Redis::new(&options).expect("NAVREE_TEST_REDIS_URL is a Redis URL");
        redis
            .connection()
            .ping()
            .await
            .expect("the server at NAVREE_TEST_REDIS_URL answers");
        Some(TestRedis { url, prefix, redis })
    }

    /// Options for an app on this server and prefix.
    pub fn options(&self) -> RedisOptions {
        RedisOptions {
            url: self.url.clone(),
            prefix: self.prefix.clone(),
            ..RedisOptions::default()
        }
    }

    /// Every key under the test's prefix.
    pub async fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = redis::cmd("KEYS")
            .arg(format!("{}*", self.prefix))
            .query_async(&mut self.redis.manager())
            .await
            .unwrap();
        keys.sort();
        keys
    }

    /// Deletes every key under the test's prefix.
    pub async fn cleanup(&self) {
        let keys = self.keys().await;
        if !keys.is_empty() {
            let _: i64 = redis::cmd("DEL")
                .arg(&keys)
                .query_async(&mut self.redis.manager())
                .await
                .unwrap();
        }
    }
}
