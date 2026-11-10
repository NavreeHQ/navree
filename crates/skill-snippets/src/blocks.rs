//! The fenced code blocks of a Markdown file, and how a Rust block becomes
//! a module. Shared by `build.rs` (which writes the modules) and the crate's
//! tests (which check the markers).

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
    /// A Rust block that is compiled: every `rust` block not marked
    /// `rust,ignore`.
    pub fn compiled(&self) -> bool {
        self.lang == "rust" && !self.ignored()
    }

    /// Marked `ignore`: shown, never compiled.
    pub fn ignored(&self) -> bool {
        self.flags.iter().any(|flag| flag == "ignore")
    }
}

/// Every fenced block of `text`, in order. A fence opened with N backticks
/// closes on a line of N or more backticks and nothing else, at any
/// indentation. An unclosed fence is an error naming its line.
pub fn blocks(text: &str) -> Result<Vec<Block>, String> {
    let mut out = Vec::new();
    let mut open: Option<(usize, usize, usize, String)> = None; // line, ticks, indent, info
    let mut code = String::new();
    for (index, line) in text.split('\n').enumerate() {
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

/// Attributes that make a function a test: dropped, so the test's body is
/// compiled as a plain function and nothing runs.
const TEST_ATTRIBUTES: &[&str] = &[
    "test",
    "tokio::test",
    "navree::test",
    "ignore",
    "should_panic",
    "cfg(test)",
];

/// The block's code as a module body: inner attributes (`#![...]`) are moved
/// to the top, inner doc comments become plain comments, and test
/// attributes go, so a test is checked like any other function.
pub fn module_body(code: &str) -> (Vec<String>, String) {
    let mut inner = Vec::new();
    let mut body = String::with_capacity(code.len());
    for line in code.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#![") {
            inner.push(trimmed.to_owned());
            body.push('\n'); // keep the line numbers
            continue;
        }
        if let Some(name) = trimmed
            .strip_prefix("#[")
            .and_then(|rest| rest.strip_suffix(']'))
        {
            let path = name.split('(').next().unwrap_or(name).trim();
            if TEST_ATTRIBUTES.contains(&name) || TEST_ATTRIBUTES.contains(&path) {
                body.push('\n'); // keep the line numbers
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
    (inner, body)
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
    fn indented_blocks_lose_their_indentation() {
        let text = "1. Step:\n\n   ```rust\n   fn a() {\n       b();\n   }\n   ```\n";
        let found = blocks(text).unwrap();
        assert_eq!(found[0].code, "fn a() {\n    b();\n}\n");
    }

    #[test]
    fn a_longer_fence_holds_a_shorter_one() {
        let text = "````text\n```rust\nfn a() {}\n```\n````\n";
        let found = blocks(text).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].lang, "text");
        assert!(found[0].code.contains("```rust"));
    }

    #[test]
    fn titles_and_spaces_are_flags() {
        let found = blocks("```rust title=\"src/app.rs\"\nfn a() {}\n```\n").unwrap();
        assert_eq!(found[0].lang, "rust");
        assert_eq!(found[0].flags, ["title=\"src/app.rs\""]);
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
    fn tests_become_plain_functions_and_inner_attributes_move_up() {
        let code = "#![allow(dead_code)]\n//! A test.\n#[tokio::test]\nasync fn t() {}\n#[cfg(test)]\nmod m {}\n#[cfg(feature = \"x\")]\nfn f() {}\n";
        let (inner, body) = module_body(code);
        assert_eq!(inner, ["#![allow(dead_code)]"]);
        assert_eq!(
            body,
            "\n// A test.\n\nasync fn t() {}\n\nmod m {}\n#[cfg(feature = \"x\")]\nfn f() {}\n"
        );
    }
}
