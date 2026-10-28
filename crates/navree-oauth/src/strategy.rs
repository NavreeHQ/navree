//! The `oauth` authentication strategy, the [`OAuthToken`] extractor and
//! the layer that adds `WWW-Authenticate: Bearer error="insufficient_scope"`
//! to 403s caused by a token's scopes.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use axum::extract::{FromRequestParts, OptionalFromRequestParts, Request};
use axum::middleware::{Next, from_fn};
use axum::response::Response;
use chrono::{DateTime, Utc};
use http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use http::request::Parts;
use http::{HeaderValue, StatusCode};
use navree_access::{ALL, Ability, AbilityFactory, MANAGE, Rules};
use navree_auth::Strategy;
use navree_contracts::{AuthVia, BoxFuture, Principal};
use navree_http::{AppError, GlobalLayer, LayerSlot, RouteMeta};
use navree_kernel::{AppContext, Injectable, KernelError, ProviderKey, Resolver};
use navree_workspace::PinnedWorkspace;
use uuid::Uuid;

use crate::core::{CachedToken, Core};
use crate::grants::{Grant, Grants};
use crate::token::{ACCESS_TOKEN_PREFIX, PERSONAL_ACCESS_TOKEN_PREFIX};

/// How often a personal access token's `last_used_at` is written.
const TOUCH_EVERY_SECS: i64 = 60;

/// The OAuth access token or personal access token a request was
/// authenticated with (a request extension set by the `oauth` strategy).
///
/// As an extractor, `OAuthToken` answers 401 for requests without one;
/// `Option<OAuthToken>` is `None` for requests that used another kind of
/// credentials (a first-party JWT) or none.
///
/// ```ignore
/// #[post("/export")]
/// #[can(Export, Deal)]
/// async fn export(&self, token: Option<OAuthToken>, CurrentUser(user): CurrentUser) -> Result<NoContent> {
///     if let Some(token) = &token {
///         token.require_scope("deals:export")?; // 403 insufficient_scope
///     }
///     Ok(NoContent)
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthToken {
    /// The token's id (in `oauth_access_tokens` or
    /// `personal_access_tokens`).
    pub token_id: Uuid,
    /// The client that holds it; `None` for a personal access token.
    pub client_id: Option<Uuid>,
    /// The client's name, or the personal access token's name.
    pub client_name: String,
    /// The token's scopes.
    pub scopes: Vec<String>,
    /// How it was obtained.
    pub grant: Grant,
    /// The user it acts as.
    pub user_id: Uuid,
    /// The workspace it is limited to.
    pub workspace_id: Option<Uuid>,
    /// When it expires; `None` for a personal access token that never
    /// does.
    pub expires_at: Option<DateTime<Utc>>,
}

impl OAuthToken {
    /// Whether the token has `scope`.
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// `Ok` when the token has `scope`; otherwise a 403 with
    /// `WWW-Authenticate: Bearer error="insufficient_scope", scope="<scope>"`.
    pub fn require_scope(&self, scope: &str) -> Result<(), AppError> {
        if self.has_scope(scope) {
            Ok(())
        } else {
            Err(insufficient_scope(&[scope.to_owned()]))
        }
    }

    /// Whether this is a personal access token.
    pub fn is_personal(&self) -> bool {
        self.grant == Grant::PersonalAccessToken
    }

    fn from_cached(found: &CachedToken) -> Self {
        Self {
            token_id: found.token_id,
            client_id: found.client_id,
            client_name: found.client_name.clone(),
            scopes: found.scopes.clone(),
            grant: found.grant,
            user_id: found.user_id,
            workspace_id: found.workspace_id,
            expires_at: found.expires_at,
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for OAuthToken {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<OAuthToken>()
            .cloned()
            .ok_or_else(|| {
                AppError::unauthenticated().with_detail("This route needs an OAuth access token.")
            })
    }
}

impl<S: Send + Sync> OptionalFromRequestParts<S> for OAuthToken {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        Ok(parts.extensions.get::<OAuthToken>().cloned())
    }
}

/// A 403 with `WWW-Authenticate: Bearer error="insufficient_scope"`
/// naming `scopes` (any of which would do).
pub(crate) fn insufficient_scope(scopes: &[String]) -> AppError {
    AppError::forbidden()
        .with_detail("The access token's scopes don't allow this.")
        .with_header(WWW_AUTHENTICATE, insufficient_scope_header(scopes))
}

fn insufficient_scope_header(scopes: &[String]) -> HeaderValue {
    let scope = scopes.join(" ").replace(['"', '\\'], "");
    let value = if scope.is_empty() {
        r#"Bearer error="insufficient_scope""#.to_owned()
    } else {
        format!(r#"Bearer error="insufficient_scope", scope="{scope}""#)
    };
    HeaderValue::from_str(&value)
        .unwrap_or_else(|_| HeaderValue::from_static(r#"Bearer error="insufficient_scope""#))
}

/// A 401 with `WWW-Authenticate: Bearer error="invalid_token"`.
fn invalid_token() -> AppError {
    AppError::unauthenticated()
        .with_detail("Invalid token")
        .with_header(
            WWW_AUTHENTICATE,
            HeaderValue::from_static(r#"Bearer error="invalid_token""#),
        )
}

/// The `oauth` authentication strategy (registered with navree-auth): an
/// `Authorization: Bearer` value starting with `nva_` (an OAuth access
/// token) or `nvp_` (a personal access token) is looked up by its digest
/// and authenticates the request as the token's user, with
/// `Principal::token_abilities` set to the permissions of its scopes. Other
/// bearer values are left to the other strategies. An unknown, expired or
/// revoked token is a 401 `Invalid token` with `WWW-Authenticate: Bearer
/// error="invalid_token"`.
pub struct OAuthStrategy {
    grants: Grants,
}

impl Injectable for OAuthStrategy {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Core>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(OAuthStrategy {
                grants: Grants {
                    core: r.get::<Core>().await?,
                },
            })
        })
    }
}

impl fmt::Debug for OAuthStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthStrategy").finish_non_exhaustive()
    }
}

/// The bearer value of `parts`, when it is one of ours.
fn our_bearer(parts: &Parts) -> Option<String> {
    let value = parts.headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    (token.starts_with(ACCESS_TOKEN_PREFIX) || token.starts_with(PERSONAL_ACCESS_TOKEN_PREFIX))
        .then(|| token.to_owned())
}

impl Strategy for OAuthStrategy {
    fn name(&self) -> &'static str {
        "oauth"
    }

    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(async move {
            let Some(token) = our_bearer(parts) else {
                return Ok(None);
            };
            let found = self
                .grants
                .lookup(&token)
                .await
                .map_err(|error| error.to_app_error())?
                .ok_or_else(invalid_token)?;
            let core = &self.grants.core;
            if found.grant == Grant::PersonalAccessToken {
                let now = core.now();
                let stale = found
                    .last_used_at
                    .is_none_or(|last| (now - last).num_seconds() >= TOUCH_EVERY_SECS);
                if stale {
                    let pats = crate::PersonalAccessTokens::new(core.clone());
                    let id = found.token_id;
                    if let Some(cache) = &core.cache {
                        let mut refreshed = found.clone();
                        refreshed.last_used_at = Some(now);
                        cache.insert(crate::token::digest(&token), refreshed);
                    }
                    tokio::spawn(async move { pats.touch(id).await });
                }
            }
            let abilities = core.scopes.permissions(&found.scopes);
            let oauth_token = OAuthToken::from_cached(&found);
            if let Some(workspace) = oauth_token.workspace_id {
                parts.extensions.insert(PinnedWorkspace(workspace));
            }
            parts.extensions.insert(oauth_token);
            Ok(Some(Principal {
                user_id: found.user_id,
                via: AuthVia::Token,
                token_abilities: Some(abilities),
                session_id: None,
            }))
        })
    }
}

/// The global layer (slot `AuthContext`, after the authenticator) that
/// marks 403s of OAuth-token requests whose route needs a permission the
/// token's scopes don't grant: `WWW-Authenticate: Bearer
/// error="insufficient_scope", scope="<scopes that would>"`.
pub(crate) fn insufficient_scope_layer() -> GlobalLayer {
    GlobalLayer::new(
        LayerSlot::AuthContext,
        "oauth-insufficient-scope",
        |_ctx, router| Ok(router.layer(from_fn(mark_insufficient_scope))),
    )
}

async fn mark_insufficient_scope(request: Request, next: Next) -> Response {
    let token = request.extensions().get::<OAuthToken>().cloned();
    let ctx = request.extensions().get::<AppContext>().cloned();
    let mut response = next.run(request).await;
    let (Some(token), Some(ctx)) = (token, ctx) else {
        return response;
    };
    if response.status() != StatusCode::FORBIDDEN
        || response.headers().contains_key(WWW_AUTHENTICATE)
    {
        return response;
    }
    let Some(meta) = response.extensions().get::<Arc<RouteMeta>>().cloned() else {
        return response;
    };
    let Ok(core) = ctx.get::<Core>().await else {
        return response;
    };
    let prefixes = match ctx.try_get::<AbilityFactory>().await {
        Some(factory) => factory.permission_subjects().clone(),
        None => Arc::new(BTreeMap::new()),
    };
    let needs = needed(&meta, &prefixes);
    if needs.is_empty() {
        return response;
    }
    let allowed = |permissions: &[String]| {
        let ability = token_ability(permissions, &prefixes);
        needs
            .iter()
            .all(|(action, subject)| ability.can(action, subject, None))
    };
    if allowed(&core.scopes.permissions(&token.scopes)) {
        // The token allows it: the user's own permissions refused it.
        return response;
    }
    let would: Vec<String> = core
        .scopes
        .app_scopes()
        .into_iter()
        .filter(|scope| allowed(core.scopes.permissions_of(scope)))
        .collect();
    response
        .headers_mut()
        .insert(WWW_AUTHENTICATE, insufficient_scope_header(&would));
    response
}

/// The (action, subject) pairs route `meta` checks.
fn needed(meta: &RouteMeta, prefixes: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let mut needs: Vec<(String, String)> = meta
        .access
        .permissions
        .iter()
        .map(|permission| {
            let (prefix, action) = permission.rsplit_once('.').unwrap_or((permission, ""));
            let action = if action == "*" { MANAGE } else { action };
            let subject = prefixes.get(prefix).map_or(prefix, String::as_str);
            (action.to_owned(), subject.to_owned())
        })
        .collect();
    if let Some(can) = &meta.access.can {
        needs.push((can.action.clone(), can.subject.clone()));
    }
    needs
}

/// What a token whose abilities are `permissions` may do, whoever holds
/// it: `manage all` narrowed to them.
fn token_ability(permissions: &[String], prefixes: &BTreeMap<String, String>) -> Ability {
    let mut rules = Rules::new();
    rules.can(MANAGE, ALL);
    rules.build().restrict_to(permissions, prefixes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insufficient_scope_names_the_scopes() {
        let error = insufficient_scope(&["deals:write".to_owned()]);
        assert_eq!(error.status, StatusCode::FORBIDDEN);
        assert_eq!(
            error.headers[WWW_AUTHENTICATE],
            r#"Bearer error="insufficient_scope", scope="deals:write""#
        );
        assert_eq!(
            insufficient_scope_header(&[]),
            r#"Bearer error="insufficient_scope""#
        );
        assert_eq!(
            invalid_token().headers[WWW_AUTHENTICATE],
            r#"Bearer error="invalid_token""#
        );
    }

    #[test]
    fn token_abilities_cover_permissions_and_subjects() {
        let prefixes = BTreeMap::from([("deals".to_owned(), "Deal".to_owned())]);
        let read = token_ability(&["deals.read".to_owned()], &prefixes);
        assert!(read.can("read", "Deal", None));
        assert!(!read.can("update", "Deal", None));
        let all = token_ability(&["deals.*".to_owned()], &prefixes);
        assert!(all.can("update", "Deal", None));
        let everything = token_ability(&["*".to_owned()], &prefixes);
        assert!(everything.can("delete", "Company", None));
        let none = token_ability(&[], &prefixes);
        assert!(!none.can("read", "Deal", None));
    }

    #[test]
    fn only_our_bearer_tokens_are_taken() {
        let parts = |value: &str| {
            http::Request::builder()
                .header(AUTHORIZATION, value)
                .body(())
                .unwrap()
                .into_parts()
                .0
        };
        assert_eq!(
            our_bearer(&parts("Bearer nva_abc")).as_deref(),
            Some("nva_abc")
        );
        assert_eq!(
            our_bearer(&parts("bearer  nvp_abc ")).as_deref(),
            Some("nvp_abc")
        );
        assert_eq!(our_bearer(&parts("Bearer a.b.c")), None);
        assert_eq!(our_bearer(&parts("Basic nva_abc")), None);
        assert_eq!(our_bearer(&parts("Bearer nvr_abc")), None);
    }
}
