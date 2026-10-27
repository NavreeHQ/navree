//! Enforces the crate hierarchy of the build design (spec §3) on every test run.
//!
//! Every `navree*` dependency edge in the workspace (normal, dev and build) must be
//! allowed by [`HIERARCHY`], which encodes the "Allowed runtime dependency edges"
//! table of `docs/superpowers/specs/2026-10-25-navree-framework-design.md`.
//!
//! Three exceptions, all for dev-dependencies only:
//! - A crate may dev-depend on itself, to turn on its own features in its
//!   tests (navree-workspace's `testing`).
//! - A dev-dependency may point at any navree crate in a lower layer (spec §3),
//!   so tests can boot lower-layer modules (navree-notify's tests need
//!   ViewModule for MailModule). This covers `navree-testing` (L3) for every
//!   crate in layer 4 or above.
//! - `navree-macros` tests expand the macros against the runtime crates, so
//!   `navree-macros` may dev-depend on any runtime crate (layers 0 to 7).
//!
//! It also checks the external side of the graph: `Cargo.lock` must not contain
//! aws-lc, OpenSSL, native-tls or CMake (rustls with `ring` only).
//!
//! Adding a workspace crate? Give it a row in [`HIERARCHY`] first.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

/// One row of the hierarchy: a workspace crate, its layer, and the navree crates
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

/// Spec §3, "Allowed runtime dependency edges", the ecosystem layers of the
/// ecosystem build plan (§3: L6 libraries, L7 packages built on them, L8 the
/// entry points), and the app layers (L9 and up) whose crates may use
/// anything below them. Kept one row per crate, like the spec.
#[rustfmt::skip]
const HIERARCHY: &[Rule] = &[
    // L0
    rule("navree-kernel", 0, &[]),
    rule("navree-macros", 0, &[]),
    // Dock: local development services (`navree dock`). A tool crate with
    // no navree dependencies; the CLI uses it and the facade re-exports it
    // behind the opt-in `dock` feature.
    rule("navree-dock", 0, &[]),
    // L1
    rule("navree-contracts", 1, &["navree-kernel"]),
    rule("navree-config", 1, &["navree-kernel", "navree-contracts"]),
    // Telemetry events and trace context: every emitter (L2 and up) uses it.
    rule("navree-telemetry", 1, &["navree-kernel", "navree-contracts"]),
    // L2
    rule("navree-http", 2, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry"]),
    rule("navree-log", 2, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry"]),
    rule("navree-crypto", 2, &["navree-kernel", "navree-contracts", "navree-config"]),
    rule("navree-events", 2, &["navree-kernel", "navree-contracts", "navree-config"]),
    rule("navree-view", 2, &["navree-kernel", "navree-contracts", "navree-config"]),
    rule("navree-cache", 2, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry"]),
    rule("navree-http-client", 2, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry"]),
    // L3
    rule("navree-db", 3, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http"]),
    rule("navree-testing", 3, &["navree-kernel", "navree-contracts", "navree-config", "navree-http"]),
    rule("navree-observe", 3, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http"]),
    rule("navree-api", 3, &["navree-kernel", "navree-contracts", "navree-config", "navree-http"]),
    rule("navree-mail", 3, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-view"]),
    rule("navree-realtime", 3, &["navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto"]),
    rule("navree-storage", 3, &["navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto"]),
    // L4
    rule("navree-session", 4, &["navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto", "navree-db"]),
    rule("navree-validation", 4, &["navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-db"]),
    rule("navree-audit", 4, &["navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-db"]),
    rule("navree-queue", 4, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-db"]),
    rule("navree-schedule", 4, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-db"]),
    rule("navree-access", 4, &["navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-db", "navree-cache"]),
    rule("navree-notify", 4, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http", "navree-db", "navree-mail"]),
    // L5
    rule("navree-auth", 5, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-db",
        "navree-crypto", "navree-session", "navree-validation", "navree-cache", "navree-mail",
        "navree-access",
    ]),
    // Workspaces: user ids come from the request's `Principal`, so the
    // module needs no navree-auth (a sibling at L5).
    rule("navree-workspace", 5, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-db",
        "navree-crypto", "navree-events", "navree-mail", "navree-access", "navree-telemetry",
        // `navree_workspace::testing` (feature `testing`): isolation suites
        // on navree-testing's TestApp.
        "navree-testing",
    ]),
    // L6: ecosystem libraries (ecosystem build plan §3).
    // Social sign-in: links provider accounts to navree-auth users and
    // issues their tokens.
    rule("navree-social", 6, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto",
        "navree-cache", "navree-events", "navree-http-client", "navree-db", "navree-audit",
        "navree-auth",
    ]),
    // Webhooks: signed outgoing deliveries through the queue (a job
    // dispatcher from navree-contracts) and the HTTP client, endpoints in the
    // database, routes scoped to the request's workspace.
    rule("navree-webhooks", 6, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto",
        "navree-events", "navree-http-client", "navree-db", "navree-queue", "navree-access",
        "navree-workspace",
    ]),
    // OAuth 2.1 and OpenID Connect provider: clients, grants and personal
    // access tokens for navree-auth users, scopes mapped to navree-access
    // permissions, workspace-limited clients.
    rule("navree-oauth", 6, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto",
        "navree-db", "navree-access", "navree-auth", "navree-workspace",
        // `navree_oauth::testing` drives the flows with navree-testing's TestClient.
        "navree-testing",
    ]),
    // Feature flags: user ids come from the request's `Principal`; navree-auth
    // only gives `From<&User> for FlagScope` (the crate's default `auth`
    // feature).
    rule("navree-flags", 6, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-cache",
        "navree-db", "navree-events", "navree-audit", "navree-access", "navree-auth",
        "navree-workspace",
    ]),
    // GraphQL: `#[resolver]` fields with route-style access, per-request
    // units of work and dataloaders, subscriptions over WebSockets.
    rule("navree-graphql", 6, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-events",
        "navree-db", "navree-validation", "navree-access", "navree-auth", "navree-workspace",
    ]),
    // gRPC: `#[grpc_service]` (navree-macros) expands into its call pipeline;
    // navree-testing backs `navree_grpc::testing::TestChannel`.
    rule("navree-grpc", 6, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-macros", "navree-http",
        "navree-testing", "navree-db", "navree-validation",
    ]),
    // L7: packages built on other packages.
    // Billing: Stripe and Paddle over navree-http-client, billables from
    // navree-auth (users) and navree-workspace (workspaces).
    rule("navree-billing", 7, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-cache",
        "navree-events", "navree-http-client", "navree-db", "navree-queue", "navree-schedule",
        "navree-access", "navree-auth", "navree-workspace",
    ]),
    // L8: entry points.
    rule("navree", 8, RUNTIME_CRATES),
    // The CLI uses no runtime crate; navree-dock runs `navree dock`.
    rule("navree-cli", 8, &["navree-dock"]),
    // L9 and up (apps): anything below, except the CLI binary.
    // The CRM (plan "Wave 6 layout"): its crates use only the `navree` facade.
    // crm-core is shared; crm-testkit boots it for tests (a dev-dependency
    // of the crates above it); the feature crates build on crm-core; the
    // crm-api binary assembles them.
    rule("crm-core", 9, &["navree"]),
    rule("crm-testkit", 10, &["navree", "crm-core"]),
    rule("crm-users", 11, &["navree", "crm-core"]),
    rule("crm-companies", 11, &["navree", "crm-core"]),
    rule("crm-deals", 11, &["navree", "crm-core"]),
    rule("crm-activities", 11, &["navree", "crm-core"]),
    rule("crm-api", 12, CRM_API_DEPENDENCIES),
];

/// What the crm-api binary may depend on: the facade and the CRM crates.
const CRM_API_DEPENDENCIES: &[&str] = &[
    "navree",
    "crm-core",
    "crm-users",
    "crm-companies",
    "crm-deals",
    "crm-activities",
];

/// Every runtime crate (layers 0 to 7) plus navree-macros and navree-dock:
/// what the facade may re-export and what navree-macros tests may
/// dev-depend on.
const RUNTIME_CRATES: &[&str] = &[
    "navree-kernel",
    "navree-macros",
    "navree-dock",
    "navree-contracts",
    "navree-config",
    "navree-telemetry",
    "navree-http",
    "navree-log",
    "navree-crypto",
    "navree-events",
    "navree-view",
    "navree-cache",
    "navree-http-client",
    "navree-db",
    "navree-testing",
    "navree-observe",
    "navree-api",
    "navree-mail",
    "navree-realtime",
    "navree-storage",
    "navree-session",
    "navree-validation",
    "navree-queue",
    "navree-audit",
    "navree-access",
    "navree-schedule",
    "navree-notify",
    "navree-auth",
    "navree-workspace",
    "navree-social",
    "navree-webhooks",
    "navree-oauth",
    "navree-flags",
    "navree-graphql",
    "navree-grpc",
    "navree-billing",
];

/// Cargo dependency kind of an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Normal,
    Dev,
    Build,
}

/// A dependency edge between two navree crates.
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

/// Every dependency edge from a workspace package to a navree crate or to
/// another workspace package.
fn navree_edges(metadata: &Value) -> Vec<Edge> {
    let members = workspace_crates(metadata);
    let is_internal =
        |name: &str| name == "navree" || name.starts_with("navree-") || members.contains(name);
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
        // A crate's tests may turn on its own features (navree-workspace's
        // `testing`) through a dev-dependency on itself.
        if from == to {
            return None;
        }
        // Tests may use any navree crate in a lower layer (spec §3); this is how
        // navree-testing (L3) serves every crate in layer 4 and above.
        if to_rule.layer < from_rule.layer {
            return None;
        }
        // Proc-macro tests expand against the runtime crates they target.
        if from == "navree-macros" && RUNTIME_CRATES.contains(&to.as_str()) && to != "navree-macros"
        {
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
    let edges = navree_edges(&workspace_metadata());
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

/// The §3 table allows one same-layer edge (navree-config -> navree-contracts, both
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
fn navree_edges_are_read_from_cargo_metadata_with_their_kind() {
    let metadata = json!({
        "packages": [
            {
                "name": "navree-db",
                "dependencies": [
                    { "name": "navree-http", "kind": null, "rename": null },
                    { "name": "sea-orm", "kind": null, "rename": null },
                    { "name": "navree-kernel", "kind": "dev", "rename": "kernel" },
                    { "name": "navree-config", "kind": "build", "rename": null }
                ]
            },
            {
                "name": "some-app",
                "dependencies": [
                    { "name": "navree-http", "kind": null, "rename": null },
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
        navree_edges(&metadata),
        vec![
            edge("navree-db", "navree-http", Kind::Normal),
            edge("navree-db", "navree-kernel", Kind::Dev),
            edge("navree-db", "navree-config", Kind::Build),
            edge("some-app", "navree-http", Kind::Normal),
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
        edge("navree-contracts", "navree-kernel", Kind::Normal),
        edge("navree-auth", "navree-session", Kind::Normal),
        // `impl navree_access::Subject for navree_auth::user::Entity`.
        edge("navree-auth", "navree-access", Kind::Normal),
        edge("navree", "navree-auth", Kind::Normal),
        edge("navree", "navree-macros", Kind::Normal),
        edge("crm-api", "navree", Kind::Normal),
        edge("navree-db", "navree-http", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn upward_normal_edge_is_reported() {
    let found = violations(&[edge("navree-kernel", "navree-http", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("navree-kernel") && found[0].contains("navree-http"));
}

#[test]
fn sideways_edge_within_a_layer_is_reported() {
    let found = violations(&[edge("navree-http", "navree-crypto", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn forbidden_dev_and_build_edges_are_reported() {
    let found = violations(&[
        edge("navree-http", "navree-db", Kind::Dev),
        edge("navree-queue", "navree-http", Kind::Build),
    ]);
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn navree_testing_is_an_allowed_dev_dependency_from_layer_4_up() {
    let edges = [
        edge("navree-session", "navree-testing", Kind::Dev),
        edge("navree-auth", "navree-testing", Kind::Dev),
        edge("crm-api", "navree-testing", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn dev_dependency_on_any_lower_layer_crate_is_allowed() {
    // Spec §3: tests may boot lower-layer modules (MailModule needs ViewModule).
    let edges = [
        edge("navree-notify", "navree-view", Kind::Dev),
        edge("navree-auth", "navree-view", Kind::Dev),
        edge("navree-session", "navree-cache", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn dev_dependency_within_the_same_layer_or_upwards_is_reported() {
    let found = violations(&[
        edge("navree-notify", "navree-queue", Kind::Dev),
        edge("navree-view", "navree-http", Kind::Dev),
        edge("navree-session", "navree-auth", Kind::Dev),
    ]);
    assert_eq!(found.len(), 3, "{found:?}");
}

#[test]
fn navree_testing_is_not_a_normal_dependency_of_layer_4() {
    let found = violations(&[edge("navree-session", "navree-testing", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn navree_testing_is_not_a_dev_dependency_below_layer_4() {
    let found = violations(&[edge("navree-db", "navree-testing", Kind::Dev)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn a_crate_may_dev_depend_on_itself_only() {
    // navree-workspace's tests turn on its own `testing` feature.
    assert_eq!(
        violations(&[edge("navree-workspace", "navree-workspace", Kind::Dev)]),
        Vec::<String>::new()
    );
    let found = violations(&[edge("navree-workspace", "navree-workspace", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn navree_macros_may_dev_depend_on_runtime_crates_only() {
    assert_eq!(
        violations(&[
            edge("navree-macros", "navree-kernel", Kind::Dev),
            edge("navree-macros", "navree-http", Kind::Dev),
            edge("navree-macros", "navree-access", Kind::Dev),
        ]),
        Vec::<String>::new()
    );
    let found = violations(&[
        edge("navree-macros", "navree-http", Kind::Normal),
        edge("navree-macros", "navree", Kind::Dev),
        edge("navree-macros", "navree-cli", Kind::Dev),
    ]);
    assert_eq!(found.len(), 3, "{found:?}");
}

#[test]
fn navree_cli_depends_on_navree_dock_only() {
    let found = violations(&[edge("navree-cli", "navree-kernel", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        violations(&[edge("navree-cli", "navree-dock", Kind::Normal)]),
        Vec::<String>::new()
    );
    let found = violations(&[edge("navree-dock", "navree-config", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn unknown_navree_crates_are_reported() {
    let found = violations(&[
        edge("navree-redis", "navree-kernel", Kind::Normal),
        edge("navree-http", "navree-redis", Kind::Normal),
    ]);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found.iter().all(|m| m.contains("navree-redis")),
        "{found:?}"
    );
}
