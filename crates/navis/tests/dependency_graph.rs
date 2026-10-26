//! Enforces the crate hierarchy of the build design (spec §3) on every test run.
//!
//! Every `navis*` dependency edge in the workspace (normal, dev and build) must be
//! allowed by [`HIERARCHY`], which encodes the "Allowed runtime dependency edges"
//! table of `docs/superpowers/specs/2026-10-25-keel-framework-design.md`.
//!
//! Two exceptions, both for dev-dependencies only:
//! - A dev-dependency may point at any navis crate in a lower layer (spec §3),
//!   so tests can boot lower-layer modules (navis-notify's tests need
//!   ViewModule for MailModule). This covers `navis-testing` (L3) for every
//!   crate in layer 4 or above.
//! - `navis-macros` tests expand the macros against the runtime crates, so
//!   `navis-macros` may dev-depend on any runtime crate (layers 0 to 5).
//!
//! It also checks the external side of the graph: `Cargo.lock` must not contain
//! aws-lc, OpenSSL, native-tls or CMake (rustls with `ring` only).
//!
//! Adding a workspace crate? Give it a row in [`HIERARCHY`] first.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

/// One row of the hierarchy: a workspace crate, its layer, and the navis crates
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
    rule("navis-kernel", 0, &[]),
    rule("navis-macros", 0, &[]),
    // L1
    rule("navis-contracts", 1, &["navis-kernel"]),
    rule("navis-config", 1, &["navis-kernel", "navis-contracts"]),
    // L2
    rule("navis-http", 2, &["navis-kernel", "navis-contracts", "navis-config"]),
    rule("navis-log", 2, &["navis-kernel", "navis-contracts", "navis-config"]),
    rule("navis-crypto", 2, &["navis-kernel", "navis-contracts", "navis-config"]),
    rule("navis-events", 2, &["navis-kernel", "navis-contracts", "navis-config"]),
    rule("navis-view", 2, &["navis-kernel", "navis-contracts", "navis-config"]),
    rule("navis-cache", 2, &["navis-kernel", "navis-contracts", "navis-config"]),
    rule("navis-http-client", 2, &["navis-kernel", "navis-contracts", "navis-config"]),
    // L3
    rule("navis-db", 3, &["navis-kernel", "navis-contracts", "navis-config", "navis-http"]),
    rule("navis-testing", 3, &["navis-kernel", "navis-contracts", "navis-config", "navis-http"]),
    rule("navis-observe", 3, &["navis-kernel", "navis-contracts", "navis-config", "navis-http"]),
    rule("navis-api", 3, &["navis-kernel", "navis-contracts", "navis-config", "navis-http"]),
    rule("navis-mail", 3, &["navis-kernel", "navis-contracts", "navis-config", "navis-view"]),
    rule("navis-realtime", 3, &["navis-kernel", "navis-contracts", "navis-config", "navis-http", "navis-crypto"]),
    rule("navis-storage", 3, &["navis-kernel", "navis-contracts", "navis-config", "navis-http", "navis-crypto"]),
    // L4
    rule("navis-session", 4, &["navis-kernel", "navis-contracts", "navis-config", "navis-http", "navis-crypto", "navis-db"]),
    rule("navis-validation", 4, &["navis-kernel", "navis-contracts", "navis-config", "navis-http", "navis-db"]),
    rule("navis-audit", 4, &["navis-kernel", "navis-contracts", "navis-config", "navis-http", "navis-db"]),
    rule("navis-queue", 4, &["navis-kernel", "navis-contracts", "navis-config", "navis-db"]),
    rule("navis-schedule", 4, &["navis-kernel", "navis-contracts", "navis-config", "navis-db"]),
    rule("navis-access", 4, &["navis-kernel", "navis-contracts", "navis-config", "navis-http", "navis-db", "navis-cache"]),
    rule("navis-notify", 4, &["navis-kernel", "navis-contracts", "navis-config", "navis-http", "navis-db", "navis-mail"]),
    // L5
    rule("navis-auth", 5, &[
        "navis-kernel", "navis-contracts", "navis-config", "navis-http", "navis-db",
        "navis-crypto", "navis-session", "navis-validation", "navis-cache", "navis-mail",
        "navis-access",
    ]),
    // L6
    rule("navis", 6, RUNTIME_CRATES),
    rule("navis-cli", 6, &[]),
    // L7 (apps): anything below, except the CLI binary.
    // The CRM (plan "Wave 6 layout"): its crates use only the `navis` facade.
    // crm-core is shared; crm-testkit boots it for tests (a dev-dependency
    // of the crates above it); the feature crates build on crm-core; the
    // crm-api binary assembles them.
    rule("crm-core", 7, &["navis"]),
    rule("crm-testkit", 8, &["navis", "crm-core"]),
    rule("crm-users", 9, &["navis", "crm-core"]),
    rule("crm-companies", 9, &["navis", "crm-core"]),
    rule("crm-deals", 9, &["navis", "crm-core"]),
    rule("crm-activities", 9, &["navis", "crm-core"]),
    rule("crm-api", 10, CRM_API_DEPENDENCIES),
];

/// What the crm-api binary may depend on: the facade and the CRM crates.
const CRM_API_DEPENDENCIES: &[&str] = &[
    "navis",
    "crm-core",
    "crm-users",
    "crm-companies",
    "crm-deals",
    "crm-activities",
];

/// Every runtime crate (layers 0 to 5) plus navis-macros: what the facade may
/// re-export and what navis-macros tests may dev-depend on.
const RUNTIME_CRATES: &[&str] = &[
    "navis-kernel",
    "navis-macros",
    "navis-contracts",
    "navis-config",
    "navis-http",
    "navis-log",
    "navis-crypto",
    "navis-events",
    "navis-view",
    "navis-cache",
    "navis-http-client",
    "navis-db",
    "navis-testing",
    "navis-observe",
    "navis-api",
    "navis-mail",
    "navis-realtime",
    "navis-storage",
    "navis-session",
    "navis-validation",
    "navis-queue",
    "navis-audit",
    "navis-access",
    "navis-schedule",
    "navis-notify",
    "navis-auth",
];

/// Cargo dependency kind of an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Normal,
    Dev,
    Build,
}

/// A dependency edge between two navis crates.
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

/// Every dependency edge from a workspace package to a navis crate or to
/// another workspace package.
fn navis_edges(metadata: &Value) -> Vec<Edge> {
    let members = workspace_crates(metadata);
    let is_internal =
        |name: &str| name == "navis" || name.starts_with("navis-") || members.contains(name);
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
        // Tests may use any navis crate in a lower layer (spec §3); this is how
        // navis-testing (L3) serves every crate in layer 4 and above.
        if to_rule.layer < from_rule.layer {
            return None;
        }
        // Proc-macro tests expand against the runtime crates they target.
        if from == "navis-macros" && RUNTIME_CRATES.contains(&to.as_str()) && to != "navis-macros" {
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
    let edges = navis_edges(&workspace_metadata());
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

/// The §3 table allows one same-layer edge (navis-config -> navis-contracts, both
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
fn navis_edges_are_read_from_cargo_metadata_with_their_kind() {
    let metadata = json!({
        "packages": [
            {
                "name": "navis-db",
                "dependencies": [
                    { "name": "navis-http", "kind": null, "rename": null },
                    { "name": "sea-orm", "kind": null, "rename": null },
                    { "name": "navis-kernel", "kind": "dev", "rename": "kernel" },
                    { "name": "navis-config", "kind": "build", "rename": null }
                ]
            },
            {
                "name": "some-app",
                "dependencies": [
                    { "name": "navis-http", "kind": null, "rename": null },
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
        navis_edges(&metadata),
        vec![
            edge("navis-db", "navis-http", Kind::Normal),
            edge("navis-db", "navis-kernel", Kind::Dev),
            edge("navis-db", "navis-config", Kind::Build),
            edge("some-app", "navis-http", Kind::Normal),
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
        edge("navis-contracts", "navis-kernel", Kind::Normal),
        edge("navis-auth", "navis-session", Kind::Normal),
        // `impl navis_access::Subject for navis_auth::user::Entity`.
        edge("navis-auth", "navis-access", Kind::Normal),
        edge("navis", "navis-auth", Kind::Normal),
        edge("navis", "navis-macros", Kind::Normal),
        edge("crm-api", "navis", Kind::Normal),
        edge("navis-db", "navis-http", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn upward_normal_edge_is_reported() {
    let found = violations(&[edge("navis-kernel", "navis-http", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("navis-kernel") && found[0].contains("navis-http"));
}

#[test]
fn sideways_edge_within_a_layer_is_reported() {
    let found = violations(&[edge("navis-http", "navis-crypto", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn forbidden_dev_and_build_edges_are_reported() {
    let found = violations(&[
        edge("navis-http", "navis-db", Kind::Dev),
        edge("navis-queue", "navis-http", Kind::Build),
    ]);
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn navis_testing_is_an_allowed_dev_dependency_from_layer_4_up() {
    let edges = [
        edge("navis-session", "navis-testing", Kind::Dev),
        edge("navis-auth", "navis-testing", Kind::Dev),
        edge("crm-api", "navis-testing", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn dev_dependency_on_any_lower_layer_crate_is_allowed() {
    // Spec §3: tests may boot lower-layer modules (MailModule needs ViewModule).
    let edges = [
        edge("navis-notify", "navis-view", Kind::Dev),
        edge("navis-auth", "navis-view", Kind::Dev),
        edge("navis-session", "navis-cache", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn dev_dependency_within_the_same_layer_or_upwards_is_reported() {
    let found = violations(&[
        edge("navis-notify", "navis-queue", Kind::Dev),
        edge("navis-view", "navis-http", Kind::Dev),
        edge("navis-session", "navis-auth", Kind::Dev),
    ]);
    assert_eq!(found.len(), 3, "{found:?}");
}

#[test]
fn navis_testing_is_not_a_normal_dependency_of_layer_4() {
    let found = violations(&[edge("navis-session", "navis-testing", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn navis_testing_is_not_a_dev_dependency_below_layer_4() {
    let found = violations(&[edge("navis-db", "navis-testing", Kind::Dev)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn navis_macros_may_dev_depend_on_runtime_crates_only() {
    assert_eq!(
        violations(&[
            edge("navis-macros", "navis-kernel", Kind::Dev),
            edge("navis-macros", "navis-http", Kind::Dev),
            edge("navis-macros", "navis-access", Kind::Dev),
        ]),
        Vec::<String>::new()
    );
    let found = violations(&[
        edge("navis-macros", "navis-http", Kind::Normal),
        edge("navis-macros", "navis", Kind::Dev),
        edge("navis-macros", "navis-cli", Kind::Dev),
    ]);
    assert_eq!(found.len(), 3, "{found:?}");
}

#[test]
fn navis_cli_has_no_navis_dependencies() {
    let found = violations(&[edge("navis-cli", "navis-kernel", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn unknown_navis_crates_are_reported() {
    let found = violations(&[
        edge("navis-redis", "navis-kernel", Kind::Normal),
        edge("navis-http", "navis-redis", Kind::Normal),
    ]);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|m| m.contains("navis-redis")), "{found:?}");
}
