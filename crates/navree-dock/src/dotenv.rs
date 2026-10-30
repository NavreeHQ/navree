//! The settings as text: the `.env` block `navree dock up` keeps up to
//! date, and the lines `navree dock env` prints.

use std::path::Path;

use crate::error::DockError;
use crate::exec::shell_word;

/// The line that opens Dock's block in `.env`.
pub const BLOCK_START: &str = "# dock:start (written by navree dock up; edit dock.toml instead)";

/// The line that closes Dock's block in `.env`.
pub const BLOCK_END: &str = "# dock:end";

/// What [`update_dotenv`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DotenvUpdate {
    /// The new contents of `.env`.
    pub text: String,
    /// Whether they differ from the old ones.
    pub changed: bool,
    /// Settings that `.env` sets again below the block, so its value wins.
    pub overridden: Vec<String>,
}

/// `.env`'s text with Dock's block (between [`BLOCK_START`] and
/// [`BLOCK_END`]) holding `settings`: replaced where it is, else appended.
/// With no settings, the block is removed. Line endings follow the file.
pub fn update_dotenv(existing: &str, settings: &[(String, String)]) -> DotenvUpdate {
    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let lines: Vec<&str> = existing.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim_start().starts_with("# dock:start"));
    let end = start.and_then(|start| {
        lines[start..]
            .iter()
            .position(|line| line.trim() == BLOCK_END)
            .map(|offset| start + offset)
    });

    let mut block: Vec<String> = Vec::new();
    if !settings.is_empty() {
        block.push(BLOCK_START.to_owned());
        block.extend(
            settings
                .iter()
                .map(|(name, value)| format!("{name}={}", dotenv_value(value))),
        );
        block.push(BLOCK_END.to_owned());
    }

    let (before, after): (Vec<&str>, Vec<&str>) = match (start, end) {
        (Some(start), Some(end)) => (lines[..start].to_vec(), lines[end + 1..].to_vec()),
        _ => (lines.clone(), Vec::new()),
    };
    let mut out: Vec<String> = before.iter().map(|l| (*l).to_owned()).collect();
    if start.is_none() || end.is_none() {
        // Appending: one blank line between the file and the block.
        while out.last().is_some_and(|line| line.trim().is_empty()) {
            out.pop();
        }
        if !out.is_empty() && !block.is_empty() {
            out.push(String::new());
        }
    } else if block.is_empty() {
        // Removing: drop the blank line that separated the block.
        while out.last().is_some_and(|line| line.trim().is_empty()) {
            out.pop();
        }
    }
    out.extend(block);
    out.extend(after.iter().map(|l| (*l).to_owned()));

    let mut text = out.join(newline);
    if !text.is_empty() {
        text.push_str(newline);
    }
    let overridden = settings
        .iter()
        .filter(|(name, _)| {
            after
                .iter()
                .any(|line| entry_name(line).is_some_and(|key| key == name))
        })
        .map(|(name, _)| name.clone())
        .collect();
    DotenvUpdate {
        changed: text != existing,
        text,
        overridden,
    }
}

/// Rewrites `<root>/.env` with the block for `settings` (creating the file).
pub fn write_dotenv(root: &Path, settings: &[(String, String)]) -> Result<DotenvUpdate, DockError> {
    let path = root.join(".env");
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(DockError::io(path, error)),
    };
    let update = update_dotenv(&existing, settings);
    if update.changed {
        std::fs::write(&path, &update.text).map_err(|e| DockError::io(&path, e))?;
    }
    Ok(update)
}

/// The key of a `KEY=value` line (with an optional `export `), if it is one.
fn entry_name(line: &str) -> Option<&str> {
    let line = line.trim_start();
    if line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, _) = line.split_once('=')?;
    Some(key.trim())
}

/// A value as `.env` writes it: bare when that reads back the same (in
/// Navree and in a shell that sources the file), else single-quoted (taken
/// literally), else double-quoted with escapes.
fn dotenv_value(value: &str) -> String {
    let bare = value.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '_' | '-' | '.' | '/' | ':' | '@' | '%' | '+' | '=' | ',' | '~'
            )
    });
    if bare {
        value.to_owned()
    } else if !value.contains('\'') {
        format!("'{value}'")
    } else {
        let escaped = value
            .replace('\\', r"\\")
            .replace('"', "\\\"")
            .replace('$', r"\$");
        format!("\"{escaped}\"")
    }
}

/// The settings as `NAME=value` lines (the format of `.env` files and of
/// GitHub Actions' `$GITHUB_ENV`), or, with `export`, as shell `export`
/// lines with quoted values.
pub fn env_lines(settings: &[(String, String)], export: bool) -> String {
    settings
        .iter()
        .map(|(name, value)| {
            if export {
                format!("export {name}={}\n", shell_word(value))
            } else {
                format!("{name}={value}\n")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Vec<(String, String)> {
        vec![
            (
                "APP_DATABASE__URL".into(),
                "postgres://navree:navree@127.0.0.1:5432/crm_api".into(),
            ),
            ("APP_MAIL__TRANSPORT".into(), "smtp://127.0.0.1:1025".into()),
        ]
    }

    const BLOCK: &str = "# dock:start (written by navree dock up; edit dock.toml instead)\nAPP_DATABASE__URL=postgres://navree:navree@127.0.0.1:5432/crm_api\nAPP_MAIL__TRANSPORT=smtp://127.0.0.1:1025\n# dock:end\n";

    #[test]
    fn the_block_is_appended_after_a_blank_line() {
        let existing = "APP_KEY=base64:abc=\n";
        let update = update_dotenv(existing, &settings());
        assert_eq!(update.text, format!("APP_KEY=base64:abc=\n\n{BLOCK}"));
        assert!(update.changed);
        assert!(update.overridden.is_empty());
        assert_eq!(update_dotenv("", &settings()).text, BLOCK);
        assert_eq!(
            update_dotenv("APP_KEY=x", &settings()).text,
            format!("APP_KEY=x\n\n{BLOCK}")
        );
    }

    #[test]
    fn the_block_is_replaced_in_place() {
        let existing = "APP_KEY=x\n\n# dock:start (old wording)\nREDIS_URL=redis://127.0.0.1:6379\n# dock:end\nAPP_LOG__LEVEL=debug\n";
        let update = update_dotenv(existing, &settings());
        assert_eq!(
            update.text,
            format!("APP_KEY=x\n\n{BLOCK}APP_LOG__LEVEL=debug\n")
        );
        let again = update_dotenv(&update.text, &settings());
        assert!(!again.changed);
        assert_eq!(again.text, update.text);
    }

    #[test]
    fn no_settings_remove_the_block() {
        let existing = format!("APP_KEY=x\n\n{BLOCK}");
        assert_eq!(update_dotenv(&existing, &[]).text, "APP_KEY=x\n");
        assert_eq!(update_dotenv("APP_KEY=x\n", &[]).text, "APP_KEY=x\n");
        assert!(!update_dotenv("", &[]).changed);
    }

    #[test]
    fn windows_line_endings_are_kept() {
        let update = update_dotenv("APP_KEY=x\r\n", &settings());
        assert!(update.text.starts_with("APP_KEY=x\r\n\r\n# dock:start"));
        assert!(!update.text.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn keys_set_again_below_the_block_are_reported() {
        let existing = format!(
            "APP_DATABASE__URL=sqlite://a.db\n\n{BLOCK}export APP_MAIL__TRANSPORT=log\n# APP_DATABASE__URL=x\n"
        );
        let update = update_dotenv(&existing, &settings());
        assert_eq!(update.overridden, ["APP_MAIL__TRANSPORT"]);
    }

    #[test]
    fn values_are_quoted_only_when_needed() {
        assert_eq!(
            dotenv_value("redis://127.0.0.1:6379"),
            "redis://127.0.0.1:6379"
        );
        assert_eq!(dotenv_value("a b"), "'a b'");
        assert_eq!(dotenv_value("x #y"), "'x #y'");
        assert_eq!(dotenv_value("${HOME}"), "'${HOME}'");
        assert_eq!(dotenv_value("pg://h/db?a=1&b=2"), "'pg://h/db?a=1&b=2'");
        assert_eq!(dotenv_value(r#"it's "$5""#), r#""it's \"\$5\"""#);
        assert_eq!(dotenv_value(""), "");
    }

    #[test]
    fn env_lines_plain_and_exported() {
        let settings = vec![
            (
                "APP_SEARCH__URL".to_owned(),
                "http://127.0.0.1:7700".to_owned(),
            ),
            ("X".to_owned(), "a b".to_owned()),
        ];
        assert_eq!(
            env_lines(&settings, false),
            "APP_SEARCH__URL=http://127.0.0.1:7700\nX=a b\n"
        );
        assert_eq!(
            env_lines(&settings, true),
            "export APP_SEARCH__URL=http://127.0.0.1:7700\nexport X='a b'\n"
        );
    }

    #[test]
    fn write_dotenv_creates_and_updates_the_file() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let first = write_dotenv(dir.path(), &settings()).unwrap();
        assert!(first.changed);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".env")).unwrap(),
            BLOCK
        );
        let second = write_dotenv(dir.path(), &settings()).unwrap();
        assert!(!second.changed);
    }
}
