//! Pattern syntax and wildcard matching.

/// Checks that `pattern` is a valid pattern: one or more lower-case
/// segments separated by dots, each made of letters, digits, `_` and `-`.
/// With `wildcards`, a segment may be `*` (exactly one segment) and the last
/// segment may be `>` (one or more segments), as event patterns allow.
///
/// ```
/// use navree_micro::check_pattern;
///
/// assert!(check_pattern("deals.get", false).is_ok());
/// assert!(check_pattern("deal.*", true).is_ok());
/// assert!(check_pattern("deal.*", false).is_err());
/// assert!(check_pattern("Deals.Get", false).is_err());
/// ```
pub fn check_pattern(pattern: &str, wildcards: bool) -> Result<(), String> {
    if pattern.is_empty() {
        return Err("a pattern cannot be empty".to_owned());
    }
    if pattern.len() > 255 {
        return Err("a pattern is at most 255 characters".to_owned());
    }
    let segments: Vec<&str> = pattern.split('.').collect();
    let last = segments.len() - 1;
    for (i, segment) in segments.iter().enumerate() {
        match *segment {
            "" => return Err("segments are separated by single dots".to_owned()),
            "*" | ">" if !wildcards => {
                return Err(
                    "a message pattern names exactly one handler, so it has no wildcards"
                        .to_owned(),
                );
            }
            "*" => {}
            ">" if i == last => {}
            ">" => return Err("`>` may only be the last segment".to_owned()),
            segment => {
                if !segment.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'
                }) {
                    return Err(format!(
                        "segment `{segment}` may only hold lower-case letters, digits, `_` and `-`"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Whether `pattern` has a wildcard segment.
pub fn has_wildcard(pattern: &str) -> bool {
    pattern.split('.').any(|segment| segment == "*" || segment == ">")
}

/// Whether the subscription pattern `subscribed` (which may hold `*` and a
/// trailing `>`) matches the concrete pattern `pattern`.
///
/// ```
/// use navree_micro::pattern_matches;
///
/// assert!(pattern_matches("deal.*", "deal.won"));
/// assert!(!pattern_matches("deal.*", "deal.won.late"));
/// assert!(pattern_matches("deal.>", "deal.won.late"));
/// assert!(pattern_matches("deal.won", "deal.won"));
/// ```
pub fn pattern_matches(subscribed: &str, pattern: &str) -> bool {
    let mut wanted = subscribed.split('.');
    let mut actual = pattern.split('.');
    loop {
        match (wanted.next(), actual.next()) {
            (Some(">"), Some(_)) => return true,
            (Some("*"), Some(_)) => {}
            (Some(w), Some(a)) if w == a => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

/// The TCP route of `pattern` among `routes` (`deals.*`, `deals.>` or a
/// full pattern): the longest matching key wins.
pub(crate) fn route_for<'a, V>(
    routes: impl IntoIterator<Item = (&'a String, &'a V)>,
    pattern: &str,
) -> Option<&'a V> {
    routes
        .into_iter()
        .filter(|(key, _)| route_matches(key, pattern))
        .max_by_key(|(key, _)| key.len())
        .map(|(_, value)| value)
}

/// Whether TCP route key `key` covers `pattern`: a full pattern, or a
/// prefix ending in `.*` / `.>` that covers every pattern under it.
pub(crate) fn route_matches(key: &str, pattern: &str) -> bool {
    match key.strip_suffix(".*").or_else(|| key.strip_suffix(".>")) {
        Some(prefix) => {
            pattern == prefix
                || pattern
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('.'))
        }
        None if key == "*" || key == ">" => true,
        None => key == pattern,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn valid_patterns() {
        for pattern in ["deals", "deals.get", "deals.get.v2", "a-b.c_d.9"] {
            assert_eq!(check_pattern(pattern, false), Ok(()), "{pattern}");
        }
        for pattern in ["deal.*", "deal.>", "*.won", "*"] {
            assert_eq!(check_pattern(pattern, true), Ok(()), "{pattern}");
        }
    }

    #[test]
    fn invalid_patterns() {
        for (pattern, wildcards) in [
            ("", true),
            ("deals..get", true),
            (".deals", true),
            ("deals.", true),
            ("Deals.get", true),
            ("deals get", true),
            ("deals/get", true),
            ("deal.*", false),
            ("deal.>", false),
            ("deal.>.won", true),
        ] {
            assert!(check_pattern(pattern, wildcards).is_err(), "{pattern}");
        }
        assert!(check_pattern(&"a".repeat(256), false).is_err());
    }

    #[test]
    fn wildcards_match_one_or_more_segments() {
        assert!(pattern_matches("*", "deals"));
        assert!(!pattern_matches("*", "deals.get"));
        assert!(pattern_matches("*.won", "deal.won"));
        assert!(pattern_matches(">", "a.b.c"));
        assert!(!pattern_matches("deal.>", "deal"));
        assert!(!pattern_matches("deal.won", "deal.lost"));
        assert!(!pattern_matches("deal.won", "deal.won.x"));
        assert!(!pattern_matches("deal.won.x", "deal.won"));
        assert!(has_wildcard("deal.*") && has_wildcard("deal.>") && !has_wildcard("deal.won"));
    }

    #[test]
    fn the_longest_route_wins() {
        let routes = BTreeMap::from([
            ("deals.*".to_owned(), "deals:4000"),
            ("deals.reports.*".to_owned(), "reports:4000"),
            ("deals.get".to_owned(), "reader:4000"),
            ("companies.*".to_owned(), "companies:4000"),
        ]);
        assert_eq!(route_for(&routes, "deals.move"), Some(&"deals:4000"));
        assert_eq!(route_for(&routes, "deals.get"), Some(&"reader:4000"));
        assert_eq!(route_for(&routes, "deals.reports.build"), Some(&"reports:4000"));
        assert_eq!(route_for(&routes, "dealsx.get"), None);
        assert_eq!(route_for(&routes, "users.get"), None);
        assert!(route_matches("deals.*", "deals"));
        assert!(route_matches("*", "anything.at.all"));
    }
}
