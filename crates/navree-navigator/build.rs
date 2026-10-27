//! Embeds the agent skills and the docs into the crate, so `navree
//! skills:install` and Navigator's docs search work offline and always match
//! this version of Navree.
//!
//! - Skills: every file of the `navree` and `navree-*` folders of the
//!   repository's `skills/` (or of a `skills/` copy next to this manifest,
//!   which a published crate carries).
//! - Docs: every `.mdx` page of `apps/website/content/docs` (or of a `docs/`
//!   copy next to this manifest), keyed by its slug (`ai/navigator`).
//!
//! The generated `embedded.rs` holds two tables of `include_bytes!` /
//! `include_str!` entries; Cargo reruns this script when a file changes.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    println!("cargo:rerun-if-changed=build.rs");

    let skills = first_dir(&[manifest.join("skills"), manifest.join("../../skills")]);
    let docs = first_dir(&[
        manifest.join("docs"),
        manifest.join("../../apps/website/content/docs"),
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
        println!("cargo:warning=no skills/ directory found: `navree skills:install` will install nothing");
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
    code.push_str("];\n");

    std::fs::write(out.join("embedded.rs"), code).expect("write embedded.rs");
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
