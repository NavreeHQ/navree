//! The configuration reference: every section of the framework, written from
//! the sections themselves, and the rules every section follows.
//!
//! The page `apps/website/content/docs/configuration/reference.mdx` is
//! generated. Rewrite it with
//! `NAVREE_CONFIG_RECORD=1 cargo test -p navree --features social,oauth,flags,graphql,grpc,documents,media,billing,search,mcp,ai,redis,micro,i18n,mongo --test config_reference`.
//! The page is compared (and the sections are counted) only when every
//! module feature is on; with fewer, the rules below still run on the
//! sections that are there.

use std::path::Path;

use serde_json::Value;

use navree::config::{
    ConfigRegistry, ConfigSection, FRAMEWORK_SECTIONS, SectionRegistration, reference_markdown,
};

/// Whether the build has every module that has a section.
const COMPLETE: bool = cfg!(all(
    feature = "ai",
    feature = "api",
    feature = "audit",
    feature = "auth",
    feature = "billing",
    feature = "cache",
    feature = "crypto",
    feature = "db",
    feature = "directory",
    feature = "documents",
    feature = "domains",
    feature = "flags",
    feature = "fleet",
    feature = "graphql",
    feature = "grpc",
    feature = "helm",
    feature = "http",
    feature = "http-client",
    feature = "i18n",
    feature = "integrations",
    feature = "lighthouse",
    feature = "log",
    feature = "logbook",
    feature = "mail",
    feature = "mcp",
    feature = "media",
    feature = "micro",
    feature = "mongo",
    feature = "notify",
    feature = "oauth",
    feature = "observe",
    feature = "orm",
    feature = "queue",
    feature = "realtime",
    feature = "redis",
    feature = "schema",
    feature = "search",
    feature = "session",
    feature = "settings",
    feature = "sheets",
    feature = "social",
    feature = "sso",
    feature = "storage",
    feature = "time",
    feature = "ui",
    feature = "validation",
    feature = "view",
    feature = "webhooks",
    feature = "workflows",
    feature = "workspace"
));

/// The registration of every section the build has.
fn sections() -> Vec<SectionRegistration> {
    let mut sections: Vec<SectionRegistration> = Vec::new();
    #[cfg(feature = "ai")]
    sections.push(SectionRegistration::of::<navree::ai::AiOptions>());
    #[cfg(feature = "api")]
    sections.push(SectionRegistration::of::<navree::api::ApiOptions>());
    sections.push(SectionRegistration::of::<navree::config::AppOptions>());
    #[cfg(feature = "audit")]
    sections.push(SectionRegistration::of::<navree::audit::AuditOptions>());
    #[cfg(feature = "auth")]
    sections.push(SectionRegistration::of::<navree::auth::AuthOptions>());
    #[cfg(feature = "billing")]
    sections.push(SectionRegistration::of::<navree::billing::BillingOptions>());
    #[cfg(feature = "cache")]
    sections.push(SectionRegistration::of::<navree::cache::CacheOptions>());
    #[cfg(feature = "db")]
    sections.push(SectionRegistration::of::<navree::db::DbOptions>());
    #[cfg(feature = "mongo")]
    sections.push(SectionRegistration::of::<navree::mongo::MongoOptions>());
    #[cfg(feature = "directory")]
    sections.push(SectionRegistration::of::<navree::directory::DirectoryOptions>());
    #[cfg(feature = "documents")]
    sections.push(SectionRegistration::of::<navree::documents::DocumentsOptions>());
    #[cfg(feature = "domains")]
    sections.push(SectionRegistration::of::<navree::domains::DomainsOptions>());
    #[cfg(feature = "flags")]
    sections.push(SectionRegistration::of::<navree::flags::FlagsOptions>());
    #[cfg(feature = "fleet")]
    sections.push(SectionRegistration::of::<navree::fleet::FleetOptions>());
    #[cfg(feature = "graphql")]
    sections.push(SectionRegistration::of::<navree::graphql::GraphqlOptions>());
    #[cfg(feature = "grpc")]
    sections.push(SectionRegistration::of::<navree::grpc::GrpcOptions>());
    #[cfg(feature = "crypto")]
    sections.push(SectionRegistration::of::<navree::crypto::HashingOptions>());
    #[cfg(feature = "helm")]
    sections.push(SectionRegistration::of::<navree::helm::HelmOptions>());
    #[cfg(feature = "http")]
    sections.push(SectionRegistration::of::<navree::http::HttpOptions>());
    #[cfg(feature = "http-client")]
    sections.push(SectionRegistration::of::<
        navree::http_client::HttpClientOptions,
    >());
    #[cfg(feature = "i18n")]
    sections.push(SectionRegistration::of::<navree::i18n::I18nOptions>());
    #[cfg(feature = "integrations")]
    sections.push(SectionRegistration::of::<
        navree::integrations::IntegrationsOptions,
    >());
    #[cfg(feature = "lighthouse")]
    sections.push(SectionRegistration::of::<
        navree::lighthouse::LighthouseOptions,
    >());
    #[cfg(feature = "log")]
    sections.push(SectionRegistration::of::<navree::log::LogOptions>());
    #[cfg(feature = "logbook")]
    sections.push(SectionRegistration::of::<navree::logbook::LogbookOptions>());
    #[cfg(feature = "mail")]
    sections.push(SectionRegistration::of::<navree::mail::MailOptions>());
    #[cfg(feature = "mcp")]
    sections.push(SectionRegistration::of::<navree::mcp::McpOptions>());
    #[cfg(feature = "media")]
    sections.push(SectionRegistration::of::<navree::media::MediaOptions>());
    #[cfg(feature = "micro")]
    sections.push(SectionRegistration::of::<navree::micro::MicroOptions>());
    #[cfg(feature = "notify")]
    sections.push(SectionRegistration::of::<navree::notify::NotifyOptions>());
    #[cfg(feature = "oauth")]
    sections.push(SectionRegistration::of::<navree::oauth::OAuthOptions>());
    #[cfg(feature = "observe")]
    sections.push(SectionRegistration::of::<navree::observe::ObserveOptions>());
    #[cfg(feature = "observe")]
    sections.push(SectionRegistration::of::<navree::observe::TracingOptions>());
    #[cfg(feature = "orm")]
    sections.push(SectionRegistration::of::<navree::orm::OrmOptions>());
    #[cfg(feature = "queue")]
    sections.push(SectionRegistration::of::<navree::queue::QueueOptions>());
    #[cfg(feature = "realtime")]
    sections.push(SectionRegistration::of::<navree::realtime::RealtimeOptions>());
    #[cfg(feature = "redis")]
    sections.push(SectionRegistration::of::<navree::redis::RedisOptions>());
    #[cfg(feature = "schema")]
    sections.push(SectionRegistration::of::<navree::schema::SchemaOptions>());
    #[cfg(feature = "search")]
    sections.push(SectionRegistration::of::<navree::search::SearchOptions>());
    #[cfg(feature = "session")]
    sections.push(SectionRegistration::of::<navree::session::SessionOptions>());
    #[cfg(feature = "settings")]
    sections.push(SectionRegistration::of::<navree::settings::SettingsOptions>());
    #[cfg(feature = "sheets")]
    sections.push(SectionRegistration::of::<navree::sheets::SheetsOptions>());
    #[cfg(feature = "social")]
    sections.push(SectionRegistration::of::<navree::social::SocialOptions>());
    #[cfg(feature = "sso")]
    sections.push(SectionRegistration::of::<navree::sso::SsoOptions>());
    #[cfg(feature = "storage")]
    sections.push(SectionRegistration::of::<navree::storage::StorageOptions>());
    #[cfg(feature = "time")]
    sections.push(SectionRegistration::of::<navree::time::TimeOptions>());
    #[cfg(feature = "ui")]
    sections.push(SectionRegistration::of::<navree::ui::UiOptions>());
    #[cfg(feature = "validation")]
    sections.push(SectionRegistration::of::<
        navree::validation::ValidationOptions,
    >());
    #[cfg(feature = "view")]
    sections.push(SectionRegistration::of::<navree::view::ViewOptions>());
    #[cfg(feature = "webhooks")]
    sections.push(SectionRegistration::of::<navree::webhooks::WebhooksOptions>());
    #[cfg(feature = "workflows")]
    sections.push(SectionRegistration::of::<navree::workflows::WorkflowsOptions>());
    #[cfg(feature = "workspace")]
    sections.push(SectionRegistration::of::<navree::workspace::WorkspaceConfig>());
    sections
}

#[test]
fn every_section_describes_itself() {
    for registration in sections() {
        let key = registration.key();
        let schema = registration
            .schema()
            .unwrap_or_else(|| panic!("section `{key}` does not describe its keys"));
        assert_eq!(schema.key, key);
        assert!(!schema.fields.is_empty(), "section `{key}` has no fields");
        assert!(
            schema.fields.iter().all(|field| !field.name.is_empty()),
            "section `{key}` has an unnamed field"
        );
    }
}

#[test]
fn no_credential_is_a_plain_string() {
    let registry = ConfigRegistry::new(&sections());
    assert!(
        registry.unmarked_secrets().is_empty(),
        "make these `Secret<T>` or mark them `#[config(secret)]` (or `#[config(public)]` when the name misleads): {:?}",
        registry.unmarked_secrets()
    );
}

#[test]
fn credentials_are_listed_and_their_defaults_hold_no_value() {
    let registry = ConfigRegistry::new(&sections());
    let defaults = registry.defaults_tree();
    let secrets = registry.secret_paths();
    assert!(!secrets.is_empty());
    for secret in secrets {
        let mut node = Some(&defaults);
        for part in secret.split('.') {
            node = node.and_then(|node| node.get(part));
        }
        match node {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => assert!(
                items.iter().all(Value::is_null),
                "the default of `{secret}` shows a value"
            ),
            Some(other) => panic!("the default of `{secret}` shows {other}"),
        }
    }
}

#[test]
fn the_registry_has_every_section_the_framework_reads() {
    if !COMPLETE {
        return;
    }
    let registry = ConfigRegistry::new(&sections());
    let keys: Vec<&str> = registry
        .sections()
        .iter()
        .map(|section| section.key)
        .collect();
    for key in FRAMEWORK_SECTIONS {
        assert!(
            keys.contains(key),
            "section `{key}` is not in the registry: {keys:?}"
        );
    }
    for key in &keys {
        assert!(
            FRAMEWORK_SECTIONS.contains(key),
            "add `{key}` to FRAMEWORK_SECTIONS"
        );
    }
}

#[test]
fn the_reference_page_is_current() {
    if !COMPLETE {
        return;
    }
    let registry = ConfigRegistry::new(&sections());
    let page = reference_markdown(&registry);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/website/content/docs/configuration/reference.mdx");
    if std::env::var_os("NAVREE_CONFIG_RECORD").is_some_and(|value| value == "1") {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        std::fs::write(&path, &page).expect("write the reference");
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        committed == page,
        "{} is out of date: rewrite it with NAVREE_CONFIG_RECORD=1 (see the top of this file)",
        path.display()
    );
}

#[test]
fn the_section_trait_is_the_one_the_derive_writes() {
    // `ConfigSection::KEY` and the schema's key are one string.
    assert_eq!(<navree::config::AppOptions as ConfigSection>::KEY, "app");
}
