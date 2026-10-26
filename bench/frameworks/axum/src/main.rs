//! Axum + sqlx implementation of the Navree benchmark API (bench/SPEC.md).

mod auth;
mod deals;
mod error;
mod extract;
mod world;

use std::{env, sync::Arc};

use axum::{Json, Router, routing::get};
use serde::Serialize;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::auth::JwtConfig;

/// One pool for the whole process: at most 64 connections (SPEC §4).
const MAX_DB_CONNECTIONS: u32 = 64;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub jwt: Arc<JwtConfig>,
}

#[tokio::main]
async fn main() {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    let jwt_issuer = env::var("JWT_ISSUER").unwrap_or_else(|_| "navree".to_owned());

    let db = PgPoolOptions::new()
        .max_connections(MAX_DB_CONNECTIONS)
        .connect(&database_url)
        .await
        .expect("failed to connect to PostgreSQL");
    let state = AppState {
        db,
        jwt: Arc::new(JwtConfig::new(&jwt_secret, &jwt_issuer)),
    };

    let app = Router::new()
        .route("/json", get(json))
        .route("/db", get(world::single))
        .route("/queries", get(world::queries))
        .route("/api/deals", get(deals::list).post(deals::create))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind 0.0.0.0:8080");
    axum::serve(listener, app).await.expect("server error");
}

#[derive(Serialize)]
struct Message {
    message: &'static str,
}

async fn json() -> Json<Message> {
    Json(Message {
        message: "Hello, World!",
    })
}
