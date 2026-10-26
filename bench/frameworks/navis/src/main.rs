//! `navis-bench`: the benchmark API (bench/SPEC.md) as a Navis application.
//! Run it from this directory (it reads `config/app.toml`):
//!
//! ```text
//! DATABASE_URL=postgres://... JWT_SECRET=... cargo run --release -- serve
//! ```
#![forbid(unsafe_code)]

mod app;
mod deals;
mod world;

use navis::prelude::*;

fn main() -> Result<(), BoxError> {
    app::app(sources()).run()
}

/// Navis's conventional sources (`.env`, `config/app.toml`, `APP_*`
/// variables) plus the variables the benchmark harness sets.
fn sources() -> ConfigSources {
    const MAPPED: [(&str, &str); 3] = [
        ("DATABASE_URL", "database.url"),
        ("JWT_SECRET", "auth.jwt.secret"),
        ("JWT_ISSUER", "auth.jwt.issuer"),
    ];
    MAPPED
        .into_iter()
        .filter_map(|(var, key)| std::env::var(var).ok().map(|value| (key, value)))
        .fold(ConfigSources::defaults(), |sources, (key, value)| {
            sources.set(key, value)
        })
}
