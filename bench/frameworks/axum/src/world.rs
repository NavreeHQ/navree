use axum::{
    Json,
    extract::{Query, State},
};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};

use crate::{AppState, error::AppError};

#[derive(Serialize, FromRow)]
pub struct World {
    id: i32,
    #[serde(rename = "randomNumber")]
    #[sqlx(rename = "randomnumber")]
    random_number: i32,
}

async fn random_world(db: &PgPool) -> Result<World, sqlx::Error> {
    let id: i32 = rand::rng().random_range(1..=10_000);
    sqlx::query_as("SELECT id, randomnumber FROM world WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
}

pub async fn single(State(state): State<AppState>) -> Result<Json<World>, AppError> {
    Ok(Json(random_world(&state.db).await?))
}

#[derive(Deserialize)]
pub struct QueriesParams {
    queries: Option<String>,
}

pub async fn queries(
    State(state): State<AppState>,
    Query(params): Query<QueriesParams>,
) -> Result<Json<Vec<World>>, AppError> {
    let count = params
        .queries
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(1)
        .clamp(1, 500) as usize;

    let mut worlds = Vec::with_capacity(count);
    for _ in 0..count {
        worlds.push(random_world(&state.db).await?);
    }
    Ok(Json(worlds))
}
