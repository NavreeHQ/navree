//! The `plugins` section in `docs/manifest.ts` and the one-line install on each
//! opt-in package page.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Extracts the `pages: [...]` array of the section with `slug: "plugins"` from
/// `docs/manifest.ts`.
fn plugins_section_pages(manifest_ts: &str) -> Vec<String> {
    let marker = "slug: \"plugins\"";
    let start = manifest_ts
        .find(marker)
        .expect("docs/manifest.ts has slug: \"plugins\"");
    let after_slug = &manifest_ts[start..];
    let section_end = after_slug
        .find("\n  },")
        .expect("plugins section closes in docs/manifest.ts");
    let section = &after_slug[..section_end];
    let pages_start = section
        .find("pages:")
        .expect("plugins section has pages: [...]");
    let after_pages = &section[pages_start..];
    let open = after_pages.find('[').expect("pages opens with [");
    let close = after_pages[open..].find(']').expect("pages closes with ]");
    let body = &after_pages[open + 1..open + close];

    let mut slugs = Vec::new();
    for line in body.lines() {
        let code = line.split("//").next().unwrap_or(line);
        let mut rest = code;
        while let Some(q1) = rest.find('"') {
            let after_q1 = &rest[q1 + 1..];
            let q2 = after_q1.find('"').expect("closing quote in pages array");
            slugs.push(after_q1[..q2].to_owned());
            rest = &after_q1[q2 + 1..];
        }
    }
    slugs
}

/// Checks that `mdx` starts with `---` front matter holding non-empty `title`
/// and `description` keys.
fn has_title_and_description(mdx: &str) -> bool {
    let normalized = mdx.replace("\r\n", "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return false;
    };
    let Some(end) = rest.find("\n---") else {
        return false;
    };
    let front = &rest[..end];
    let mut has_title = false;
    let mut has_description = false;
    for line in front.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim().trim_matches('"');
            if key.trim() == "title" && !value.is_empty() {
                has_title = true;
            }
            if key.trim() == "description" && !value.is_empty() {
                has_description = true;
            }
        }
    }
    has_title && has_description
}

#[test]
fn the_plugins_section_lists_every_page_and_every_page_exists() {
    let root = repo_root();
    let manifest_ts =
        std::fs::read_to_string(root.join("docs/manifest.ts")).expect("read docs/manifest.ts");
    let pages = plugins_section_pages(&manifest_ts);
    let expected = [
        "overview",
        "installing",
        "writing",
        "resources",
        "ui-components",
        "drivers",
        "existing-crates",
        "testing",
        "releasing",
    ];
    assert_eq!(pages, expected, "docs/manifest.ts plugins pages order");

    let plugins_dir = root.join("docs/plugins");
    for slug in &pages {
        let file = plugins_dir.join(format!("{slug}.mdx"));
        assert!(file.is_file(), "missing docs/plugins/{slug}.mdx");
        let text = std::fs::read_to_string(&file).expect("read plugin docs page");
        assert!(
            has_title_and_description(&text),
            "docs/plugins/{slug}.mdx must start with title and description front matter"
        );
    }

    let listed: BTreeSet<String> = pages.into_iter().collect();
    let mut on_disk = BTreeSet::new();
    for entry in std::fs::read_dir(&plugins_dir).expect("read docs/plugins") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|ext| ext == "mdx") {
            let stem = path
                .file_stem()
                .expect("file stem")
                .to_string_lossy()
                .into_owned();
            on_disk.insert(stem);
        }
    }
    assert_eq!(
        on_disk, listed,
        "every docs/plugins/*.mdx file must be listed in docs/manifest.ts"
    );
}

/// Finds the byte offset right after the closing fence of the first `toml`
/// block whose body turns `feature` on (`features = [` with `"FEATURE"`).
fn end_of_first_feature_toml_block(mdx: &str, feature: &str) -> Option<usize> {
    let quoted = format!("\"{feature}\"");
    let mut offset = 0;
    let mut open_body_start: Option<usize> = None;
    for line in mdx.split('\n') {
        let line_len = line.len();
        let trimmed = line.trim_start();
        if open_body_start.is_none() {
            if trimmed.starts_with("```toml") {
                open_body_start = Some(offset + line_len + 1);
            }
        } else if trimmed.trim_end() == "```" {
            let body_start = open_body_start.take().unwrap();
            let body = &mdx[body_start..offset];
            if body.contains("features = [") && body.contains(&quoted) {
                return Some((offset + line_len + 1).min(mdx.len()));
            }
        }
        offset += line_len + 1;
    }
    None
}

#[test]
fn every_opt_in_package_page_shows_its_one_line_install() {
    const OPT_IN_PAGES: &[(&str, &str)] = &[
        ("social", "docs/packages/social-sign-in.mdx"),
        ("oauth", "docs/packages/oauth-server.mdx"),
        ("flags", "docs/packages/feature-flags.mdx"),
        ("graphql", "docs/microservices/graphql.mdx"),
        ("grpc", "docs/microservices/grpc.mdx"),
        ("documents", "docs/packages/documents.mdx"),
        ("media", "docs/packages/media.mdx"),
        ("billing", "docs/packages/billing.mdx"),
        ("search", "docs/packages/search.mdx"),
        ("ai", "docs/ai/ai-sdk.mdx"),
        ("mcp", "docs/ai/mcp-servers.mdx"),
        ("redis", "docs/digging-deeper/redis.mdx"),
        ("micro", "docs/microservices/introduction.mdx"),
        ("i18n", "docs/packages/localization.mdx"),
        ("mongo", "docs/orm/mongodb.mdx"),
        ("browser", "docs/packages/browser-testing.mdx"),
    ];
    const TOOL_PAGES: &[&str] = &[
        "docs/tools/dock.mdx",
        "docs/tools/harbor.mdx",
        "docs/ai/navigator.mdx",
    ];

    let root = repo_root();
    for (name, rel_path) in OPT_IN_PAGES {
        let text = std::fs::read_to_string(root.join(rel_path))
            .unwrap_or_else(|err| panic!("read {rel_path}: {err}"))
            .replace("\r\n", "\n");
        let after_toml = end_of_first_feature_toml_block(&text, name)
            .unwrap_or_else(|| panic!("{rel_path} has no toml block turning `{name}` on"));
        let window: String = text[after_toml..]
            .lines()
            .skip_while(|line| line.trim().is_empty())
            .take(6)
            .collect::<Vec<_>>()
            .join("\n");
        let cmd = format!("```bash\nnavree plugin:add {name}\n```");
        assert!(
            window.contains(&cmd),
            "{rel_path} must show `{cmd}` right after its first `{name}` feature toml block; got:\n{window}"
        );
        assert!(
            window.contains("/docs/plugins/installing/"),
            "{rel_path} must link to /docs/plugins/installing/ beside its plugin:add block"
        );
        if *name == "browser" {
            assert!(
                window.contains("it adds the feature as a dev-dependency"),
                "{rel_path} must note that browser is added as a dev-dependency"
            );
        } else {
            assert!(
                window.contains(
                    "it turns the feature on and wires the module, and prints what is left to do."
                ),
                "{rel_path} must describe turning the feature on and wiring the module"
            );
        }
    }

    for rel_path in TOOL_PAGES {
        let text = std::fs::read_to_string(root.join(rel_path))
            .unwrap_or_else(|err| panic!("read {rel_path}: {err}"));
        assert!(
            !text.contains("plugin:add"),
            "tool page {rel_path} must not show plugin:add"
        );
    }
}
