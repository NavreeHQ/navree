//! The Rust code of the website's UI and component pages, compiled against the current
//! `navree` crate.
//!
//! `build.rs` turns every `rust` code block of `docs/ui/**` and
//! `components/**` into a module, so `cargo test -p docs-snippets` fails when a page's code
//! doesn't compile. A block that isn't meant to compile (a signature, a sketch with `...`)
//! is fenced as ` ```rust,ignore `: it still reads as Rust, and the build skips it.
//!
//! A block is compiled as written when it is items (functions, types, impls), and as the
//! body of an async function when it is a fragment: statements, or one expression. Every
//! block carries its own `use` lines. A page's blocks nest, so a block also sees the
//! imports and items of the blocks above it, as a reader does. An error points at
//! `OUT_DIR/docs/<section>/<page>.mdx/<line>.rs`: `<line>` is the line of the block's
//! opening fence.
//!
//! **`pending.txt`** lists the pages not compiled yet, one `section/page.mdx` a line. The
//! pages written before the check existed are on it: many of their blocks lean on what the
//! text before them said (a `report`, an `uow`) and need a line or two to stand alone. A
//! page leaves the list when its blocks compile, and no page joins it: a new page, or a page
//! taken off, is compiled from then on. The list only shrinks.
#![forbid(unsafe_code)]

pub mod blocks;

include!(concat!(env!("OUT_DIR"), "/snippets.rs"));

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use super::blocks::blocks;

    const SECTIONS: [&str; 2] = ["ui", "components"];

    fn manifest() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Every `.mdx` page of the compiled sections, as `section/page.mdx` with its text.
    fn pages() -> Vec<(String, String)> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "mdx") {
                    out.push(path);
                }
            }
        }
        let docs = manifest().join("../../docs");
        let mut all = Vec::new();
        for section in SECTIONS {
            let dir = docs.join(section);
            let mut files = Vec::new();
            walk(&dir, &mut files);
            files.sort();
            for path in files {
                let name = format!(
                    "{section}/{}",
                    path.strip_prefix(&dir)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                );
                all.push((name, std::fs::read_to_string(&path).unwrap()));
            }
        }
        all
    }

    fn pending() -> Vec<String> {
        std::fs::read_to_string(manifest().join("pending.txt"))
            .unwrap()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn every_fence_closes_and_only_rust_blocks_are_ignored() {
        let pages = pages();
        assert!(
            pages.len() > 100,
            "{} pages: are the docs there?",
            pages.len()
        );
        let mut problems = Vec::new();
        for (name, text) in pages {
            match blocks(&text) {
                Err(error) => problems.push(format!("{name}: {error}")),
                Ok(found) => {
                    for block in found {
                        if block.ignored() && block.lang != "rust" {
                            problems.push(format!(
                                "{name}:{}: `ignore` on a `{}` block: only Rust blocks are compiled",
                                block.line, block.lang
                            ));
                        }
                    }
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    /// The pages on the list when the check began. The list only shrinks.
    const MOST_PENDING: usize = 106;

    #[test]
    fn the_pending_list_is_sorted_and_names_pages_that_still_have_rust_blocks() {
        let pending = pending();
        assert!(
            pending.len() <= MOST_PENDING,
            "{} pages are pending, and no page joins the list",
            pending.len()
        );
        let mut sorted = pending.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(pending, sorted, "pending.txt is sorted, one page a line");
        let pages = pages();
        let mut problems = Vec::new();
        for name in &pending {
            match pages.iter().find(|(page, _)| page == name) {
                None => problems.push(format!("{name}: there is no such page")),
                Some((_, text)) => {
                    let has_block = blocks(text)
                        .map(|found| found.iter().any(super::blocks::Block::compiled))
                        .unwrap_or(false);
                    if !has_block {
                        problems.push(format!(
                            "{name}: no Rust block to compile; take it off the list"
                        ));
                    }
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn pages_with_rust_blocks_are_compiled_unless_they_are_pending() {
        let pending: BTreeSet<String> = pending().into_iter().collect();
        let mut compiled_pages = 0;
        let mut compiled_blocks = 0;
        let mut waiting_blocks = 0;
        for (name, text) in pages() {
            let count = blocks(&text)
                .map(|found| found.iter().filter(|block| block.compiled()).count())
                .unwrap_or(0);
            if pending.contains(&name) {
                waiting_blocks += count;
            } else if count > 0 {
                compiled_pages += 1;
                compiled_blocks += count;
            }
        }
        println!(
            "{compiled_blocks} Rust blocks on {compiled_pages} pages are compiled; \
             {waiting_blocks} blocks on {} pages are pending",
            pending.len()
        );
        assert!(compiled_pages > 0, "no page is compiled");
    }
}
