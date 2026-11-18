//! The first-party index against each crate's manifest and the facade's features (read from disk: the CLI depends on no runtime crate).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use navree_cli::plugin::index::{Index, IndexKind};
use navree_cli::plugin::manifest::{self, AppFacts};

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn facade_features() -> BTreeMap<String, Vec<String>> {
    let path = crates_dir().join("navree/Cargo.toml");
    let text = std::fs::read_to_string(&path).expect("crates/navree/Cargo.toml exists");
    let doc: toml::Value = toml::from_str(&text).expect("crates/navree/Cargo.toml parses");
    let table = doc["features"]
        .as_table()
        .expect("crates/navree/Cargo.toml has [features]");
    table
        .iter()
        .map(|(name, list)| {
            let items = list
                .as_array()
                .expect("each facade feature is an array")
                .iter()
                .map(|v| {
                    v.as_str()
                        .expect("each feature item is a string")
                        .to_owned()
                })
                .collect();
            (name.clone(), items)
        })
        .collect()
}

fn kernel_first_party_pairs() -> Vec<(String, String)> {
    let path = crates_dir().join("navree-kernel/src/plugin.rs");
    let text = std::fs::read_to_string(&path).expect("crates/navree-kernel/src/plugin.rs exists");
    let after = text
        .split("pub const FIRST_PARTY")
        .nth(1)
        .expect("FIRST_PARTY constant exists");
    let body = after
        .split("];")
        .next()
        .expect("FIRST_PARTY closes with ];");
    let mut pairs = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("(\"") {
            let parts: Vec<&str> = rest.split('"').collect();
            if parts.len() >= 3 {
                pairs.push((parts[0].to_owned(), parts[2].to_owned()));
            }
        }
    }
    pairs
}

fn walk_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn crate_has_module_impl(crate_src: &Path) -> bool {
    let mut files = Vec::new();
    walk_rs_files(crate_src, &mut files);
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for line in text.lines() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("//")
                && trimmed.starts_with("impl")
                && trimmed.contains("Module for ")
            {
                return true;
            }
        }
    }
    false
}

#[test]
fn the_index_matches_each_crates_manifest_and_the_facade_features() {
    let index = Index::embedded();
    let crates = crates_dir();
    let features = facade_features();

    let facts = AppFacts {
        first_party: true,
        navree_version: "1.4.0".to_owned(),
        navree_features: features.keys().cloned().collect(),
        allowed_features: index.manifest_features.iter().cloned().collect(),
        reserved: index.reserved.iter().cloned().collect(),
        roles: BTreeSet::new(),
        demo_guest_role: None,
        granted: Vec::new(),
    };

    let mut module_crates = BTreeSet::new();
    let mut indexed_first_party = Vec::new();

    for entry in &index.packages {
        let manifest_path = crates.join(&entry.krate).join("Cargo.toml");
        let manifest_text = std::fs::read_to_string(&manifest_path)
            .unwrap_or_else(|err| panic!("missing {}: {err}", manifest_path.display()));
        let doc: toml::Value = toml::from_str(&manifest_text)
            .unwrap_or_else(|err| panic!("invalid {}: {err}", manifest_path.display()));

        let feature_items = features
            .get(&entry.feature)
            .unwrap_or_else(|| panic!("facade lacks feature `{}`", entry.feature));

        if matches!(
            entry.kind,
            IndexKind::Module | IndexKind::Dev | IndexKind::Tool
        ) {
            let dep = format!("dep:{}", entry.krate);
            assert!(
                feature_items.contains(&dep),
                "facade feature `{}` must contain `{dep}`",
                entry.feature
            );
        }

        if entry.kind == IndexKind::Module {
            module_crates.insert(entry.krate.clone());
            indexed_first_party.push((entry.krate.clone(), entry.krate.replace('-', "_")));

            let expected_comment = format!(
                "# What `navree plugin:add {}` reads; the CLI's index holds the same line.",
                entry.name
            );
            assert!(
                manifest_text.contains(&expected_comment),
                "{} lacks comment `{expected_comment}`",
                manifest_path.display()
            );

            let plugin_toml = doc
                .get("package")
                .and_then(|p| p.get("metadata"))
                .and_then(|m| m.get("navree"))
                .and_then(|n| n.get("plugin"))
                .unwrap_or_else(|| {
                    panic!(
                        "{} lacks [package.metadata.navree.plugin]",
                        manifest_path.display()
                    )
                });
            let plugin_json =
                serde_json::to_value(plugin_toml).expect("plugin table converts to JSON");
            let checked = manifest::read(&entry.krate, &plugin_json, &facts)
                .unwrap_or_else(|err| panic!("{} plugin table failed grammar: {err}", entry.krate));

            let expected_module = format!(
                "navree::{}::{}",
                entry.feature.replace('-', "_"),
                checked.module.as_str()
            );
            assert_eq!(
                entry.module.as_deref(),
                Some(expected_module.as_str()),
                "{} module mismatch between index.toml and Cargo.toml",
                entry.krate
            );
        }
    }

    for dir_entry in std::fs::read_dir(&crates)
        .expect("crates/ is readable")
        .flatten()
    {
        let dir = dir_entry.path();
        let cargo_toml = dir.join("Cargo.toml");
        if !cargo_toml.is_file() {
            continue;
        }
        let crate_dir_name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .expect("UTF-8 crate folder")
            .to_owned();
        let text = std::fs::read_to_string(&cargo_toml).expect("Cargo.toml readable");
        let doc: toml::Value = toml::from_str(&text).expect("Cargo.toml parses");
        let has_plugin_table = doc
            .get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("navree"))
            .and_then(|n| n.get("plugin"))
            .is_some();
        assert_eq!(
            has_plugin_table,
            module_crates.contains(&crate_dir_name),
            "unexpected [package.metadata.navree.plugin] presence on {crate_dir_name}"
        );
    }

    let kernel_pairs = kernel_first_party_pairs();
    assert_eq!(kernel_pairs.len(), 31);
    assert_eq!(indexed_first_party, kernel_pairs);
}

#[test]
fn the_feature_allow_list_matches_the_facades_module_features() {
    let crates = crates_dir();
    let features = facade_features();
    let excluded: BTreeSet<&str> = [
        "dock",
        "harbor",
        "navigator",
        "browser",
        "mongo",
        "testing",
        "documents",
    ]
    .into_iter()
    .collect();

    let mut computed = BTreeSet::new();
    for (feature, list) in &features {
        if excluded.contains(feature.as_str()) {
            continue;
        }
        let dep = format!("dep:navree-{feature}");
        if list.contains(&dep)
            && crate_has_module_impl(&crates.join(format!("navree-{feature}/src")))
        {
            computed.insert(feature.clone());
        }
    }
    computed.insert("documents-native".to_owned());
    computed.insert("documents-docx".to_owned());

    let computed: Vec<String> = computed.into_iter().collect();
    assert_eq!(computed, Index::embedded().manifest_features);
}
