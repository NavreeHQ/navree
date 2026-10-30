//! A small blog built the way a Navree app is: only the `navree` facade and its
//! macros (plus `serde` and `utoipa`, whose derives name
//! their own crates). `fullstack_blog.rs` drives it over HTTP.
//!
//! ```text
//! blog/
//! ├── mod.rs           # app(): the root module list, configured once
//! ├── abilities.rs     # who may do what (ability rules)
//! └── posts/           # PostsModule: entity, migration, DTOs, service, controller
//! ```

pub mod abilities;
pub mod posts;

use navree::prelude::*;
use serde_json::Value;

/// The blog's root: every module the app uses, each configured once, as in
/// a generated `src/app.rs`.
pub fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        // Platform.
        .module(ConfigModule::for_root(sources))
        .module(CryptoModule::from_config())
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(ValidationModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(QueueModule::from_config())
        // Identity, access and the audit trail.
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities: abilities::define,
            super_admin_role: Some("admin".to_owned()),
        }))
        .module(AuditModule::from_config())
        // OpenAPI at /api/openapi.json.
        .module(ApiModule::from_config())
        // Features.
        .module(posts::PostsModule)
}

/// Hermetic settings for one test: the database at `database_url`, an
/// application key, mail kept in memory and fast password hashing.
pub fn sources(database_url: String) -> ConfigSources {
    ConfigSources::new()
        .set("app.key", navree::crypto::generate_key())
        .set("database.url", database_url)
        .set("database.migrate_on_boot", true)
        .set("http.per_ip_rate_limit", Value::Null)
        .set("mail.transport", "array")
        .set("mail.from", "Blog <blog@example.com>")
        .set("hashing.memory_kib", 1024)
        .set("hashing.iterations", 1)
        .set("auth.registration", false)
        .set("api.title", "Blog API")
}
