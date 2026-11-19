//! Section-wide checks across `docs/plugins/*.mdx`: front-matter `title` and one-sentence
//! `description`, closed fences, and no private section identifiers in prose.

use std::fs;
use std::path::{Path, PathBuf};

fn plugins_mdx_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("read docs/plugins directory")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("mdx"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "expected pages in docs/plugins");
    paths
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        trimmed[1..trimmed.len() - 1].replace("\\\"", "\"")
    } else {
        trimmed.to_owned()
    }
}

fn parse_front_matter(page: &str, file_name: &str) -> (String, String, usize) {
    let rest = page
        .strip_prefix("---\n")
        .unwrap_or_else(|| panic!("{file_name}: missing opening front-matter fence"));
    let end = rest
        .find("\n---\n")
        .unwrap_or_else(|| panic!("{file_name}: missing closing front-matter fence"));
    let header = &rest[..end];
    let body_start_line = 2 + header.lines().count();

    let mut title = None;
    let mut description = None;
    for line in header.lines() {
        if let Some(value) = line.strip_prefix("title:") {
            title = Some(unquote(value));
        } else if let Some(value) = line.strip_prefix("description:") {
            description = Some(unquote(value));
        }
    }

    let title = title.unwrap_or_else(|| panic!("{file_name}: missing front-matter `title`"));
    let description =
        description.unwrap_or_else(|| panic!("{file_name}: missing front-matter `description`"));
    (title, description, body_start_line)
}

/// Returns `(line_number, line)` for every line of `page` after line 1 that lies outside a
/// fenced code block (matching the fence rules of `skill-snippets`).
fn non_code_lines<'a>(page: &'a str, file_name: &str) -> Vec<(usize, &'a str)> {
    let mut out = Vec::new();
    let mut open_ticks: Option<(usize, usize)> = None;
    for (index, line) in page.split('\n').enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim_start();
        let ticks = trimmed.chars().take_while(|&ch| ch == '`').count();
        match open_ticks {
            None if ticks >= 3 => {
                open_ticks = Some((line_no, ticks));
            }
            None => out.push((line_no, line)),
            Some((_, opened)) => {
                if ticks >= opened && trimmed.trim_end().len() == ticks {
                    open_ticks = None;
                }
            }
        }
    }
    if let Some((opened_at, _)) = open_ticks {
        panic!("{file_name}: fenced code block opened on line {opened_at} never closes");
    }
    out
}

/// Finds a whole-word token made of an uppercase letter from the private-document set,
/// an optional dot, and one or two ASCII digits.
fn first_private_id(line: &str) -> Option<&str> {
    const LETTERS: &[u8] = b"FGHIKMN";
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if LETTERS.contains(&bytes[i]) {
            let prev_is_word = i > 0
                && (bytes[i - 1].is_ascii_alphanumeric()
                    || bytes[i - 1] == b'_'
                    || bytes[i - 1] == b'-');
            if !prev_is_word {
                let mut cursor = i + 1;
                if cursor < bytes.len() && bytes[cursor] == b'.' {
                    cursor += 1;
                }
                let digits_start = cursor;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                let digit_count = cursor - digits_start;
                if (1..=2).contains(&digit_count) {
                    let next_continues = if cursor >= bytes.len() {
                        false
                    } else if bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_' {
                        true
                    } else if bytes[cursor] == b'.' && cursor + 1 < bytes.len() {
                        bytes[cursor + 1].is_ascii_alphanumeric()
                    } else {
                        false
                    };
                    if !next_continues {
                        return Some(&line[i..cursor]);
                    }
                }
            }
        }
        i += 1;
    }
    None
}

#[test]
fn every_plugins_page_has_a_title_a_description_and_no_private_id() {
    for path in plugins_mdx_files() {
        let name = path
            .file_name()
            .and_then(|file| file.to_str())
            .unwrap_or("unknown.mdx");
        let raw = fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {name}: {err}"));
        let page = raw.replace("\r\n", "\n");

        let (title, description, body_start_line) = parse_front_matter(&page, name);
        assert!(!title.trim().is_empty(), "{name}: empty `title`");
        assert!(
            !description.trim().is_empty(),
            "{name}: empty `description`"
        );
        let char_count = description.chars().count();
        assert!(
            char_count <= 300,
            "{name}: `description` is {char_count} characters (at most 300): {description:?}"
        );
        let without_end = description
            .trim()
            .strip_suffix('.')
            .unwrap_or(description.trim());
        assert!(
            !without_end.contains(". ")
                && !without_end.contains("! ")
                && !without_end.contains("? "),
            "{name}: `description` must be one sentence: {description:?}"
        );

        for (line_no, line) in non_code_lines(&page, name) {
            if line_no <= body_start_line {
                continue;
            }
            if let Some(found) = first_private_id(line) {
                panic!(
                    "{name}:{line_no}: unexpected private identifier `{found}` in prose: {line}"
                );
            }
        }
    }
}
