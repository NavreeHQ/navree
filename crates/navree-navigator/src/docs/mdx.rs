//! Docs pages from their MDX source: front-matter, the site's components
//! turned into plain Markdown, and the page split into its `##` sections.
//!
//! | Component | Markdown |
//! |---|---|
//! | `<Callout type title>` | a block quote that starts with "**Tip: title**" |
//! | `<Tabs>` / `<Tab label>` | a bold line per tab |
//! | `<Steps>` / `<Step title>` | a bold line per step |
//! | `<LinkCards>` / `<LinkCard href title>` | a list of links |
//! | `<FileTree>` | a `text` code block |
//! | `<Diagram>`, `<CodeGroup>`, other tags | removed (their text stays) |
//!
//! Root-relative links become absolute (`https://navree.com/docs/...`).

use super::{DocPage, DocSection, SITE};

/// Converts the MDX page `slug` into a [`DocPage`].
pub fn page_from_mdx(slug: &str, source: &str) -> DocPage {
    let source = source.replace("\r\n", "\n");
    let (front, body) = split_front_matter(&source);
    let title = front_value(front, "title").unwrap_or_else(|| slug.to_owned());
    let description = front_value(front, "description").unwrap_or_default();
    let planned = front_value(front, "status").as_deref() == Some("planned");
    let markdown = to_markdown(body);
    DocPage {
        slug: slug.to_owned(),
        title,
        description,
        planned,
        sections: split_sections(&markdown),
    }
}

/// (front-matter, body) of a page that starts with `---`.
fn split_front_matter(source: &str) -> (&str, &str) {
    let Some(rest) = source.strip_prefix("---\n") else {
        return ("", source);
    };
    match rest.find("\n---\n") {
        Some(end) => (&rest[..end], &rest[end + 5..]),
        None => ("", source),
    }
}

/// The value of `key: "value"` in front-matter.
fn front_value(front: &str, key: &str) -> Option<String> {
    front.lines().find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix(':')?.trim();
        Some(unquote(value))
    })
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value[1..value.len() - 1].replace("\\\"", "\"")
    } else {
        value.to_owned()
    }
}

/// The value of attribute `name` in a tag (`label="Claude Code"`).
fn attribute(tag: &str, name: &str) -> Option<String> {
    let pattern = format!("{name}=");
    let mut search = tag;
    while let Some(at) = search.find(&pattern) {
        let before = search[..at].chars().last();
        let rest = &search[at + pattern.len()..];
        if before.is_some_and(|c| c.is_whitespace()) {
            let quote = rest.chars().next()?;
            if quote == '"' || quote == '\'' {
                let end = rest[1..].find(quote)?;
                return Some(rest[1..=end].to_owned());
            }
            if let Some(inner) = rest.strip_prefix('{') {
                let end = inner.find('}')?;
                return Some(unquote(inner[..end].trim().trim_matches('`')));
            }
        }
        search = rest;
    }
    None
}

/// The component name of a line that is only a tag (`<Tab label="x">`,
/// `</Tabs>`, `<Diagram name="y" />`), with whether it closes.
fn tag_line(line: &str) -> Option<(&str, bool)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
        return None;
    }
    let closing = trimmed.starts_with("</");
    let name_start = if closing { 2 } else { 1 };
    let name: &str = trimmed[name_start..]
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()?;
    if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        Some((name, closing))
    } else {
        None
    }
}

/// The page body as plain Markdown.
fn to_markdown(body: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut fence: Option<String> = None;
    let mut quote_depth = 0usize;
    let mut in_file_tree = false;
    let mut link_card: Option<(String, String, Vec<String>)> = None;
    for line in body.lines() {
        if let Some(marker) = &fence {
            let prefix = if quote_depth > 0 { "> " } else { "" };
            out.push(format!("{prefix}{line}"));
            if line.trim_start().starts_with(marker.as_str()) {
                fence = None;
            }
            continue;
        }
        let trimmed = line.trim_start();
        if in_file_tree {
            if trimmed.starts_with("`}</FileTree>") || trimmed.starts_with("</FileTree>") {
                out.push("```".to_owned());
                in_file_tree = false;
            } else {
                out.push(line.to_owned());
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker: String = trimmed.chars().take_while(|c| *c == '`' || *c == '~').collect();
            fence = Some(marker);
            let prefix = if quote_depth > 0 { "> " } else { "" };
            out.push(format!("{prefix}{}", strip_code_title(line)));
            continue;
        }
        if trimmed.starts_with("import ") && trimmed.contains(" from ") {
            continue;
        }
        if trimmed.starts_with("<FileTree") {
            in_file_tree = true;
            out.push("```text".to_owned());
            continue;
        }
        if let Some((name, closing)) = tag_line(line) {
            match (name, closing) {
                ("Tab", false) => {
                    if let Some(label) = attribute(trimmed, "label") {
                        out.push(format!("**{label}**"));
                    }
                }
                ("Step", false) => {
                    if let Some(title) = attribute(trimmed, "title") {
                        out.push(format!("**{title}**"));
                    }
                }
                ("Callout", false) => {
                    let kind = attribute(trimmed, "type").unwrap_or_else(|| "note".to_owned());
                    let mut kind_title = kind[..1].to_uppercase() + &kind[1..];
                    if let Some(title) = attribute(trimmed, "title") {
                        kind_title = format!("{kind_title}: {title}");
                    }
                    out.push(format!("> **{kind_title}**"));
                    out.push(">".to_owned());
                    quote_depth += 1;
                }
                ("Callout", true) => quote_depth = quote_depth.saturating_sub(1),
                ("LinkCard", false) => {
                    let href = attribute(trimmed, "href").unwrap_or_default();
                    let title = attribute(trimmed, "title").unwrap_or_else(|| href.clone());
                    link_card = Some((title, absolute(&href), Vec::new()));
                }
                ("LinkCard", true) => {
                    if let Some((title, href, text)) = link_card.take() {
                        let text = text.join(" ");
                        let text = text.trim();
                        if text.is_empty() {
                            out.push(format!("- [{title}]({href})"));
                        } else {
                            out.push(format!("- [{title}]({href}): {text}"));
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let converted = absolute_links(&strip_inline_tags(line));
        if let Some((_, _, text)) = &mut link_card {
            text.push(converted.trim().to_owned());
            continue;
        }
        if quote_depth > 0 {
            let trimmed = converted.trim();
            out.push(if trimmed.is_empty() { ">".to_owned() } else { format!("> {trimmed}") });
        } else {
            out.push(converted);
        }
    }
    let mut markdown = out.join("\n");
    while markdown.contains("\n\n\n") {
        markdown = markdown.replace("\n\n\n", "\n\n");
    }
    markdown.trim().to_owned()
}

/// A code fence line without its `title="..."` meta.
fn strip_code_title(line: &str) -> String {
    match line.find(" title=") {
        Some(at) => line[..at].to_owned(),
        None => line.to_owned(),
    }
}

/// Removes inline components (`<Badge>queue</Badge>` keeps `queue`) outside
/// code spans.
fn strip_inline_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '`' {
            in_code = !in_code;
            out.push(c);
            continue;
        }
        if !in_code && c == '<' {
            let rest = &line[i + 1..];
            let name = rest.trim_start_matches('/');
            if name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && let Some(end) = rest.find('>')
            {
                for _ in 0..=end {
                    chars.next();
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// `/docs/x/` → `https://navree.com/docs/x/`; other links unchanged.
fn absolute(href: &str) -> String {
    if href.starts_with('/') && !href.starts_with("//") {
        format!("{SITE}{href}")
    } else {
        href.to_owned()
    }
}

/// Every `](/...)` link made absolute.
fn absolute_links(line: &str) -> String {
    line.replace("](/", &format!("]({SITE}/"))
}

/// Splits Markdown into its `## ` sections; text before the first heading
/// is a section without a heading.
fn split_sections(markdown: &str) -> Vec<DocSection> {
    let mut sections = Vec::new();
    let mut heading = String::new();
    let mut lines: Vec<&str> = Vec::new();
    let mut fence = false;
    let mut flush = |heading: &str, lines: &mut Vec<&str>| {
        let text = lines.join("\n").trim().to_owned();
        if !text.is_empty() || !heading.is_empty() {
            sections.push(DocSection {
                anchor: slugify(heading),
                heading: heading.to_owned(),
                markdown: text,
            });
        }
        lines.clear();
    };
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fence = !fence;
        }
        if !fence && let Some(title) = line.strip_prefix("## ") {
            flush(&heading, &mut lines);
            heading = title.trim().to_owned();
            continue;
        }
        lines.push(line);
    }
    flush(&heading, &mut lines);
    sections
}

/// The anchor of a heading, as the site makes it: lower case, punctuation
/// dropped, spaces as hyphens.
pub fn slugify(heading: &str) -> String {
    heading
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"---
title: "Navigator"
description: "`navree mcp`: the MCP server for your coding agent."
status: "planned"
---

import { Thing } from "../x";

## Introduction

Navigator answers from the app. See [skills](/docs/ai/agent-skills/).

<Tabs>
<Tab label="Claude Code">

```sh title="terminal"
claude mcp add navree -- navree mcp
```

</Tab>
</Tabs>

<Callout type="tip" title="Read-only">
Nothing is written. <Badge>dev</Badge> only.
</Callout>

## The tools

<Diagram name="module-graph" />

<FileTree title="skills/">{`
  navree/
    SKILL.md
`}</FileTree>

<LinkCards>
  <LinkCard href="/docs/ai/navigator/" title="Navigator">
    The MCP server.
  </LinkCard>
</LinkCards>
"#;

    #[test]
    fn front_matter_and_sections() {
        let page = page_from_mdx("ai/navigator", PAGE);
        assert_eq!(page.title, "Navigator");
        assert_eq!(page.description, "`navree mcp`: the MCP server for your coding agent.");
        assert!(page.planned);
        let headings: Vec<&str> = page.sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, ["Introduction", "The tools"]);
        assert_eq!(page.sections[1].anchor, "the-tools");
    }

    #[test]
    fn components_become_markdown() {
        let page = page_from_mdx("ai/navigator", PAGE);
        let intro = &page.sections[0].markdown;
        assert!(!intro.contains("import"), "{intro}");
        assert!(intro.contains("(https://navree.com/docs/ai/agent-skills/)"), "{intro}");
        assert!(intro.contains("**Claude Code**"), "{intro}");
        assert!(intro.contains("```sh\nclaude mcp add navree -- navree mcp\n```"), "{intro}");
        assert!(intro.contains("> **Tip: Read-only**"), "{intro}");
        assert!(intro.contains("> Nothing is written. dev only."), "{intro}");
        assert!(!intro.contains("<Tab"), "{intro}");
        let tools = &page.sections[1].markdown;
        assert!(!tools.contains("Diagram"), "{tools}");
        assert!(tools.contains("```text\n  navree/\n    SKILL.md\n```"), "{tools}");
        assert!(
            tools.contains("- [Navigator](https://navree.com/docs/ai/navigator/): The MCP server."),
            "{tools}"
        );
    }

    #[test]
    fn code_is_left_alone() {
        let page = page_from_mdx(
            "x",
            "## Code\n\n```rust\nlet v: Vec<Thing> = Vec::new(); // <Tab label=\"no\">\n## not a heading\n```\n",
        );
        assert_eq!(page.sections.len(), 1);
        assert!(page.sections[0].markdown.contains("Vec<Thing>"));
        assert!(page.sections[0].markdown.contains("## not a heading"));
        assert!(page.sections[0].markdown.contains("<Tab label=\"no\">"));
    }

    #[test]
    fn slugs_follow_the_site() {
        assert_eq!(slugify("search_docs and read_doc"), "search_docs-and-read_doc");
        assert_eq!(slugify("What `leaves` your machine?"), "what-leaves-your-machine");
    }
}
