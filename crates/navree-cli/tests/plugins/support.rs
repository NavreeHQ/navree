//! Scripted `Runner` and temporary app builders for the plugin command tests.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use navree_cli::forward::{Invocation, Runner};
use navree_cli::plugin::index::Index;
use serde_json::{Value, json};

/// Converts a string slice, owned string, or optional string slice into an optional
/// owned string so builder calls in sibling test files take either form cleanly.
pub trait OptStr {
    fn into_opt_str(self) -> Option<String>;
}

impl OptStr for &str {
    fn into_opt_str(self) -> Option<String> {
        Some(self.to_owned())
    }
}

impl OptStr for String {
    fn into_opt_str(self) -> Option<String> {
        Some(self)
    }
}

impl OptStr for Option<&str> {
    fn into_opt_str(self) -> Option<String> {
        self.map(str::to_owned)
    }
}

#[derive(Clone, Debug)]
struct RegisteredPackage {
    name: String,
    version: String,
    metadata: Option<Value>,
    lib_name: Option<String>,
}

#[derive(Clone, Debug)]
struct RegisteredPathPackage {
    dir: PathBuf,
    name: String,
    version: String,
    metadata: Option<Value>,
    lib_name: Option<String>,
}

/// Scripted Cargo and app runner for `plugin:add`, `plugin:remove`, `plugin:list` and `check`.
#[derive(Default)]
pub struct ScriptedCargo {
    /// Every invocation's `to_string()` in call order.
    pub ran: Vec<String>,
    /// Working directory of each recorded invocation.
    pub ran_in: Vec<PathBuf>,
    /// Environment variables set on each recorded invocation.
    pub envs: Vec<Vec<(String, String)>>,
    packages: Vec<RegisteredPackage>,
    transitive_packages: Vec<RegisteredPackage>,
    path_packages: Vec<RegisteredPathPackage>,
    resolved_features: BTreeSet<String>,
    navree_feature_edges: BTreeMap<String, Vec<String>>,
    failures: Vec<(String, String)>,
    hooks: Vec<(String, Box<dyn FnMut()>)>,
    plugins_json_out: Option<String>,
}

fn canonical_name(name: &str) -> String {
    name.replace('-', "_")
}

fn parse_plugin_metadata(table_toml: Option<String>) -> Option<Value> {
    let raw = table_toml?;
    let parsed: toml::Value = toml::from_str(&raw).expect("scripted plugin table TOML must parse");
    if let Some(metadata) = parsed.get("package").and_then(|p| p.get("metadata")) {
        return Some(serde_json::to_value(metadata).expect("metadata converts to JSON"));
    }
    if parsed.get("navree").is_some() {
        return Some(serde_json::to_value(parsed).expect("metadata converts to JSON"));
    }
    let plugin = serde_json::to_value(parsed).expect("plugin table converts to JSON");
    Some(json!({ "navree": { "plugin": plugin } }))
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

fn find_workspace_lock_dir(start: &Path) -> PathBuf {
    for ancestor in start.ancestors() {
        let cargo_toml = ancestor.join("Cargo.toml");
        if let Ok(text) = std::fs::read_to_string(&cargo_toml) {
            if let Ok(doc) = toml::from_str::<toml::Value>(&text) {
                if doc.get("workspace").is_some() {
                    return ancestor.to_path_buf();
                }
            }
        }
    }
    start.to_path_buf()
}

fn insert_dependency_line(cargo_toml: &str, dep_name: &str, rhs: &str) -> String {
    let newline = if cargo_toml.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let normalized = cargo_toml.replace("\r\n", "\n");
    let mut lines: Vec<String> = normalized.lines().map(str::to_owned).collect();
    let had_trailing_newline = normalized.ends_with('\n');

    let wanted = canonical_name(dep_name);
    let new_line = format!("{dep_name} = {rhs}");

    let mut in_deps = false;
    let mut deps_header_idx = None;
    let mut last_entry_idx = None;
    let mut existing_idx = None;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_deps {
                break;
            }
            if trimmed == "[dependencies]" {
                in_deps = true;
                deps_header_idx = Some(idx);
            }
            continue;
        }
        if in_deps {
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                last_entry_idx = Some(idx);
                if let Some((key, _)) = trimmed.split_once('=') {
                    if canonical_name(key.trim()) == wanted {
                        existing_idx = Some(idx);
                    }
                }
            }
        }
    }

    if let Some(idx) = existing_idx {
        lines[idx] = new_line;
    } else if let Some(header_idx) = deps_header_idx {
        let insert_at = last_entry_idx.unwrap_or(header_idx) + 1;
        lines.insert(insert_at, new_line);
    } else {
        if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
            lines.push(String::new());
        }
        lines.push("[dependencies]".to_owned());
        lines.push(new_line);
    }

    let mut out = lines.join(newline);
    if had_trailing_newline || cargo_toml.is_empty() {
        out.push_str(newline);
    }
    out
}

fn remove_dependency_line(cargo_toml: &str, dep_name: &str) -> Option<String> {
    let newline = if cargo_toml.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let normalized = cargo_toml.replace("\r\n", "\n");
    let mut lines: Vec<String> = normalized.lines().map(str::to_owned).collect();
    let had_trailing_newline = normalized.ends_with('\n');
    let wanted = canonical_name(dep_name);

    let mut in_deps = false;
    let mut remove_idx = None;
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_deps {
                break;
            }
            if trimmed == "[dependencies]" {
                in_deps = true;
            }
            continue;
        }
        if in_deps && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some((key, _)) = trimmed.split_once('=') {
                if canonical_name(key.trim()) == wanted {
                    remove_idx = Some(idx);
                    break;
                }
            }
        }
    }

    let idx = remove_idx?;
    lines.remove(idx);
    let mut out = lines.join(newline);
    if had_trailing_newline {
        out.push_str(newline);
    }
    Some(out)
}

impl ScriptedCargo {
    /// Creates an empty scripted Cargo runner.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a registry package returned by `cargo add` and `cargo metadata`.
    pub fn package(
        mut self,
        name: &str,
        version: &str,
        table_toml: impl OptStr,
        lib_name: impl OptStr,
    ) -> Self {
        self.packages.push(RegisteredPackage {
            name: name.to_owned(),
            version: version.to_owned(),
            metadata: parse_plugin_metadata(table_toml.into_opt_str()),
            lib_name: lib_name.into_opt_str(),
        });
        self
    }

    /// Registers a local `--path` package returned by `cargo add --path` and `cargo metadata`.
    pub fn path_package(
        mut self,
        dir: impl AsRef<Path>,
        name: &str,
        version: &str,
        table_toml: impl OptStr,
    ) -> Self {
        self.path_packages.push(RegisteredPathPackage {
            dir: dir.as_ref().to_path_buf(),
            name: name.to_owned(),
            version: version.to_owned(),
            metadata: parse_plugin_metadata(table_toml.into_opt_str()),
            lib_name: Some(name.replace('-', "_")),
        });
        self
    }

    /// Registers a transitive resolved package (in `resolve.nodes`, not a direct root dependency).
    pub fn transitive_package(mut self, name: &str, version: &str, lib_name: impl OptStr) -> Self {
        self.transitive_packages.push(RegisteredPackage {
            name: name.to_owned(),
            version: version.to_owned(),
            metadata: None,
            lib_name: lib_name.into_opt_str(),
        });
        self
    }

    /// Sets features resolved as enabled on `navree` in `cargo metadata`.
    pub fn resolved(mut self, features: &[&str]) -> Self {
        self.resolved_features
            .extend(features.iter().map(|s| (*s).to_owned()));
        self
    }

    /// Adds custom feature edges on the scripted `navree` package.
    pub fn navree_feature(mut self, feature: &str, implies: &[&str]) -> Self {
        self.navree_feature_edges.insert(
            feature.to_owned(),
            implies.iter().map(|s| (*s).to_owned()).collect(),
        );
        self
    }

    /// Causes invocations starting with `prefix` to fail with `stderr`.
    pub fn fail(mut self, prefix: &str, stderr: &str) -> Self {
        self.failures.push((prefix.to_owned(), stderr.to_owned()));
        self
    }

    /// Runs `closure` right before answering an invocation starting with `prefix`.
    pub fn during(mut self, prefix: &str, closure: impl FnMut() + 'static) -> Self {
        self.hooks.push((prefix.to_owned(), Box::new(closure)));
        self
    }

    /// Sets the JSON printed by `cargo run --quiet -- plugins --json`.
    pub fn plugins_json(mut self, json: &str) -> Self {
        self.plugins_json_out = Some(json.to_owned());
        self
    }

    fn dispatch(
        &mut self,
        invocation: &Invocation,
        dir: &Path,
    ) -> std::io::Result<(bool, String, String)> {
        let cmd = invocation.to_string();
        self.ran.push(cmd.clone());
        self.ran_in.push(dir.to_path_buf());
        self.envs.push(invocation.env.clone());

        let failure = self
            .failures
            .iter()
            .find(|(prefix, _)| cmd.starts_with(prefix))
            .map(|(_, stderr)| stderr.clone());

        if let Some(stderr) = failure {
            self.run_hooks(&cmd);
            return Ok((false, String::new(), stderr));
        }

        let result = if invocation.program == "cargo" {
            self.handle_cargo(&invocation.args, dir)?
        } else {
            (true, String::new(), String::new())
        };

        self.run_hooks(&cmd);
        Ok(result)
    }

    fn run_hooks(&mut self, cmd: &str) {
        for (prefix, hook) in &mut self.hooks {
            if cmd.starts_with(prefix.as_str()) {
                hook();
            }
        }
    }

    fn handle_cargo(&self, args: &[String], dir: &Path) -> std::io::Result<(bool, String, String)> {
        match args.first().map(String::as_str) {
            Some("add") => self.handle_cargo_add(&args[1..], dir),
            Some("remove") => self.handle_cargo_remove(&args[1..], dir),
            Some("metadata") => Ok((true, self.build_metadata_json(dir)?, String::new())),
            Some("run") => self.handle_cargo_run(&args[1..], dir),
            _ => Ok((true, String::new(), String::new())),
        }
    }

    fn find_registered_package(&self, name: &str) -> Option<&RegisteredPackage> {
        let wanted = canonical_name(name);
        self.packages
            .iter()
            .find(|pkg| canonical_name(&pkg.name) == wanted)
    }

    fn find_registered_package_ver(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> Option<&RegisteredPackage> {
        let wanted = canonical_name(name);
        if let Some(ver) = version
            && let Some(exact) = self
                .packages
                .iter()
                .find(|pkg| canonical_name(&pkg.name) == wanted && pkg.version == ver)
        {
            return Some(exact);
        }
        self.find_registered_package(name)
    }

    fn find_registered_path_package(
        &self,
        run_dir: &Path,
        rel_or_abs: &Path,
    ) -> Option<&RegisteredPathPackage> {
        let target = if rel_or_abs.is_absolute() {
            rel_or_abs.to_path_buf()
        } else {
            run_dir.join(rel_or_abs)
        };
        self.path_packages.iter().find(|pkg| {
            let candidate = if pkg.dir.is_absolute() {
                pkg.dir.clone()
            } else {
                run_dir.join(&pkg.dir)
            };
            same_dir(&candidate, &target)
        })
    }

    fn handle_cargo_add(
        &self,
        args: &[String],
        dir: &Path,
    ) -> std::io::Result<(bool, String, String)> {
        let cargo_toml_path = dir.join("Cargo.toml");
        let existing = std::fs::read_to_string(&cargo_toml_path)?;

        let (dep_name, rhs) = if let Some(pos) = args.iter().position(|a| a == "--path") {
            let path_arg = args.get(pos + 1).expect("--path takes a directory");
            let pkg_dir = dir.join(path_arg);
            let name =
                if let Some(reg) = self.find_registered_path_package(dir, Path::new(path_arg)) {
                    reg.name.clone()
                } else if let Ok(pkg_toml) = std::fs::read_to_string(pkg_dir.join("Cargo.toml")) {
                    let doc: toml::Value =
                        toml::from_str(&pkg_toml).expect("path package Cargo.toml parses");
                    doc["package"]["name"]
                        .as_str()
                        .expect("path package has [package] name")
                        .to_owned()
                } else {
                    args.iter()
                        .enumerate()
                        .find(|&(i, a)| i != pos && i != pos + 1 && !a.starts_with('-'))
                        .map(|(_, a)| a.replace('_', "-"))
                        .unwrap_or_else(|| "unknown-plugin".to_owned())
                };
            let slash_path = path_arg.replace('\\', "/");
            (name, format!("{{ path = \"{slash_path}\" }}"))
        } else {
            let spec = args
                .iter()
                .find(|a| !a.starts_with('-'))
                .expect("cargo add takes a package spec");
            let (raw_name, req_opt) = match spec.split_once('@') {
                Some((n, r)) => (n, Some(r)),
                None => (spec.as_str(), None),
            };
            if let Some(reg) = self.find_registered_package_ver(raw_name, req_opt) {
                (reg.name.clone(), format!("\"{}\"", reg.version))
            } else {
                let name = raw_name.replace('_', "-");
                let ver = req_opt.unwrap_or("0.1.0");
                (name, format!("\"{ver}\""))
            }
        };

        let updated = insert_dependency_line(&existing, &dep_name, &rhs);
        std::fs::write(&cargo_toml_path, updated)?;

        let lock_dir = find_workspace_lock_dir(dir);
        let lock_path = lock_dir.join("Cargo.lock");
        let mut lock_text = std::fs::read_to_string(&lock_path).unwrap_or_default();
        if !lock_text.is_empty() && !lock_text.ends_with('\n') {
            lock_text.push('\n');
        }
        lock_text.push_str(&format!("# {dep_name}\n"));
        std::fs::write(&lock_path, lock_text)?;

        Ok((true, String::new(), String::new()))
    }

    fn handle_cargo_remove(
        &self,
        args: &[String],
        dir: &Path,
    ) -> std::io::Result<(bool, String, String)> {
        let pkg = args
            .iter()
            .find(|a| !a.starts_with('-'))
            .expect("cargo remove takes a package name");
        let cargo_toml_path = dir.join("Cargo.toml");
        let existing = std::fs::read_to_string(&cargo_toml_path)?;
        let Some(updated) = remove_dependency_line(&existing, pkg) else {
            return Ok((
                false,
                String::new(),
                format!("error: the dependency `{pkg}` could not be found in `[dependencies]`"),
            ));
        };
        std::fs::write(&cargo_toml_path, updated)?;
        Ok((true, String::new(), String::new()))
    }

    fn handle_cargo_run(
        &self,
        args: &[String],
        dir: &Path,
    ) -> std::io::Result<(bool, String, String)> {
        let app_args = match args.iter().position(|a| a == "--") {
            Some(pos) => &args[pos + 1..],
            None => args,
        };
        match app_args.first().map(String::as_str) {
            Some("config:publish") => {
                if let Some(pos) = app_args.iter().position(|a| a == "--plugin") {
                    if let Some(ns) = app_args.get(pos + 1) {
                        let config_dir = dir.join("config");
                        std::fs::create_dir_all(&config_dir)?;
                        let target = config_dir.join(format!("{ns}.toml"));
                        if target.exists() {
                            return Ok((
                                true,
                                format!(
                                    "config/{ns}.toml: exists, kept (use --force to replace it)\n"
                                ),
                                String::new(),
                            ));
                        }
                        std::fs::write(&target, format!("# [{ns}]\n"))?;
                        return Ok((true, format!("wrote config/{ns}.toml\n"), String::new()));
                    }
                }
                Ok((true, String::new(), String::new()))
            }
            Some("plugins") if app_args.iter().any(|a| a == "--json") => {
                let out = self
                    .plugins_json_out
                    .clone()
                    .unwrap_or_else(|| r#"{"navree":"1.4.0","plugins":[]}"#.to_owned());
                Ok((true, out, String::new()))
            }
            _ => Ok((true, String::new(), String::new())),
        }
    }

    fn navree_features_map(&self) -> BTreeMap<String, Vec<String>> {
        let mut features = BTreeMap::new();
        let index = Index::embedded();
        for f in &index.manifest_features {
            features.insert(f.clone(), Vec::new());
        }
        for entry in &index.packages {
            features.entry(entry.feature.clone()).or_default();
        }
        for extra in [
            "default",
            "full",
            "postgres",
            "mysql",
            "sqlite",
            "mongodb",
            "schema",
            "testing",
            "ui-native-preview",
            "ui-image",
            "ui-extension-files",
            "documents-chrome",
            "storage-s3",
            "mail-inbound",
            "validation-html",
            "validation-phone",
        ] {
            features.entry(extra.to_owned()).or_default();
        }
        let full_list: Vec<String> = [
            "access",
            "api",
            "audit",
            "auth",
            "cache",
            "crypto",
            "db",
            "directory",
            "domains",
            "events",
            "fleet",
            "helm",
            "http",
            "http-client",
            "integrations",
            "lighthouse",
            "log",
            "logbook",
            "mail",
            "notify",
            "observe",
            "orm",
            "queue",
            "realtime",
            "schedule",
            "schema",
            "session",
            "settings",
            "sheets",
            "sso",
            "storage",
            "telemetry",
            "testing",
            "time",
            "ui",
            "validation",
            "view",
            "webhooks",
            "workflows",
            "workspace",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        features.insert("default".to_owned(), vec!["full".to_owned()]);
        features.insert("full".to_owned(), full_list);
        for (k, v) in &self.navree_feature_edges {
            features.insert(k.clone(), v.clone());
        }
        features
    }

    fn expand_navree_features(
        table: &BTreeMap<String, Vec<String>>,
        seeds: &BTreeSet<String>,
    ) -> Vec<String> {
        let mut out = BTreeSet::new();
        let mut stack: Vec<String> = seeds.iter().cloned().collect();
        while let Some(feat) = stack.pop() {
            if !out.insert(feat.clone()) {
                continue;
            }
            if let Some(implies) = table.get(&feat) {
                for next in implies {
                    if !next.starts_with("dep:") && !next.contains('/') {
                        stack.push(next.clone());
                    }
                }
            }
        }
        out.into_iter().collect()
    }

    fn build_metadata_json(&self, dir: &Path) -> std::io::Result<String> {
        let cargo_toml_path = dir.join("Cargo.toml");
        let text = std::fs::read_to_string(&cargo_toml_path)?;
        let doc: toml::Value = toml::from_str(&text).expect("Cargo.toml parses");

        let navree_features = self.navree_features_map();
        let navree_id = "registry+https://github.com/rust-lang/crates.io-index#navree@1.4.0";

        let Some(pkg_table) = doc.get("package") else {
            let mut virtual_packages = vec![json!({
                "id": navree_id,
                "name": "navree",
                "version": "1.4.0",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
                "manifest_path": "/cargo-cache/navree-1.4.0/Cargo.toml",
                "targets": [{ "kind": ["lib"], "name": "navree" }],
                "features": navree_features,
                "metadata": null
            })];
            let mut virtual_nodes = Vec::new();
            for reg in &self.transitive_packages {
                let source = "registry+https://github.com/rust-lang/crates.io-index";
                let pkg_id = format!("{source}#{}@{}", reg.name, reg.version);
                let targets = match &reg.lib_name {
                    Some(lib) => json!([{ "kind": ["lib"], "name": lib }]),
                    None => json!([]),
                };
                virtual_packages.push(json!({
                    "id": pkg_id,
                    "name": reg.name,
                    "version": reg.version,
                    "source": source,
                    "manifest_path": format!("/cargo-cache/{}-{}/Cargo.toml", reg.name, reg.version),
                    "targets": targets,
                    "features": {},
                    "metadata": reg.metadata
                }));
                virtual_nodes.push(json!({
                    "id": pkg_id,
                    "deps": [],
                    "features": []
                }));
            }
            let virtual_json = json!({
                "packages": virtual_packages,
                "resolve": {
                    "root": null,
                    "nodes": virtual_nodes
                }
            });
            return Ok(virtual_json.to_string());
        };

        let root_name = pkg_table
            .get("name")
            .and_then(toml::Value::as_str)
            .unwrap_or("app");
        let root_version = pkg_table
            .get("version")
            .and_then(toml::Value::as_str)
            .unwrap_or("0.1.0");
        let abs_manifest = std::path::absolute(&cargo_toml_path).unwrap_or(cargo_toml_path);
        let root_id = format!("path+file:///{root_name}#{root_version}");

        let mut packages = vec![
            json!({
                "id": root_id,
                "name": root_name,
                "version": root_version,
                "source": null,
                "manifest_path": abs_manifest,
                "targets": [{ "kind": ["bin"], "name": root_name }],
                "features": {},
                "metadata": null
            }),
            json!({
                "id": navree_id,
                "name": "navree",
                "version": "1.4.0",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
                "manifest_path": "/cargo-cache/navree-1.4.0/Cargo.toml",
                "targets": [{ "kind": ["lib"], "name": "navree" }],
                "features": navree_features,
                "metadata": null
            }),
        ];

        let mut added_ids = BTreeSet::new();
        let mut root_deps = Vec::new();
        let mut resolved_nodes = Vec::new();
        let mut navree_seed_features = self.resolved_features.clone();

        if let Some(deps_table) = doc.get("dependencies").and_then(toml::Value::as_table) {
            for (dep_key, dep_val) in deps_table {
                if dep_key == "navree" {
                    if let Some(feats) = dep_val.get("features").and_then(toml::Value::as_array) {
                        for f in feats.iter().filter_map(toml::Value::as_str) {
                            navree_seed_features.insert(f.to_owned());
                        }
                    }
                    root_deps.push(json!({
                        "name": "navree",
                        "pkg": navree_id,
                        "dep_kinds": [{ "kind": null, "target": null }]
                    }));
                    continue;
                }

                if let Some(path_str) = dep_val.get("path").and_then(toml::Value::as_str) {
                    let dep_dir = dir.join(path_str);
                    let (pkg_name, pkg_ver, metadata, lib_name) = if let Some(reg) =
                        self.find_registered_path_package(dir, Path::new(path_str))
                    {
                        (
                            reg.name.clone(),
                            reg.version.clone(),
                            reg.metadata.clone(),
                            reg.lib_name.clone(),
                        )
                    } else if let Ok(sub_text) = std::fs::read_to_string(dep_dir.join("Cargo.toml"))
                    {
                        let sub_doc: toml::Value =
                            toml::from_str(&sub_text).expect("path dependency Cargo.toml parses");
                        let name = sub_doc["package"]["name"]
                            .as_str()
                            .unwrap_or(dep_key)
                            .to_owned();
                        let ver = sub_doc
                            .get("package")
                            .and_then(|p| p.get("version"))
                            .and_then(toml::Value::as_str)
                            .unwrap_or("0.1.0")
                            .to_owned();
                        let lib = sub_doc
                            .get("lib")
                            .and_then(|l| l.get("name"))
                            .and_then(toml::Value::as_str)
                            .map(str::to_owned)
                            .or_else(|| Some(name.replace('-', "_")));
                        let meta = sub_doc
                            .get("package")
                            .and_then(|p| p.get("metadata"))
                            .map(|m| serde_json::to_value(m).expect("metadata converts"));
                        (name, ver, meta, lib)
                    } else {
                        (
                            dep_key.clone(),
                            "0.1.0".to_owned(),
                            None,
                            Some(dep_key.replace('-', "_")),
                        )
                    };

                    let abs_sub = std::path::absolute(dep_dir.join("Cargo.toml"))
                        .unwrap_or_else(|_| dep_dir.join("Cargo.toml"));
                    let pkg_id = format!("path+file:///{path_str}#{pkg_name}@{pkg_ver}");
                    let targets = match &lib_name {
                        Some(lib) => json!([{ "kind": ["lib"], "name": lib }]),
                        None => json!([]),
                    };
                    if added_ids.insert(pkg_id.clone()) {
                        packages.push(json!({
                            "id": pkg_id,
                            "name": pkg_name,
                            "version": pkg_ver,
                            "source": null,
                            "manifest_path": abs_sub,
                            "targets": targets,
                            "features": {},
                            "metadata": metadata
                        }));
                    }
                    root_deps.push(json!({
                        "name": lib_name.unwrap_or_else(|| pkg_name.replace('-', "_")),
                        "pkg": pkg_id,
                        "dep_kinds": [{ "kind": null, "target": null }]
                    }));
                    resolved_nodes.push(json!({
                        "id": pkg_id,
                        "deps": [],
                        "features": []
                    }));
                } else {
                    let is_git = dep_val.get("git").is_some();
                    let source = if is_git {
                        let url = dep_val["git"]
                            .as_str()
                            .unwrap_or("https://example.invalid/repo");
                        format!("git+{url}")
                    } else {
                        "registry+https://github.com/rust-lang/crates.io-index".to_owned()
                    };
                    let explicit_pkg = dep_val
                        .as_table()
                        .and_then(|t| t.get("package"))
                        .and_then(toml::Value::as_str);
                    let lookup_name = explicit_pkg.unwrap_or(dep_key.as_str());
                    let asked_ver = dep_val
                        .as_str()
                        .or_else(|| dep_val.get("version").and_then(toml::Value::as_str));
                    let (pkg_name, pkg_ver, metadata, lib_name) = if let Some(reg) =
                        self.find_registered_package_ver(lookup_name, asked_ver)
                    {
                        (
                            reg.name.clone(),
                            reg.version.clone(),
                            reg.metadata.clone(),
                            reg.lib_name.clone(),
                        )
                    } else {
                        let ver = asked_ver.unwrap_or("0.1.0").to_owned();
                        (
                            lookup_name.to_owned(),
                            ver,
                            None,
                            Some(lookup_name.replace('-', "_")),
                        )
                    };
                    let pkg_id = format!("{source}#{pkg_name}@{pkg_ver}");
                    let targets = match &lib_name {
                        Some(lib) => json!([{ "kind": ["lib"], "name": lib }]),
                        None => json!([]),
                    };
                    if added_ids.insert(pkg_id.clone()) {
                        packages.push(json!({
                            "id": pkg_id,
                            "name": pkg_name,
                            "version": pkg_ver,
                            "source": source,
                            "manifest_path": format!("/cargo-cache/{pkg_name}-{pkg_ver}/Cargo.toml"),
                            "targets": targets,
                            "features": {},
                            "metadata": metadata
                        }));
                    }
                    let extern_name = if explicit_pkg.is_some() {
                        dep_key.replace('-', "_")
                    } else {
                        lib_name.unwrap_or_else(|| pkg_name.replace('-', "_"))
                    };
                    root_deps.push(json!({
                        "name": extern_name,
                        "pkg": pkg_id,
                        "dep_kinds": [{ "kind": null, "target": null }]
                    }));
                    resolved_nodes.push(json!({
                        "id": pkg_id,
                        "deps": [],
                        "features": []
                    }));
                }
            }
        }

        for reg in &self.transitive_packages {
            let source = "registry+https://github.com/rust-lang/crates.io-index";
            let pkg_id = format!("{source}#{}@{}", reg.name, reg.version);
            if added_ids.insert(pkg_id.clone()) {
                let targets = match &reg.lib_name {
                    Some(lib) => json!([{ "kind": ["lib"], "name": lib }]),
                    None => json!([]),
                };
                packages.push(json!({
                    "id": pkg_id,
                    "name": reg.name,
                    "version": reg.version,
                    "source": source,
                    "manifest_path": format!("/cargo-cache/{}-{}/Cargo.toml", reg.name, reg.version),
                    "targets": targets,
                    "features": {},
                    "metadata": reg.metadata
                }));
            }
            resolved_nodes.push(json!({
                "id": pkg_id,
                "deps": [],
                "features": []
            }));
        }

        for reg in &self.packages {
            let source = "registry+https://github.com/rust-lang/crates.io-index";
            let pkg_id = format!("{source}#{}@{}", reg.name, reg.version);
            if added_ids.insert(pkg_id) {
                let targets = match &reg.lib_name {
                    Some(lib) => json!([{ "kind": ["lib"], "name": lib }]),
                    None => json!([]),
                };
                packages.push(json!({
                    "id": format!("{source}#{}@{}", reg.name, reg.version),
                    "name": reg.name,
                    "version": reg.version,
                    "source": source,
                    "manifest_path": format!("/cargo-cache/{}-{}/Cargo.toml", reg.name, reg.version),
                    "targets": targets,
                    "features": {},
                    "metadata": reg.metadata
                }));
            }
        }

        for reg in &self.path_packages {
            let sub_dir = if reg.dir.is_absolute() {
                reg.dir.clone()
            } else {
                dir.join(&reg.dir)
            };
            let rel_key = reg.dir.to_string_lossy().replace('\\', "/");
            let pkg_id = format!("path+file:///{rel_key}#{}@{}", reg.name, reg.version);
            if added_ids.insert(pkg_id.clone()) {
                let abs_sub = std::path::absolute(sub_dir.join("Cargo.toml"))
                    .unwrap_or_else(|_| sub_dir.join("Cargo.toml"));
                let targets = match &reg.lib_name {
                    Some(lib) => json!([{ "kind": ["lib"], "name": lib }]),
                    None => json!([]),
                };
                packages.push(json!({
                    "id": pkg_id,
                    "name": reg.name,
                    "version": reg.version,
                    "source": null,
                    "manifest_path": abs_sub,
                    "targets": targets,
                    "features": {},
                    "metadata": reg.metadata
                }));
            }
        }

        let enabled_navree = Self::expand_navree_features(&navree_features, &navree_seed_features);
        let mut nodes = vec![
            json!({
                "id": root_id,
                "deps": root_deps,
                "features": []
            }),
            json!({
                "id": navree_id,
                "deps": [],
                "features": enabled_navree
            }),
        ];
        nodes.extend(resolved_nodes);

        let full = json!({
            "packages": packages,
            "resolve": {
                "root": root_id,
                "nodes": nodes
            }
        });
        Ok(full.to_string())
    }
}

impl Runner for ScriptedCargo {
    fn run(&mut self, invocation: &Invocation, dir: &Path) -> std::io::Result<bool> {
        let (ok, _, _) = self.dispatch(invocation, dir)?;
        Ok(ok)
    }

    fn capture(&mut self, invocation: &Invocation, dir: &Path) -> std::io::Result<(bool, String)> {
        let (ok, out, _) = self.dispatch(invocation, dir)?;
        Ok((ok, out))
    }

    fn capture_both(
        &mut self,
        invocation: &Invocation,
        dir: &Path,
    ) -> std::io::Result<(bool, String, String)> {
        self.dispatch(invocation, dir)
    }
}

const MINIMAL_APP_RS: &str = "\
//! The root module: every module the app uses, each configured once.

use navree::config::{ConfigModule, ConfigSources};
use navree::kernel::{AppBuilder, Navree};

use crate::modules;

pub fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(modules::users::UsersModule)
    // navree:modules
}
";

const MINIMAL_ENV_EXAMPLE: &str = "\
APP_KEY=
APP_MAIL__FROM=\"Kesar <hello@kesar.example>\"
";

const MINIMAL_CONFIG_APP_TOML: &str = "\
[http]
addr = \"127.0.0.1:8080\"
";

/// Writes a minimal generated-app fixture under `dir` and returns `dir`.
pub fn app(dir: &Path, cargo_toml: &str) -> PathBuf {
    std::fs::create_dir_all(dir.join("src")).expect("create src/");
    std::fs::create_dir_all(dir.join("config")).expect("create config/");
    std::fs::write(dir.join("Cargo.toml"), cargo_toml).expect("write Cargo.toml");
    std::fs::write(dir.join("src/app.rs"), MINIMAL_APP_RS).expect("write src/app.rs");
    std::fs::write(dir.join(".env.example"), MINIMAL_ENV_EXAMPLE).expect("write .env.example");
    std::fs::write(dir.join("config/app.toml"), MINIMAL_CONFIG_APP_TOML)
        .expect("write config/app.toml");
    dir.to_path_buf()
}

/// Writes a `micro`-style Cargo workspace under `dir` with the service member in `crates/svc`,
/// returning `dir/crates/svc`.
pub fn workspace_app(dir: &Path, member_cargo_toml: &str) -> PathBuf {
    let member_dir = dir.join("crates/svc");
    std::fs::create_dir_all(member_dir.join("src")).expect("create crates/svc/src");
    std::fs::create_dir_all(dir.join("config")).expect("create workspace config/");

    let root_cargo_toml = "\
[workspace]
resolver = \"3\"
members = [\"crates/*\"]
default-members = [\"crates/svc\"]

[workspace.package]
version = \"0.1.0\"
edition = \"2024\"
rust-version = \"1.95\"
publish = false

[workspace.dependencies]
navree = { version = \"1.4.0\", default-features = false, features = [\"postgres\"] }
";
    std::fs::write(dir.join("Cargo.toml"), root_cargo_toml).expect("write root Cargo.toml");
    std::fs::write(dir.join("Cargo.lock"), "# workspace lock\nversion = 4\n")
        .expect("write root Cargo.lock");
    std::fs::write(dir.join("config/app.toml"), MINIMAL_CONFIG_APP_TOML)
        .expect("write root config/app.toml");
    std::fs::write(dir.join(".env.example"), MINIMAL_ENV_EXAMPLE).expect("write root .env.example");
    std::fs::write(member_dir.join("Cargo.toml"), member_cargo_toml)
        .expect("write member Cargo.toml");
    std::fs::write(member_dir.join("src/app.rs"), MINIMAL_APP_RS).expect("write member src/app.rs");
    member_dir
}

fn collect_files(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out);
        } else {
            let rel = path
                .strip_prefix(root)
                .expect("under root")
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&path).expect("read file bytes");
            out.insert(rel, bytes);
        }
    }
}

/// Reads every file's bytes recursively under `root`, keyed by `/`-separated relative path.
pub fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    collect_files(root, root, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_cargo_builders_accept_str_and_option_without_ambiguity() {
        let _runner = ScriptedCargo::new()
            .package(
                "navree-plugin-khata",
                "1.0.0",
                "module = \"KhataModule::from_config()\"",
                "navree_plugin_khata",
            )
            .package("reqwest", "0.12.0", None, None)
            .path_package("plugins/notices", "navree-plugin-notices", "0.1.0", None)
            .resolved(&["ui"]);
    }
}
