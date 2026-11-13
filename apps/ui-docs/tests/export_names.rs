//! `use navree::ui::*;` and `use navree::ui::charts::*;` are written side by side in
//! the docs and in apps. Two glob imports that bring in one name make the name
//! ambiguous the moment it is used (`E0659`), so a name the crate root exports must
//! not also be a different item in `charts`. A name that is added to either side
//! after a release breaks the code that imports both, so this test fails first.
//!
//! It reads the source of `crates/navree-ui/src/lib.rs` and `charts/mod.rs` with
//! `syn` and takes, from each, the public items defined there and the names of its
//! `pub use` lists. A name on both sides is fine only when both name the same item,
//! that is, the same path from the crate root.
//!
//! What it does not do:
//! - a glob (`pub use components::*`) is followed one level: the public items and
//!   `pub use` lists of that module's own file. A glob inside that file is not
//!   followed, and the test fails when it finds one, so the limit cannot go unseen;
//! - `#[cfg(...)]` is ignored, so a name behind a feature counts as exported;
//! - a path through an item re-exported more than once (`a::X` that is `b::X`) is
//!   taken as written, so the same item reached by two different paths is reported.
//!   Say so in `SAME_ITEMS` below, with the reason;
//! - names are compared whatever their kind (a type, a function, a module), which
//!   is stricter than the compiler, which keeps types and values apart.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::{Item, UseTree, Visibility};

/// Names that two paths reach as one item. Empty: add a pair here only with the reason.
const SAME_ITEMS: &[(&str, &str)] = &[];

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/navree-ui/src")
}

fn parse(path: &Path) -> syn::File {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    syn::parse_file(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn is_public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// The path of a `use` prefix from the crate root. `module` is where the file's items
/// live (empty for `lib.rs`, `["charts"]` for `charts/mod.rs`) and `children` the
/// modules the file declares, which is how a relative path is told from another crate's.
fn resolve(prefix: &[String], module: &[String], children: &[String]) -> Vec<String> {
    let Some(first) = prefix.first() else {
        return module.to_vec();
    };
    match first.as_str() {
        "crate" => prefix[1..].to_vec(),
        "self" => [module, &prefix[1..]].concat(),
        "super" => [&module[..module.len().saturating_sub(1)], &prefix[1..]].concat(),
        name if children.iter().any(|child| child == name) => [module, prefix].concat(),
        _ => prefix.to_vec(),
    }
}

/// Every name a `use` tree brings in, with the prefix it came by.
fn flatten(tree: &UseTree, prefix: &mut Vec<String>, out: &mut Vec<(Vec<String>, String, String)>) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            flatten(&path.tree, prefix, out);
            prefix.pop();
        }
        UseTree::Name(name) => out.push((
            prefix.clone(),
            name.ident.to_string(),
            name.ident.to_string(),
        )),
        UseTree::Rename(rename) => {
            out.push((
                prefix.clone(),
                rename.ident.to_string(),
                rename.rename.to_string(),
            ));
        }
        UseTree::Glob(_) => out.push((prefix.clone(), "*".into(), "*".into())),
        UseTree::Group(group) => {
            for tree in &group.items {
                flatten(tree, prefix, out);
            }
        }
    }
}

/// The module file for `module` under `dir`: `dir/module/mod.rs` or `dir/module.rs`.
fn module_file(dir: &Path, module: &str) -> PathBuf {
    let nested = dir.join(module).join("mod.rs");
    if nested.exists() {
        nested
    } else {
        dir.join(format!("{module}.rs"))
    }
}

/// The public names of the file at `file`, whose items live at `module`, as name to the
/// path of the item from the crate root. `follow` says whether a glob is followed.
fn exports(file: &Path, module: &[String], follow: bool) -> BTreeMap<String, String> {
    let parsed = parse(file);
    let dir = file.parent().expect("a source file has a directory");
    let children: Vec<String> = parsed
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Mod(item) => Some(item.ident.to_string()),
            _ => None,
        })
        .collect();
    let mut names = BTreeMap::new();
    for item in &parsed.items {
        if let Some(name) = defined_name(item) {
            names.insert(name.clone(), [module, &[name]].concat().join("::"));
        }
        let Item::Use(item) = item else { continue };
        if !is_public(&item.vis) {
            continue;
        }
        let mut flat = Vec::new();
        flatten(&item.tree, &mut Vec::new(), &mut flat);
        for (prefix, name, shown) in flat {
            if name == "*" {
                let target = resolve(&prefix, module, &children);
                let local = prefix.len() == 1 && children.contains(&prefix[0]);
                assert!(
                    follow && local,
                    "{}: `pub use {}::*` is a glob this test does not follow",
                    file.display(),
                    prefix.join("::")
                );
                names.extend(exports(&module_file(dir, &prefix[0]), &target, false));
            } else if shown != "_" {
                let mut path = resolve(&prefix, module, &children);
                path.push(name);
                names.insert(shown, path.join("::"));
            }
        }
    }
    names
}

/// The name of a public item defined in the file (a `use` is read apart).
fn defined_name(item: &Item) -> Option<String> {
    let (vis, ident) = match item {
        Item::Struct(item) => (&item.vis, &item.ident),
        Item::Enum(item) => (&item.vis, &item.ident),
        Item::Union(item) => (&item.vis, &item.ident),
        Item::Trait(item) => (&item.vis, &item.ident),
        Item::Type(item) => (&item.vis, &item.ident),
        Item::Fn(item) => (&item.vis, &item.sig.ident),
        Item::Const(item) => (&item.vis, &item.ident),
        Item::Static(item) => (&item.vis, &item.ident),
        Item::Mod(item) => (&item.vis, &item.ident),
        _ => return None,
    };
    is_public(vis).then(|| ident.to_string())
}

#[test]
fn no_name_is_two_different_items_at_the_crate_root_and_in_charts() {
    let src = src();
    let root = exports(&src.join("lib.rs"), &[], true);
    let charts = exports(&src.join("charts/mod.rs"), &["charts".to_string()], true);
    assert!(root.len() > 100, "{} names at the root", root.len());
    assert!(charts.len() > 50, "{} names in charts", charts.len());

    let clashes: Vec<String> = charts
        .iter()
        .filter_map(|(name, charts_path)| {
            let root_path = root.get(name)?;
            if root_path == charts_path
                || SAME_ITEMS
                    .iter()
                    .any(|(a, b)| (a, b) == (&root_path.as_str(), &charts_path.as_str()))
            {
                return None;
            }
            Some(format!(
                "`{name}`: navree_ui::{root_path} at the root, navree_ui::{charts_path} in charts"
            ))
        })
        .collect();
    assert!(
        clashes.is_empty(),
        "`use navree::ui::*;` with `use navree::ui::charts::*;` cannot name:\n  {}\nRename the one that is not \
         released yet.",
        clashes.join("\n  ")
    );
}

/// The test reads what it is meant to: a name known to be on each side is found.
#[test]
fn the_reader_finds_the_names_it_is_built_on() {
    let src = src();
    let root = exports(&src.join("lib.rs"), &[], true);
    let charts = exports(&src.join("charts/mod.rs"), &["charts".to_string()], true);
    assert_eq!(
        root.get("SortableGrid").map(String::as_str),
        Some("sortable::SortableGrid")
    );
    assert_eq!(
        root.get("Button")
            .map(|path| path.starts_with("components::")),
        Some(true)
    );
    assert_eq!(root.get("country_flags").map(String::as_str), Some("flags"));
    assert_eq!(
        charts.get("Donut").map(String::as_str),
        Some("charts::donut::Donut")
    );
    assert_eq!(
        charts.get("MAX_SERIES").map(String::as_str),
        Some("charts::MAX_SERIES")
    );
    assert!(
        !root.contains_key("Tile") && !root.contains_key("Block"),
        "the picture types are not at the root"
    );
}
