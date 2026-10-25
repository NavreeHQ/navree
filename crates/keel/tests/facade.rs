//! The facade's surface: one module per crate (each re-exporting the whole
//! crate), the macros and the most used kernel types at the root, the
//! prelude, `keel::Result`, and the Cargo features (one per module crate,
//! each turning on what its crate builds on).

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use keel::prelude::*;
use serde_json::Value;

/// `keel::<module>::<Item>` is the very item of the underlying crate.
fn same<A: ?Sized + 'static, B: ?Sized + 'static>() -> bool {
    TypeId::of::<A>() == TypeId::of::<B>()
}

#[test]
fn every_module_crate_is_reexported_whole() {
    // L0-L1
    assert!(same::<keel::kernel::Keel, keel::Keel>());
    assert!(same::<keel::kernel::ModuleBuilder, keel::ModuleBuilder>());
    assert!(same::<keel::contracts::RequestId, keel::contracts::RequestId>());
    assert!(same::<keel::config::ConfigModule, ConfigModule>());
    // L2
    assert!(same::<keel::http::HttpModule, HttpModule>());
    assert!(same::<keel::log::LoggerModule, LoggerModule>());
    assert!(same::<keel::crypto::CryptoModule, CryptoModule>());
    assert!(same::<keel::events::EventsModule, EventsModule>());
    assert!(same::<keel::view::ViewModule, ViewModule>());
    assert!(same::<keel::cache::CacheModule, CacheModule>());
    assert!(same::<keel::http_client::HttpClientModule, HttpClientModule>());
    // L3
    assert!(same::<keel::db::DatabaseModule, DatabaseModule>());
    assert!(same::<keel::testing::TestApp, keel::testing::TestApp>());
    assert!(same::<keel::observe::ObserveModule, ObserveModule>());
    assert!(same::<keel::api::ApiModule, ApiModule>());
    assert!(same::<keel::mail::MailModule, MailModule>());
    assert!(same::<keel::realtime::RealtimeModule, RealtimeModule>());
    assert!(same::<keel::storage::StorageModule, StorageModule>());
    // L4
    assert!(same::<keel::session::SessionModule, SessionModule>());
    assert!(same::<keel::validation::ValidationModule, ValidationModule>());
    assert!(same::<keel::queue::QueueModule, QueueModule>());
    assert!(same::<keel::audit::AuditModule, AuditModule>());
    assert!(same::<keel::access::AccessModule, AccessModule>());
    assert!(same::<keel::schedule::ScheduleModule, ScheduleModule>());
    assert!(same::<keel::notify::NotifyModule, NotifyModule>());
    // L5
    assert!(same::<keel::auth::AuthModule, AuthModule>());
}

/// The kernel names an app's `main` and root module use most.
#[test]
fn kernel_essentials_sit_at_the_root() {
    assert!(same::<keel::Keel, keel::kernel::Keel>());
    assert!(same::<keel::AppBuilder, keel::kernel::AppBuilder>());
    assert!(same::<keel::AppContext, keel::kernel::AppContext>());
    assert!(same::<keel::BoxError, keel::kernel::BoxError>());
    assert!(same::<dyn keel::Module, dyn keel::kernel::Module>());
}

#[test]
fn prelude_brings_the_everyday_types() {
    // Kernel and HTTP.
    assert!(same::<Keel, keel::kernel::Keel>());
    assert!(same::<dyn Module, dyn keel::kernel::Module>());
    assert!(same::<Arc<u8>, std::sync::Arc<u8>>());
    assert!(same::<AppError, keel::http::AppError>());
    assert!(same::<Json<u8>, keel::http::Json<u8>>());
    assert!(same::<Created<u8>, keel::http::Created<u8>>());
    assert!(same::<NoContent, keel::http::NoContent>());
    assert!(same::<Scoped<u8>, keel::http::Scoped<u8>>());
    assert!(same::<Path<u8>, keel::axum::extract::Path<u8>>());
    assert!(same::<Query<u8>, keel::axum::extract::Query<u8>>());
    // Data, validation, identity and access.
    assert!(same::<Uow, keel::db::Uow>());
    assert!(same::<Page<u8>, keel::db::Page<u8>>());
    assert!(same::<PageQuery, keel::db::PageQuery>());
    assert!(same::<Valid<u8>, keel::validation::Valid<u8>>());
    assert!(same::<CurrentUser, keel::auth::CurrentUser>());
    assert!(same::<Abilities, keel::access::Abilities>());
    assert!(same::<Rules, keel::access::Rules>());
    assert!(same::<AbilityCtx, keel::access::AbilityCtx>());
    for (marker, name) in [
        (TypeId::of::<Read>(), Read::NAME),
        (TypeId::of::<Create>(), Create::NAME),
        (TypeId::of::<Update>(), Update::NAME),
        (TypeId::of::<Delete>(), Delete::NAME),
        (TypeId::of::<Manage>(), Manage::NAME),
    ] {
        assert_ne!(marker, TypeId::of::<()>(), "{name}");
    }
    assert_eq!(
        (READ, CREATE, UPDATE, DELETE, MANAGE, ALL),
        ("read", "create", "update", "delete", "manage", "all")
    );
    let _: fn(&str) -> keel::access::FieldRef = field;
    // The two `Factory` types: the DI factory keeps its name, the model
    // factory is renamed.
    assert!(same::<Factory<u8>, keel::kernel::Factory<u8>>());
    assert!(same::<
        dyn ModelFactory<Entity = DummyEntity>,
        dyn keel::db::Factory<Entity = DummyEntity>,
    >());
}

/// A stand-in entity type for the model factory check.
type DummyEntity = keel::access::entity::role::Entity;

/// A subject and an action declared by an app, with the prelude only.
struct Invoice;

impl Subject for Invoice {
    const SUBJECT: &'static str = "Invoice";
    const PERMISSION_PREFIX: &'static str = "invoices";
}

struct Dispatch;

impl ActionMarker for Dispatch {
    const NAME: &'static str = "send";
}

#[test]
fn prelude_traits_are_implementable() {
    assert_eq!(<Invoice as Subject>::SUBJECT, "Invoice");
    assert_eq!(<Dispatch as ActionMarker>::NAME, "send");
}

#[test]
fn result_defaults_to_app_error() {
    fn find(id: u32) -> keel::Result<u32> {
        if id == 0 {
            return Err(AppError::not_found());
        }
        Ok(id)
    }
    assert_eq!(find(7).ok(), Some(7));
    assert_eq!(find(0).unwrap_err().status.as_u16(), 404);
    // The prelude's `Result` is the same alias.
    let other: Result<u8, std::fmt::Error> = Err(std::fmt::Error);
    assert!(other.is_err());
}

#[test]
fn module_builder_extensions_come_with_the_prelude() {
    // `m.migrations(..)`, `m.controller::<C>()`, `m.config::<T>()`, `m.job::<J>()`,
    // `m.templates(..)`, ... resolve with the prelude alone.
    struct Wired;

    impl Module for Wired {
        fn register(&self, m: &mut ModuleBuilder) {
            m.migrations(keel::db::MigrationSet::new("wired"));
            m.templates(&[("wired.txt", "hi")]);
        }
    }

    let report = Keel::app()
        .module(Wired)
        .validate()
        .expect("the graph is valid");
    assert!(report.modules.iter().any(|m| m.name == "Wired"));
}

/// Runs `cargo metadata --no-deps` on the workspace.
fn workspace_metadata() -> Value {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .expect("failed to run `cargo metadata`");
    assert!(
        output.status.success(),
        "`cargo metadata` failed:
{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("`cargo metadata` printed invalid JSON")
}

/// The facade's features as read from its manifest.
struct Features {
    /// Feature name -> its list (features and `dep:` items).
    table: BTreeMap<String, Vec<String>>,
    /// Optional keel crate -> the feature that turns it on.
    feature_of: BTreeMap<String, String>,
    /// Workspace package name -> its normal (runtime) keel dependencies.
    runtime_deps: BTreeMap<String, Vec<String>>,
}

/// Always compiled into the facade (not behind a feature).
const ALWAYS: [&str; 4] = [
    "keel-kernel",
    "keel-contracts",
    "keel-config",
    "keel-macros",
];

impl Features {
    fn read() -> Features {
        let metadata = workspace_metadata();
        let packages = metadata["packages"].as_array().expect("packages");
        let facade = packages
            .iter()
            .find(|p| p["name"] == "keel")
            .expect("the keel package");
        let table: BTreeMap<String, Vec<String>> = facade["features"]
            .as_object()
            .expect("features")
            .iter()
            .map(|(name, list)| {
                let items = list
                    .as_array()
                    .expect("a feature is a list")
                    .iter()
                    .map(|item| item.as_str().expect("a string").to_owned())
                    .collect();
                (name.clone(), items)
            })
            .collect();
        let feature_of = table
            .iter()
            .flat_map(|(name, items)| {
                items.iter().filter_map(move |item| {
                    item.strip_prefix("dep:")
                        .filter(|dep| dep.starts_with("keel-"))
                        .map(|dep| (dep.to_owned(), name.clone()))
                })
            })
            .collect();
        let runtime_deps = packages
            .iter()
            .map(|package| {
                let deps = package["dependencies"]
                    .as_array()
                    .expect("dependencies")
                    .iter()
                    .filter(|dep| dep["kind"].is_null())
                    .filter_map(|dep| dep["name"].as_str())
                    .filter(|name| name.starts_with("keel-"))
                    .map(str::to_owned)
                    .collect();
                (package["name"].as_str().expect("name").to_owned(), deps)
            })
            .collect();
        Features {
            table,
            feature_of,
            runtime_deps,
        }
    }

    /// Every feature `feature` turns on, itself included (transitively).
    fn closure(&self, feature: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![feature.to_owned()];
        while let Some(next) = stack.pop() {
            if !seen.insert(next.clone()) {
                continue;
            }
            for item in self.table.get(&next).into_iter().flatten() {
                if self.table.contains_key(item) {
                    stack.push(item.clone());
                }
            }
        }
        seen
    }
}

#[test]
fn every_keel_dependency_is_always_on_or_behind_its_own_feature() {
    let features = Features::read();
    let mut problems = Vec::new();
    for dep in &features.runtime_deps["keel"] {
        if !ALWAYS.contains(&dep.as_str()) && !features.feature_of.contains_key(dep) {
            problems.push(format!("{dep} is neither always on nor behind a feature"));
        }
    }
    for (krate, feature) in &features.feature_of {
        let expected = krate.trim_start_matches("keel-");
        if feature != expected {
            problems.push(format!(
                "{krate} is behind `{feature}`, expected `{expected}`"
            ));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn module_features_turn_on_the_features_of_their_keel_dependencies() {
    let features = Features::read();
    let mut missing = Vec::new();
    for (krate, feature) in &features.feature_of {
        let enabled = features.closure(feature);
        for dep in &features.runtime_deps[krate] {
            if ALWAYS.contains(&dep.as_str()) {
                continue;
            }
            match features.feature_of.get(dep) {
                Some(needed) if enabled.contains(needed) => {}
                Some(needed) => missing.push(format!(
                    "`{feature}` must turn on `{needed}`: {krate} depends on {dep}"
                )),
                None => missing.push(format!(
                    "no facade feature turns on {dep} (used by {krate})"
                )),
            }
        }
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

/// The other direction: a module feature turns on nothing its crate does
/// not build on, so `default-features = false, features = ["auth"]` pulls
/// in only what keel-auth needs.
#[test]
fn module_features_turn_on_only_the_features_of_their_keel_dependencies() {
    let features = Features::read();
    let mut extra = Vec::new();
    for (krate, feature) in &features.feature_of {
        let needed: BTreeSet<&str> = features.runtime_deps[krate]
            .iter()
            .filter_map(|dep| features.feature_of.get(dep))
            .map(String::as_str)
            .collect();
        for item in &features.table[feature] {
            if features.table.contains_key(item) && !needed.contains(item.as_str()) {
                extra.push(format!(
                    "`{feature}` turns on `{item}`, but {krate} does not depend on its crate"
                ));
            }
        }
    }
    assert!(extra.is_empty(), "{extra:#?}");
}

#[test]
fn full_is_the_default_and_turns_on_every_module() {
    let features = Features::read();
    assert_eq!(features.table["default"], ["full"]);
    let full = features.closure("full");
    let missing: Vec<&String> = features
        .feature_of
        .values()
        .filter(|feature| !full.contains(*feature))
        .collect();
    assert!(missing.is_empty(), "`full` lacks {missing:?}");
}
