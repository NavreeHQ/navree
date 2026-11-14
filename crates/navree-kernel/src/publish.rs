//! Publishing the files a plugin ships into the app: the rule every published
//! path follows.

use std::collections::HashSet;

/// Whether `path` is a safe relative path inside a publish folder: non-empty,
/// at most 255 bytes, with `/` as the only separator; every segment non-empty,
/// at most 128 bytes and neither `.` nor `..`; no `\`, `:` or control
/// character; none of `<>"|?*`; no segment ending in `.` or a space; no
/// segment whose part before its first `.`, without trailing spaces, is
/// `CON`, `PRN`, `AUX`, `NUL`, `COM0` to `COM9` or `LPT0` to `LPT9` in any
/// case. Not public API; the boot check, `plugin:publish` and Navree's own
/// macros use it.
pub fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 255
        && !path.chars().any(is_refused_char)
        && path.split('/').all(is_safe_segment)
}

/// A character no published path holds: `\`, `:`, `<>"|?*` or a control
/// character.
fn is_refused_char(c: char) -> bool {
    c.is_control() || matches!(c, '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
}

/// Whether one segment of a published path is safe (see
/// [`is_safe_relative_path`]).
fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 128
        && segment != "."
        && segment != ".."
        && !segment.ends_with('.')
        && !segment.ends_with(' ')
        && !is_device_name(segment)
}

/// Whether `segment` names a Windows device: its part before the first `.`,
/// without trailing spaces, is `CON`, `PRN`, `AUX`, `NUL`, `COM0` to `COM9` or
/// `LPT0` to `LPT9`, ignoring ASCII case.
fn is_device_name(segment: &str) -> bool {
    let stem = segment
        .split_once('.')
        .map_or(segment, |(stem, _)| stem)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    match stem.as_bytes() {
        b"CON" | b"PRN" | b"AUX" | b"NUL" => true,
        [b'C', b'O', b'M', digit] | [b'L', b'P', b'T', digit] => digit.is_ascii_digit(),
        _ => false,
    }
}

/// Whether `path`, a file under the `locales` tag, starts with a locale: at
/// least two segments, the first a language of 2 to 8 letters followed by any
/// number of `-` and 1 to 8 letters or digits (`en`, `pa-IN`, `zh-Hant-TW`).
pub(crate) fn is_locale_path(path: &str) -> bool {
    let Some((locale, _)) = path.split_once('/') else {
        return false;
    };
    let mut parts = locale.split('-');
    parts.next().is_some_and(|language| {
        (2..=8).contains(&language.len()) && language.bytes().all(|b| b.is_ascii_alphabetic())
    }) && parts.all(|part| {
        (1..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

/// The first of `paths` that equals an earlier one once both are lower-cased:
/// on a disk that ignores case, the two are one file.
pub(crate) fn first_case_collision<'a>(
    paths: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .find(|path| !seen.insert(path.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paths the rule accepts.
    fn accepted_paths() -> Vec<String> {
        let mut paths: Vec<String> = [
            "list.html",
            "admin/list.html",
            ".gitkeep",
            "a.b.c",
            "console.html",
            "con-list.html",
            "com10.css",
            "lpt.txt",
            "ਸੂਚਨਾ.html",
        ]
        .map(String::from)
        .to_vec();
        paths.push("a".repeat(128));
        paths.push(format!("{}/{}", "a".repeat(127), "b".repeat(127)));
        paths
    }

    /// Paths the rule refuses, one or more per shape.
    fn refused_paths() -> Vec<String> {
        let mut paths: Vec<String> = [
            "",
            "/",
            "/etc/navree.css",
            "../x",
            "a/../x",
            "./x",
            "a/./x",
            "a//b",
            "a/",
            "a\\b",
            "c:/x",
            "C:x",
            "banner.css:hidden",
            "a\u{1}b",
            "a\tb",
            "a\u{7f}b",
            "a<b",
            "a>b",
            "a\"b",
            "a|b",
            "a?b",
            "a*b",
            "list.html.",
            "list.html ",
            "...",
            ". ",
            "a/.../b",
            "con",
            "CON",
            "con.html",
            "Con.tar.gz",
            "nul",
            "aux.txt",
            "prn",
            "com0",
            "com1.css",
            "COM9",
            "lpt0",
            "lpt9",
            "Lpt5.log",
            "con .txt",
        ]
        .map(String::from)
        .to_vec();
        paths.push("a".repeat(129));
        paths.push(format!("{}/{}", "a".repeat(128), "b".repeat(127)));
        paths
    }

    #[test]
    fn the_publish_path_rule_accepts_plain_paths_and_refuses_every_unsafe_shape() {
        for good in accepted_paths() {
            assert!(is_safe_relative_path(&good), "{good:?}");
        }
        for bad in refused_paths() {
            assert!(!is_safe_relative_path(&bad), "{bad:?}");
        }
        for good in [
            "en/announcements.ftl",
            "pa-IN/admin/announcements.ftl",
            "zh-Hant-TW/x.ftl",
        ] {
            assert!(is_locale_path(good), "{good}");
        }
        for bad in [
            "announcements.ftl",
            "e/x.ftl",
            "en_GB/x.ftl",
            "123/x.ftl",
            "en-/x.ftl",
        ] {
            assert!(!is_locale_path(bad), "{bad}");
        }
        assert_eq!(
            first_case_collision(["list.html", "show.html", "List.html"]),
            Some("List.html")
        );
        assert_eq!(
            first_case_collision(["list.html", "list.html"]),
            Some("list.html")
        );
        assert_eq!(first_case_collision(["a/list.html", "b/list.html"]), None);
    }
}
