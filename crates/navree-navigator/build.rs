//! Embeds the agent skills, the docs and the UI component catalog into the
//! crate, so `navree skills:install`, Navigator's docs search and
//! `list_components` work offline and always match this version of Navree.
//!
//! - Skills: every file of the `navree` and `navree-*` folders of the
//!   repository's `skills/` (or of a `skills/` copy next to this manifest,
//!   which a published crate carries).
//! - Docs: every `.mdx` page of `apps/website/content/docs` (or of a `docs/`
//!   copy next to this manifest), keyed by its slug (`ai/navigator`).
//! - Components: `apps/website/content/ui/catalog.json` (or a `catalog.json`
//!   copy next to this manifest), cut down to each component's id, name,
//!   group, summary, Rust types and docs slug.
//!
//! The generated `embedded.rs` holds two tables of `include_bytes!` /
//! `include_str!` entries and the compact catalog; Cargo reruns this script
//! when a file changes.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

fn main() {
    let manifest =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    println!("cargo:rerun-if-changed=build.rs");

    let skills = first_dir(&[manifest.join("skills"), manifest.join("../../skills")]);
    let docs = first_dir(&[
        manifest.join("docs"),
        manifest.join("../../apps/website/content/docs"),
    ]);
    let catalog = first_file(&[
        manifest.join("catalog.json"),
        manifest.join("../../apps/website/content/ui/catalog.json"),
    ]);

    let mut code = String::new();
    code.push_str("/// Every file of the embedded skills: (path with `/`, contents).\n");
    code.push_str("pub(crate) static SKILL_FILES: &[(&str, &[u8])] = &[\n");
    if let Some(dir) = &skills {
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut files = Vec::new();
        for entry in read_dir_sorted(dir) {
            let name = file_name(&entry);
            if entry.is_dir() && (name == "navree" || name.starts_with("navree-")) {
                walk(&entry, &mut files);
            }
        }
        files.sort();
        for file in files {
            let relative = relative(dir, &file);
            let _ = writeln!(
                code,
                "    ({relative:?}, include_bytes!({:?})),",
                file.display().to_string()
            );
        }
    } else {
        println!(
            "cargo:warning=no skills/ directory found: `navree skills:install` will install nothing"
        );
    }
    code.push_str("];\n\n");

    code.push_str("/// Every embedded docs page: (slug, MDX source).\n");
    code.push_str("pub(crate) static DOC_PAGES: &[(&str, &str)] = &[\n");
    if let Some(dir) = &docs {
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut files = Vec::new();
        walk(dir, &mut files);
        files.retain(|f| f.extension().is_some_and(|e| e == "mdx"));
        files.sort();
        for file in files {
            let relative = relative(dir, &file);
            let slug = relative.trim_end_matches(".mdx").trim_end_matches("/index");
            let _ = writeln!(
                code,
                "    ({slug:?}, include_str!({:?})),",
                file.display().to_string()
            );
        }
    } else {
        println!("cargo:warning=no docs directory found: Navigator embeds no docs");
    }
    code.push_str("];\n\n");

    code.push_str("/// The component catalog as compact JSON (empty when the build found none).\n");
    let compact = catalog.as_deref().and_then(|file| {
        println!("cargo:rerun-if-changed={}", file.display());
        compact_catalog(file)
    });
    if let Some(compact) = compact {
        let file = out.join("ui_catalog.json");
        std::fs::write(&file, compact).expect("write ui_catalog.json");
        let _ = writeln!(
            code,
            "pub(crate) static UI_CATALOG: &str = include_str!({:?});",
            file.display().to_string()
        );
    } else {
        println!("cargo:warning=no ui/catalog.json found: `list_components` lists nothing");
        code.push_str("pub(crate) static UI_CATALOG: &str = \"\";\n");
    }

    std::fs::write(out.join("embedded.rs"), code).expect("write embedded.rs");
}

/// The first of `candidates` that is a file.
fn first_file(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|file| file.is_file()).cloned()
}

/// The group of the catalog's entries that are in no group (the foundations
/// and guides: themes, icons, flags...).
const GUIDES: &str = "guides";

/// The catalog at `file`, cut down to what `list_components` answers with:
/// `{"groups": [{id, title}], "components": [{id, name, group, summary,
/// docs, types}]}`. The examples (most of the file) stay out; `docs` is the
/// slug `read_doc` takes, empty for a page that isn't under `/docs/`. An
/// entry in no group goes into [`GUIDES`]. `None` when the file isn't a
/// catalog.
fn compact_catalog(file: &Path) -> Option<String> {
    let catalog: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    let mut groups: Vec<Value> = catalog["groups"]
        .as_array()?
        .iter()
        .map(|group| json!({ "id": group["id"], "title": group["title"] }))
        .collect();
    let components: Vec<Value> = catalog["components"]
        .as_array()?
        .iter()
        .filter(|component| component["id"].as_str().is_some_and(|id| !id.is_empty()))
        .map(|component| {
            let url = component["url"].as_str().unwrap_or_default();
            let docs = url
                .strip_prefix("/docs/")
                .map_or("", |slug| slug.trim_matches('/'));
            json!({
                "id": component["id"],
                "name": component["name"].as_str().unwrap_or_default(),
                "group": component["group"].as_str().unwrap_or(GUIDES),
                "summary": component["summary"].as_str().unwrap_or_default(),
                "docs": docs,
                "types": component.get("types").filter(|types| types.is_array()).cloned().unwrap_or_else(|| json!([])),
            })
        })
        .collect();
    if components
        .iter()
        .any(|component| component["group"] == GUIDES)
    {
        groups.push(json!({ "id": GUIDES, "title": "Foundations and guides" }));
    }
    Some(json!({ "groups": groups, "components": components }).to_string())
}

/// The first of `candidates` that is a directory.
fn first_dir(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|dir| dir.is_dir()).cloned()
}

fn read_dir_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    entries.sort();
    entries
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in read_dir_sorted(dir) {
        if entry.is_dir() {
            walk(&entry, out);
        } else {
            out.push(entry);
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `file` relative to `dir`, with `/` separators.
fn relative(dir: &Path, file: &Path) -> String {
    file.strip_prefix(dir)
        .unwrap_or(file)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}
