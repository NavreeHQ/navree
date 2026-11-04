//! The switch route (feature `session`): `POST {workspace.switch_path}`
//! stores the workspace a browser switches to, for the `session` resolver.

use std::collections::BTreeMap;
use std::sync::Arc;

use http::header::{CONTENT_TYPE, LOCATION};
use http::request::Parts;
use http::{HeaderValue, Method, StatusCode};
use navree_contracts::{AuditEntry, AuditKind, AuditSink, RequestId};
use navree_http::body::Bytes;
use navree_http::{
    AppError, ClientIp, Controller, CurrentPrincipal, IntoResponse, Json, Response, RouteDef,
};
use navree_kernel::{AppContext, BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_session::Session;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::controller::api;
use crate::current::{app_error, guest_member};
use crate::entity::Workspace;
use crate::resolve::{SESSION_KEY, SessionChoice};
use crate::{WORKSPACE_SUBJECT, WorkspaceConfig, WorkspaceError, Workspaces};

/// The longest workspace identifier (a slug or an id) the route reads.
const MAX_IDENTIFIER: usize = 255;

/// The longest `next` address the route follows.
const MAX_NEXT: usize = 2048;

/// The route's name.
pub const SWITCH_ROUTE: &str = "workspace.switch";

/// `POST {workspace.switch_path}` (default `/workspace/switch`, no API
/// prefix): the signed-in user switches this browser to a workspace, which
/// the `session` resolver then names on every request.
///
/// - **Body**: a form (`workspace` = a slug or an id, optional `next`), as
///   [`WorkspaceSwitcher`](crate::WorkspaceSwitcher) posts it, or JSON
///   `{"workspace", "next"?}`. A form may also name both in the query string.
///   The session layer checks the CSRF token first (`_csrf` in the form, or
///   `X-XSRF-TOKEN`).
/// - **Checks**: 404 `workspace_not_found` for an unknown workspace; 403
///   `not_a_member` when the user may not enter it (no membership, no
///   grant of its organization and, for a demo workspace, no
///   `workspace.demo.guest_role`). Nothing is stored then.
/// - **On success** the choice is stored in the session for this user, the
///   session gets a new id and CSRF token (a privilege change), and
///   `workspace.switched` goes to the audit trail when the app has a
///   `dyn AuditSink`. A form is answered `303 See Other` to `next` when it
///   is a path on this site, else to `/`; JSON with `200 {"id", "slug",
///   "name"}`.
pub(crate) struct SwitchController {
    workspaces: Arc<Workspaces>,
    config: Arc<WorkspaceConfig>,
}

impl Injectable for SwitchController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Workspaces>(),
            ProviderKey::of::<WorkspaceConfig>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(SwitchController {
                workspaces: r.get::<Workspaces>().await?,
                config: r.get::<WorkspaceConfig>().await?,
            })
        })
    }
}

impl Controller for SwitchController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.config.routes {
            return Vec::new();
        }
        let (workspaces, config) = (self.workspaces.clone(), self.config.clone());
        let handler = move |CurrentPrincipal(me): CurrentPrincipal,
                            session: Session,
                            parts: Parts,
                            body: Bytes| {
            let (workspaces, config) = (workspaces.clone(), config.clone());
            async move { switch(&workspaces, &config, me.user_id, &session, &parts, &body).await }
        };
        vec![
            RouteDef::on(
                Method::POST,
                format!("!{}", self.config.switch_path),
                handler,
            )
            .authenticated()
            .throttle(30, std::time::Duration::from_secs(60))
            .name(SWITCH_ROUTE)
            .api(api(
                "Switch this browser to a workspace",
                &[303, 400, 401, 403, 404, 429],
                None,
                None,
            )),
        ]
    }
}

/// What the route reads from the request.
#[derive(Debug, Default, Deserialize)]
struct SwitchInput {
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    next: Option<String>,
}

/// Switches `user`'s browser session to the workspace the request names.
async fn switch(
    workspaces: &Workspaces,
    config: &WorkspaceConfig,
    user: Uuid,
    session: &Session,
    parts: &Parts,
    body: &[u8],
) -> Result<Response, AppError> {
    let json = is_json(parts);
    let input = read_input(parts, body, json)?;
    let identifier = input
        .workspace
        .as_deref()
        .map(str::trim)
        .filter(|identifier| !identifier.is_empty() && identifier.len() <= MAX_IDENTIFIER)
        .ok_or_else(|| {
            AppError::validation(BTreeMap::from([(
                "workspace".to_owned(),
                vec!["Choose a workspace.".to_owned()],
            )]))
        })?;
    let workspace = workspaces.resolve(identifier).await.map_err(app_error)?;
    if !may_enter(workspaces, config, &workspace, user).await? {
        return Err(app_error(WorkspaceError::NotMember));
    }
    let previous = session
        .get::<SessionChoice>(SESSION_KEY)
        .filter(|choice| choice.user == user)
        .map(|choice| choice.id);
    session
        .insert(
            SESSION_KEY,
            &SessionChoice {
                id: workspace.id,
                user,
            },
        )
        .map_err(AppError::internal)?;
    // A new role in a new workspace: a new session id and CSRF token.
    session.regenerate();
    audit(parts, user, previous, &workspace).await;
    if json {
        return Ok(Json(json!({
            "id": workspace.id,
            "slug": workspace.slug,
            "name": workspace.name,
        }))
        .into_response());
    }
    let mut response = StatusCode::SEE_OTHER.into_response();
    let location = safe_next(input.next.as_deref());
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(&location).unwrap_or_else(|_| HeaderValue::from_static("/")),
    );
    Ok(response)
}

/// Whether `user` may enter `workspace`: a member, a grant of its
/// organization, or (a demo workspace with `workspace.demo.guest_role`) a
/// guest, as [`CurrentWorkspace`](crate::CurrentWorkspace) admits them.
async fn may_enter(
    workspaces: &Workspaces,
    config: &WorkspaceConfig,
    workspace: &Workspace,
    user: Uuid,
) -> Result<bool, AppError> {
    if workspaces
        .admission(workspace, user)
        .await
        .map_err(app_error)?
        .is_some()
    {
        return Ok(true);
    }
    Ok(guest_member(config, workspace, user).is_some())
}

/// Writes `workspace.switched` to the app's audit trail, when it has one.
async fn audit(parts: &Parts, user: Uuid, previous: Option<Uuid>, workspace: &Workspace) {
    let Some(ctx) = parts.extensions.get::<AppContext>() else {
        return;
    };
    let Some(sink) = ctx.try_get::<dyn AuditSink>().await else {
        return;
    };
    let mut entry = AuditEntry::new(AuditKind::Access, "workspace.switched")
        .actor(user)
        .subject(WORKSPACE_SUBJECT, workspace.id.to_string())
        .metadata(json!({ "from": previous, "to": workspace.id }));
    let ClientIp(ip) = ClientIp::from_parts(parts);
    if !ip.is_unspecified() {
        entry = entry.ip(ip.to_canonical().to_string());
    }
    if let Some(request_id) = parts.extensions.get::<RequestId>() {
        entry = entry.request_id(request_id.as_str());
    }
    if let Some(user_agent) = parts
        .headers
        .get(http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
    {
        entry = entry.user_agent(user_agent);
    }
    if let Err(error) = sink.record(entry).await {
        tracing::warn!(target: "navree::workspace", %error, "could not audit a workspace switch");
    }
}

/// Whether the request's body is JSON (`application/json` or `+json`).
fn is_json(parts: &Parts) -> bool {
    parts
        .headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(|kind| kind.trim().to_ascii_lowercase())
        .is_some_and(|kind| kind == "application/json" || kind.ends_with("+json"))
}

/// The `workspace` and `next` the request sends: in its JSON body, or in its
/// form body, then its query string.
fn read_input(parts: &Parts, body: &[u8], json: bool) -> Result<SwitchInput, AppError> {
    if json {
        return serde_json::from_slice(body)
            .map_err(|_| AppError::bad_request("The body is not a JSON object with `workspace`."));
    }
    let mut input = SwitchInput::default();
    let query = parts.uri.query().unwrap_or("").as_bytes();
    // The body first, so a form's own fields win over its address.
    for (name, value) in form_urlencoded::parse(body).chain(form_urlencoded::parse(query)) {
        match name.as_ref() {
            "workspace" if input.workspace.is_none() => input.workspace = Some(value.into_owned()),
            "next" if input.next.is_none() => input.next = Some(value.into_owned()),
            _ => {}
        }
    }
    Ok(input)
}

/// `next` when it is a path on this site (one leading `/`, not `//` or
/// `/\`, no backslash or control character, at most 2 KiB), else `/`: the
/// route never sends a browser to another site.
pub(crate) fn safe_next(next: Option<&str>) -> String {
    let Some(next) = next.map(str::trim) else {
        return "/".to_owned();
    };
    let bytes = next.as_bytes();
    let local = bytes.first() == Some(&b'/')
        && !matches!(bytes.get(1), Some(b'/' | b'\\'))
        && !next.contains('\\')
        && next.len() <= MAX_NEXT
        && !bytes.iter().any(|byte| *byte < 0x20 || *byte == 0x7f);
    if local {
        next.to_owned()
    } else {
        "/".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // MOD-003: the switch route never sends a browser off the site.
    #[test]
    fn next_is_followed_only_to_a_path_on_this_site() {
        for (next, expected) in [
            (Some("/deals?stage=won"), "/deals?stage=won"),
            (Some("/"), "/"),
            (Some("  /w/acme  "), "/w/acme"),
            (None, "/"),
            (Some(""), "/"),
            (Some("deals"), "/"),
            (Some("//evil.test/"), "/"),
            (Some("/\\evil.test"), "/"),
            (Some("/a\\b"), "/"),
            (Some("https://evil.test/"), "/"),
            (Some("javascript:alert(1)"), "/"),
            (Some("/a\r\nSet-Cookie: x=1"), "/"),
        ] {
            assert_eq!(safe_next(next), expected, "{next:?}");
        }
        let long = format!("/{}", "a".repeat(MAX_NEXT));
        assert_eq!(safe_next(Some(&long)), "/");
    }

    fn parts(content_type: Option<&str>, uri: &str) -> Parts {
        let mut request = http::Request::builder().method(Method::POST).uri(uri);
        if let Some(kind) = content_type {
            request = request.header(CONTENT_TYPE, kind);
        }
        request.body(()).unwrap().into_parts().0
    }

    // MOD-003: a form, its query string, or JSON name the workspace.
    #[test]
    fn the_workspace_comes_from_a_form_its_address_or_json() {
        let form = parts(
            Some("application/x-www-form-urlencoded"),
            "/workspace/switch?workspace=from-query&next=%2Fq",
        );
        let input = read_input(&form, b"workspace=acme&next=%2Fdeals", false).unwrap();
        assert_eq!(input.workspace.as_deref(), Some("acme"));
        assert_eq!(input.next.as_deref(), Some("/deals"));
        let input = read_input(&form, b"", false).unwrap();
        assert_eq!(input.workspace.as_deref(), Some("from-query"));
        assert_eq!(input.next.as_deref(), Some("/q"));

        let json = parts(Some("application/json; charset=utf-8"), "/workspace/switch");
        assert!(is_json(&json));
        assert!(!is_json(&form));
        let input = read_input(&json, br#"{"workspace": "globex"}"#, true).unwrap();
        assert_eq!(input.workspace.as_deref(), Some("globex"));
        assert_eq!(input.next, None);
        assert!(read_input(&json, b"workspace=acme", true).is_err());
    }
}
