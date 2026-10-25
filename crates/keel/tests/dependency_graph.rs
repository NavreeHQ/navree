//! Enforces the crate hierarchy of the build design (spec §3) on every test run.
//!
//! Every `keel*` dependency edge in the workspace (normal, dev and build) must be
//! allowed by [`HIERARCHY`], which encodes the "Allowed runtime dependency edges"
//! table of `docs/superpowers/specs/2026-10-25-keel-framework-design.md`.
//!
//! Two exceptions, both for dev-dependencies only:
//! - A dev-dependency may point at any keel crate in a lower layer (spec §3),
//!   so tests can boot lower-layer modules (keel-notify's tests need
//!   ViewModule for MailModule). This covers `keel-testing` (L3) for every
//!   crate in layer 4 or above.
//! - `keel-macros` tests expand the macros against the runtime crates, so
//!   `keel-macros` may dev-depend on any runtime crate (layers 0 to 5).
//!
//! It also checks the external side of the graph: `Cargo.lock` must not contain
//! aws-lc, OpenSSL, native-tls or CMake (rustls with `ring` only).
//!
//! Adding a workspace crate? Give it a row in [`HIERARCHY`] first.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

/// One row of the hierarchy: a workspace crate, its layer, and the keel crates
/// it may depend on.
struct Rule {
    krate: &'static str,
    layer: u8,
    allowed: &'static [&'static str],
}

const fn rule(krate: &'static str, layer: u8, allowed: &'static [&'static str]) -> Rule {
    Rule {
        krate,
        layer,
        allowed,
    }
}

/// Spec §3, "Allowed runtime dependency edges", plus the app layer (L7) whose
/// crates may use anything below them. Kept one row per crate, like the spec.
#[rustfmt::skip]
const HIERARCHY: &[Rule] = &[
    // L0
    rule("keel-kernel", 0, &[]),
    rule("keel-macros", 0, &[]),
    // L1
    rule("keel-contracts", 1, &["keel-kernel"]),
    rule("keel-config", 1, &["keel-kernel", "keel-contracts"]),
    // L2
    rule("keel-http", 2, &["keel-kernel", "keel-contracts", "keel-config"]),
    rule("keel-log", 2, &["keel-kernel", "keel-contracts", "keel-config"]),
    rule("keel-crypto", 2, &["keel-kernel", "keel-contracts", "keel-config"]),
    rule("keel-events", 2, &["keel-kernel", "keel-contracts", "keel-config"]),
    rule("keel-view", 2, &["keel-kernel", "keel-contracts", "keel-config"]),
    rule("keel-cache", 2, &["keel-kernel", "keel-contracts", "keel-config"]),
    rule("keel-http-client", 2, &["keel-kernel", "keel-contracts", "keel-config"]),
    // L3
    rule("keel-db", 3, &["keel-kernel", "keel-contracts", "keel-config", "keel-http"]),
    rule("keel-testing", 3, &["keel-kernel", "keel-contracts", "keel-config", "keel-http"]),
    rule("keel-observe", 3, &["keel-kernel", "keel-contracts", "keel-config", "keel-http"]),
    rule("keel-api", 3, &["keel-kernel", "keel-contracts", "keel-config", "keel-http"]),
    rule("keel-mail", 3, &["keel-kernel", "keel-contracts", "keel-config", "keel-view"]),
    rule("keel-realtime", 3, &["keel-kernel", "keel-contracts", "keel-config", "keel-http", "keel-crypto"]),
    rule("keel-storage", 3, &["keel-kernel", "keel-contracts", "keel-config", "keel-http", "keel-crypto"]),
    // L4
    rule("keel-session", 4, &["keel-kernel", "keel-contracts", "keel-config", "keel-http", "keel-crypto", "keel-db"]),
    rule("keel-validation", 4, &["keel-kernel", "keel-contracts", "keel-config", "keel-http", "keel-db"]),
    rule("keel-audit", 4, &["keel-kernel", "keel-contracts", "keel-config", "keel-http", "keel-db"]),
    rule("keel-queue", 4, &["keel-kernel", "keel-contracts", "keel-config", "keel-db"]),
    rule("keel-schedule", 4, &["keel-kernel", "keel-contracts", "keel-config", "keel-db"]),
    rule("keel-access", 4, &["keel-kernel", "keel-contracts", "keel-config", "keel-http", "keel-db", "keel-cache"]),
    rule("keel-notify", 4, &["keel-kernel", "keel-contracts", "keel-config", "keel-http", "keel-db", "keel-mail"]),
    // L5
    rule("keel-auth", 5, &[
        "keel-kernel", "keel-contracts", "keel-config", "keel-http", "keel-db",
        "keel-crypto", "keel-session", "keel-validation", "keel-cache", "keel-mail",
        "keel-access",
    ]),
    // L6
    rule("keel", 6, RUNTIME_CRATES),
    rule("keel-cli", 6, &[]),
    // L7 (apps): anything below, except the CLI binary.
    // The CRM (plan "Wave 6 layout"): its crates use only the `keel` facade.
    // crm-core is shared; crm-testkit boots it for tests (a dev-dependency
    // of the crates above it); the feature crates build on crm-core; the
    // crm-api binary assembles them.
    rule("crm-core", 7, &["keel"]),
    rule("crm-testkit", 8, &["keel", "crm-core"]),
    rule("crm-users", 9, &["keel", "crm-core"]),
    rule("crm-companies", 9, &["keel", "crm-core"]),
    rule("crm-deals", 9, &["keel", "crm-core"]),
    rule("crm-activities", 9, &["keel", "crm-core"]),
    rule("crm-api", 10, CRM_API_DEPENDENCIES),
];

/// What the crm-api binary may depend on: the facade and the CRM crates.
const CRM_API_DEPENDENCIES: &[&str] = &[
    "keel",
    "crm-core",
    "crm-users",
    "crm-companies",
    "crm-deals",
    "crm-activities",
];

/// Every runtime crate (layers 0 to 5) plus keel-macros: what the facade may
/// re-export and what keel-macros tests may dev-depend on.
const RUNTIME_CRATES: &[&str] = &[
    "keel-kernel",
    "keel-macros",
    "keel-contracts",
    "keel-config",
    "keel-http",
    "keel-log",
    "keel-crypto",
    "keel-events",
    "keel-view",
    "keel-cache",
    "keel-http-client",
    "keel-db",
    "keel-testing",
    "keel-observe",
    "keel-api",
    "keel-mail",
    "keel-realtime",
    "keel-storage",
    "keel-session",
    "keel-validation",
    "keel-queue",
    "keel-audit",
    "keel-access",
    "keel-schedule",
    "keel-notify",
    "keel-auth",
];

/// Cargo dependency kind of an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Normal,
    Dev,
    Build,
}

/// A dependency edge between two keel crates.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Edge {
    from: String,
    to: String,
    kind: Kind,
}

fn edge(from: &str, to: &str, kind: Kind) -> Edge {
    Edge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind,
    }
}

/// Runs `cargo metadata --no-deps` on the workspace.
fn workspace_metadata() -> Value {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
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
        "`cargo metadata` failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("`cargo metadata` printed invalid JSON")
}

/// The `packages` array of `cargo metadata --no-deps` output (workspace
/// members only).
fn packages(metadata: &Value) -> &[Value] {
    metadata["packages"]
        .as_array()
        .expect("cargo metadata has a `packages` array")
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("cargo metadata entry without string `{key}`: {value}"))
}

/// Names of the workspace member packages.
fn workspace_crates(metadata: &Value) -> BTreeSet<String> {
    packages(metadata)
        .iter()
        .map(|p| str_field(p, "name").to_owned())
        .collect()
}

/// Every dependency edge from a workspace package to a keel crate or to
/// another workspace package.
fn keel_edges(metadata: &Value) -> Vec<Edge> {
    let members = workspace_crates(metadata);
    let is_internal =
        |name: &str| name == "keel" || name.starts_with("keel-") || members.contains(name);
    let mut edges = Vec::new();
    for package in packages(metadata) {
        let from = str_field(package, "name");
        let deps = package["dependencies"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        for dep in deps {
            let to = str_field(dep, "name");
            if !is_internal(to) {
                continue;
            }
            let kind = match dep["kind"].as_str() {
                None => Kind::Normal,
                Some("dev") => Kind::Dev,
                Some("build") => Kind::Build,
                Some(other) => panic!("unknown dependency kind `{other}` in {from} -> {to}"),
            };
            edges.push(edge(from, to, kind));
        }
    }
    edges
}

/// Packages that pull in aws-lc, OpenSSL or a CMake build.
const FORBIDDEN_NATIVE_PACKAGES: &[&str] = &[
    "aws-lc-fips-sys",
    "aws-lc-rs",
    "aws-lc-sys",
    "cmake",
    "native-tls",
    "openssl",
    "openssl-sys",
];

/// Names of forbidden packages listed in a `Cargo.lock`.
fn forbidden_native_packages(lock: &str) -> Vec<String> {
    lock.lines()
        .filter_map(|line| line.trim().strip_prefix("name = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|name| FORBIDDEN_NATIVE_PACKAGES.contains(name))
        .map(str::to_owned)
        .collect()
}

fn find_rule(krate: &str) -> Option<&'static Rule> {
    HIERARCHY.iter().find(|r| r.krate == krate)
}

/// Edges not allowed by the hierarchy, as human-readable messages.
fn violations(edges: &[Edge]) -> Vec<String> {
    edges.iter().filter_map(violation).collect()
}

fn violation(edge: &Edge) -> Option<String> {
    let Edge { from, to, kind } = edge;
    let Some(from_rule) = find_rule(from) else {
        return Some(format!(
            "{from} -> {to} ({kind:?}): `{from}` is not in the HIERARCHY table; add it with its layer"
        ));
    };
    let Some(to_rule) = find_rule(to) else {
        return Some(format!(
            "{from} -> {to} ({kind:?}): `{to}` is not in the HIERARCHY table; add it with its layer"
        ));
    };
    if from_rule.allowed.contains(&to.as_str()) {
        return None;
    }
    if *kind == Kind::Dev {
        // Tests may use any keel crate in a lower layer (spec §3); this is how
        // keel-testing (L3) serves every crate in layer 4 and above.
        if to_rule.layer < from_rule.layer {
            return None;
        }
        // Proc-macro tests expand against the runtime crates they target.
        if from == "keel-macros" && RUNTIME_CRATES.contains(&to.as_str()) && to != "keel-macros" {
            return None;
        }
    }
    Some(format!(
        "{from} (L{}) -> {to} (L{}) ({kind:?}) is not an allowed edge; {from} may depend on: [{}]",
        from_rule.layer,
        to_rule.layer,
        from_rule.allowed.join(", ")
    ))
}

#[test]
fn workspace_dependency_edges_follow_the_hierarchy() {
    let edges = keel_edges(&workspace_metadata());
    let found = violations(&edges);
    assert!(
        found.is_empty(),
        "crate hierarchy violations (spec §3):\n  {}",
        found.join("\n  ")
    );
}

/// Global constraint: rustls with the `ring` provider; never require
/// aws-lc (cmake/NASM on Windows), OpenSSL or native-tls.
#[test]
fn lockfile_has_no_aws_lc_openssl_native_tls_or_cmake() {
    let lockfile = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = std::fs::read_to_string(&lockfile).expect("Cargo.lock is readable");
    let found = forbidden_native_packages(&lock);
    assert!(
        found.is_empty(),
        "Cargo.lock contains forbidden native TLS/build packages: {found:?}; \
         use rustls with the `ring` provider (see root Cargo.toml)"
    );
}

#[test]
fn hierarchy_table_lists_exactly_the_workspace_crates() {
    let in_workspace = workspace_crates(&workspace_metadata());
    let in_table: BTreeSet<String> = HIERARCHY.iter().map(|r| r.krate.to_owned()).collect();
    assert_eq!(
        in_workspace, in_table,
        "every workspace crate needs a row in HIERARCHY (and every row a crate)"
    );
}

/// The §3 table allows one same-layer edge (keel-config -> keel-contracts, both
/// L1), so the invariant is "never upwards" plus "no cycles".
#[test]
fn hierarchy_table_never_points_up_and_has_no_cycles() {
    for rule in HIERARCHY {
        for dep in rule.allowed {
            let dep_rule = find_rule(dep).unwrap_or_else(|| panic!("{dep} is not in HIERARCHY"));
            assert!(
                dep_rule.layer <= rule.layer,
                "{} (L{}) may not depend on {dep} (L{})",
                rule.krate,
                rule.layer,
                dep_rule.layer
            );
        }
    }

    // Depth-first search over the allowed edges; `path` is the current chain.
    fn visit(krate: &'static str, path: &mut Vec<&'static str>) {
        if let Some(start) = path.iter().position(|k| *k == krate) {
            panic!(
                "cycle in HIERARCHY: {} -> {krate}",
                path[start..].join(" -> ")
            );
        }
        path.push(krate);
        for dep in find_rule(krate).map_or(&[][..], |r| r.allowed) {
            visit(dep, path);
        }
        path.pop();
    }
    for rule in HIERARCHY {
        visit(rule.krate, &mut Vec::new());
    }
}

#[test]
fn keel_edges_are_read_from_cargo_metadata_with_their_kind() {
    let metadata = json!({
        "packages": [
            {
                "name": "keel-db",
                "dependencies": [
                    { "name": "keel-http", "kind": null, "rename": null },
                    { "name": "sea-orm", "kind": null, "rename": null },
                    { "name": "keel-kernel", "kind": "dev", "rename": "kernel" },
                    { "name": "keel-config", "kind": "build", "rename": null }
                ]
            },
            {
                "name": "some-app",
                "dependencies": [
                    { "name": "keel-http", "kind": null, "rename": null },
                    { "name": "tokio", "kind": "dev", "rename": null }
                ]
            },
            {
                "name": "other-app",
                "dependencies": [{ "name": "some-app", "kind": "dev", "rename": null }]
            }
        ]
    });
    assert_eq!(
        keel_edges(&metadata),
        vec![
            edge("keel-db", "keel-http", Kind::Normal),
            edge("keel-db", "keel-kernel", Kind::Dev),
            edge("keel-db", "keel-config", Kind::Build),
            edge("some-app", "keel-http", Kind::Normal),
            edge("other-app", "some-app", Kind::Dev),
        ]
    );
}

#[test]
fn forbidden_native_packages_are_found_in_a_lockfile() {
    let lock = r#"
version = 4

[[package]]
name = "aws-lc-sys"
version = "0.30.0"

[[package]]
name = "ring"
version = "0.17.14"

[[package]]
name = "openssl-sys"
version = "0.9.109"
"#;
    assert_eq!(
        forbidden_native_packages(lock),
        vec!["aws-lc-sys".to_owned(), "openssl-sys".to_owned()]
    );
}

#[test]
fn allowed_edges_pass() {
    let edges = [
        edge("keel-contracts", "keel-kernel", Kind::Normal),
        edge("keel-auth", "keel-session", Kind::Normal),
        // `impl keel_access::Subject for keel_auth::user::Entity`.
        edge("keel-auth", "keel-access", Kind::Normal),
        edge("keel", "keel-auth", Kind::Normal),
        edge("keel", "keel-macros", Kind::Normal),
        edge("crm-api", "keel", Kind::Normal),
        edge("keel-db", "keel-http", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn upward_normal_edge_is_reported() {
    let found = violations(&[edge("keel-kernel", "keel-http", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("keel-kernel") && found[0].contains("keel-http"));
}

#[test]
fn sideways_edge_within_a_layer_is_reported() {
    let found = violations(&[edge("keel-http", "keel-crypto", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn forbidden_dev_and_build_edges_are_reported() {
    let found = violations(&[
        edge("keel-http", "keel-db", Kind::Dev),
        edge("keel-queue", "keel-http", Kind::Build),
    ]);
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn keel_testing_is_an_allowed_dev_dependency_from_layer_4_up() {
    let edges = [
        edge("keel-session", "keel-testing", Kind::Dev),
        edge("keel-auth", "keel-testing", Kind::Dev),
        edge("crm-api", "keel-testing", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn dev_dependency_on_any_lower_layer_crate_is_allowed() {
    // Spec §3: tests may boot lower-layer modules (MailModule needs ViewModule).
    let edges = [
        edge("keel-notify", "keel-view", Kind::Dev),
        edge("keel-auth", "keel-view", Kind::Dev),
        edge("keel-session", "keel-cache", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn dev_dependency_within_the_same_layer_or_upwards_is_reported() {
    let found = violations(&[
        edge("keel-notify", "keel-queue", Kind::Dev),
        edge("keel-view", "keel-http", Kind::Dev),
        edge("keel-session", "keel-auth", Kind::Dev),
    ]);
    assert_eq!(found.len(), 3, "{found:?}");
}

#[test]
fn keel_testing_is_not_a_normal_dependency_of_layer_4() {
    let found = violations(&[edge("keel-session", "keel-testing", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn keel_testing_is_not_a_dev_dependency_below_layer_4() {
    let found = violations(&[edge("keel-db", "keel-testing", Kind::Dev)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn keel_macros_may_dev_depend_on_runtime_crates_only() {
    assert_eq!(
        violations(&[
            edge("keel-macros", "keel-kernel", Kind::Dev),
            edge("keel-macros", "keel-http", Kind::Dev),
            edge("keel-macros", "keel-access", Kind::Dev),
        ]),
        Vec::<String>::new()
    );
    let found = violations(&[
        edge("keel-macros", "keel-http", Kind::Normal),
        edge("keel-macros", "keel", Kind::Dev),
        edge("keel-macros", "keel-cli", Kind::Dev),
    ]);
    assert_eq!(found.len(), 3, "{found:?}");
}

#[test]
fn keel_cli_has_no_keel_dependencies() {
    let found = violations(&[edge("keel-cli", "keel-kernel", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn unknown_keel_crates_are_reported() {
    let found = violations(&[
        edge("keel-redis", "keel-kernel", Kind::Normal),
        edge("keel-http", "keel-redis", Kind::Normal),
    ]);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|m| m.contains("keel-redis")), "{found:?}");
}
