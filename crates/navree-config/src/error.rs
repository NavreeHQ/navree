//! [`ConfigError`]: everything that can go wrong while loading or reading configuration.

use std::path::{Path, PathBuf};

/// An error raised while loading configuration sources or reading a section.
///
/// Messages never contain configuration values: a value that fails to parse
/// may be a password or an API key.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// A `.env` file (or string given to [`parse_dotenv`](crate::parse_dotenv)) is malformed.
    #[error("invalid .env syntax{} on line {line}: {message}", in_file(.path.as_deref()))]
    Dotenv {
        /// The file, when the source came from one.
        path: Option<PathBuf>,
        /// The 1-based line number.
        line: usize,
        /// What is wrong.
        message: String,
    },
    /// A TOML file is malformed.
    #[error("invalid TOML in `{}` at line {line}, column {column}: {message}", .path.display())]
    Toml {
        /// The file.
        path: PathBuf,
        /// The 1-based line of the error.
        line: usize,
        /// The 1-based column of the error.
        column: usize,
        /// What is wrong (without the source excerpt, which may hold secrets).
        message: String,
    },
    /// [`Config::section`](crate::Config::section) found nothing at the key.
    #[error("configuration section `{key}` is missing")]
    MissingSection {
        /// The section key (a dotted path).
        key: String,
    },
    /// A section does not deserialize into the requested type.
    #[error("configuration section `{key}` is invalid: {message}")]
    Section {
        /// The section key (a dotted path).
        key: String,
        /// What is wrong.
        message: String,
    },
    /// [`ConfigSection::validate`](crate::ConfigSection::validate) rejected a section.
    #[error("configuration section `{key}` failed validation: {message}")]
    Validation {
        /// The section key.
        key: String,
        /// The message returned by `validate`.
        message: String,
    },
    /// A configuration file exists but cannot be read.
    #[error("cannot read configuration file `{}`: {source}", .path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
}

/// Masks the values serde and config-rs quote in their errors
/// (`invalid type: string "…"`), keeping keys and expected types.
pub(crate) fn redact_values(message: &str) -> String {
    const MARKER: &str = "string \"";
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = rest.find(MARKER) {
        out.push_str(&rest[..start]);
        out.push_str("string [redacted]");
        let quoted = &rest[start + MARKER.len()..];
        rest = match crate::dotenv::closing_quote(quoted, '"') {
            Some(end) => &quoted[end + 1..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// ` in `path`` when a path is known, else nothing.
fn in_file(path: Option<&Path>) -> String {
    path.map(|p| format!(" in `{}`", p.display()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::redact_values;

    #[test]
    fn redact_values_masks_quoted_strings_including_escaped_quotes() {
        assert_eq!(
            redact_values(
                r#"invalid type: string "pa\"ss hunter2", expected u16 for key `port`; string "x""#
            ),
            "invalid type: string [redacted], expected u16 for key `port`; string [redacted]"
        );
        // Field names are not values.
        assert_eq!(
            redact_values("missing configuration field \"url\""),
            "missing configuration field \"url\""
        );
    }
}
