//! Keeping secrets out of `Debug` output and error messages.
//!
//! A value is secret when its key looks secret (see [`is_secret_key`]) or when
//! it is the password of a URL. The redacted view of the tree masks those
//! values; error messages are scrubbed of them.

use crate::value::{Kind, Value};

/// What masked values are replaced with.
pub(crate) const REDACTED: &str = "[redacted]";

/// Key fragments that mark a value as secret (matched case-insensitively
/// anywhere in the key).
const SECRET_KEY_PARTS: [&str; 11] = [
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "key",
    "authorization",
    "cookie",
    "dsn",
    "credential",
    "connection_string",
];

/// Whether a key (or a dotted path) names a secret: it contains one of
/// `password`, `passwd`, `pwd`, `secret`, `token`, `key`, `authorization`,
/// `cookie`, `dsn`, `credential` or `connection_string`, in any case.
pub(crate) fn is_secret_key(key: &str) -> bool {
    let key = key.to_lowercase();
    SECRET_KEY_PARTS.iter().any(|part| key.contains(part))
}

/// The tree as JSON with every secret masked: the subtree of a secret-looking
/// key is replaced whole by `"[redacted]"`, and URL passwords (and the values
/// of secret-looking query parameters) are masked in every string.
pub(crate) fn redacted_json(value: &Value) -> serde_json::Value {
    use serde_json::Value as Json;
    match &value.kind {
        Kind::Null => Json::Null,
        Kind::Bool(b) => Json::Bool(*b),
        Kind::Int(i) => Json::from(*i),
        Kind::UInt(u) => Json::from(*u),
        Kind::Float(f) => serde_json::Number::from_f64(*f).map_or(Json::Null, Json::Number),
        Kind::String(s) => Json::String(mask_urls(s, None)),
        Kind::Array(items) => Json::Array(items.iter().map(redacted_json).collect()),
        Kind::Table(map) => Json::Object(
            map.iter()
                .map(|(key, value)| {
                    let value = if is_secret_key(key) {
                        Json::String(REDACTED.to_owned())
                    } else {
                        redacted_json(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
    }
}

/// Masks the password of every `scheme://user:password@host` URL in `s`, and
/// the value of every query or fragment parameter whose name looks secret
/// (`?token=...&sslmode=require`). When `found` is given, the masked values
/// are pushed to it.
pub(crate) fn mask_urls(s: &str, mut found: Option<&mut Vec<String>>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find("://") {
        let authority_start = at + 3;
        out.push_str(&rest[..authority_start]);
        let tail = &rest[authority_start..];
        // The URL ends at whitespace or a comma (a list of URLs).
        let url_end = tail
            .find(|c: char| c.is_whitespace() || c == ',')
            .unwrap_or(tail.len());
        let (url, after) = tail.split_at(url_end);
        let authority_end = url.find(['/', '?', '#']).unwrap_or(url.len());
        let (authority, path_and_query) = url.split_at(authority_end);
        let userinfo = authority
            .rfind('@')
            .map(|end| (end, &authority[..end]))
            .and_then(|(end, userinfo)| Some((end, userinfo.find(':')?)));
        match userinfo {
            Some((userinfo_end, colon)) => {
                out.push_str(&authority[..colon]);
                out.push(':');
                out.push_str(REDACTED);
                out.push_str(&authority[userinfo_end..]);
                if let Some(found) = found.as_deref_mut() {
                    found.push(authority[colon + 1..userinfo_end].to_owned());
                }
            }
            None => out.push_str(authority),
        }
        out.push_str(&mask_parameters(path_and_query, found.as_deref_mut()));
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Masks the values of secret-looking parameters after the first `?` or `#`,
/// keeping every separator as it was.
fn mask_parameters(path_and_query: &str, mut found: Option<&mut Vec<String>>) -> String {
    let Some(start) = path_and_query.find(['?', '#']) else {
        return path_and_query.to_owned();
    };
    let (head, mut rest) = path_and_query.split_at(start + 1);
    let mut out = String::from(head);
    loop {
        let end = rest.find(['&', ';', '#']).unwrap_or(rest.len());
        let (parameter, tail) = rest.split_at(end);
        match parameter.split_once('=') {
            Some((name, value)) if is_secret_key(name) && !value.is_empty() => {
                out.push_str(name);
                out.push('=');
                out.push_str(REDACTED);
                if let Some(found) = found.as_deref_mut() {
                    found.push(value.to_owned());
                }
            }
            _ => out.push_str(parameter),
        }
        let Some(separator) = tail.chars().next() else {
            break;
        };
        out.push(separator);
        rest = &tail[separator.len_utf8()..];
    }
    out
}

/// Every secret string in the tree: the scalars under a secret-looking key
/// and the URL passwords in other strings.
pub(crate) fn secret_values(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    collect(value, false, &mut found);
    found.retain(|s| !s.is_empty());
    found
}

fn collect(value: &Value, under_secret: bool, found: &mut Vec<String>) {
    match &value.kind {
        Kind::String(s) if under_secret => found.push(s.clone()),
        Kind::String(s) => {
            mask_urls(s, Some(found));
        }
        Kind::Int(i) if under_secret => found.push(i.to_string()),
        Kind::UInt(u) if under_secret => found.push(u.to_string()),
        Kind::Float(f) if under_secret => found.push(f.to_string()),
        Kind::Array(items) => items.iter().for_each(|v| collect(v, under_secret, found)),
        Kind::Table(map) => {
            for (key, value) in map {
                collect(value, under_secret || is_secret_key(key), found);
            }
        }
        _ => {}
    }
}

/// Replaces `secrets` in `message`. A secret of four characters or more is
/// replaced wherever it appears; a shorter one only when quoted (`"x"`,
/// `` `x` `` or `'x'`), so that a one-digit PIN does not eat every digit.
pub(crate) fn scrub(mut message: String, secrets: &[String]) -> String {
    // Longest first, so a secret that contains another is replaced whole.
    let mut secrets: Vec<&String> = secrets.iter().collect();
    secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    secrets.dedup();
    for secret in secrets {
        if secret.chars().count() >= 4 {
            message = message.replace(secret.as_str(), REDACTED);
        } else {
            for quote in ['"', '`', '\''] {
                message = message.replace(&format!("{quote}{secret}{quote}"), REDACTED);
            }
        }
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_keys_are_found_in_any_case_anywhere_in_the_key() {
        for key in [
            "password",
            "DB_PASSWORD",
            "passwd",
            "pwd",
            "client_secret",
            "api_token",
            "app.key",
            "Authorization",
            "session_cookie",
            "sentry.dsn",
            "credentials",
            "connection_string",
        ] {
            assert!(is_secret_key(key), "{key}");
        }
        for key in ["url", "host", "port", "level", "origins", "name"] {
            assert!(!is_secret_key(key), "{key}");
        }
    }

    #[test]
    fn url_passwords_are_masked_wherever_the_url_is() {
        for (input, expected) in [
            (
                "postgres://navree:hunter2@db:5432/app?sslmode=require",
                "postgres://navree:[redacted]@db:5432/app?sslmode=require",
            ),
            (
                "redis://:hunter2@cache:6379/0",
                "redis://:[redacted]@cache:6379/0",
            ),
            ("amqp://guest@mq/", "amqp://guest@mq/"),
            (
                "https://example.com/users/a@b:c",
                "https://example.com/users/a@b:c",
            ),
            (
                "user:hunter2@host is not a URL",
                "user:hunter2@host is not a URL",
            ),
            (
                "primary=postgres://u:p1@a/db,replica=postgres://u:p2@b/db",
                "primary=postgres://u:[redacted]@a/db,replica=postgres://u:[redacted]@b/db",
            ),
            (
                "see https://u:hunter2@h/x for details",
                "see https://u:[redacted]@h/x for details",
            ),
            ("postgres://u:p%40ss@h/d", "postgres://u:[redacted]@h/d"),
            ("postgres://u:pa@ss@h/d", "postgres://u:[redacted]@h/d"),
        ] {
            assert_eq!(mask_urls(input, None), expected, "{input}");
        }
    }

    #[test]
    fn secret_looking_url_parameters_are_masked() {
        for (input, expected) in [
            (
                "postgres://h/d?password=hunter2&sslmode=require",
                "postgres://h/d?password=[redacted]&sslmode=require",
            ),
            (
                "https://h/cb?code=1&access_token=abc#state=x",
                "https://h/cb?code=1&access_token=[redacted]#state=x",
            ),
            ("https://h/p?token=", "https://h/p?token="),
            ("https://h/p?note=a=b", "https://h/p?note=a=b"),
        ] {
            assert_eq!(mask_urls(input, None), expected, "{input}");
        }
    }

    #[test]
    fn masking_reports_what_it_masked() {
        let mut found = Vec::new();
        mask_urls("postgres://u:hunter2@h/d?token=abc", Some(&mut found));
        assert_eq!(found, ["hunter2", "abc"]);
    }

    #[test]
    fn scrub_removes_secrets_but_not_short_digits_everywhere() {
        let secrets = vec!["hunter2-long".to_owned(), "12".to_owned(), "x".to_owned()];
        assert_eq!(
            scrub(
                "bad value hunter2-long and `12` and 12 and \"x\" and xx".to_owned(),
                &secrets
            ),
            "bad value [redacted] and [redacted] and 12 and [redacted] and xx"
        );
    }
}
