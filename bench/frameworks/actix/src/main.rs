//! Actix Web + sqlx implementation of the Navis benchmark API (bench/SPEC.md).

mod auth;
mod deals;
mod error;
mod world;

use std::env;

use actix_web::{App, HttpServer, Responder, get, web};
use serde::Serialize;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::auth::JwtConfig;

/// One pool shared by every worker: at most 64 connections (SPEC §4).
const MAX_DB_CONNECTIONS: u32 = 64;

pub struct AppState {
    pub db: PgPool,
    pub jwt: JwtConfig,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    let jwt_issuer = env::var("JWT_ISSUER").unwrap_or_else(|_| "navis".to_owned());

    let db = PgPoolOptions::new()
        .max_connections(MAX_DB_CONNECTIONS)
        .connect(&database_url)
        .await
        .expect("failed to connect to PostgreSQL");
    let state = web::Data::new(AppState {
        db,
        jwt: JwtConfig::new(&jwt_secret, &jwt_issuer),
    });

    // Default worker count: one per CPU.
    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .app_data(web::JsonConfig::default().error_handler(error::json_error))
            .app_data(web::QueryConfig::default().error_handler(error::query_error))
            .service(json)
            .service(world::single)
            .service(world::queries)
            .service(deals::list)
            .service(deals::create)
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}

#[derive(Serialize)]
struct Message {
    message: &'static str,
}

#[get("/json")]
async fn json() -> impl Responder {
    web::Json(Message {
        message: "Hello, World!",
    })
}
