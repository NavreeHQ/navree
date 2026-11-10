//! Normalizers: messages without their variable parts, SQL statements
//! without literals, and the path patterns of `lighthouse.ignore.routes`.

/// `message` with its variable parts replaced, so `deal 0199c1d2-… not
/// found` and `deal 0199c1e7-… not found` read the same: quoted strings
/// become `"?"`, email addresses `<email>`, UUIDs `<uuid>`, hexadecimal ids
/// (eight or more hex digits, or `0x…`) `<hex>`, and numbers `<n>`.
///
/// ```
/// use navree_lighthouse::normalize_message;
///
/// assert_eq!(
///     normalize_message("deal 0199c1d2-6f7e-7a31-b5d4-2f0c8e9a1b23 not found"),
///     "deal <uuid> not found"
/// );
/// assert_eq!(
///     normalize_message("user 'anu@example.com' has 3 invoices"),
///     "user \"?\" has <n> invoices"
/// );
/// ```
pub fn normalize_message(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let chars: Vec<char> = message.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '\'' | '"' | '`') {
            if let Some(end) = chars[i + 1..].iter().position(|&d| d == c) {
                out.push_str("\"?\"");
                i += end + 2;
                continue;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if is_token_char(c) {
            let start = i;
            while i < chars.len() && is_token_char(chars[i]) {
                i += 1;
            }
            // A trailing dot ends a sentence, not the token.
            let mut end = i;
            while end > start && chars[end - 1] == '.' {
                end -= 1;
            }
            let token: String = chars[start..end].iter().collect();
            out.push_str(&normalize_token(&token));
            out.extend(&chars[end..i]);
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn is_token_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '@' | '+')
}

/// One word of a message, normalized.
fn normalize_token(token: &str) -> String {
    if is_email(token) {
        return "<email>".to_owned();
    }
    if is_uuid(token) {
        return "<uuid>".to_owned();
    }
    if is_hex_id(token) {
        return "<hex>".to_owned();
    }
    replace_numbers(token)
}

fn is_email(token: &str) -> bool {
    let Some((local, domain)) = token.split_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn is_uuid(token: &str) -> bool {
    let groups: Vec<&str> = token.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_hex_id(token: &str) -> bool {
    if let Some(rest) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit());
    }
    token.len() >= 8
        && token.chars().all(|c| c.is_ascii_hexdigit())
        && token.chars().any(|c| c.is_ascii_digit())
}

/// Replaces every run of digits (with a decimal part) by `<n>`.
fn replace_numbers(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    let mut chars = token.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            while let Some(&next) = chars.peek() {
                if next.is_ascii_digit() {
                    chars.next();
                } else if next == '.' {
                    let mut lookahead = chars.clone();
                    lookahead.next();
                    if lookahead.peek().is_some_and(char::is_ascii_digit) {
                        chars.next();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            out.push_str("<n>");
        } else {
            out.push(c);
        }
    }
    out
}

/// `sql` with its literals replaced by `?`: quoted strings, numbers and
/// numbered placeholders (`$1`); `IN (...)` lists and multi-row `VALUES`
/// collapse to one item; whitespace collapses to single spaces. Quoted
/// identifiers (`"deals"`) are kept.
///
/// ```
/// use navree_lighthouse::normalize_sql;
///
/// assert_eq!(
///     normalize_sql("SELECT * FROM \"deals\" WHERE id IN ($1, $2, $3) AND amount > 100"),
///     "SELECT * FROM \"deals\" WHERE id IN (?) AND amount > ?"
/// );
/// assert_eq!(
///     normalize_sql("select * from users where email = 'anu@example.com'"),
///     "select * from users where email = ?"
/// );
/// ```
pub fn normalize_sql(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    let mut pending_space = false;
    let push = |out: &mut String, text: &str, pending_space: &mut bool| {
        if *pending_space && !out.is_empty() {
            out.push(' ');
        }
        *pending_space = false;
        out.push_str(text);
    };
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            pending_space = true;
            i += 1;
            continue;
        }
        if c == '\'' {
            // A string literal ('' escapes a quote).
            i += 1;
            while i < chars.len() {
                if chars[i] == '\'' {
                    if chars.get(i + 1) == Some(&'\'') {
                        i += 2;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            i += 1;
            push(&mut out, "?", &mut pending_space);
            continue;
        }
        if c == '"' || c == '`' {
            // A quoted identifier: kept as written.
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != c {
                i += 1;
            }
            i = (i + 1).min(chars.len());
            let ident: String = chars[start..i].iter().collect();
            push(&mut out, &ident, &mut pending_space);
            continue;
        }
        if c == '$' && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            push(&mut out, "?", &mut pending_space);
            continue;
        }
        let previous_is_word = out
            .chars()
            .last()
            .is_some_and(|p| (p.is_alphanumeric() || p == '_') && !pending_space);
        if c.is_ascii_digit() && !previous_is_word {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            push(&mut out, "?", &mut pending_space);
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            push(&mut out, &word, &mut pending_space);
            continue;
        }
        let mut buf = [0u8; 4];
        push(&mut out, c.encode_utf8(&mut buf), &mut pending_space);
        i += 1;
    }
    collapse_lists(&out)
}

/// `(?, ?, ?)` becomes `(?)`, and `VALUES (?), (?)` becomes `VALUES (?)`.
fn collapse_lists(sql: &str) -> String {
    let mut text = sql.to_owned();
    loop {
        let next = text.replace("?, ?", "?").replace("?,?", "?");
        if next == text {
            break;
        }
        text = next;
    }
    loop {
        let next = text.replace("(?), (?)", "(?)").replace("(?),(?)", "(?)");
        if next == text {
            break;
        }
        text = next;
    }
    text
}

/// Whether `path` matches the pattern: exact, or with a trailing `*`
/// segment matching one or more segments (`/health/*` matches
/// `/health/live` and `/health/a/b`, not `/health`).
pub fn path_matches(pattern: &str, path: &str) -> bool {
    match pattern.strip_suffix("/*") {
        Some(prefix) => path
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some_and(|rest| !rest.is_empty()),
        None => pattern.trim_end_matches('/') == path.trim_end_matches('/'),
    }
}

/// The host of a URL (`https://api.example.com:8443/x` → `api.example.com:8443`).
pub(crate) fn url_host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    host.to_ascii_lowercase()
}

/// The kind of failure of an outgoing call without a response: `timeout`,
/// `connect` or `error`.
pub(crate) fn outgoing_error_kind(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        "timeout"
    } else if lower.contains("connect") || lower.contains("dns") || lower.contains("resolve") {
        "connect"
    } else {
        "error"
    }
}

/// The first line of `text`, at most `max` characters.
pub(crate) fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    truncate(line, max)
}

/// `text` cut to at most `max` characters (with an ellipsis).
pub(crate) fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_lose_their_variable_parts() {
        assert_eq!(
            normalize_message("deal 0199c1d2-6f7e-7a31-b5d4-2f0c8e9a1b23 not found"),
            normalize_message("deal 0199c1e7-0000-7a31-b5d4-2f0c8e9a1b99 not found"),
        );
        assert_eq!(
            normalize_message("cannot charge invoice 1042: card declined"),
            "cannot charge invoice <n>: card declined"
        );
        assert_eq!(
            normalize_message("no account for anu@example.com."),
            "no account for <email>."
        );
        assert_eq!(
            normalize_message("object 5f2b8c9d1e3a4b7c missing"),
            "object <hex> missing"
        );
        assert_eq!(normalize_message("at 0xdeadbeef"), "at <hex>");
        assert_eq!(
            normalize_message("took 1.5 s over \"budget\""),
            "took <n> s over \"?\""
        );
        // Words that look like hex but have no digit stay.
        assert_eq!(normalize_message("deadbeef cafe"), "deadbeef cafe");
        // An unterminated quote is kept.
        assert_eq!(normalize_message("it's 3"), "it's <n>");
    }

    #[test]
    fn sql_loses_its_literals() {
        assert_eq!(
            normalize_sql("SELECT  *\n FROM contacts WHERE id = $1"),
            "SELECT * FROM contacts WHERE id = ?"
        );
        assert_eq!(
            normalize_sql("SELECT a FROM t WHERE x IN (1, 2, 3) AND y IN ('a','b')"),
            "SELECT a FROM t WHERE x IN (?) AND y IN (?)"
        );
        assert_eq!(
            normalize_sql("INSERT INTO t (a, b) VALUES (?, ?), (?, ?), (?, ?)"),
            "INSERT INTO t (a, b) VALUES (?)"
        );
        assert_eq!(
            normalize_sql("SELECT * FROM t2 WHERE name = 'O''Brien' LIMIT 10"),
            "SELECT * FROM t2 WHERE name = ? LIMIT ?"
        );
        assert_eq!(
            normalize_sql("SELECT \"deals\".\"id\" FROM \"deals\" WHERE \"stage\" <> $1"),
            "SELECT \"deals\".\"id\" FROM \"deals\" WHERE \"stage\" <> ?"
        );
    }

    #[test]
    fn route_patterns_match_segments() {
        assert!(path_matches("/health/*", "/health/live"));
        assert!(path_matches("/health/*", "/health/a/b"));
        assert!(!path_matches("/health/*", "/health"));
        assert!(!path_matches("/health/*", "/healthz/live"));
        assert!(path_matches("/metrics", "/metrics"));
        assert!(path_matches("/metrics", "/metrics/"));
        assert!(!path_matches("/metrics", "/metrics/x"));
    }

    #[test]
    fn hosts_and_error_kinds() {
        assert_eq!(
            url_host("https://user@API.Example.com:8443/v1/x?y"),
            "api.example.com:8443"
        );
        assert_eq!(url_host("http://localhost/x"), "localhost");
        assert_eq!(outgoing_error_kind("operation timed out"), "timeout");
        assert_eq!(outgoing_error_kind("error trying to connect"), "connect");
        assert_eq!(outgoing_error_kind("broken pipe"), "error");
        assert_eq!(first_line("a\nb", 10), "a");
        assert_eq!(truncate("abcdef", 4), "abc…");
    }
}
