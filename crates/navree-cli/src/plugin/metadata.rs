//! Reading `cargo metadata --format-version 1` and building captured Cargo
//! invocations.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::forward::Invocation;
use crate::plugin::manifest::shown;

/// What `cargo metadata` says. With no root package (a virtual workspace where
/// `resolve.root` is null) the root is `None`, and `dependency`, `dependency_at`,
/// `plugin_dependencies` and `navree` find and list nothing.
#[derive(Clone, Debug)]
pub(crate) struct Metadata {
    packages: Vec<Package>,
    root_id: Option<String>,
    direct_normal_deps: BTreeSet<String>,
    resolved_ids: BTreeSet<String>,
    resolved_features: BTreeMap<String, BTreeSet<String>>,
}

/// One package entry from `cargo metadata`.
#[derive(Clone, Debug)]
pub(crate) struct Package {
    pub id: String,
    pub name: String,
    pub version: String,
    /// `registry+..`, `git+..`, or `None` for a workspace or path package.
    pub source: Option<String>,
    pub manifest_dir: PathBuf,
    /// The library target's name (`targets` entry with a Cargo library kind).
    pub lib_name: Option<String>,
    /// `metadata.navree.plugin`, as written in the package's `Cargo.toml`.
    pub plugin_table: Option<Value>,
    pub features: BTreeMap<String, Vec<String>>,
}

fn canonical_name(name: &str) -> String {
    name.replace('-', "_")
}

fn is_lib_target_kind(kind: &str) -> bool {
    matches!(
        kind,
        "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
    )
}

fn extract_lib_name(pkg: &Value) -> Option<String> {
    let targets = pkg.get("targets")?.as_array()?;
    for target in targets {
        let is_lib = target
            .get("kind")
            .and_then(Value::as_array)
            .is_some_and(|kinds| {
                kinds
                    .iter()
                    .filter_map(Value::as_str)
                    .any(is_lib_target_kind)
            });
        if is_lib {
            return target
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
    }
    None
}

fn extract_features(pkg: &Value) -> Result<BTreeMap<String, Vec<String>>, String> {
    let Some(val) = pkg.get("features") else {
        return Ok(BTreeMap::new());
    };
    if val.is_null() {
        return Ok(BTreeMap::new());
    }
    let obj = val
        .as_object()
        .ok_or_else(|| "package `features` must be an object".to_owned())?;
    let mut out = BTreeMap::new();
    for (key, list) in obj {
        let arr = list
            .as_array()
            .ok_or_else(|| format!("feature `{key}` must be an array"))?;
        let mut items = Vec::with_capacity(arr.len());
        for item in arr {
            let s = item
                .as_str()
                .ok_or_else(|| format!("feature `{key}` items must be strings"))?;
            items.push(s.to_owned());
        }
        out.insert(key.clone(), items);
    }
    Ok(out)
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut parts = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(parts.last(), Some(Component::Normal(_))) {
                    parts.pop();
                } else {
                    parts.push(comp);
                }
            }
            other => parts.push(other),
        }
    }
    parts.iter().collect()
}

fn same_dir(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(cl), Ok(cr)) => cl == cr,
        _ => normalize_lexical(left) == normalize_lexical(right),
    }
}

impl Metadata {
    /// Parses `cargo metadata --format-version 1` JSON output. A null `resolve.root`
    /// parses cleanly so virtual workspaces do not fail `navree check`.
    pub(crate) fn parse(json: &str) -> Result<Metadata, String> {
        let doc: Value = serde_json::from_str(json).map_err(|err| err.to_string())?;
        let root_obj = doc
            .as_object()
            .ok_or_else(|| "cargo metadata must be a JSON object".to_owned())?;

        let raw_packages = root_obj
            .get("packages")
            .and_then(Value::as_array)
            .ok_or_else(|| "cargo metadata lacks `packages` array".to_owned())?;

        let mut packages = Vec::with_capacity(raw_packages.len());
        for pkg in raw_packages {
            let id = pkg
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "package lacks `id`".to_owned())?
                .to_owned();
            let name = pkg
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "package lacks `name`".to_owned())?
                .to_owned();
            let version = pkg
                .get("version")
                .and_then(Value::as_str)
                .ok_or_else(|| "package lacks `version`".to_owned())?
                .to_owned();
            let source = match pkg.get("source") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.clone()),
                Some(_) => return Err(format!("package `{name}` has non-string `source`")),
            };
            let manifest_dir = match pkg.get("manifest_path").and_then(Value::as_str) {
                Some(manifest_path) => Path::new(manifest_path)
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf(),
                None => PathBuf::new(),
            };
            let lib_name = extract_lib_name(pkg);
            let plugin_table = pkg
                .get("metadata")
                .and_then(|m| m.get("navree"))
                .and_then(|n| n.get("plugin"))
                .filter(|v| !v.is_null())
                .cloned();
            let features = extract_features(pkg)?;

            packages.push(Package {
                id,
                name,
                version,
                source,
                manifest_dir,
                lib_name,
                plugin_table,
                features,
            });
        }

        let resolve = root_obj
            .get("resolve")
            .and_then(Value::as_object)
            .ok_or_else(|| "cargo metadata lacks `resolve` object".to_owned())?;

        let root_id = match resolve.get("root") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => return Err("`resolve.root` must be a string or null".to_owned()),
        };

        let nodes = resolve
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| "`resolve.nodes` must be an array".to_owned())?;

        let mut direct_normal_deps = BTreeSet::new();
        let mut resolved_ids = BTreeSet::new();
        let mut resolved_features = BTreeMap::new();

        for node in nodes {
            let node_id = node
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "resolve node lacks `id`".to_owned())?;
            resolved_ids.insert(node_id.to_owned());

            let mut feat_set = BTreeSet::new();
            if let Some(feats) = node.get("features").and_then(Value::as_array) {
                for feat in feats {
                    if let Some(s) = feat.as_str() {
                        feat_set.insert(s.to_owned());
                    }
                }
            }
            resolved_features.insert(node_id.to_owned(), feat_set);

            if root_id.as_deref() == Some(node_id) {
                if let Some(deps) = node.get("deps").and_then(Value::as_array) {
                    for dep in deps {
                        let Some(pkg_id) = dep.get("pkg").and_then(Value::as_str) else {
                            continue;
                        };
                        let is_normal = match dep.get("dep_kinds").and_then(Value::as_array) {
                            None => true,
                            Some(kinds) if kinds.is_empty() => true,
                            Some(kinds) => kinds.iter().any(|dk| {
                                dk["kind"].is_null() || dk["kind"].as_str() == Some("normal")
                            }),
                        };
                        if is_normal {
                            direct_normal_deps.insert(pkg_id.to_owned());
                        }
                    }
                } else if let Some(deps) = node.get("dependencies").and_then(Value::as_array) {
                    for dep in deps {
                        if let Some(pkg_id) = dep.as_str() {
                            direct_normal_deps.insert(pkg_id.to_owned());
                        }
                    }
                }
            }
        }

        Ok(Metadata {
            packages,
            root_id,
            direct_normal_deps,
            resolved_ids,
            resolved_features,
        })
    }

    /// The root package's folder; `None` for a virtual workspace.
    pub(crate) fn root_dir(&self) -> Option<&Path> {
        let root_id = self.root_id.as_deref()?;
        self.packages
            .iter()
            .find(|pkg| pkg.id == root_id)
            .map(|pkg| pkg.manifest_dir.as_path())
    }

    /// Every package in `packages`.
    pub(crate) fn packages(&self) -> &[Package] {
        &self.packages
    }

    /// Every resolved package when there is a root package (`None`/empty for a virtual workspace).
    pub(crate) fn resolved_packages(&self) -> Vec<&Package> {
        if self.root_id.is_none() {
            return Vec::new();
        }
        self.packages
            .iter()
            .filter(|pkg| self.resolved_ids.contains(&pkg.id))
            .collect()
    }

    /// A direct normal dependency of the root by name (`-` and `_` are one), from a registry or git source.
    pub(crate) fn dependency(&self, name: &str) -> Option<&Package> {
        let wanted = canonical_name(name);
        self.packages.iter().find(|pkg| {
            self.direct_normal_deps.contains(&pkg.id)
                && pkg.source.is_some()
                && canonical_name(&pkg.name) == wanted
        })
    }

    /// A direct normal dependency whose manifest folder is `dir` (both canonicalised).
    pub(crate) fn dependency_at(&self, dir: &Path) -> Option<&Package> {
        self.packages.iter().find(|pkg| {
            self.direct_normal_deps.contains(&pkg.id) && same_dir(&pkg.manifest_dir, dir)
        })
    }

    /// The `navree` facade package for this app; `None` for a virtual workspace.
    pub(crate) fn navree(&self) -> Option<&Package> {
        self.root_id.as_ref()?;
        self.packages
            .iter()
            .find(|pkg| self.direct_normal_deps.contains(&pkg.id) && pkg.name == "navree")
            .or_else(|| self.packages.iter().find(|pkg| pkg.name == "navree"))
    }

    /// Resolved features enabled on `navree` (`resolve.nodes`).
    pub(crate) fn navree_enabled(&self) -> BTreeSet<String> {
        let Some(navree) = self.navree() else {
            return BTreeSet::new();
        };
        self.resolved_features
            .get(&navree.id)
            .cloned()
            .unwrap_or_default()
    }

    /// Direct normal dependencies whose metadata has `navree.plugin`, sorted by package name.
    pub(crate) fn plugin_dependencies(&self) -> Vec<&Package> {
        let mut found: Vec<&Package> = self
            .packages
            .iter()
            .filter(|pkg| self.direct_normal_deps.contains(&pkg.id) && pkg.plugin_table.is_some())
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        found
    }
}

/// Builds a `cargo ARGS` invocation with `CARGO_TERM_COLOR=never` set in its environment.
pub(crate) fn cargo(args: &[&str]) -> Invocation {
    Invocation::new("cargo", args.iter().copied())
        .with_env(vec![("CARGO_TERM_COLOR".to_owned(), "never".to_owned())])
}

/// Builds `cargo metadata --format-version 1` with colour disabled.
pub(crate) fn invocation() -> Invocation {
    cargo(&["metadata", "--format-version", "1"])
}

/// Last line of `standard_error` starting with `error:`, else the last non-empty line,
/// passed through `shown`; empty text returns an empty string.
pub(crate) fn cargo_error_line(standard_error: &str) -> String {
    let non_empty: Vec<&str> = standard_error
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let chosen = non_empty
        .iter()
        .rev()
        .copied()
        .find(|line| line.starts_with("error:"))
        .or_else(|| non_empty.last().copied());
    match chosen {
        Some(line) => shown(line),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn metadata_finds_the_package_cargo_added_by_source_and_folder() {
        let temp = tempfile::tempdir().expect("tempdir");
        let app_dir = temp.path();
        let path_plugin_dir = app_dir.join("plugins/khata");
        std::fs::create_dir_all(&path_plugin_dir).expect("create plugins/khata");

        let raw = json!({
            "packages": [
                {
                    "id": "path+file:///kesar#0.1.0",
                    "name": "kesar",
                    "version": "0.1.0",
                    "source": null,
                    "manifest_path": app_dir.join("Cargo.toml"),
                    "targets": [{ "kind": ["bin"], "name": "kesar" }],
                    "features": {},
                    "metadata": null
                },
                {
                    "id": "registry+https://github.com/rust-lang/crates.io-index#navree@1.4.0",
                    "name": "navree",
                    "version": "1.4.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/cache/navree-1.4.0/Cargo.toml",
                    "targets": [{ "kind": ["lib"], "name": "navree" }],
                    "features": {
                        "postgres": [],
                        "ui": [],
                        "workspace": []
                    },
                    "metadata": null
                },
                {
                    "id": "registry+https://github.com/rust-lang/crates.io-index#navree-plugin-khata@1.0.0",
                    "name": "navree-plugin-khata",
                    "version": "1.0.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/cache/navree-plugin-khata-1.0.0/Cargo.toml",
                    "targets": [{ "kind": ["lib"], "name": "navree_plugin_khata" }],
                    "features": {},
                    "metadata": {
                        "navree": {
                            "plugin": { "module": "RegistryKhata::from_config()" }
                        }
                    }
                },
                {
                    "id": "path+file:///kesar/plugins/khata#0.1.0",
                    "name": "navree-plugin-khata",
                    "version": "0.1.0",
                    "source": null,
                    "manifest_path": path_plugin_dir.join("Cargo.toml"),
                    "targets": [{ "kind": ["lib"], "name": "navree_plugin_khata" }],
                    "features": {},
                    "metadata": {
                        "navree": {
                            "plugin": { "module": "LocalKhata::from_config()" }
                        }
                    }
                },
                {
                    "id": "registry+https://github.com/rust-lang/crates.io-index#reqwest@0.12.0",
                    "name": "reqwest",
                    "version": "0.12.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/cache/reqwest-0.12.0/Cargo.toml",
                    "targets": [{ "kind": ["bin"], "name": "reqwest-cli" }],
                    "features": {},
                    "metadata": null
                },
                {
                    "id": "registry+https://github.com/rust-lang/crates.io-index#navree-plugin-devonly@0.2.0",
                    "name": "navree-plugin-devonly",
                    "version": "0.2.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/cache/navree-plugin-devonly-0.2.0/Cargo.toml",
                    "targets": [{ "kind": ["lib"], "name": "navree_plugin_devonly" }],
                    "features": {},
                    "metadata": {
                        "navree": {
                            "plugin": { "module": "DevOnly::new()" }
                        }
                    }
                }
            ],
            "resolve": {
                "root": "path+file:///kesar#0.1.0",
                "nodes": [
                    {
                        "id": "path+file:///kesar#0.1.0",
                        "deps": [
                            {
                                "name": "navree",
                                "pkg": "registry+https://github.com/rust-lang/crates.io-index#navree@1.4.0",
                                "dep_kinds": [{ "kind": null, "target": null }]
                            },
                            {
                                "name": "navree_plugin_khata",
                                "pkg": "registry+https://github.com/rust-lang/crates.io-index#navree-plugin-khata@1.0.0",
                                "dep_kinds": [{ "kind": null, "target": null }]
                            },
                            {
                                "name": "local_khata",
                                "pkg": "path+file:///kesar/plugins/khata#0.1.0",
                                "dep_kinds": [{ "kind": null, "target": null }]
                            },
                            {
                                "name": "reqwest",
                                "pkg": "registry+https://github.com/rust-lang/crates.io-index#reqwest@0.12.0",
                                "dep_kinds": [{ "kind": null, "target": null }]
                            },
                            {
                                "name": "navree_plugin_devonly",
                                "pkg": "registry+https://github.com/rust-lang/crates.io-index#navree-plugin-devonly@0.2.0",
                                "dep_kinds": [{ "kind": "dev", "target": null }]
                            }
                        ],
                        "features": []
                    },
                    {
                        "id": "registry+https://github.com/rust-lang/crates.io-index#navree@1.4.0",
                        "deps": [],
                        "features": ["postgres", "ui"]
                    }
                ]
            }
        });

        let metadata = Metadata::parse(&raw.to_string()).expect("metadata parses");
        assert_eq!(metadata.root_dir(), Some(app_dir));

        let reg = metadata
            .dependency("navree-plugin-khata")
            .expect("registry khata found");
        assert_eq!(reg.version, "1.0.0");
        assert_eq!(
            reg.plugin_table.as_ref().unwrap()["module"],
            "RegistryKhata::from_config()"
        );
        assert_eq!(
            metadata
                .dependency("navree_plugin_khata")
                .map(|p| p.id.as_str()),
            Some(reg.id.as_str())
        );

        let local = metadata
            .dependency_at(&path_plugin_dir)
            .expect("path khata found");
        assert_eq!(local.version, "0.1.0");
        assert_eq!(
            local.plugin_table.as_ref().unwrap()["module"],
            "LocalKhata::from_config()"
        );

        assert_eq!(
            metadata.navree_enabled(),
            ["postgres".to_owned(), "ui".to_owned()]
                .into_iter()
                .collect()
        );

        let plugin_deps = metadata.plugin_dependencies();
        assert_eq!(plugin_deps.len(), 2);
        assert!(plugin_deps.iter().all(|p| p.name == "navree-plugin-khata"));

        let reqwest = metadata.dependency("reqwest").expect("reqwest found");
        assert_eq!(reqwest.lib_name, None);
    }

    #[test]
    fn metadata_with_no_root_package_lists_nothing() {
        let raw = json!({
            "packages": [
                {
                    "id": "path+file:///ws/crates/svc#0.1.0",
                    "name": "svc",
                    "version": "0.1.0",
                    "source": null,
                    "manifest_path": "/ws/crates/svc/Cargo.toml",
                    "targets": [{ "kind": ["bin"], "name": "svc" }],
                    "features": {},
                    "metadata": null
                },
                {
                    "id": "registry+https://github.com/rust-lang/crates.io-index#navree@1.4.0",
                    "name": "navree",
                    "version": "1.4.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/cache/navree-1.4.0/Cargo.toml",
                    "targets": [{ "kind": ["lib"], "name": "navree" }],
                    "features": { "ui": [] },
                    "metadata": null
                },
                {
                    "id": "registry+https://github.com/rust-lang/crates.io-index#navree-plugin-notices@1.0.0",
                    "name": "navree-plugin-notices",
                    "version": "1.0.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/cache/navree-plugin-notices-1.0.0/Cargo.toml",
                    "targets": [{ "kind": ["lib"], "name": "navree_plugin_notices" }],
                    "features": {},
                    "metadata": {
                        "navree": {
                            "plugin": { "module": "NoticesModule::from_config()" }
                        }
                    }
                }
            ],
            "resolve": {
                "root": null,
                "nodes": [
                    {
                        "id": "path+file:///ws/crates/svc#0.1.0",
                        "deps": [
                            {
                                "name": "navree",
                                "pkg": "registry+https://github.com/rust-lang/crates.io-index#navree@1.4.0",
                                "dep_kinds": [{ "kind": null, "target": null }]
                            },
                            {
                                "name": "navree_plugin_notices",
                                "pkg": "registry+https://github.com/rust-lang/crates.io-index#navree-plugin-notices@1.0.0",
                                "dep_kinds": [{ "kind": null, "target": null }]
                            }
                        ],
                        "features": []
                    },
                    {
                        "id": "registry+https://github.com/rust-lang/crates.io-index#navree@1.4.0",
                        "deps": [],
                        "features": ["ui"]
                    }
                ]
            }
        });

        let metadata = Metadata::parse(&raw.to_string()).expect("virtual workspace parses");
        assert_eq!(metadata.root_dir(), None);
        assert!(metadata.dependency("navree-plugin-notices").is_none());
        assert!(
            metadata
                .dependency_at(Path::new("/ws/crates/svc"))
                .is_none()
        );
        assert!(metadata.plugin_dependencies().is_empty());
        assert!(metadata.navree().is_none());
        assert!(metadata.navree_enabled().is_empty());
        assert!(metadata.resolved_packages().is_empty());
    }

    #[test]
    fn metadata_gives_each_packages_library_name() {
        let raw = json!({
            "packages": [
                {
                    "id": "app#0.1.0",
                    "name": "kesar",
                    "version": "0.1.0",
                    "source": null,
                    "manifest_path": "/kesar/Cargo.toml",
                    "targets": [{ "kind": ["bin"], "name": "kesar" }],
                    "features": {},
                    "metadata": null
                },
                {
                    "id": "khata#1.0.0",
                    "name": "navree-plugin-khata",
                    "version": "1.0.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/cache/khata/Cargo.toml",
                    "targets": [
                        { "kind": ["custom-build"], "name": "build-script-build" },
                        { "kind": ["rlib"], "name": "navree_plugin_khata" }
                    ],
                    "features": {},
                    "metadata": null
                },
                {
                    "id": "macros#1.0.0",
                    "name": "khata-macros",
                    "version": "1.0.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/cache/khata-macros/Cargo.toml",
                    "targets": [{ "kind": ["proc-macro"], "name": "khata_macros" }],
                    "features": {},
                    "metadata": null
                },
                {
                    "id": "impostor#0.1.0",
                    "name": "ludhiana-ledger",
                    "version": "0.1.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": "/cache/ludhiana-ledger/Cargo.toml",
                    "targets": [{ "kind": ["lib"], "name": "navree_billing" }],
                    "features": {},
                    "metadata": null
                }
            ],
            "resolve": {
                "root": "app#0.1.0",
                "nodes": [
                    { "id": "app#0.1.0", "deps": [], "features": [] },
                    { "id": "khata#1.0.0", "deps": [], "features": [] },
                    { "id": "macros#1.0.0", "deps": [], "features": [] },
                    { "id": "impostor#0.1.0", "deps": [], "features": [] }
                ]
            }
        });

        let metadata = Metadata::parse(&raw.to_string()).expect("metadata parses");
        let resolved: BTreeMap<&str, Option<&str>> = metadata
            .resolved_packages()
            .into_iter()
            .map(|pkg| (pkg.name.as_str(), pkg.lib_name.as_deref()))
            .collect();
        assert_eq!(resolved.get("kesar"), Some(&None));
        assert_eq!(
            resolved.get("navree-plugin-khata"),
            Some(&Some("navree_plugin_khata"))
        );
        assert_eq!(resolved.get("khata-macros"), Some(&Some("khata_macros")));
        assert_eq!(
            resolved.get("ludhiana-ledger"),
            Some(&Some("navree_billing"))
        );
    }

    #[test]
    fn a_cargo_error_line_is_the_last_error_line_shown_escaped() {
        assert_eq!(
            cargo_error_line("warning: a\nerror: could not find `khata` in registry\nnote: b\n"),
            "error: could not find `khata` in registry"
        );
        assert_eq!(
            cargo_error_line("updating crates.io index\nnetwork unreachable\n"),
            "network unreachable"
        );
        assert_eq!(
            cargo_error_line("error: bad \u{1b}[31mcolour\n"),
            "error: bad \\x1b[31mcolour"
        );
        let long = format!("error: {}", "a".repeat(300));
        let cut = cargo_error_line(&long);
        assert_eq!(cut, format!("error: {}...", "a".repeat(73)));
        assert_eq!(cargo_error_line(""), "");
        assert_eq!(cargo_error_line("   \n\t\n"), "");
    }

    #[test]
    fn captured_cargo_calls_run_without_colour() {
        let add = cargo(&["add", "x"]);
        assert_eq!(add.to_string(), "cargo add x");
        assert_eq!(
            add.env,
            vec![("CARGO_TERM_COLOR".to_owned(), "never".to_owned())]
        );
        assert_eq!(
            invocation().to_string(),
            "cargo metadata --format-version 1"
        );
    }
}
