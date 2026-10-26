//! The resolvers: where a request names its workspace.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::header::{AUTHORIZATION, HOST};
use http::request::Parts;
use navree_contracts::AuthState;

use crate::{ResolverKind, WorkspaceConfig};

/// The workspace identifier (a slug or an id) the request names, and the
/// resolver that found it: the configured resolvers run in order and the
/// first that finds one wins.
pub(crate) fn identifier(
    config: &WorkspaceConfig,
    parts: &Parts,
) -> Option<(ResolverKind, String)> {
    config.resolvers.iter().find_map(|kind| {
        let found = match kind {
            ResolverKind::Header => from_header(parts, &config.header),
            ResolverKind::Subdomain => config
                .base_domain
                .as_deref()
                .and_then(|base| from_subdomain(host(parts)?, base)),
            ResolverKind::Path => from_path(parts.uri.path(), &config.path_segments()),
            ResolverKind::Claim => from_claim(parts, &config.claim),
        };
        found.map(|identifier| (*kind, identifier))
    })
}

/// The trimmed value of header `name`, if present and not empty.
pub(crate) fn from_header(parts: &Parts, name: &str) -> Option<String> {
    let value = parts.headers.get(name)?.to_str().ok()?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// The request's host: the `Host` header, else the URI's authority.
fn host(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .or_else(|| parts.uri.authority().map(|authority| authority.as_str()))
}

/// The single label in front of `base` in `host` (`acme.app.example.com`
/// under `app.example.com` → `acme`); ports are ignored and the comparison
/// is case-insensitive. Nothing for the base domain itself or deeper
/// subdomains.
pub(crate) fn from_subdomain(host: &str, base: &str) -> Option<String> {
    let host = strip_port(host.trim()).to_ascii_lowercase();
    let base = strip_port(base.trim()).to_ascii_lowercase();
    let label = host.strip_suffix(&base)?.strip_suffix('.')?;
    (!label.is_empty() && !label.contains('.')).then(|| label.to_owned())
}

fn strip_port(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host,
    }
}

/// The path segment after the first occurrence of `prefix`'s segments
/// (`/api/w/acme/deals` with `["w"]` → `acme`).
pub(crate) fn from_path(path: &str, prefix: &[&str]) -> Option<String> {
    if prefix.is_empty() {
        return None;
    }
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    segments
        .windows(prefix.len() + 1)
        .find(|window| window[..prefix.len()] == *prefix)
        .map(|window| window[prefix.len()].to_owned())
}

/// Claim `claim` of the request's bearer JWT, for an authenticated request.
///
/// The token's signature was verified by the authentication layer (an
/// unverifiable bearer token never yields a principal), so its payload is
/// read here without verifying it again. A string claim is taken as is, a
/// number as its decimal form.
pub(crate) fn from_claim(parts: &Parts, claim: &str) -> Option<String> {
    let authenticated = parts
        .extensions
        .get::<AuthState>()
        .is_some_and(|state| state.0.is_some());
    if !authenticated {
        return None;
    }
    let header = parts.headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = header.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    claim_of(token.trim(), claim)
}

/// Claim `claim` of the JWT `token`'s payload (not verified).
pub(crate) fn claim_of(token: &str, claim: &str) -> Option<String> {
    let mut parts = token.split('.');
    let (_header, payload, _signature) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    match claims.get(claim)? {
        serde_json::Value::String(value) if !value.trim().is_empty() => {
            Some(value.trim().to_owned())
        }
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use navree_contracts::{AuthVia, Principal};
    use uuid::Uuid;

    use super::*;

    fn parts(uri: &str, headers: &[(&str, &str)], signed_in: bool) -> Parts {
        let mut builder = http::Request::builder().uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let (mut parts, ()) = builder.body(()).unwrap().into_parts();
        let principal = signed_in.then(|| Principal {
            user_id: Uuid::now_v7(),
            via: AuthVia::Token,
            token_abilities: None,
            session_id: None,
        });
        parts.extensions.insert(AuthState(principal));
        parts
    }

    fn jwt(claims: serde_json::Value) -> String {
        format!(
            "eyJhbGciOiJIUzI1NiJ9.{}.c2ln",
            URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    #[test]
    fn subdomains_name_workspaces() {
        let base = "app.example.com";
        assert_eq!(
            from_subdomain("acme.app.example.com", base).as_deref(),
            Some("acme")
        );
        assert_eq!(
            from_subdomain("ACME.App.Example.com:8443", base).as_deref(),
            Some("acme")
        );
        assert_eq!(from_subdomain("app.example.com", base), None);
        assert_eq!(from_subdomain("x.acme.app.example.com", base), None);
        assert_eq!(from_subdomain("acme.other.com", base), None);
        assert_eq!(from_subdomain("evilapp.example.com", base), None);
        assert_eq!(
            from_subdomain("acme.localhost:3000", "localhost:3000").as_deref(),
            Some("acme")
        );
    }

    #[test]
    fn paths_name_workspaces_after_the_prefix() {
        assert_eq!(
            from_path("/api/w/acme/deals", &["w"]).as_deref(),
            Some("acme")
        );
        assert_eq!(from_path("/w/acme", &["w"]).as_deref(), Some("acme"));
        assert_eq!(
            from_path("/api/teams/t/acme/x", &["teams", "t"]).as_deref(),
            Some("acme")
        );
        assert_eq!(from_path("/api/w", &["w"]), None);
        assert_eq!(from_path("/api/deals", &["w"]), None);
    }

    #[test]
    fn claims_come_from_the_bearer_token_of_signed_in_requests() {
        let token = jwt(serde_json::json!({ "sub": "x", "workspace": "acme", "org": 42 }));
        let bearer = format!("Bearer {token}");
        let signed_in = parts("/", &[("authorization", bearer.as_str())], true);
        assert_eq!(from_claim(&signed_in, "workspace").as_deref(), Some("acme"));
        assert_eq!(from_claim(&signed_in, "org").as_deref(), Some("42"));
        assert_eq!(from_claim(&signed_in, "missing"), None);
        let guest = parts("/", &[("authorization", bearer.as_str())], false);
        assert_eq!(from_claim(&guest, "workspace"), None);
        assert_eq!(claim_of("not-a-jwt", "workspace"), None);
        assert_eq!(claim_of("a.b.c.d", "workspace"), None);
    }

    #[test]
    fn resolvers_run_in_the_configured_order() {
        let token = jwt(serde_json::json!({ "workspace": "from-claim" }));
        let bearer = format!("Bearer {token}");
        let request = parts(
            "/api/w/from-path/deals",
            &[
                ("x-workspace", "from-header"),
                ("host", "from-subdomain.app.test"),
                ("authorization", bearer.as_str()),
            ],
            true,
        );
        let mut config = WorkspaceConfig {
            base_domain: Some("app.test".into()),
            ..WorkspaceConfig::default()
        };
        let found = |config: &WorkspaceConfig| identifier(config, &request).map(|(_, id)| id);
        assert_eq!(found(&config).as_deref(), Some("from-header"));
        config.resolvers = vec![ResolverKind::Subdomain, ResolverKind::Header];
        assert_eq!(found(&config).as_deref(), Some("from-subdomain"));
        config.resolvers = vec![ResolverKind::Claim, ResolverKind::Path];
        assert_eq!(found(&config).as_deref(), Some("from-claim"));
        config.resolvers = vec![ResolverKind::Path];
        assert_eq!(
            identifier(&config, &request),
            Some((ResolverKind::Path, "from-path".to_owned()))
        );
        let bare = parts("/api/deals", &[], true);
        assert_eq!(identifier(&WorkspaceConfig::default(), &bare), None);
        let empty_header = parts("/api/deals", &[("x-workspace", "  ")], true);
        assert_eq!(identifier(&WorkspaceConfig::default(), &empty_header), None);
    }
}
