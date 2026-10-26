//! `GET /json`, `GET /db` and `GET /queries?queries=N`.

use navis::db::Db;
use navis::prelude::*;
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};

use super::entity::{self as world, Entity as World};

/// The most rows `/queries` reads.
const MAX_QUERIES: usize = 500;

/// Public, unversioned routes at the root (`!` makes the prefix absolute:
/// no `/api` prefix).
#[injectable]
pub struct WorldController {
    db: Arc<Db>,
}

#[controller("!/")]
impl WorldController {
    /// `{"message": "Hello, World!"}`.
    #[get("/json")]
    #[public]
    async fn json(&self) -> Json<Message> {
        Json(Message {
            message: "Hello, World!",
        })
    }

    /// One random row.
    #[get("/db")]
    #[public]
    async fn db(&self) -> Result<Json<world::Model>> {
        Ok(Json(random_row(&self.db).await?))
    }

    /// `queries` random rows (1..=500; 1 when missing or not a number),
    /// one query each.
    #[get("/queries")]
    #[public]
    async fn queries(
        &self,
        Query(params): Query<QueriesParams>,
    ) -> Result<Json<Vec<world::Model>>> {
        let count = params.count();
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            rows.push(random_row(&self.db).await?);
        }
        Ok(Json(rows))
    }
}

/// The `/json` body.
#[derive(Serialize)]
pub struct Message {
    message: &'static str,
}

/// `?queries=N`.
#[derive(Deserialize)]
pub struct QueriesParams {
    queries: Option<String>,
}

impl QueriesParams {
    /// N clamped to 1..=500; 1 when missing or not an integer.
    fn count(&self) -> usize {
        self.queries
            .as_deref()
            .and_then(|n| n.trim().parse::<i64>().ok())
            .map_or(1, |n| n.clamp(1, MAX_QUERIES as i64) as usize)
    }
}

/// The row with a uniformly random id in 1..=10000, read from the pool
/// (no unit-of-work transaction for a single read).
async fn random_row(db: &Db) -> Result<world::Model> {
    let id: i32 = rand::random_range(1..=10_000);
    World::find_by_id(id)
        .one(db.conn())
        .await?
        .ok_or_else(AppError::not_found)
}
