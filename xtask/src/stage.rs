//! `release stage`: copies what Navigator embeds into its own crate folder,
//! so the published crate carries it.
//!
//! Navigator's build script embeds the agent skills, the docs pages and the
//! UI component catalog. In the repository it reads them from `skills/` and
//! `docs/`; a published crate only has its own folder, so before packaging
//! they are copied next to its manifest, where the build script looks
//! first:
//!
//! | From | To (in `crates/navree-navigator/`) |
//! |---|---|
//! | `skills/navree`, `skills/navree-*` | `skills/` |
//! | every `.mdx` page of `docs/` (not `docs/ui-data/`) | `docs/` |
//! | `docs/ui-data/catalog.json` | `catalog.json` |
//!
//! The copies are ignored by git (the crate's `.gitignore`), so a staged
//! checkout is still clean to git; cargo counts them as changes, so it is
//! given `--allow-dirty` when it packages. `release stage --clean` removes them; until
//! then a local build embeds the copies, so stage again after editing the
//! docs or the skills.

use std::path::{Path, PathBuf};

/// Navigator's folder, relative to the workspace root.
pub const NAVIGATOR: &str = "crates/navree-navigator";

/// What was staged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Staged {
    /// Files of the skills.
    pub skill_files: usize,
    /// Docs pages.
    pub doc_pages: usize,
}

/// The docs folder.
fn docs_source(root: &Path) -> Option<PathBuf> {
    let dir = root.join("docs");
    (dir.join("prologue").is_dir() || dir.join("index.mdx").is_file()).then_some(dir)
}

/// The UI component catalog.
fn catalog_source(root: &Path) -> Option<PathBuf> {
    let file = root.join("docs/ui-data/catalog.json");
    file.is_file().then_some(file)
}

/// Removes the staged copies.
pub fn clean(root: &Path) -> Result<(), String> {
    let navigator = root.join(NAVIGATOR);
    for dir in ["skills", "docs"] {
        let dir = navigator.join(dir);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .map_err(|e| format!("cannot remove {}: {e}", dir.display()))?;
        }
    }
    let catalog = navigator.join("catalog.json");
    if catalog.exists() {
        std::fs::remove_file(&catalog)
            .map_err(|e| format!("cannot remove {}: {e}", catalog.display()))?;
    }
    Ok(())
}

/// Stages fresh copies (removing earlier ones first).
pub fn stage(root: &Path) -> Result<Staged, String> {
    let navigator = root.join(NAVIGATOR);
    if !navigator.join("Cargo.toml").is_file() {
        return Err(format!("{} is not a crate folder", navigator.display()));
    }
    clean(root)?;

    let skills = root.join("skills");
    let mut skill_files = 0;
    for entry in sorted_entries(&skills)? {
        let name = entry
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        if entry.is_dir() && (name == "navree" || name.starts_with("navree-")) {
            skill_files += copy_tree(
                &entry,
                &navigator.join("skills").join(&name),
                &|_: &Path| true,
            )?;
        }
    }
    if skill_files == 0 {
        return Err(format!("no skills found in {}", skills.display()));
    }

    let docs = docs_source(root).ok_or("no docs folder found (docs/)")?;
    let doc_pages = copy_tree(&docs, &navigator.join("docs"), &|path: &Path| {
        path.extension().is_some_and(|e| e == "mdx")
            && !path
                .strip_prefix(&docs)
                .is_ok_and(|rel| rel.starts_with("ui-data"))
    })?;
    if doc_pages == 0 {
        return Err(format!("no docs pages found in {}", docs.display()));
    }

    let catalog = catalog_source(root).ok_or("no UI catalog found (docs/ui-data/catalog.json)")?;
    std::fs::copy(&catalog, navigator.join("catalog.json"))
        .map_err(|e| format!("cannot copy {}: {e}", catalog.display()))?;

    Ok(Staged {
        skill_files,
        doc_pages,
    })
}

fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    Ok(entries)
}

/// Copies every file under `from` that `keep` accepts to the same place
/// under `to`; returns how many.
fn copy_tree(from: &Path, to: &Path, keep: &dyn Fn(&Path) -> bool) -> Result<usize, String> {
    let mut copied = 0;
    for entry in sorted_entries(from)? {
        let target = to.join(entry.file_name().unwrap_or_default());
        if entry.is_dir() {
            copied += copy_tree(&entry, &target, keep)?;
        } else if keep(&entry) {
            std::fs::create_dir_all(to)
                .map_err(|e| format!("cannot create {}: {e}", to.display()))?;
            std::fs::copy(&entry, &target)
                .map_err(|e| format!("cannot copy {}: {e}", entry.display()))?;
            copied += 1;
        }
    }
    Ok(copied)
}
