//! The fenced code blocks of a docs page, and how a Rust block becomes a module. Shared by
//! `build.rs` (which writes the modules) and the crate's tests.

/// One fenced code block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// The line of the opening fence, from 1.
    pub line: usize,
    /// The first word of the info string: `rust`, `toml`, `text`, ...
    pub lang: String,
    /// The other words of the info string: `ignore`, `title="..."`, ...
    pub flags: Vec<String>,
    /// The lines between the fences, without the fence's indentation.
    pub code: String,
}

impl Block {
    /// A Rust block that is compiled: every `rust` block not marked `rust,ignore`.
    pub fn compiled(&self) -> bool {
        self.lang == "rust" && !self.ignored()
    }

    /// Marked `ignore`: shown, never compiled.
    pub fn ignored(&self) -> bool {
        self.flags.iter().any(|flag| flag == "ignore")
    }
}

/// Every fenced block of `text`, in order. A fence opened with N backticks closes on a line
/// of N or more backticks and nothing else, at any indentation. An unclosed fence is an
/// error naming its line.
pub fn blocks(text: &str) -> Result<Vec<Block>, String> {
    let mut out = Vec::new();
    let mut open: Option<(usize, usize, usize, String)> = None; // line, ticks, indent, info
    let mut code = String::new();
    for (index, line) in text.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let ticks = trimmed.chars().take_while(|c| *c == '`').count();
        match &open {
            None if ticks >= 3 => {
                open = Some((index + 1, ticks, indent, trimmed[ticks..].trim().to_owned()));
                code.clear();
            }
            None => {}
            Some((start, opened, at, info)) => {
                if ticks >= *opened && trimmed.trim_end().len() == ticks {
                    let mut words = info.split([',', ' ']).filter(|word| !word.is_empty());
                    out.push(Block {
                        line: *start,
                        lang: words.next().unwrap_or_default().to_owned(),
                        flags: words.map(str::to_owned).collect(),
                        code: std::mem::take(&mut code),
                    });
                    open = None;
                } else {
                    let cut = line.len().min(*at);
                    let strip = line[..cut].trim().is_empty();
                    code.push_str(if strip { &line[cut..] } else { line });
                    code.push('\n');
                }
            }
        }
    }
    match open {
        Some((line, ..)) => Err(format!("the fence opened on line {line} never closes")),
        None => Ok(out),
    }
}

/// Attributes that make a function a test: dropped, so the test's body is compiled as a
/// plain function and nothing runs.
const TEST_ATTRIBUTES: &[&str] = &[
    "test",
    "tokio::test",
    "navree::test",
    "ignore",
    "should_panic",
    "cfg(test)",
];

/// A block ready to be a module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    /// Inner attributes (`#![...]`), which go at the top of the module.
    pub inner: Vec<String>,
    /// The leading `use` statements and comments, which stay at the module's top level so
    /// the blocks below the page's first see them too.
    pub head: String,
    /// The rest of the block.
    pub body: String,
    /// Whether the rest is items (functions, types, impls) rather than a fragment: a
    /// fragment is wrapped in a function.
    pub items: bool,
}

/// Splits a block into the parts a module is built from.
pub fn prepare(code: &str) -> Prepared {
    let mut inner = Vec::new();
    let mut head = String::new();
    let mut body = String::new();
    let mut in_head = true;
    let mut in_use = false;
    for line in code.lines() {
        let trimmed = line.trim();
        if in_use {
            head.push_str(line);
            head.push('\n');
            if trimmed.ends_with(';') {
                in_use = false;
            }
            continue;
        }
        if trimmed.starts_with("#![") {
            inner.push(trimmed.to_owned());
            continue;
        }
        if in_head {
            if trimmed.is_empty() || trimmed.starts_with("//") {
                match trimmed.strip_prefix("//!") {
                    // An inner doc comment is only allowed first in a file: here it is a comment.
                    Some(rest) => {
                        head.push_str("//");
                        head.push_str(rest);
                    }
                    None => head.push_str(line),
                }
                head.push('\n');
                continue;
            }
            if trimmed.starts_with("use ") || trimmed.starts_with("pub use ") {
                head.push_str(line);
                head.push('\n');
                in_use = !trimmed.ends_with(';');
                continue;
            }
            in_head = false;
        }
        if let Some(name) = trimmed
            .strip_prefix("#[")
            .and_then(|rest| rest.strip_suffix(']'))
        {
            let path = name.split('(').next().unwrap_or(name).trim();
            if TEST_ATTRIBUTES.contains(&name) || TEST_ATTRIBUTES.contains(&path) {
                body.push('\n');
                continue;
            }
        }
        if let Some(rest) = line.trim_start().strip_prefix("//!") {
            body.push_str("//");
            body.push_str(rest);
        } else {
            body.push_str(line);
        }
        body.push('\n');
    }
    let items = body.lines().any(is_item_line);
    Prepared {
        inner,
        head,
        body,
        items,
    }
}

/// Whether a line, not indented, starts an item.
fn is_item_line(line: &str) -> bool {
    if line.starts_with(char::is_whitespace) {
        return false;
    }
    const STARTS: &[&str] = &[
        "fn ",
        "async fn ",
        "pub fn ",
        "pub async fn ",
        "pub(crate) fn ",
        "struct ",
        "pub struct ",
        "enum ",
        "pub enum ",
        "impl ",
        "impl<",
        "trait ",
        "pub trait ",
        "const ",
        "pub const ",
        "static ",
        "type ",
        "pub type ",
        "mod ",
        "pub mod ",
        "macro_rules!",
        "extern ",
        "#[",
    ];
    STARTS.iter().any(|start| line.starts_with(start))
}

/// The module's source for a prepared block: the page's blocks nest, each a module inside
/// the one before it, so a block sees what a reader of the page has seen.
pub fn module_source(prepared: &Prepared, line: usize, page: &str, next: Option<&str>) -> String {
    let mut code =
        String::from("#![allow(warnings, clippy::all, clippy::pedantic, clippy::nursery)] ");
    for attribute in &prepared.inner {
        code.push_str(attribute);
        code.push(' ');
    }
    code.push_str(&format!(
        "use super::*; // docs/{page}, the block at line {line}\n"
    ));
    code.push_str(&prepared.head);
    if prepared.items {
        code.push_str(&prepared.body);
    } else {
        code.push_str(&format!(
            "async fn snippet_{line}() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {{\n    let _ = {{\n"
        ));
        code.push_str(&prepared.body);
        code.push_str("\n    };\n    Ok(())\n}\n");
    }
    if let Some(next) = next {
        code.push_str(&format!("#[path = {next:?}]\nmod next;\n"));
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_blocks_with_their_language_flags_and_line() {
        let text = "# Title\n\n```rust\nfn a() {}\n```\n\ntext\n\n```rust,ignore\nfn b(\n```\n";
        let found = blocks(text).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].line, 3);
        assert_eq!(found[0].code, "fn a() {}\n");
        assert!(found[0].compiled());
        assert_eq!(found[1].flags, ["ignore"]);
        assert!(!found[1].compiled());
    }

    #[test]
    fn indented_blocks_lose_their_indentation_and_crlf_is_read() {
        let text = "1. Step:\r\n\r\n   ```rust\r\n   fn a() {\r\n       b();\r\n   }\r\n   ```\r\n";
        let found = blocks(text).unwrap();
        assert_eq!(found[0].code, "fn a() {\n    b();\n}\n");
    }

    #[test]
    fn an_unclosed_fence_is_an_error() {
        assert!(
            blocks("```rust\nfn a() {}\n")
                .unwrap_err()
                .contains("line 1")
        );
    }

    #[test]
    fn leading_uses_stay_at_the_top_and_the_rest_is_a_fragment() {
        let prepared = prepare(
            "use navree::ui::*;\nuse navree::ui::{\n    Button,\n    Card,\n};\n\nCard::new().body(Button::primary(\"Save\"))\n",
        );
        assert!(prepared.head.contains("use navree::ui::*;"));
        assert!(prepared.head.contains("    Card,\n};"));
        assert_eq!(
            prepared.body.trim(),
            "Card::new().body(Button::primary(\"Save\"))"
        );
        assert!(!prepared.items);
    }

    #[test]
    fn a_block_of_items_is_not_wrapped() {
        let prepared = prepare(
            "use navree::ui::*;\n\nstruct Page;\n\nimpl Page {\n    fn card() -> Card { Card::new() }\n}\n",
        );
        assert!(prepared.items);
        let source = module_source(&prepared, 3, "ui/x.mdx", None);
        assert!(!source.contains("async fn snippet_"));
        assert!(source.contains("struct Page;"));
    }

    #[test]
    fn a_fragment_is_wrapped_in_a_function_named_for_its_line() {
        let prepared = prepare("let n = 1;\nn + 1\n");
        let source = module_source(&prepared, 12, "components/x.mdx", Some("/out/next.rs"));
        assert!(source.contains("async fn snippet_12()"));
        assert!(source.contains("let _ = {\nlet n = 1;\nn + 1"));
        assert!(source.contains("#[path = \"/out/next.rs\"]\nmod next;"));
    }

    #[test]
    fn tests_become_plain_functions_and_inner_attributes_move_up() {
        let prepared =
            prepare("#![allow(dead_code)]\n//! A test.\n#[tokio::test]\nasync fn t() {}\n");
        assert_eq!(prepared.inner, ["#![allow(dead_code)]"]);
        assert!(prepared.items);
        assert_eq!(prepared.head, "// A test.\n");
        assert_eq!(prepared.body, "\nasync fn t() {}\n");
    }
}
