//! The docs that print the release version, and the release notes.
//!
//! `release prepare` runs [`update`], which sets the version in:
//!
//! - `docs/getting-started/installation.mdx`: the `navree <version>` line a
//!   `navree --version` prints, and `navree = "<version>"`;
//! - `docs/getting-started/starter-kits.mdx`: `kit_version = "<version>"`;
//! - `docs/prologue/introduction.mdx` and `docs/prologue/roadmap.mdx`: the
//!   series (`1.0` for `1.0.1`), which changes only with the minor version;
//! - `docs/configuration/reference.mdx`: the default `User-Agent` of the HTTP
//!   client, `navree-http-client/<version>`. The page is written from the
//!   configuration sections, and this is the one default that comes from the
//!   crate's version; a value of this form is the only text changed;
//! - `docs/prologue/release-notes.mdx`: a `## Navree <version>` section at the
//!   top, written from the version's section of `CHANGELOG.md`;
//! - the other places that name the CLI's own version, listed in
//!   [`FOLLOWING`]: what `navree new` writes into an app, and the examples
//!   of the version in the docs and the agent skills. A version that is not
//!   Navree's (`api.version`, an OpenAPI example) is not among them.
//!
//! Every function takes the text of a page and returns the new text, so a
//! second run changes nothing. A page that has no place to change is an
//! error: the page was reorganised, and this module with it.

use std::path::{Path, PathBuf};

use crate::changelog;
use crate::prepare::is_valid_version;

const INSTALLATION: &str = "docs/getting-started/installation.mdx";
const STARTER_KITS: &str = "docs/getting-started/starter-kits.mdx";
const INTRODUCTION: &str = "docs/prologue/introduction.mdx";
const ROADMAP: &str = "docs/prologue/roadmap.mdx";
const REFERENCE: &str = "docs/configuration/reference.mdx";
const RELEASE_NOTES: &str = "docs/prologue/release-notes.mdx";

/// The places, besides the pages above, where the text right after a prefix
/// is Navree's version: (file, the text before the version). Each is the
/// version `navree new` writes (`navree = "1.0.1"`, the `^1.0.1` range of
/// the front-end packages) or an example of what the CLI, a docs file or a
/// log prints, which follows the release.
pub const FOLLOWING: &[(&str, &str)] = &[
    ("docs/digging-deeper/cli.mdx", "else `navree = \""),
    ("docs/getting-started/directory-structure.mdx", "or `\""),
    ("docs/database/postgresql.mdx", "navree = { version = \""),
    ("docs/frontend/introduction.mdx", "(`^"),
    (
        "docs/digging-deeper/http-client.mdx",
        "for example `navree-http-client/",
    ),
    ("docs/ai/docs-for-agents.mdx", "- Version: "),
    ("docs/ai/navigator.mdx", "{\"version\": \""),
    ("skills/navree-new-app/SKILL.md", "navree = \""),
    (
        "skills/navree-cli-generators/SKILL.md",
        "`navree new` writes `navree = \"",
    ),
];

/// `1.0` for `1.0.1` and for `1.0.0-rc.1`.
pub fn series(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// Whether `token` is a series: `1.0`.
fn is_series(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    parts.len() == 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Replaces the version-shaped text that follows each `prefix` with `new`;
/// returns the text and how many places it found (those already equal to
/// `new` included). The text after a prefix is read up to the first
/// character a version does not have, without the full stop that ends a
/// sentence, and is replaced only when `accept` takes it, so
/// `navree-http-client/<version>` stays as it is.
fn replace_after(
    text: &str,
    prefix: &str,
    accept: impl Fn(&str) -> bool,
    new: &str,
) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut found = 0;
    let mut rest = text;
    while let Some(at) = rest.find(prefix) {
        let after = at + prefix.len();
        out.push_str(&rest[..after]);
        rest = &rest[after..];
        let span = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-'))
            .unwrap_or(rest.len());
        let token = rest[..span].trim_end_matches(['.', '-']);
        if !token.is_empty() && accept(token) {
            out.push_str(new);
            found += 1;
            rest = &rest[token.len()..];
        }
    }
    out.push_str(rest);
    (out, found)
}

fn replaced(page: &str, what: &str, (text, found): (String, usize)) -> Result<String, String> {
    if found == 0 {
        Err(format!("{page}: no {what} to set"))
    } else {
        Ok(text)
    }
}

/// The installation page with `navree <version>` (the output of
/// `navree --version`, a line of its own) and `navree = "<version>"` set to
/// `version`.
pub fn set_installation(text: &str, version: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut printed = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        if content
            .strip_prefix("navree ")
            .is_some_and(is_valid_version)
        {
            out.push_str("navree ");
            out.push_str(version);
            out.push_str(&line[content.len()..]);
            printed += 1;
        } else {
            out.push_str(line);
        }
    }
    if printed == 0 {
        return Err(format!(
            "{INSTALLATION}: no `navree <version>` line (the output of `navree --version`) to set"
        ));
    }
    replaced(
        INSTALLATION,
        "`navree = \"<version>\"`",
        replace_after(&out, "navree = \"", is_valid_version, version),
    )
}

/// The starter kits page with `kit_version = "<version>"` set to `version`.
pub fn set_kit_version(text: &str, version: &str) -> Result<String, String> {
    replaced(
        STARTER_KITS,
        "`kit_version = \"<version>\"`",
        replace_after(text, "kit_version = \"", is_valid_version, version),
    )
}

/// `text` of `page` with the series after `prefix` set to the series of
/// `version`.
fn set_series_after(page: &str, text: &str, prefix: &str, version: &str) -> Result<String, String> {
    replaced(
        page,
        &format!("series after `{prefix}`"),
        replace_after(text, prefix, is_series, &series(version)),
    )
}

/// The introduction with the series in "This documentation describes Navree
/// 1.0." set to the series of `version`.
pub fn set_introduction(text: &str, version: &str) -> Result<String, String> {
    set_series_after(
        INTRODUCTION,
        text,
        "This documentation describes Navree ",
        version,
    )
}

/// The roadmap with the series in "Where Navree goes after 1.0" set to the
/// series of `version`.
pub fn set_roadmap(text: &str, version: &str) -> Result<String, String> {
    set_series_after(ROADMAP, text, "Where Navree goes after ", version)
}

/// `text` of `page` with the version after `prefix` set to `version`, for a
/// place of [`FOLLOWING`].
pub fn set_following(
    page: &str,
    text: &str,
    prefix: &str,
    version: &str,
) -> Result<String, String> {
    replaced(
        page,
        &format!("version after `{prefix}`"),
        replace_after(text, prefix, is_valid_version, version),
    )
}

/// The configuration reference with the default `User-Agent` of the HTTP
/// client, `"navree-http-client/<version>"`, set to `version`. That is the
/// default as the generator writes it (`HttpClientOptions::default()` reads
/// the crate's `CARGO_PKG_VERSION`, and the table prints it as JSON in code
/// quotes); no other default in the page comes from the version.
pub fn set_reference(text: &str, version: &str) -> Result<String, String> {
    replaced(
        REFERENCE,
        "default `\"navree-http-client/<version>\"`",
        replace_after(text, "\"navree-http-client/", is_valid_version, version),
    )
}

/// The kind of release `version` is, for the first sentence of its notes.
fn kind(version: &str) -> &'static str {
    let numbers: Vec<&str> = version
        .split('-')
        .next()
        .unwrap_or(version)
        .split('.')
        .collect();
    if version.contains('-') {
        "pre-release"
    } else if numbers.get(2).is_some_and(|n| *n != "0") {
        "patch release"
    } else if numbers.get(1).is_some_and(|n| *n != "0") {
        "minor release"
    } else {
        "major release"
    }
}

/// Markdown for MDX: a `{` or `<` outside code spans would start an
/// expression or a tag, so it is escaped; links to the website's docs are
/// made site-relative like the links of the other pages.
fn mdx(line: &str) -> String {
    let line = line.replace("](https://navree.com/docs/", "](/docs/");
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    for c in line.chars() {
        match c {
            '`' => in_code = !in_code,
            '{' | '<' if !in_code => out.push('\\'),
            _ => {}
        }
        out.push(c);
    }
    out
}

#[derive(PartialEq)]
enum Block {
    Heading,
    Entry,
    Paragraph,
}

/// A changelog section body as the release notes print it: the headings
/// and the entries of the changelog, each entry on one line (the changelog
/// wraps them), with blank lines between blocks and none between entries.
fn reflow(body: &str) -> String {
    let mut blocks: Vec<(Block, String)> = Vec::new();
    let mut open = false;
    for line in body.lines() {
        let text = line.trim();
        if text.is_empty() {
            open = false;
        } else if text.starts_with("### ") {
            blocks.push((Block::Heading, text.to_owned()));
            open = false;
        } else if text.starts_with("- ") {
            let indent = &line[..line.len() - line.trim_start().len()];
            blocks.push((Block::Entry, format!("{indent}{text}")));
            open = true;
        } else if let (true, Some((_, last))) = (open, blocks.last_mut()) {
            last.push(' ');
            last.push_str(text);
        } else {
            blocks.push((Block::Paragraph, text.to_owned()));
            open = true;
        }
    }
    let mut out = String::new();
    for (i, (block, text)) in blocks.iter().enumerate() {
        if i > 0 {
            let joined = *block == Block::Entry && blocks[i - 1].0 == Block::Entry;
            out.push_str(if joined { "\n" } else { "\n\n" });
        }
        out.push_str(&mdx(text));
    }
    out.push('\n');
    out
}

/// The `## Navree <version>` section of the release notes, from the
/// changelog's section for `version`: a sentence on what kind of release it
/// is and how to update, then the changelog's headings and entries. `None`
/// when the changelog has no section for the version.
pub fn release_notes_section(changelog_text: &str, version: &str) -> Option<String> {
    let body = changelog::section_body(changelog_text, version)?;
    let update = if version.contains('-') {
        format!(
            "To try it, set `navree = \"{version}\"` in your app's `Cargo.toml` and run `cargo install navree-cli --version {version}` for the command-line tool."
        )
    } else {
        "To update, run `cargo update` in your app, which takes it within the range your `Cargo.toml` allows, and `cargo install navree-cli --force` for the command-line tool."
            .to_owned()
    };
    let mut section = format!(
        "## Navree {version}\n\nNavree {version} is a {kind}. {update}\n",
        kind = kind(version)
    );
    if !body.trim().is_empty() {
        section.push('\n');
        section.push_str(&reflow(&body));
    }
    Some(section)
}

/// The release notes with the section for `version` above the newest one,
/// written from `changelog_text`. Unchanged when the page has the section.
pub fn add_release_notes(
    text: &str,
    version: &str,
    changelog_text: &str,
) -> Result<String, String> {
    let heading = format!("## Navree {version}");
    if text.lines().any(|line| line.trim_end() == heading) {
        return Ok(text.to_owned());
    }
    let section = release_notes_section(changelog_text, version).ok_or_else(|| {
        format!("CHANGELOG.md has no section for {version}; the release notes are written from it")
    })?;
    let mut block = format!("{section}\n");
    if text.contains("\r\n") {
        block = block.replace('\n', "\r\n");
    }
    let mut out = String::with_capacity(text.len() + block.len());
    let mut placed = false;
    for line in text.split_inclusive('\n') {
        if !placed && line.starts_with("## Navree ") {
            out.push_str(&block);
            placed = true;
        }
        out.push_str(line);
    }
    if !placed {
        return Err(format!(
            "{RELEASE_NOTES}: no `## Navree <version>` section to put the new one above"
        ));
    }
    Ok(out)
}

/// Sets the version in the docs and adds the release notes; the paths of
/// the files that changed.
pub fn update(root: &Path, version: &str, changelog_text: &str) -> Result<Vec<PathBuf>, String> {
    type Edit<'a> = Box<dyn Fn(&str) -> Result<String, String> + 'a>;
    let mut edits: Vec<(&str, Edit<'_>)> = vec![
        (INSTALLATION, Box::new(|t| set_installation(t, version))),
        (STARTER_KITS, Box::new(|t| set_kit_version(t, version))),
        (INTRODUCTION, Box::new(|t| set_introduction(t, version))),
        (ROADMAP, Box::new(|t| set_roadmap(t, version))),
        (REFERENCE, Box::new(|t| set_reference(t, version))),
        (
            RELEASE_NOTES,
            Box::new(|t| add_release_notes(t, version, changelog_text)),
        ),
    ];
    for &(page, prefix) in FOLLOWING {
        edits.push((
            page,
            Box::new(move |t| set_following(page, t, prefix, version)),
        ));
    }
    // Every page is computed before any is written, so a page that cannot
    // be updated leaves the docs as they were.
    let mut new_pages = Vec::new();
    for (page, edit) in edits {
        let file = root.join(page);
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let new = edit(&text)?;
        if new != text {
            new_pages.push((file, new));
        }
    }
    let mut changed = Vec::new();
    for (file, new) in new_pages {
        std::fs::write(&file, new).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
        changed.push(file);
    }
    Ok(changed)
}
