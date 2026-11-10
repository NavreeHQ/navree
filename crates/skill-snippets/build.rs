//! Writes every compiled Rust block of `skills/**/*.md` to `OUT_DIR` as a
//! module, so building this crate compiles the skills' code.
//!
//! A page's blocks nest: each block is a module inside the one before it and
//! starts with `use super::*;`, so it sees what a reader of the page has seen
//! (`navree::prelude::*`, then the imports and items of the blocks above it),
//! and its own items and imports take precedence. Block files live at
//! `OUT_DIR/skills/<skill>/<file>.md/<line>.rs`, where `<line>` is the line of
//! the opening fence: line `n` of the file is line `<line> + n - 1` of the
//! Markdown.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[path = "src/blocks.rs"]
#[allow(dead_code)]
mod blocks;

fn main() {
    let manifest =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let skills = manifest.join("../../skills");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/blocks.rs");
    println!("cargo:rerun-if-changed={}", skills.display());

    let mut pages = Vec::new();
    walk(&skills, &mut pages);
    pages.retain(|path| path.extension().is_some_and(|ext| ext == "md"));
    pages.sort();

    let root = out.join("skills");
    let _ = std::fs::remove_dir_all(&root);
    let mut lib = String::new();
    for page in pages {
        let relative = relative(&skills, &page);
        let text = std::fs::read_to_string(&page).expect("a skill file is UTF-8");
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
            let (inner, body) = blocks::module_body(&block.code);
            let mut code = String::from(
                "#![allow(warnings, clippy::all, clippy::pedantic, clippy::nursery)] ",
            );
            for attribute in inner {
                code.push_str(&attribute);
                code.push(' ');
            }
            let _ = writeln!(
                code,
                "use super::*; // skills/{relative}, the block at line {}",
                block.line
            );
            code.push_str(&body);
            if let Some(next) = files.get(index + 1) {
                let _ = writeln!(
                    code,
                    "#[path = {:?}]\nmod next;",
                    next.display().to_string()
                );
            }
            std::fs::write(&files[index], code).expect("write a block");
        }
        // Every page starts where a Navree file does: with the prelude.
        let _ = writeln!(
            lib,
            "mod {} {{\n    #[allow(unused_imports)]\n    use navree::prelude::*;\n    #[path = {:?}]\n    mod first;\n}}",
            module_name(&relative),
            files[0].display().to_string(),
        );
    }
    std::fs::write(out.join("snippets.rs"), lib).expect("write snippets.rs");
}

/// `navree-orm/references/queries.md` as `page_navree_orm_references_queries`.
fn module_name(relative: &str) -> String {
    let stem = relative.trim_end_matches(".md");
    let mut name = String::from("page_");
    for c in stem.chars() {
        name.push(match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' => c.to_ascii_lowercase(),
            '/' => '_',
            _ => '_',
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
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}
