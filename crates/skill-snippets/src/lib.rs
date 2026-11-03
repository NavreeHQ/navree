//! The agent skills' Rust code, compiled against the current `navree` crate.
//!
//! `build.rs` turns every `rust` code block of `skills/**/*.md` into a module,
//! so `cargo test -p skill-snippets` fails when a skill's code doesn't
//! compile. A block that isn't meant to compile (a fragment, a signature, a
//! sketch with `...`) is fenced as ` ```rust,ignore `: it still reads as Rust,
//! and the build skips it.
//!
//! Every page starts with `use navree::prelude::*;`, as a Navree file does,
//! and its blocks nest, so a block sees the imports and items of the blocks
//! above it, as a reader does. Tests in a block are compiled as plain
//! functions and never run. An error points at
//! `OUT_DIR/skills/<skill>/<file>.md/<line>.rs`: `<line>` is the line of the
//! block's opening fence, and line `n` of that file is line `<line> + n - 1`
//! of the Markdown.
//!
//! The tests below check the skills' text: fences close, only Rust blocks
//! carry `ignore`, line endings are LF, and local links and links to
//! navree.com/docs resolve.
#![forbid(unsafe_code)]

pub mod blocks;

include!(concat!(env!("OUT_DIR"), "/snippets.rs"));

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::blocks::blocks;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Every `.md` file under `skills/`, with its path from the repository.
    fn pages() -> Vec<(String, String)> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "md") {
                    out.push(path);
                }
            }
        }
        let root = repo();
        let mut files = Vec::new();
        walk(&root.join("skills"), &mut files);
        files.sort();
        files
            .into_iter()
            .map(|path| {
                let name = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                (name, std::fs::read_to_string(&path).unwrap())
            })
            .collect()
    }

    #[test]
    fn every_fence_closes_and_only_rust_blocks_are_ignored() {
        let mut problems = Vec::new();
        let mut compiled = 0;
        let pages = pages();
        assert!(
            pages.len() > 60,
            "{} skill files: is `skills/` there?",
            pages.len()
        );
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
                        compiled += usize::from(block.compiled());
                    }
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        assert!(compiled > 0, "no Rust block is compiled");
    }

    #[test]
    fn line_endings_are_lf() {
        let crlf: Vec<String> = pages()
            .into_iter()
            .filter(|(_, text)| text.contains('\r'))
            .map(|(name, _)| name)
            .collect();
        assert!(crlf.is_empty(), "CR line endings in {crlf:?}");
    }

    /// The targets of `[text](target)` links, outside code.
    fn links(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut in_fence = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
            let mut rest = line;
            while let Some(start) = rest.find("](") {
                let after = &rest[start + 2..];
                let Some(end) = after.find(')') else { break };
                out.push(after[..end].to_owned());
                rest = &after[end..];
            }
        }
        out
    }

    #[test]
    fn local_links_resolve() {
        let root = repo();
        let mut problems = Vec::new();
        for (name, text) in pages() {
            let dir = root.join(&name);
            let dir = dir.parent().unwrap();
            for target in links(&text) {
                if target.starts_with("http")
                    || target.starts_with('#')
                    || target.starts_with("mailto:")
                {
                    continue;
                }
                let path = target.split('#').next().unwrap();
                if !dir.join(path).exists() {
                    problems.push(format!("{name}: [..]({target}) does not resolve"));
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn docs_links_name_a_page() {
        let docs = repo().join("apps/website/content/docs");
        let mut problems = Vec::new();
        for (name, text) in pages() {
            let mut rest = text.as_str();
            while let Some(start) = rest.find("https://navree.com/docs/") {
                let after = &rest[start + "https://navree.com/docs/".len()..];
                let end = after
                    .find(|c: char| c.is_whitespace() || ")`>\"'|".contains(c))
                    .unwrap_or(after.len());
                let slug = after[..end]
                    .split('#')
                    .next()
                    .unwrap()
                    .trim_end_matches(['/', '.', ',', ';', ':']);
                rest = &after[end..];
                if slug.is_empty() || slug.ends_with(".md") || slug.contains('<') {
                    continue;
                }
                let page = docs.join(format!("{slug}.mdx"));
                let index = docs.join(slug).join("index.mdx");
                if !page.exists() && !index.exists() {
                    problems.push(format!(
                        "{name}: https://navree.com/docs/{slug}/ has no page"
                    ));
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
