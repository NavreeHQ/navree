//! Path normalisation: every disk operation goes through [`normalize`].

use crate::StorageError;

/// The longest normalised path accepted, in bytes (what object stores allow).
const MAX_PATH_BYTES: usize = 1024;

/// Normalises a relative file path:
///
/// - `\` is a separator like `/` (so Windows-style paths cannot escape the
///   root on any platform);
/// - leading, trailing and repeated separators and `.` segments are dropped;
/// - `..`, any other segment made only of dots and spaces (Windows trims
///   trailing dots and spaces, so `. .` or `...` could alias `..` or `.`),
///   control characters and paths over 1024 bytes are rejected.
///
/// With `allow_root`, an empty result (the disk root) is accepted, for list
/// prefixes; otherwise it is an error.
pub(crate) fn normalize(path: &str, allow_root: bool) -> Result<String, StorageError> {
    let invalid = |reason| StorageError::InvalidPath {
        path: path.to_owned(),
        reason,
    };
    if path.chars().any(char::is_control) {
        return Err(invalid("control characters are not allowed"));
    }
    let mut normalized = String::with_capacity(path.len());
    for segment in path.split(['/', '\\']) {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment.chars().all(|c| c == '.' || c == ' ') {
            return Err(invalid(
                "`..` and segments made only of dots or spaces are not allowed",
            ));
        }
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(segment);
    }
    if normalized.is_empty() && !allow_root {
        return Err(invalid("the path is empty"));
    }
    if normalized.len() > MAX_PATH_BYTES {
        return Err(invalid("the path is longer than 1024 bytes"));
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(path: &str) -> String {
        normalize(path, false).unwrap_or_else(|e| panic!("{path:?}: {e}"))
    }

    fn rejected(path: &str) -> &'static str {
        match normalize(path, true) {
            Err(StorageError::InvalidPath { reason, .. }) => reason,
            other => panic!("{path:?} must be rejected: {other:?}"),
        }
    }

    #[test]
    fn separators_and_dots_are_normalised() {
        assert_eq!(ok("a/b/c.txt"), "a/b/c.txt");
        assert_eq!(ok("/a//b/./c.txt/"), "a/b/c.txt");
        assert_eq!(ok(r"a\b\c.txt"), "a/b/c.txt");
        assert_eq!(ok("./a"), "a");
        assert_eq!(ok("a/.hidden/..file"), "a/.hidden/..file");
        assert_eq!(ok("my file (1).txt"), "my file (1).txt");
        assert_eq!(normalize("/", true).unwrap(), "");
        assert_eq!(normalize("./", true).unwrap(), "");
    }

    #[test]
    fn dot_segments_and_control_characters_are_rejected() {
        assert!(rejected("../a").contains(".."));
        assert!(rejected(r"a\..\..\b").contains(".."));
        assert!(rejected("a/.../b").contains("dots"));
        assert!(rejected("a/. ./b").contains("dots"));
        assert!(rejected("a/ /b").contains("spaces"));
        assert!(rejected("a\0b").contains("control"));
        assert!(rejected("a\r\nb").contains("control"));
        assert!(rejected(&"a".repeat(1025)).contains("1024"));
        assert!(matches!(
            normalize("/", false),
            Err(StorageError::InvalidPath {
                reason: "the path is empty",
                ..
            })
        ));
    }

    #[test]
    fn error_message_escapes_the_path() {
        let error = normalize("a\n../b", false).unwrap_err().to_string();
        assert!(error.contains(r#""a\n../b""#), "{error}");
    }
}
