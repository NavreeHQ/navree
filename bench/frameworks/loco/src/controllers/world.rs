use loco_rs::prelude::*;
use serde::Deserialize;

use crate::{models::world, views::world::WorldResponse};

/// `/queries` runs between 1 and this many queries.
const MAX_QUERIES: usize = 500;

#[derive(Debug, Deserialize)]
pub struct QueriesParams {
    queries: Option<String>,
}

impl QueriesParams {
    /// `queries`, clamped to 1..=500; missing or not a number means 1.
    fn count(&self) -> usize {
        self.queries
            .as_deref()
            .and_then(|value| value.parse::<i64>().ok())
            .map_or(1, |n| usize::try_from(n.clamp(1, MAX_QUERIES as i64)).unwrap_or(1))
    }
}

#[debug_handler]
async fn db(State(ctx): State<AppContext>) -> Result<Response> {
    let row = world::Model::find_random(&ctx.db).await?;
    format::json(WorldResponse::from(row))
}

/// N separate single-row queries, one after another.
#[debug_handler]
async fn queries(State(ctx): State<AppContext>, Query(params): Query<QueriesParams>) -> Result<Response> {
    let count = params.count();
    let mut rows = Vec::with_capacity(count);
    for _ in 0..count {
        rows.push(WorldResponse::from(world::Model::find_random(&ctx.db).await?));
    }
    format::json(rows)
}

pub fn routes() -> Routes {
    Routes::new().add("/db", get(db)).add("/queries", get(queries))
}
