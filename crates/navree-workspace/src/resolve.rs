//! The resolvers: where a request names its workspace, organization or
//! partition.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::header::{AUTHORIZATION, HOST};
use http::request::Parts;
use navree_contracts::AuthState;
use uuid::Uuid;

use crate::{OrganizationOptions, PartitionOptions, ResolverKind, WorkspaceConfig};

/// The workspace a request's host belongs to: a request extension that
/// navree-domains' `DomainsModule` sets for a request to a workspace's
/// verified custom domain (`crm.agra-corp.com`). The `domain` resolver
/// (`workspace.resolvers = ["domain", ...]`) reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainWorkspace {
    /// The workspace the domain belongs to.
    pub workspace_id: Uuid,
    /// The host the request came in on, lower-case, without a port.
    pub host: String,
}

/// The settings of one level's resolvers.
#[derive(Clone, Debug)]
pub(crate) struct Lookup<'a> {
    pub(crate) resolvers: &'a [ResolverKind],
    pub(crate) header: &'a str,
    pub(crate) base_domain: Option<&'a str>,
    pub(crate) path: Vec<&'a str>,
    pub(crate) claim: &'a str,
}

impl<'a> Lookup<'a> {
    /// The workspace resolvers (`workspace.*`).
    pub(crate) fn workspace(config: &'a WorkspaceConfig) -> Self {
        Self {
            resolvers: &config.resolvers,
            header: &config.header,
            base_domain: config.base_domain.as_deref(),
            path: config.path_segments(),
            claim: &config.claim,
        }
    }

    /// The organization resolvers (`workspace.organizations.*`).
    pub(crate) fn organization(options: &'a OrganizationOptions) -> Self {
        Self {
            resolvers: &options.resolvers,
            header: &options.header,
            base_domain: options.base_domain.as_deref(),
            path: options.path_segments(),
            claim: &options.claim,
        }
    }

    /// The partition resolvers (`workspace.partitions.*`; no subdomains).
    pub(crate) fn partition(options: &'a PartitionOptions) -> Self {
        Self {
            resolvers: &options.resolvers,
            header: &options.header,
            base_domain: None,
            path: options.path_segments(),
            claim: &options.claim,
        }
    }

    /// The identifier (a slug or an id) the request names, and the
    /// resolver that found it: the resolvers run in order and the first
    /// that finds one wins.
    pub(crate) fn find(&self, parts: &Parts) -> Option<(ResolverKind, String)> {
        self.find_skipping(parts, None)
    }

    /// [`find`](Self::find) without resolver `skip` (the `session` one,
    /// after its choice turned out stale).
    pub(crate) fn find_skipping(
        &self,
        parts: &Parts,
        skip: Option<ResolverKind>,
    ) -> Option<(ResolverKind, String)> {
        self.resolvers
            .iter()
            .filter(|kind| Some(**kind) != skip)
            .find_map(|kind| {
                let found = match kind {
                    ResolverKind::Header => from_header(parts, self.header),
                    ResolverKind::Subdomain => self
                        .base_domain
                        .and_then(|base| from_subdomain(host(parts)?, base)),
                    ResolverKind::Path => from_path(parts.uri.path(), &self.path),
                    ResolverKind::Claim => from_claim(parts, self.claim),
                    // Custom domains name workspaces only (the configuration
                    // check keeps `domain` out of the other levels' lists).
                    ResolverKind::Domain => parts
                        .extensions
                        .get::<DomainWorkspace>()
                        .map(|domain| domain.workspace_id.to_string()),
                    // A browser's choice names workspaces only (the
                    // configuration check keeps `session` out of the other
                    // levels' lists).
                    ResolverKind::Session => from_session(parts).map(|id| id.to_string()),
                };
                found.map(|identifier| (*kind, identifier))
            })
    }
}

/// The session key holding the workspace a browser switched to: a
/// [`SessionChoice`], written by the switch route
/// ([`WorkspaceConfig::switch_path`]) and read by the `session` resolver.
pub const SESSION_KEY: &str = "workspace.current";

/// The workspace a signed-in user switched to in this browser, as the
/// session keeps it under [`SESSION_KEY`]: `{"id": .., "user": ..}`. It
/// counts only while that user is the one signed in, so a choice outlives
/// no sign-in of somebody else on the same browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionChoice {
    /// The workspace.
    pub id: Uuid,
    /// The user who chose it.
    pub user: Uuid,
}

/// The id of the workspace the request's session names for the signed-in
/// user: `None` without the crate's `session` feature, outside navree-session,
/// for a guest, or when the choice in the session was made by another user.
#[cfg(feature = "session")]
pub(crate) fn from_session(parts: &Parts) -> Option<Uuid> {
    let user = parts
        .extensions
        .get::<AuthState>()
        .and_then(|state| state.0.as_ref())
        .map(|principal| principal.user_id)?;
    let choice: SessionChoice = parts
        .extensions
        .get::<navree_session::Session>()?
        .get(SESSION_KEY)?;
    (choice.user == user).then_some(choice.id)
}

/// Without the `session` feature the `session` resolver finds nothing.
#[cfg(not(feature = "session"))]
pub(crate) fn from_session(_parts: &Parts) -> Option<Uuid> {
    None
}

/// Forgets the session's workspace choice (it named a workspace the user may
/// no longer enter, or one that is gone).
#[cfg(feature = "session")]
pub(crate) fn forget_session_choice(parts: &Parts) {
    if let Some(session) = parts.extensions.get::<navree_session::Session>() {
        session.remove(SESSION_KEY);
    }
}

/// Without the `session` feature there is no choice to forget.
#[cfg(not(feature = "session"))]
pub(crate) fn forget_session_choice(_parts: &Parts) {}

/// Stores the first workspace `user` belongs to (the oldest) as the
/// session's choice, for a browser that has chosen none; whether there was
/// one to store.
#[cfg(feature = "session")]
pub(crate) async fn choose_first_workspace(
    parts: &Parts,
    workspaces: &crate::Workspaces,
    user: Uuid,
) -> bool {
    let Some(session) = parts.extensions.get::<navree_session::Session>() else {
        return false;
    };
    let Ok(memberships) = workspaces.for_user(user).await else {
        return false;
    };
    let Some(first) = memberships
        .into_iter()
        .min_by_key(|membership| (membership.workspace.created_at, membership.workspace.id))
    else {
        return false;
    };
    session
        .insert(
            SESSION_KEY,
            &SessionChoice {
                id: first.workspace.id,
                user,
            },
        )
        .is_ok()
}

/// Without the `session` feature a browser chooses nothing.
#[cfg(not(feature = "session"))]
pub(crate) async fn choose_first_workspace(
    _parts: &Parts,
    _workspaces: &crate::Workspaces,
    _user: Uuid,
) -> bool {
    false
}

/// The workspace identifier (a slug or an id) a request names, and the
/// resolver that found it: the resolvers of `config.resolvers` run in
/// order and the first that finds one wins. Nothing is loaded or checked
/// (use [`CurrentWorkspace`](crate::CurrentWorkspace) for the workspace
/// itself); for middleware that needs to know which workspace a request is
/// about, such as navree-i18n's locale negotiation.
///
/// ```
/// use navree_workspace::{ResolverKind, WorkspaceConfig, requested_workspace};
///
/// let (parts, ()) = http::Request::builder()
///     .header("X-Workspace", "agra")
///     .body(())
///     .unwrap()
///     .into_parts();
/// assert_eq!(
///     requested_workspace(&WorkspaceConfig::default(), &parts),
///     Some((ResolverKind::Header, "agra".to_owned()))
/// );
/// ```
pub fn requested_workspace(
    config: &WorkspaceConfig,
    parts: &Parts,
) -> Option<(ResolverKind, String)> {
    identifier(config, parts)
}

/// The workspace identifier (a slug or an id) the request names, and the
/// resolver that found it: the configured resolvers run in order and the
/// first that finds one wins.
pub(crate) fn identifier(
    config: &WorkspaceConfig,
    parts: &Parts,
) -> Option<(ResolverKind, String)> {
    Lookup::workspace(config).find(parts)
}

/// [`identifier`] without the `session` resolver (its choice was stale).
pub(crate) fn identifier_without_session(
    config: &WorkspaceConfig,
    parts: &Parts,
) -> Option<(ResolverKind, String)> {
    Lookup::workspace(config).find_skipping(parts, Some(ResolverKind::Session))
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

/// The single label in front of `base` in `host` (`agra.app.example.com`
/// under `app.example.com` → `agra`); ports are ignored and the comparison
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
/// (`/api/w/agra/deals` with `["w"]` → `agra`).
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
            from_subdomain("agra.app.example.com", base).as_deref(),
            Some("agra")
        );
        assert_eq!(
            from_subdomain("AGRA.App.Example.com:8443", base).as_deref(),
            Some("agra")
        );
        assert_eq!(from_subdomain("app.example.com", base), None);
        assert_eq!(from_subdomain("x.agra.app.example.com", base), None);
        assert_eq!(from_subdomain("agra.other.com", base), None);
        assert_eq!(from_subdomain("evilapp.example.com", base), None);
        assert_eq!(
            from_subdomain("agra.localhost:3000", "localhost:3000").as_deref(),
            Some("agra")
        );
    }

    #[test]
    fn paths_name_workspaces_after_the_prefix() {
        assert_eq!(
            from_path("/api/w/agra/deals", &["w"]).as_deref(),
            Some("agra")
        );
        assert_eq!(from_path("/w/agra", &["w"]).as_deref(), Some("agra"));
        assert_eq!(
            from_path("/api/teams/t/agra/x", &["teams", "t"]).as_deref(),
            Some("agra")
        );
        assert_eq!(from_path("/api/w", &["w"]), None);
        assert_eq!(from_path("/api/deals", &["w"]), None);
    }

    #[test]
    fn claims_come_from_the_bearer_token_of_signed_in_requests() {
        let token = jwt(serde_json::json!({ "sub": "x", "workspace": "agra", "org": 42 }));
        let bearer = format!("Bearer {token}");
        let signed_in = parts("/", &[("authorization", bearer.as_str())], true);
        assert_eq!(from_claim(&signed_in, "workspace").as_deref(), Some("agra"));
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

    #[test]
    fn custom_domains_name_workspaces_through_the_request_extension() {
        let workspace = Uuid::now_v7();
        let mut request = parts(
            "/api/deals",
            &[
                ("host", "crm.agra-corp.test"),
                ("x-workspace", "from-header"),
            ],
            true,
        );
        let config = WorkspaceConfig {
            resolvers: vec![ResolverKind::Domain, ResolverKind::Header],
            ..WorkspaceConfig::default()
        };
        // No custom domain matched the host: the next resolver answers.
        assert_eq!(
            identifier(&config, &request),
            Some((ResolverKind::Header, "from-header".to_owned()))
        );
        request.extensions.insert(DomainWorkspace {
            workspace_id: workspace,
            host: "crm.agra-corp.test".to_owned(),
        });
        assert_eq!(
            identifier(&config, &request),
            Some((ResolverKind::Domain, workspace.to_string()))
        );
        assert_eq!(
            requested_workspace(&config, &request),
            Some((ResolverKind::Domain, workspace.to_string()))
        );
        // Without `domain` in the list, the extension is ignored.
        assert_eq!(
            identifier(&WorkspaceConfig::default(), &request).map(|(kind, _)| kind),
            Some(ResolverKind::Header)
        );
    }

    // The session names the workspace a browser switched to, for
    // the user who switched only, and can be skipped once it turns out stale.
    #[cfg(feature = "session")]
    #[test]
    fn the_session_names_the_workspace_its_user_switched_to() {
        let workspace = Uuid::now_v7();
        let mut request = parts("/deals", &[("x-workspace", "from-header")], true);
        let user = request
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id)
            .unwrap();
        let session = navree_session::Session::detached();
        request.extensions.insert(session.clone());
        let config = WorkspaceConfig {
            resolvers: vec![ResolverKind::Session, ResolverKind::Header],
            ..WorkspaceConfig::default()
        };
        // Nothing chosen yet: the next resolver answers.
        assert_eq!(
            identifier(&config, &request),
            Some((ResolverKind::Header, "from-header".to_owned()))
        );
        session
            .insert(
                SESSION_KEY,
                &SessionChoice {
                    id: workspace,
                    user,
                },
            )
            .unwrap();
        assert_eq!(
            identifier(&config, &request),
            Some((ResolverKind::Session, workspace.to_string()))
        );
        assert_eq!(
            identifier_without_session(&config, &request),
            Some((ResolverKind::Header, "from-header".to_owned()))
        );
        // Another user's choice on the same browser counts for nothing.
        session
            .insert(
                SESSION_KEY,
                &SessionChoice {
                    id: workspace,
                    user: Uuid::now_v7(),
                },
            )
            .unwrap();
        assert_eq!(
            identifier(&config, &request).map(|(kind, _)| kind),
            Some(ResolverKind::Header)
        );
        // A guest has no choice either.
        let mut guest = parts("/deals", &[], false);
        guest.extensions.insert(session.clone());
        session
            .insert(
                SESSION_KEY,
                &SessionChoice {
                    id: workspace,
                    user,
                },
            )
            .unwrap();
        assert_eq!(identifier(&config, &guest), None);
        // Forgetting it clears the session.
        forget_session_choice(&request);
        assert!(!session.contains(SESSION_KEY));
    }

    #[test]
    fn organizations_and_partitions_have_their_own_resolvers() {
        let token = jwt(serde_json::json!({ "organization": "org-claim", "partition": "eu" }));
        let bearer = format!("Bearer {token}");
        let request = parts(
            "/api/o/agra-group/p/uk/reports",
            &[
                ("x-organization", "org-header"),
                ("x-partition", "part-header"),
                ("host", "org-sub.partners.test"),
                ("authorization", bearer.as_str()),
            ],
            true,
        );
        let mut config = WorkspaceConfig::default();
        config.organizations.base_domain = Some("partners.test".into());
        let orgs = |config: &WorkspaceConfig| {
            Lookup::organization(&config.organizations)
                .find(&request)
                .map(|(_, id)| id)
        };
        assert_eq!(orgs(&config).as_deref(), Some("org-header"));
        config.organizations.resolvers = vec![ResolverKind::Path];
        assert_eq!(orgs(&config).as_deref(), Some("agra-group"));
        config.organizations.resolvers = vec![ResolverKind::Subdomain];
        assert_eq!(orgs(&config).as_deref(), Some("org-sub"));
        config.organizations.resolvers = vec![ResolverKind::Claim];
        assert_eq!(orgs(&config).as_deref(), Some("org-claim"));

        let partitions = |config: &WorkspaceConfig| {
            Lookup::partition(&config.partitions)
                .find(&request)
                .map(|(_, id)| id)
        };
        assert_eq!(partitions(&config).as_deref(), Some("part-header"));
        config.partitions.resolvers = vec![ResolverKind::Path];
        assert_eq!(partitions(&config).as_deref(), Some("uk"));
        config.partitions.resolvers = vec![ResolverKind::Claim];
        assert_eq!(partitions(&config).as_deref(), Some("eu"));
        // The workspace resolvers don't read the organization's header.
        assert_eq!(identifier(&config, &request), None);
    }
}
