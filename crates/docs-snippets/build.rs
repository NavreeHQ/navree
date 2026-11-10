//! Writes every compiled Rust block of the UI and component pages to `OUT_DIR` as a module,
//! so building this crate compiles the pages' code.
//!
//! A page's blocks nest: each block is a module inside the one before it and starts with
//! `use super::*;`, so it sees what a reader of the page has seen (the imports of the blocks
//! above it), and its own items and imports take precedence. A block of items is compiled as
//! it is; a fragment (statements, or one expression) is wrapped in an async function.
//!
//! A page listed in `pending.txt` is not compiled yet: the list only shrinks. Block files
//! live at `OUT_DIR/docs/<section>/<page>.mdx/<line>.rs`, where `<line>` is the line of the
//! opening fence.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[path = "src/blocks.rs"]
#[allow(dead_code)]
mod blocks;

/// The sections of the docs whose Rust blocks are compiled.
const SECTIONS: [&str; 2] = ["ui", "components"];

fn main() {
    let manifest =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let docs = manifest.join("../../docs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/blocks.rs");
    println!("cargo:rerun-if-changed=pending.txt");

    let pending: BTreeSet<String> = std::fs::read_to_string(manifest.join("pending.txt"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect();

    let root = out.join("docs");
    let _ = std::fs::remove_dir_all(&root);
    let mut lib = String::new();
    for section in SECTIONS {
        let dir = docs.join(section);
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut pages = Vec::new();
        walk(&dir, &mut pages);
        pages.retain(|path| path.extension().is_some_and(|ext| ext == "mdx"));
        pages.sort();
        for page in pages {
            let relative = format!("{section}/{}", relative(&dir, &page));
            if pending.contains(&relative) {
                continue;
            }
            let text = std::fs::read_to_string(&page).expect("a docs page is UTF-8");
            // A broken fence is reported by the crate's tests, with the file.
            let Ok(found) = blocks::blocks(&text) else {
                continue;
            };
            let compiled: Vec<_> = found.into_iter().filter(blocks::Block::compiled).collect();
            if compiled.is_empty() {
                continue;
            }
            let dir = root.join(&relative);
            std::fs::create_dir_all(&dir).expect("create the page's directory");
            let files: Vec<PathBuf> = compiled
                .iter()
                .map(|block| dir.join(format!("{}.rs", block.line)))
                .collect();
            for (index, block) in compiled.iter().enumerate() {
                let prepared = blocks::prepare(&block.code);
                let next = files.get(index + 1).map(|path| path.display().to_string());
                let code = blocks::module_source(&prepared, block.line, &relative, next.as_deref());
                std::fs::write(&files[index], code).expect("write a block");
            }
            let _ = writeln!(
                lib,
                "mod {} {{\n    #[path = {:?}]\n    mod first;\n}}",
                module_name(&relative),
                files[0].display().to_string(),
            );
        }
    }
    std::fs::write(out.join("snippets.rs"), lib).expect("write snippets.rs");
}

/// `components/date-picker.mdx` as `page_components_date_picker`.
fn module_name(relative: &str) -> String {
    let stem = relative.trim_end_matches(".mdx");
    let mut name = String::from("page_");
    for c in stem.chars() {
        name.push(if c.is_ascii_alphanumeric() {
            c.to_ascii_lowercase()
        } else {
            '_'
        });
    }
    name
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// `file` relative to `dir`, with `/` separators.
fn relative(dir: &Path, file: &Path) -> String {
    file.strip_prefix(dir)
        .unwrap_or(file)
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}
