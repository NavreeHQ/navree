//! Editing `navree`'s features, resolving feature closures, reading dependency
//! sources, and choosing a scaffolded plugin's `navree` dependency in `Cargo.toml`.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

use crate::new_app::NavreeSource;

/// Which dependency table of `Cargo.toml` receives `navree` features.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DepTable {
    Dependencies,
    DevDependencies,
}

/// Where a dependency in `[dependencies]` comes from.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Registry,
    Path(String),
    Git(String),
    Workspace,
}

/// The app a plugin is made in, as far as the scaffold's `navree` dependency needs it: the app's
/// folder and `Cargo.toml`, and the Cargo workspace's folder and `Cargo.toml` (the same two when
/// the app is its own workspace), where an app's `navree = { workspace = true }` is defined.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct AppNavree<'a> {
    pub dir: &'a Path,
    pub cargo_toml: &'a str,
    pub workspace_dir: &'a Path,
    pub workspace_toml: &'a str,
}

/// Adds `features` to the `navree` dependency of `table`, each once, keeping every other byte;
/// returns `None` when all requested features are listed already. When `table` is
/// `DevDependencies` and has no `navree` entry, one is added using the source of `[dependencies]`
/// with `default-features = false`.
#[allow(dead_code)]
pub(crate) fn turn_on(
    cargo_toml: &str,
    table: DepTable,
    features: &[String],
) -> Result<Option<String>, String> {
    let mut doc: DocumentMut = cargo_toml
        .parse()
        .map_err(|err: toml_edit::TomlError| err.to_string().trim().to_owned())?;
    if features.is_empty() {
        return Ok(None);
    }

    let table_name = match table {
        DepTable::Dependencies => "dependencies",
        DepTable::DevDependencies => "dev-dependencies",
    };

    let has_entry = doc
        .get(table_name)
        .and_then(Item::as_table_like)
        .and_then(|deps| deps.get("navree"))
        .is_some_and(|item| !item.is_none());

    if !has_entry {
        return match table {
            DepTable::Dependencies => Err("missing `navree` in `[dependencies]`".to_owned()),
            DepTable::DevDependencies => {
                let inline = dev_navree_from_dependencies(&doc, features)?;
                let dev_item = doc
                    .entry("dev-dependencies")
                    .or_insert_with(|| Item::Table(Table::new()));
                let dev_table = dev_item
                    .as_table_like_mut()
                    .ok_or_else(|| "`dev-dependencies` must be a table".to_owned())?;
                dev_table.insert("navree", Item::Value(Value::InlineTable(inline)));
                Ok(Some(doc.to_string()))
            }
        };
    }

    let deps = doc
        .get_mut(table_name)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| format!("`{table_name}` must be a table"))?;
    let navree_item = deps
        .get_mut("navree")
        .ok_or_else(|| format!("missing `navree` in `[{table_name}]`"))?;

    let changed = add_features_to_item(navree_item, features)?;
    if changed {
        Ok(Some(doc.to_string()))
    } else {
        Ok(None)
    }
}

fn add_features_to_item(item: &mut Item, features: &[String]) -> Result<bool, String> {
    match item {
        Item::Value(Value::String(ver)) => {
            let version = ver.value().clone();
            let old_decor = ver.decor().clone();
            let mut arr = Array::new();
            let added = push_features(&mut arr, features);
            if !added {
                return Ok(false);
            }
            let mut inline = InlineTable::new();
            inline.insert("version", Value::from(version));
            inline.insert("features", Value::Array(arr));
            let mut new_val = Value::InlineTable(inline);
            *new_val.decor_mut() = old_decor;
            *item = Item::Value(new_val);
            Ok(true)
        }
        Item::Value(Value::InlineTable(inline)) => {
            if let Some(feat_val) = inline.get_mut("features") {
                let arr = feat_val
                    .as_array_mut()
                    .ok_or_else(|| "`features` on `navree` must be a list of strings".to_owned())?;
                return Ok(push_features(arr, features));
            }
            let mut arr = Array::new();
            if !push_features(&mut arr, features) {
                return Ok(false);
            }
            let trailing_suffix = inline.iter_mut().last().and_then(|(_, last_val)| {
                let suf = last_val.decor().suffix()?.as_str()?.to_owned();
                last_val.decor_mut().set_suffix("");
                Some(suf)
            });
            let mut arr_val = Value::Array(arr);
            if let Some(suf) = trailing_suffix {
                arr_val
                    .decor_mut()
                    .set_suffix(if suf.is_empty() { " " } else { &suf });
            }
            inline.insert("features", arr_val);
            Ok(true)
        }
        Item::Table(subtable) => {
            if let Some(feat_item) = subtable.get_mut("features") {
                let arr = feat_item
                    .as_value_mut()
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| "`features` on `navree` must be a list of strings".to_owned())?;
                return Ok(push_features(arr, features));
            }
            let mut arr = Array::new();
            if !push_features(&mut arr, features) {
                return Ok(false);
            }
            subtable.insert("features", Item::Value(Value::Array(arr)));
            Ok(true)
        }
        _ => Err("`navree` dependency must be a version string or a table".to_owned()),
    }
}

fn push_features(arr: &mut Array, features: &[String]) -> bool {
    let multiline_prefix = arr.iter().last().and_then(|val| {
        let prefix = val.decor().prefix()?.as_str()?;
        prefix.contains('\n').then(|| prefix.to_owned())
    });

    let mut to_add = Vec::new();
    for feature in features {
        let already = arr
            .iter()
            .any(|existing| existing.as_str() == Some(feature.as_str()))
            || to_add.iter().any(|added: &&str| *added == feature.as_str());
        if !already {
            to_add.push(feature.as_str());
        }
    }
    if to_add.is_empty() {
        return false;
    }

    let trailing_ws = if multiline_prefix.is_none() {
        arr.get_mut(arr.len().wrapping_sub(1)).and_then(|last| {
            let suf = last.decor().suffix()?.as_str()?.to_owned();
            if !suf.is_empty() && suf.bytes().all(|b| b == b' ' || b == b'\t') {
                last.decor_mut().set_suffix("");
                Some(suf)
            } else {
                None
            }
        })
    } else {
        None
    };

    for (idx, feature) in to_add.iter().enumerate() {
        let mut val = Value::from(*feature);
        let prefix = if arr.is_empty() {
            ""
        } else if let Some(ref nl_indent) = multiline_prefix {
            nl_indent.as_str()
        } else {
            " "
        };
        val.decor_mut().set_prefix(prefix);
        if idx + 1 == to_add.len()
            && let Some(ref ws) = trailing_ws
        {
            val.decor_mut().set_suffix(ws.as_str());
        } else {
            val.decor_mut().set_suffix("");
        }
        arr.push_formatted(val);
    }

    true
}

fn dev_navree_from_dependencies(
    doc: &DocumentMut,
    features: &[String],
) -> Result<InlineTable, String> {
    let dep_item = doc
        .get("dependencies")
        .and_then(Item::as_table_like)
        .and_then(|deps| deps.get("navree"))
        .ok_or_else(|| "missing `navree` in `[dependencies]`".to_owned())?;

    let mut inline = InlineTable::new();
    if let Some(ver) = dep_item.as_str() {
        inline.insert("version", Value::from(ver));
    } else if let Some(tbl) = dep_item.as_table_like() {
        if tbl.get("workspace").and_then(Item::as_bool) == Some(true) {
            inline.insert("workspace", Value::from(true));
        } else if let Some(path) = tbl.get("path").and_then(Item::as_str) {
            inline.insert("path", Value::from(path));
        } else if let Some(git) = tbl.get("git").and_then(Item::as_str) {
            inline.insert("git", Value::from(git));
            for key in ["branch", "tag", "rev"] {
                if let Some(val) = tbl.get(key).and_then(Item::as_str) {
                    inline.insert(key, Value::from(val));
                }
            }
        } else if let Some(ver) = tbl.get("version").and_then(Item::as_str) {
            inline.insert("version", Value::from(ver));
            if let Some(reg) = tbl.get("registry").and_then(Item::as_str) {
                inline.insert("registry", Value::from(reg));
            }
        } else {
            return Err("missing source on `navree` in `[dependencies]`".to_owned());
        }
    } else {
        return Err("`navree` in `[dependencies]` must be a version string or a table".to_owned());
    }

    inline.insert("default-features", Value::from(false));
    let mut arr = Array::new();
    push_features(&mut arr, features);
    inline.insert("features", Value::Array(arr));
    Ok(inline)
}

/// Every feature `on` turns on through the facade's feature table, `on` included
/// (`dep:` and `crate/feature` items skipped).
#[allow(dead_code)]
pub(crate) fn closure(
    table: &BTreeMap<String, Vec<String>>,
    on: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut resolved = on.clone();
    let mut queue: VecDeque<String> = on.iter().cloned().collect();
    while let Some(feature) = queue.pop_front() {
        let Some(items) = table.get(&feature) else {
            continue;
        };
        for item in items {
            if item.starts_with("dep:") || item.contains('/') {
                continue;
            }
            if resolved.insert(item.clone()) {
                queue.push_back(item.clone());
            }
        }
    }
    resolved
}

/// The source of `package` in `[dependencies]`, if present; `-` and `_` are compared as equal.
#[allow(dead_code)]
pub(crate) fn dependency_source(cargo_toml: &str, package: &str) -> Option<Source> {
    let parsed: toml::Table = cargo_toml.parse().ok()?;
    let deps = parsed.get("dependencies")?.as_table()?;
    let (_, value) = deps
        .iter()
        .find(|(key, _)| same_package_name(key, package))
        .or_else(|| {
            deps.iter().find(|(key, val)| {
                let actual = val
                    .as_table()
                    .and_then(|tbl| tbl.get("package"))
                    .and_then(toml::Value::as_str)
                    .unwrap_or(key.as_str());
                same_package_name(actual, package)
            })
        })?;
    source_from_value(value)
}

fn same_package_name(left: &str, right: &str) -> bool {
    let norm = |byte: u8| if byte == b'_' { b'-' } else { byte };
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .all(|(a, b)| norm(a) == norm(b))
}

fn source_from_value(value: &toml::Value) -> Option<Source> {
    match value {
        toml::Value::String(_) => Some(Source::Registry),
        toml::Value::Table(tbl) => {
            if tbl.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
                Some(Source::Workspace)
            } else if let Some(path) = tbl.get("path").and_then(toml::Value::as_str) {
                Some(Source::Path(path.to_owned()))
            } else if let Some(git) = tbl.get("git").and_then(toml::Value::as_str) {
                Some(Source::Git(git.to_owned()))
            } else if tbl.contains_key("version") {
                Some(Source::Registry)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Computes the right-hand side of `navree = ..` for a newly scaffolded plugin crate.
#[allow(dead_code)]
pub(crate) fn plugin_navree_dependency(app: Option<AppNavree<'_>>, plugin_dir: &Path) -> String {
    if let Some(app) = app
        && let Some(spec) = navree_spec_from_app(app, plugin_dir)
    {
        return spec;
    }
    NavreeSource::resolve(None)
        .unwrap_or_else(|_| NavreeSource::Version(NavreeSource::DEFAULT_VERSION.to_owned()))
        .dependency_without_default_features()
}

fn navree_spec_from_app(app: AppNavree<'_>, plugin_dir: &Path) -> Option<String> {
    let parsed: toml::Table = app.cargo_toml.parse().ok()?;
    let dep = parsed.get("dependencies")?.as_table()?.get("navree")?;

    let is_workspace = dep
        .as_table()
        .and_then(|tbl| tbl.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true);

    if is_workspace {
        let ws_parsed: toml::Table = app.workspace_toml.parse().ok()?;
        let target_dep = ws_parsed
            .get("workspace")?
            .as_table()?
            .get("dependencies")?
            .as_table()?
            .get("navree")?;
        format_plugin_navree_value(target_dep, app.workspace_dir, plugin_dir)
    } else {
        format_plugin_navree_value(dep, app.dir, plugin_dir)
    }
}

fn format_plugin_navree_value(
    dep: &toml::Value,
    base_dir: &Path,
    plugin_dir: &Path,
) -> Option<String> {
    match dep {
        toml::Value::String(version) => Some(format!(
            "{{ version = {}, default-features = false }}",
            toml_string(version)
        )),
        toml::Value::Table(tbl) => {
            if let Some(raw_path) = tbl.get("path").and_then(toml::Value::as_str) {
                let resolved_path = resolve_plugin_dep_path(raw_path, base_dir, plugin_dir);
                Some(format!(
                    "{{ path = {}, default-features = false }}",
                    toml_string(&resolved_path)
                ))
            } else if let Some(git) = tbl.get("git").and_then(toml::Value::as_str) {
                let mut parts = vec![format!("git = {}", toml_string(git))];
                for extra in ["branch", "tag", "rev"] {
                    if let Some(val) = tbl.get(extra).and_then(toml::Value::as_str) {
                        parts.push(format!("{extra} = {}", toml_string(val)));
                    }
                }
                parts.push("default-features = false".to_owned());
                Some(format!("{{ {} }}", parts.join(", ")))
            } else if let Some(version) = tbl.get("version").and_then(toml::Value::as_str) {
                let mut parts = vec![format!("version = {}", toml_string(version))];
                if let Some(reg) = tbl.get("registry").and_then(toml::Value::as_str) {
                    parts.push(format!("registry = {}", toml_string(reg)));
                }
                parts.push("default-features = false".to_owned());
                Some(format!("{{ {} }}", parts.join(", ")))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

fn plain_slashes(raw: &str) -> String {
    let stripped = if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        raw.to_owned()
    };
    stripped.replace('\\', "/")
}

fn is_lexical_absolute(path: &str) -> bool {
    if path.starts_with('/') || path.starts_with('\\') {
        return true;
    }
    let bytes = path.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'/' || bytes[2] == b'\\')
}

/// Splits a slash-normalized path into `(root_prefix, normalized_segments, case_insensitive_root)`.
fn parse_lexical_path(raw: &str) -> (String, Vec<String>, bool) {
    let clean = plain_slashes(raw);
    let (root, rest, ci) = if let Some(after_unc) = clean.strip_prefix("//") {
        let mut parts = after_unc.splitn(3, '/');
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        let remainder = parts.next().unwrap_or_default();
        (format!("//{server}/{share}"), remainder, true)
    } else if clean.len() >= 2
        && clean.as_bytes()[0].is_ascii_alphabetic()
        && clean.as_bytes()[1] == b':'
    {
        let drive = &clean[..2];
        let remainder = clean[2..].strip_prefix('/').unwrap_or(&clean[2..]);
        (drive.to_owned(), remainder, true)
    } else if let Some(after_slash) = clean.strip_prefix('/') {
        ("/".to_owned(), after_slash, false)
    } else {
        (String::new(), clean.as_str(), cfg!(windows))
    };

    let mut segments = Vec::new();
    push_normalized_segments(&mut segments, rest, !root.is_empty());
    (root, segments, ci)
}

fn push_normalized_segments(segments: &mut Vec<String>, rel: &str, rooted: bool) {
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if segments.last().is_some_and(|last| last != "..") {
                    segments.pop();
                } else if !rooted {
                    segments.push("..".to_owned());
                }
            }
            seg => segments.push(seg.to_owned()),
        }
    }
}

fn format_lexical_path(root: &str, segments: &[String]) -> String {
    if root.is_empty() {
        if segments.is_empty() {
            ".".to_owned()
        } else {
            segments.join("/")
        }
    } else if root == "/" {
        format!("/{}", segments.join("/"))
    } else if segments.is_empty() {
        format!("{root}/")
    } else {
        format!("{root}/{}", segments.join("/"))
    }
}

fn resolve_plugin_dep_path(raw_path: &str, base_dir: &Path, plugin_dir: &Path) -> String {
    if is_lexical_absolute(raw_path) {
        return plain_slashes(raw_path);
    }

    let base_raw = base_dir.to_string_lossy();
    let plugin_raw = plugin_dir.to_string_lossy();

    let base_effective = if is_lexical_absolute(&base_raw) {
        base_raw.into_owned()
    } else {
        std::path::absolute(base_dir)
            .unwrap_or_else(|_| base_dir.to_path_buf())
            .to_string_lossy()
            .into_owned()
    };
    let plugin_effective = if is_lexical_absolute(&plugin_raw) {
        plugin_raw.into_owned()
    } else {
        std::path::absolute(plugin_dir)
            .unwrap_or_else(|_| plugin_dir.to_path_buf())
            .to_string_lossy()
            .into_owned()
    };

    let (target_root, mut target_segs, target_ci) = parse_lexical_path(&base_effective);
    push_normalized_segments(
        &mut target_segs,
        &plain_slashes(raw_path),
        !target_root.is_empty(),
    );

    let (plugin_root, plugin_segs, plugin_ci) = parse_lexical_path(&plugin_effective);
    let ci = target_ci || plugin_ci;
    let same_root = if ci {
        target_root.eq_ignore_ascii_case(&plugin_root)
    } else {
        target_root == plugin_root
    };

    if !same_root {
        return format_lexical_path(&target_root, &target_segs);
    }

    let common = target_segs
        .iter()
        .zip(plugin_segs.iter())
        .take_while(|(a, b)| {
            if ci {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        })
        .count();

    let mut rel = Vec::new();
    for _ in common..plugin_segs.len() {
        rel.push("..".to_owned());
    }
    for seg in &target_segs[common..] {
        rel.push(seg.clone());
    }

    if rel.is_empty() {
        ".".to_owned()
    } else {
        rel.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feat(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn features_are_added_to_every_form_of_the_navree_dependency_keeping_the_rest() {
        let cases = [
            (
                "navree = \"1.4.0\"",
                "navree = { version = \"1.4.0\", features = [\"billing\"] }",
            ),
            (
                "navree = { path = \"../navree\", features = [\"postgres\"] }",
                "navree = { path = \"../navree\", features = [\"postgres\", \"billing\"] }",
            ),
            (
                "navree   =   {   version = \"1.4\" ,features=[\"postgres\"]  } # db",
                "navree   =   {   version = \"1.4\" ,features=[\"postgres\", \"billing\"]  } # db",
            ),
            (
                "navree = { workspace = true }",
                "navree = { workspace = true, features = [\"billing\"] }",
            ),
        ];

        for (before_line, after_line) in cases {
            let input = format!(
                "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n# App dependencies in Ludhiana.\n[dependencies]\nserde = \"1\"\n\n{before_line}\nuuid = \"1\"\n\n[profile.dev]\nopt-level = 1\n"
            );
            let expected = format!(
                "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n# App dependencies in Ludhiana.\n[dependencies]\nserde = \"1\"\n\n{after_line}\nuuid = \"1\"\n\n[profile.dev]\nopt-level = 1\n"
            );
            let actual = turn_on(&input, DepTable::Dependencies, &feat(&["billing"]))
                .expect("valid Cargo.toml")
                .expect("feature added");
            assert_eq!(actual, expected);
        }

        // Sub-table `[dependencies.navree]` form.
        let sub_before = "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n# App dependencies in Ludhiana.\n[dependencies]\nserde = \"1\"\n\n[dependencies.navree]\nversion = \"1.4\"\n\n[profile.dev]\nopt-level = 1\n";
        let sub_expected = "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n# App dependencies in Ludhiana.\n[dependencies]\nserde = \"1\"\n\n[dependencies.navree]\nversion = \"1.4\"\nfeatures = [\"billing\"]\n\n[profile.dev]\nopt-level = 1\n";
        let sub_actual = turn_on(sub_before, DepTable::Dependencies, &feat(&["billing"]))
            .expect("valid Cargo.toml")
            .expect("feature added");
        assert_eq!(sub_actual, sub_expected);
    }

    #[test]
    fn a_feature_already_listed_is_not_listed_again() {
        let manifest = "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { version = \"1.4.0\", features = [\"billing\"] }\n";
        assert_eq!(
            turn_on(manifest, DepTable::Dependencies, &feat(&["billing"])).unwrap(),
            None
        );

        let updated = turn_on(manifest, DepTable::Dependencies, &feat(&["billing", "ui"]))
            .unwrap()
            .expect("ui added");
        assert_eq!(
            updated,
            "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { version = \"1.4.0\", features = [\"billing\", \"ui\"] }\n"
        );
    }

    #[test]
    fn browser_goes_to_the_dev_dependencies() {
        let with_dev = "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { path = \"..\", features = [\"postgres\"] }\n\n[dev-dependencies]\nnavree = { path = \"..\", features = [\"sqlite\"] }\ntokio = \"1\"\n";
        let updated = turn_on(with_dev, DepTable::DevDependencies, &feat(&["browser"]))
            .unwrap()
            .expect("browser added");
        assert_eq!(
            updated,
            "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { path = \"..\", features = [\"postgres\"] }\n\n[dev-dependencies]\nnavree = { path = \"..\", features = [\"sqlite\", \"browser\"] }\ntokio = \"1\"\n"
        );

        let without_dev_navree = "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { path = \"..\", features = [\"postgres\"] }\n\n[dev-dependencies]\ntokio = \"1\"\n";
        let added = turn_on(
            without_dev_navree,
            DepTable::DevDependencies,
            &feat(&["browser"]),
        )
        .unwrap()
        .expect("dev navree added");
        assert_eq!(
            added,
            "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { path = \"..\", features = [\"postgres\"] }\n\n[dev-dependencies]\ntokio = \"1\"\nnavree = { path = \"..\", default-features = false, features = [\"browser\"] }\n"
        );

        let without_dev_table = "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { path = \"..\", features = [\"postgres\"] }\n";
        let added_table = turn_on(
            without_dev_table,
            DepTable::DevDependencies,
            &feat(&["browser"]),
        )
        .unwrap()
        .expect("dev table and navree added");
        assert_eq!(
            added_table,
            "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { path = \"..\", features = [\"postgres\"] }\n\n[dev-dependencies]\nnavree = { path = \"..\", default-features = false, features = [\"browser\"] }\n"
        );
    }

    #[test]
    fn the_closure_follows_features_and_skips_dep_and_crate_items() {
        let table = BTreeMap::from([
            ("ai".to_owned(), feat(&["dep:navree-ai", "auth", "mail"])),
            ("auth".to_owned(), feat(&["dep:navree-auth", "http"])),
            ("mail".to_owned(), feat(&["navree-mail?/x"])),
        ]);
        let on = BTreeSet::from(["ai".to_owned()]);
        let expected = BTreeSet::from([
            "ai".to_owned(),
            "auth".to_owned(),
            "http".to_owned(),
            "mail".to_owned(),
        ]);
        assert_eq!(closure(&table, &on), expected);
    }

    #[test]
    fn a_plugin_takes_the_apps_navree_from_the_same_place() {
        let app_dir = Path::new("C:/apps/kesar");
        let plugin_dir = Path::new("C:/apps/kesar/plugins/khata");

        // Relative path in the app becomes relative to the plugin's folder.
        let rel_toml = "[package]\nname = \"kesar\"\n\n[dependencies]\nnavree = { path = \"../navree/crates/navree\" }\n";
        assert_eq!(
            plugin_navree_dependency(
                Some(AppNavree {
                    dir: app_dir,
                    cargo_toml: rel_toml,
                    workspace_dir: app_dir,
                    workspace_toml: rel_toml,
                }),
                plugin_dir,
            ),
            "{ path = \"../../../navree/crates/navree\", default-features = false }"
        );

        // Version string.
        let ver_toml = "[package]\nname = \"kesar\"\n\n[dependencies]\nnavree = \"1.4.0\"\n";
        assert_eq!(
            plugin_navree_dependency(
                Some(AppNavree {
                    dir: app_dir,
                    cargo_toml: ver_toml,
                    workspace_dir: app_dir,
                    workspace_toml: ver_toml,
                }),
                plugin_dir,
            ),
            "{ version = \"1.4.0\", default-features = false }"
        );

        // Absolute path on the same drive stays absolute.
        let abs_toml = "[package]\nname = \"kesar\"\n\n[dependencies]\nnavree = { path = \"C:/navree/crates/navree\" }\n";
        assert_eq!(
            plugin_navree_dependency(
                Some(AppNavree {
                    dir: app_dir,
                    cargo_toml: abs_toml,
                    workspace_dir: app_dir,
                    workspace_toml: abs_toml,
                }),
                plugin_dir,
            ),
            "{ path = \"C:/navree/crates/navree\", default-features = false }"
        );

        // Path on another drive from a plugin on `C:` stays absolute on that drive.
        let other_drive_toml = "[package]\nname = \"kesar\"\n\n[dependencies]\nnavree = { path = \"D:/navree/crates/navree\" }\n";
        assert_eq!(
            plugin_navree_dependency(
                Some(AppNavree {
                    dir: app_dir,
                    cargo_toml: other_drive_toml,
                    workspace_dir: app_dir,
                    workspace_toml: other_drive_toml,
                }),
                plugin_dir,
            ),
            "{ path = \"D:/navree/crates/navree\", default-features = false }"
        );

        // Relative path in an app on `D:` while the plugin is on `C:` resolves on `D:` and stays absolute.
        let d_app = Path::new("D:/apps/kesar");
        assert_eq!(
            plugin_navree_dependency(
                Some(AppNavree {
                    dir: d_app,
                    cargo_toml: rel_toml,
                    workspace_dir: d_app,
                    workspace_toml: rel_toml,
                }),
                plugin_dir,
            ),
            "{ path = \"D:/apps/navree/crates/navree\", default-features = false }"
        );

        // Workspace member with `navree = { workspace = true }`: path is relative to workspace root.
        let svc_dir = Path::new("C:/apps/kesar/crates/svc");
        let svc_toml =
            "[package]\nname = \"svc\"\n\n[dependencies]\nnavree = { workspace = true }\n";
        let ws_toml = "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\nnavree = { path = \"../navree/crates/navree\" }\n";
        assert_eq!(
            plugin_navree_dependency(
                Some(AppNavree {
                    dir: svc_dir,
                    cargo_toml: svc_toml,
                    workspace_dir: app_dir,
                    workspace_toml: ws_toml,
                }),
                plugin_dir,
            ),
            "{ path = \"../../../navree/crates/navree\", default-features = false }"
        );

        // Git source is preserved.
        let git_toml = "[package]\nname = \"kesar\"\n\n[dependencies]\nnavree = { git = \"https://github.com/NavreeHQ/navree.git\", branch = \"main\" }\n";
        assert_eq!(
            plugin_navree_dependency(
                Some(AppNavree {
                    dir: app_dir,
                    cargo_toml: git_toml,
                    workspace_dir: app_dir,
                    workspace_toml: git_toml,
                }),
                plugin_dir,
            ),
            "{ git = \"https://github.com/NavreeHQ/navree.git\", branch = \"main\", default-features = false }"
        );

        // Outside an app: matches the CLI's own `NavreeSource::resolve(None)`.
        let expected_cli = NavreeSource::resolve(None)
            .unwrap()
            .dependency_without_default_features();
        assert_eq!(plugin_navree_dependency(None, plugin_dir), expected_cli);
    }

    #[test]
    fn the_source_of_a_dependency_is_read_from_every_form() {
        let manifest = r#"[package]
name = "kesar"
version = "0.1.0"

[dependencies]
navree = "1.4.0"
navree-plugin-khata = { path = "plugins/khata" }
navree-plugin-notices = { git = "https://example.com/notices.git" }
navree-plugin-ledger = { workspace = true }
serde = { version = "1.0", features = ["derive"] }

[dependencies.navree-plugin-announcements]
version = "0.2.0"
"#;

        assert_eq!(
            dependency_source(manifest, "navree"),
            Some(Source::Registry)
        );
        assert_eq!(dependency_source(manifest, "serde"), Some(Source::Registry));
        assert_eq!(
            dependency_source(manifest, "navree-plugin-khata"),
            Some(Source::Path("plugins/khata".to_owned()))
        );
        assert_eq!(
            dependency_source(manifest, "navree_plugin_khata"),
            Some(Source::Path("plugins/khata".to_owned()))
        );
        assert_eq!(
            dependency_source(manifest, "navree-plugin-notices"),
            Some(Source::Git("https://example.com/notices.git".to_owned()))
        );
        assert_eq!(
            dependency_source(manifest, "navree-plugin-ledger"),
            Some(Source::Workspace)
        );
        assert_eq!(
            dependency_source(manifest, "navree-plugin-announcements"),
            Some(Source::Registry)
        );
        assert_eq!(
            dependency_source(manifest, "navree_plugin_announcements"),
            Some(Source::Registry)
        );
        assert_eq!(dependency_source(manifest, "missing-crate"), None);
    }
}
