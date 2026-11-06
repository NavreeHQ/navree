//! [`Redactor`]: the one list of what counts as a secret, and the masking
//! built on it.
//!
//! Configuration, the logger, Logbook, Lighthouse, Fleet and the audit trail
//! each need to hide credentials from what they print or store. They share
//! this list of name fragments and add their own to it, so a word that is
//! secret in one place is secret in all of them. Where a value is known to be
//! a credential by its type (`Secret<T>`, `#[config(secret)]`), the exact path
//! says so and the name does not matter; a path known to be public stays
//! visible even if its name looks secret.

use std::collections::BTreeSet;

use serde_json::Value;

/// What a masked value is replaced with.
pub const REDACTED: &str = "[redacted]";

/// Finds secrets by name and by path, and masks them.
///
/// ```
/// use navree_contracts::Redactor;
///
/// let redactor = Redactor::new();
/// assert!(redactor.is_secret("Authorization"));
/// assert!(redactor.is_secret("db_password"));
/// assert!(!redactor.is_secret("username"));
///
/// // The type knows better than the name.
/// let redactor = Redactor::new()
///     .with_fragments(["key"])
///     .with_secret_paths(["billing.signing"])
///     .with_public_paths(["realtime.key"]);
/// assert!(redactor.is_secret_path("billing.signing"));
/// assert!(!redactor.is_secret_path("realtime.key"));
/// assert!(redactor.is_secret_path("app.key"));
/// ```
#[derive(Clone, Debug)]
pub struct Redactor {
    fragments: Vec<String>,
    secret_paths: BTreeSet<String>,
    public_paths: BTreeSet<String>,
}

impl Redactor {
    /// What masked values are replaced with.
    pub const REDACTED: &'static str = REDACTED;

    /// The fragments every redactor starts with: a name that contains one
    /// of them (in any case) holds a secret.
    pub const DEFAULT_FRAGMENTS: [&'static str; 14] = [
        "password",
        "passwd",
        "pwd",
        "passphrase",
        "secret",
        "token",
        "authorization",
        "cookie",
        "credential",
        "dsn",
        "api_key",
        "apikey",
        "private_key",
        "connection_string",
    ];

    /// A redactor with [`DEFAULT_FRAGMENTS`](Redactor::DEFAULT_FRAGMENTS).
    pub fn new() -> Self {
        Self::from_fragments(Self::DEFAULT_FRAGMENTS)
    }

    /// A redactor with these fragments only (matched in any case; blank
    /// ones are dropped). Use it where an app configures the list, such as
    /// `log.redact`.
    pub fn from_fragments<I, S>(fragments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Redactor {
            fragments: normalize(fragments),
            secret_paths: BTreeSet::new(),
            public_paths: BTreeSet::new(),
        }
    }

    /// Adds fragments to the list.
    #[must_use]
    pub fn with_fragments<I, S>(mut self, extra: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for fragment in normalize(extra) {
            if !self.fragments.contains(&fragment) {
                self.fragments.push(fragment);
            }
        }
        self
    }

    /// Names paths (`mail.api_key`) that are secrets whatever their names.
    #[must_use]
    pub fn with_secret_paths<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.secret_paths
            .extend(paths.into_iter().map(|path| path.as_ref().to_owned()));
        self
    }

    /// Names paths (`realtime.key`) that are public whatever their names.
    #[must_use]
    pub fn with_public_paths<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.public_paths
            .extend(paths.into_iter().map(|path| path.as_ref().to_owned()));
        self
    }

    /// The fragments, lower-cased.
    pub fn fragments(&self) -> &[String] {
        &self.fragments
    }

    /// Whether a value named `name` (a key, a header, a field) is a secret
    /// by its name: it contains a fragment, in any case.
    pub fn is_secret(&self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        self.fragments
            .iter()
            .any(|fragment| name.contains(fragment.as_str()))
    }

    /// Whether the value at the dotted `path` is a secret: a path named as a
    /// secret is, a path named as public is not, and any other is judged by
    /// its name.
    pub fn is_secret_path(&self, path: &str) -> bool {
        if self.secret_paths.contains(path) {
            return true;
        }
        if self.public_paths.contains(path) {
            return false;
        }
        self.is_secret(path)
    }

    /// Replaces, at any depth, the value of every object member whose key
    /// is a secret by name.
    pub fn mask_json(&self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, member) in map.iter_mut() {
                    if self.is_secret(key) {
                        *member = Value::String(REDACTED.to_owned());
                    } else {
                        self.mask_json(member);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| self.mask_json(item)),
            _ => {}
        }
    }
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

/// Lower-cased, trimmed, without blanks and repeats.
fn normalize<I, S>(fragments: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: Vec<String> = Vec::new();
    for fragment in fragments {
        let fragment = fragment.as_ref().trim().to_ascii_lowercase();
        if !fragment.is_empty() && !out.contains(&fragment) {
            out.push(fragment);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_default_list_finds_credentials_by_name_in_any_case() {
        let redactor = Redactor::new();
        for name in [
            "password",
            "DB_PASSWORD",
            "X-Api-Key",
            "apiKey",
            "Authorization",
            "Set-Cookie",
            "client_secret",
            "access_token",
            "sentry_dsn",
            "private_key",
            "connection_string",
        ] {
            assert!(redactor.is_secret(name), "{name}");
        }
        for name in ["username", "monkey", "keyword", "title", "key"] {
            assert!(!redactor.is_secret(name), "{name}");
        }
    }

    #[test]
    fn fragments_are_cleaned_and_extended_without_repeats() {
        let redactor = Redactor::from_fragments([" Secret ", "", "secret", "PIN"]);
        assert_eq!(redactor.fragments(), ["secret", "pin"]);
        let redactor = redactor.with_fragments(["pin", "otp"]);
        assert_eq!(redactor.fragments(), ["secret", "pin", "otp"]);
        assert!(redactor.is_secret("user_pin"));
        assert!(!redactor.is_secret("password"));
    }

    #[test]
    fn a_path_is_judged_by_type_knowledge_before_its_name() {
        let redactor = Redactor::new()
            .with_fragments(["key"])
            .with_secret_paths(["billing.signing"])
            .with_public_paths(["realtime.key"]);
        assert!(redactor.is_secret_path("billing.signing"));
        assert!(!redactor.is_secret_path("realtime.key"));
        assert!(redactor.is_secret_path("app.key"));
        assert!(redactor.is_secret_path("mail.smtp_password"));
        assert!(!redactor.is_secret_path("mail.from"));
    }

    #[test]
    fn json_members_are_masked_at_any_depth() {
        let mut value = json!({
            "user": "asha",
            "password": "hunter2",
            "nested": { "api_key": "k", "list": [{ "token": "t", "ok": 1 }] },
        });
        Redactor::new().mask_json(&mut value);
        assert_eq!(
            value,
            json!({
                "user": "asha",
                "password": "[redacted]",
                "nested": { "api_key": "[redacted]", "list": [{ "token": "[redacted]", "ok": 1 }] },
            })
        );
    }
}
