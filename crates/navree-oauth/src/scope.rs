//! Scopes: the built-in OpenID Connect scopes, the app's scopes from
//! `oauth.scopes`, and how a set of scopes becomes a token's abilities.

use std::collections::{BTreeMap, BTreeSet};

use navree_openapi::ToSchema;
use serde::Serialize;

use crate::config::OAuthOptions;

/// `openid`: OpenID Connect sign-in (an ID token and `/oauth/userinfo`).
pub const OPENID: &str = "openid";
/// `profile`: the user's name in the ID token and userinfo.
pub const PROFILE: &str = "profile";
/// `email`: the user's email address and whether it is verified.
pub const EMAIL: &str = "email";
/// `offline_access`: a refresh token.
pub const OFFLINE_ACCESS: &str = "offline_access";

/// The built-in scopes and their consent descriptions. They grant no
/// permissions.
pub const BUILT_IN_SCOPES: [(&str, &str); 4] = [
    (OPENID, "Sign you in with your account"),
    (PROFILE, "Read your name"),
    (EMAIL, "Read your email address"),
    (OFFLINE_ACCESS, "Keep access when you are not using the app"),
];

/// A scope as the consent page shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ScopeInfo {
    /// The scope name (`deals:read`).
    pub name: String,
    /// What it allows.
    pub description: String,
}

/// Every scope the server knows: the built-in ones and `oauth.scopes`.
#[derive(Clone, Debug, Default)]
pub struct ScopeRegistry {
    scopes: BTreeMap<String, (String, Vec<String>)>,
}

impl ScopeRegistry {
    /// The registry of `options`.
    pub fn new(options: &OAuthOptions) -> Self {
        let mut scopes: BTreeMap<String, (String, Vec<String>)> = BUILT_IN_SCOPES
            .iter()
            .map(|(name, description)| {
                ((*name).to_owned(), ((*description).to_owned(), Vec::new()))
            })
            .collect();
        for (name, scope) in &options.scopes {
            scopes.insert(
                name.clone(),
                (scope.description.clone(), scope.permissions.clone()),
            );
        }
        Self { scopes }
    }

    /// Whether `name` is a known scope.
    pub fn contains(&self, name: &str) -> bool {
        self.scopes.contains_key(name)
    }

    /// Every scope name, sorted.
    pub fn names(&self) -> Vec<String> {
        self.scopes.keys().cloned().collect()
    }

    /// The scopes the app defines (not the built-in ones), sorted.
    pub fn app_scopes(&self) -> Vec<String> {
        self.scopes
            .keys()
            .filter(|name| !is_built_in(name))
            .cloned()
            .collect()
    }

    /// `scopes` with their descriptions, in the given order; unknown names
    /// are skipped.
    pub fn describe(&self, scopes: &[String]) -> Vec<ScopeInfo> {
        scopes
            .iter()
            .filter_map(|name| {
                self.scopes.get(name).map(|(description, _)| ScopeInfo {
                    name: name.clone(),
                    description: description.clone(),
                })
            })
            .collect()
    }

    /// The permissions `scopes` grant together, sorted and deduplicated:
    /// a token's abilities.
    pub fn permissions(&self, scopes: &[String]) -> Vec<String> {
        let set: BTreeSet<&String> = scopes
            .iter()
            .filter_map(|name| self.scopes.get(name))
            .flat_map(|(_, permissions)| permissions)
            .collect();
        set.into_iter().cloned().collect()
    }

    /// The permissions of the single scope `name`.
    pub fn permissions_of(&self, name: &str) -> &[String] {
        self.scopes
            .get(name)
            .map_or(&[], |(_, permissions)| permissions.as_slice())
    }

    /// The first scope of `scopes` that is not known.
    pub fn unknown<'a>(&self, scopes: &'a [String]) -> Option<&'a String> {
        scopes.iter().find(|name| !self.contains(name))
    }
}

/// Whether `name` is one of the built-in scopes.
pub fn is_built_in(name: &str) -> bool {
    BUILT_IN_SCOPES
        .iter()
        .any(|(built_in, _)| *built_in == name)
}

/// The scopes of a space-separated `scope` parameter, deduplicated, in
/// order.
pub fn parse_scope(scope: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    scope
        .split(' ')
        .map(str::trim)
        .filter(|name| !name.is_empty() && seen.insert(*name))
        .map(str::to_owned)
        .collect()
}

/// `scopes` as a space-separated `scope` value.
pub fn join_scopes(scopes: &[String]) -> String {
    scopes.join(" ")
}

/// Whether every scope of `requested` is in `allowed`.
pub(crate) fn is_subset(requested: &[String], allowed: &[String]) -> bool {
    requested.iter().all(|scope| allowed.contains(scope))
}

/// Checks a scope name: 1 to 64 printable ASCII characters other than
/// space, `"` and `\` (RFC 6749 `scope-token`).
pub(crate) fn check_scope_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.len() > 64 {
        return Err("must be 1 to 64 characters long");
    }
    if !name
        .bytes()
        .all(|b| (0x21..=0x7e).contains(&b) && b != b'"' && b != b'\\')
    {
        return Err("may only contain printable ASCII characters other than space, `\"` and `\\`");
    }
    Ok(())
}

/// Checks a permission string: `*`, or `<prefix>.<action>` (`deals.read`,
/// `deals.*`).
pub(crate) fn check_permission(permission: &str) -> Result<(), &'static str> {
    if permission == "*" {
        return Ok(());
    }
    if permission.is_empty() || permission.len() > 128 {
        return Err("must be 1 to 128 characters long");
    }
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '*');
    if !permission.chars().all(allowed) {
        return Err("may only contain ASCII letters, digits, `_`, `-`, `.`, `:` and `*`");
    }
    match permission.rsplit_once('.') {
        Some((prefix, action)) if !prefix.is_empty() && !action.is_empty() => Ok(()),
        _ => Err("must be `*` or look like `<prefix>.<action>`, e.g. `deals.read`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ScopeOptions;

    fn registry() -> ScopeRegistry {
        ScopeRegistry::new(
            &OAuthOptions::default()
                .scope(
                    "deals:read",
                    ScopeOptions::new("Read your deals", ["deals.read"]),
                )
                .scope(
                    "deals:write",
                    ScopeOptions::new("Write your deals", ["deals.read", "deals.update"]),
                ),
        )
    }

    #[test]
    fn built_in_and_app_scopes_are_known() {
        let registry = registry();
        for name in ["openid", "profile", "email", "offline_access", "deals:read"] {
            assert!(registry.contains(name), "{name}");
        }
        assert!(!registry.contains("deals:delete"));
        assert_eq!(registry.app_scopes(), ["deals:read", "deals:write"]);
        assert!(is_built_in("openid") && !is_built_in("deals:read"));
    }

    #[test]
    fn permissions_are_the_union_of_the_scopes() {
        let registry = registry();
        let scopes = vec![
            "deals:read".to_owned(),
            "deals:write".to_owned(),
            "openid".to_owned(),
        ];
        assert_eq!(
            registry.permissions(&scopes),
            ["deals.read", "deals.update"]
        );
        assert!(registry.permissions(&["openid".to_owned()]).is_empty());
        assert_eq!(registry.permissions_of("deals:read"), ["deals.read"]);
    }

    #[test]
    fn scope_parameters_parse_and_describe() {
        let registry = registry();
        let scopes = parse_scope("  deals:read openid deals:read  nope ");
        assert_eq!(scopes, ["deals:read", "openid", "nope"]);
        assert_eq!(registry.unknown(&scopes).map(String::as_str), Some("nope"));
        let described = registry.describe(&scopes);
        assert_eq!(described.len(), 2);
        assert_eq!(described[0].description, "Read your deals");
        assert_eq!(join_scopes(&scopes[..2]), "deals:read openid");
        assert!(is_subset(&scopes[..1], &scopes));
        assert!(!is_subset(&scopes, &scopes[..1]));
    }

    #[test]
    fn names_and_permissions_are_checked() {
        assert!(check_scope_name("deals:read").is_ok());
        assert!(check_scope_name("").is_err());
        assert!(check_scope_name("a b").is_err());
        assert!(check_scope_name("a\"b").is_err());
        for good in ["*", "deals.read", "deals.*", "workspace.members.update"] {
            assert!(check_permission(good).is_ok(), "{good}");
        }
        for bad in ["", "deals", ".read", "deals.", "deals read", "déals.read"] {
            assert!(check_permission(bad).is_err(), "{bad}");
        }
    }
}
