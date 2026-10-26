//! Navree's own `.env` parser. It returns the entries; it never touches the
//! process environment (`std::env::set_var` is `unsafe` in edition 2024).

use crate::ConfigError;

/// Parses the contents of a `.env` file into `(KEY, value)` pairs, in file order.
///
/// The format:
///
/// - `KEY=value`; blank lines and lines starting with `#` are skipped; an
///   optional `export ` prefix is ignored; keys use ASCII letters, digits, `_`
///   and `.`, and do not start with a digit.
/// - Unquoted values are trimmed; ` #` (whitespace, then `#`) starts a comment.
///   Backslashes are literal (`C:\Users` stays as is).
/// - `"double quoted"` values may span lines and understand `\n`, `\r`, `\t`,
///   `\"`, `\\` and `\$`; other backslashes are kept.
/// - `'single quoted'` values may span lines and are taken literally.
/// - `${VAR}` in unquoted and double-quoted values expands to the latest
///   earlier entry `VAR` of the same source, or to nothing. The process
///   environment is neither read nor written.
///
/// A key that appears twice yields two entries; consumers let the later win.
/// Errors carry the line number but never the offending value.
pub fn parse_dotenv(src: &str) -> Result<Vec<(String, String)>, ConfigError> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let lines: Vec<&str> = src.lines().collect();
    let mut entries: Vec<(String, String)> = Vec::new();
    // Index of the next unread line, which is also the 1-based number of the
    // line just read.
    let mut next = 0;
    while next < lines.len() {
        let line = lines[next].trim_start();
        next += 1;
        let line_no = next;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, rest)) = strip_export(line).split_once('=') else {
            return Err(syntax(line_no, "expected `KEY=value`"));
        };
        let key = key.trim_end();
        validate_key(key).map_err(|message| syntax(line_no, message))?;
        let trimmed = rest.trim_start();
        let value = if let Some(body) = trimmed.strip_prefix('"') {
            let raw = quoted(body, '"', &lines, &mut next)?;
            expand(&raw, true, &entries).map_err(|message| syntax(line_no, message))?
        } else if let Some(body) = trimmed.strip_prefix('\'') {
            quoted(body, '\'', &lines, &mut next)?
        } else {
            expand(strip_inline_comment(rest).trim(), false, &entries)
                .map_err(|message| syntax(line_no, message))?
        };
        entries.push((key.to_owned(), value));
    }
    Ok(entries)
}

fn syntax(line: usize, message: impl Into<String>) -> ConfigError {
    ConfigError::Dotenv {
        path: None,
        line,
        message: message.into(),
    }
}

/// Drops a leading `export ` (shell syntax).
fn strip_export(line: &str) -> &str {
    match line.strip_prefix("export") {
        Some(rest) if rest.starts_with([' ', '\t']) => rest.trim_start(),
        _ => line,
    }
}

fn validate_key(key: &str) -> Result<(), &'static str> {
    let mut chars = key.chars();
    let valid = match chars.next() {
        None => return Err("missing key before `=`"),
        Some(first) => {
            (first.is_ascii_alphabetic() || first == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        }
    };
    if valid {
        Ok(())
    } else {
        Err("invalid key: use ASCII letters, digits, `_` and `.`, not starting with a digit")
    }
}

/// Cuts an unquoted value at ` #` (a `#` preceded by whitespace).
fn strip_inline_comment(value: &str) -> &str {
    let bytes = value.as_bytes();
    match (1..bytes.len()).find(|&i| bytes[i] == b'#' && bytes[i - 1].is_ascii_whitespace()) {
        Some(i) => &value[..i],
        None => value,
    }
}

/// Reads a quoted value whose opening quote has been consumed; `first` is the
/// rest of its first line. Continues on the following lines (advancing `next`)
/// until the closing quote. Returns the raw body, escapes untouched.
fn quoted(
    first: &str,
    quote: char,
    lines: &[&str],
    next: &mut usize,
) -> Result<String, ConfigError> {
    let start = *next;
    let mut body = String::new();
    let mut text = first;
    loop {
        if let Some(end) = closing_quote(text, quote) {
            body.push_str(&text[..end]);
            let trailing = text[end + 1..].trim_start();
            if !(trailing.is_empty() || trailing.starts_with('#')) {
                return Err(syntax(
                    *next,
                    "unexpected characters after the closing quote",
                ));
            }
            return Ok(body);
        }
        body.push_str(text);
        body.push('\n');
        let Some(line) = lines.get(*next) else {
            return Err(syntax(start, "unterminated quoted value"));
        };
        text = line;
        *next += 1;
    }
}

/// The byte index of the closing `quote` in `text`; `\"` does not close a
/// double-quoted value.
pub(crate) fn closing_quote(text: &str, quote: char) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if quote == '"' && c == '\\' {
            escaped = true;
        } else if c == quote {
            return Some(i);
        }
    }
    None
}

/// Expands `${VAR}` from earlier `entries` and, when `escapes` is set,
/// processes the double-quote escape sequences.
fn expand(raw: &str, escapes: bool, entries: &[(String, String)]) -> Result<String, &'static str> {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if escapes => {
                let escaped = match chars.peek() {
                    Some('n') => Some('\n'),
                    Some('r') => Some('\r'),
                    Some('t') => Some('\t'),
                    Some(&c @ ('"' | '\\' | '$')) => Some(c),
                    _ => None,
                };
                match escaped {
                    Some(c) => {
                        chars.next();
                        out.push(c);
                    }
                    None => out.push('\\'),
                }
            }
            '$' if chars.peek() == Some(&'{') => {
                chars.next();
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => name.push(c),
                        None => return Err("unterminated `${`"),
                    }
                }
                let name = name.trim();
                if name.is_empty() {
                    return Err("empty variable name in `${}`");
                }
                if let Some((_, value)) = entries.iter().rev().find(|(key, _)| key == name) {
                    out.push_str(value);
                }
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn dotenv_parses_quotes_comments_export_and_expansion() {
        let src = r#"
# a full-line comment
APP_NAME=Navree
export APP_ENV=production
  APP_URL = http://localhost:3000   # trailing comment
APP_HASH=abc#def
APP_HOST=example.com
APP_DATABASE__URL="postgres://${APP_HOST}:5432/db"
APP_PATH=${APP_HOST}/path
APP_LITERAL='${APP_HOST} stays # literal'
APP_ESCAPED="tab\there \"quoted\" \$HOME \\ C:\Users"
APP_WINDOWS=C:\Users\navree
APP_EMPTY=
APP_EMPTY_QUOTED=""
APP_UNDEFINED="${NOT_DEFINED}x"
APP_HOST=override.example.com
APP_AGAIN=${APP_HOST}
"#;
        let parsed = parse_dotenv(src).expect("valid .env");
        assert_eq!(
            parsed,
            pairs(&[
                ("APP_NAME", "Navree"),
                ("APP_ENV", "production"),
                ("APP_URL", "http://localhost:3000"),
                ("APP_HASH", "abc#def"),
                ("APP_HOST", "example.com"),
                ("APP_DATABASE__URL", "postgres://example.com:5432/db"),
                ("APP_PATH", "example.com/path"),
                ("APP_LITERAL", "${APP_HOST} stays # literal"),
                ("APP_ESCAPED", "tab\there \"quoted\" $HOME \\ C:\\Users"),
                ("APP_WINDOWS", "C:\\Users\\navree"),
                ("APP_EMPTY", ""),
                ("APP_EMPTY_QUOTED", ""),
                ("APP_UNDEFINED", "x"),
                ("APP_HOST", "override.example.com"),
                ("APP_AGAIN", "override.example.com"),
            ])
        );
    }

    #[test]
    fn dotenv_supports_multiline_quoted_values_crlf_and_bom() {
        let src = "\u{feff}APP_KEY=\"-----BEGIN-----\r\nabc\r\n-----END-----\"\r\n\
                   APP_RAW='one\r\ntwo' # note\r\n\
                   APP_NEXT=1\r\n";
        assert_eq!(
            parse_dotenv(src).expect("valid .env"),
            pairs(&[
                ("APP_KEY", "-----BEGIN-----\nabc\n-----END-----"),
                ("APP_RAW", "one\ntwo"),
                ("APP_NEXT", "1"),
            ])
        );
    }

    #[test]
    fn dotenv_reports_syntax_errors_with_line_numbers_but_not_values() {
        let cases = [
            ("APP_A=1\nhunter2\n", 2, "expected `KEY=value`"),
            ("APP_A=1\n\n1BAD=hunter2\n", 3, "invalid key"),
            (
                "APP_A=\"hunter2\nstill open\n",
                1,
                "unterminated quoted value",
            ),
            (
                "APP_A=1\nAPP_B=\"hunter2\" trailing\n",
                2,
                "after the closing quote",
            ),
            ("APP_A=${hunter2\n", 1, "unterminated `${`"),
        ];
        for (src, line, message) in cases {
            let error = parse_dotenv(src).expect_err(src);
            let text = error.to_string();
            assert!(
                matches!(error, ConfigError::Dotenv { line: l, path: None, .. } if l == line),
                "{src:?}: {text}"
            );
            assert!(text.contains(&format!("line {line}")), "{text}");
            assert!(text.contains(message), "{text}");
            assert!(!text.contains("hunter2"), "{text}");
        }
    }
}
