//! A Navree UI consent page for the OAuth server (the `ui` feature): what an app asks for, in
//! the app's shell, with Allow and Deny.
//!
//! `GET /oauth/authorize` sends the browser to `oauth.consent_url` with `?request=<id>`. With
//! this feature on, the module serves that page itself at [`CONSENT_PATH`]
//! (`oauth.consent_url = "/oauth/consent"`): the signed-in person sees who is asking (the
//! client's name, and its website when it has one), the scopes it asks for in the words of
//! `oauth.scopes`, whether they allowed these scopes before, and two buttons. Allow and Deny
//! post to the same decisions the JSON routes make (`/api/oauth/requests/{id}/approve` and
//! `deny`: the approval remembered, the audit entry written, the code issued) and send the
//! browser back to the client.
//!
//! The page needs a person signed in with the session (a guest is sent to sign in and back);
//! a request made with an OAuth or a personal access token is refused, as the JSON routes
//! refuse it. A client's redirect address is followed as an address on the web (`http` or
//! `https`); a client that registered another scheme (an app on a phone) reads its decision
//! from the JSON routes.

use std::sync::Arc;

use http::{HeaderMap, Method};
use navree_db::Uow;
use navree_http::{AppError, Controller, CurrentPrincipal, Path, Query, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_ui::form::csrf_field;
use navree_ui::html::Href;
use navree_ui::{AppShell, Button, ButtonVariant, Card, Node, Reply, View, el};
use serde::Deserialize;
use uuid::Uuid;

use crate::core::Core;
use crate::requests::{AuthorizationRequestView, Requests};
use crate::strategy::OAuthToken;

/// Where the consent page is: set `oauth.consent_url` to it.
pub const CONSENT_PATH: &str = "/oauth/consent";

/// The cookie the session layer leaves for the page's posts.
const CSRF_COOKIE: &str = "XSRF-TOKEN";

/// The words of the consent page. English by default. `{client}` is the client's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsentLabels {
    /// The heading.
    pub title: String,
    /// Under the heading.
    pub intro: String,
    /// Above the list of scopes.
    pub scopes: String,
    /// The client's website, as a link.
    pub website: String,
    /// When the person allowed these scopes before.
    pub already: String,
    /// The button that allows.
    pub allow: String,
    /// The button that refuses.
    pub deny: String,
}

impl Default for ConsentLabels {
    fn default() -> Self {
        Self {
            title: "Authorize {client}".to_owned(),
            intro: "{client} wants to use your account.".to_owned(),
            scopes: "It will be able to".to_owned(),
            website: "Website".to_owned(),
            already: "You allowed these before.".to_owned(),
            allow: "Allow".to_owned(),
            deny: "Deny".to_owned(),
        }
    }
}

/// The consent page for `view`, with the token its posts carry when the session has one.
pub fn consent_card(
    view: &AuthorizationRequestView,
    csrf: Option<&str>,
    labels: &ConsentLabels,
) -> Node {
    let name = &view.client.name;
    let scopes: Vec<Node> = view
        .scopes
        .iter()
        .map(|scope| {
            el("li")
                .slot("consent-scope")
                .children((
                    el("strong").child(scope.name.clone()),
                    el("span").child(scope.description.clone()),
                ))
                .into()
        })
        .collect();
    let website = view.client.website.as_deref().map(|site| {
        el("p").slot("consent-website").child(
            el("a")
                .attr("href", Href::from(site))
                .attr("rel", "noopener noreferrer")
                .child(format!("{}: {site}", labels.website)),
        )
    });
    let post = |decision: &str, button: Button| {
        el("form")
            .attr("method", "post")
            .attr("action", format!("{CONSENT_PATH}/{}/{decision}", view.id))
            .flag("data-ui-form", true)
            .children((
                csrf.map(|token| csrf_field(token.to_owned())),
                button.submit(),
            ))
    };
    Card::titled(labels.title.replace("{client}", name))
        .description(labels.intro.replace("{client}", name))
        .body((
            website,
            el("p").child(labels.scopes.clone()),
            el("ul").slot("consent-scopes").children(scopes),
            view.already_approved.then(|| {
                el("p")
                    .slot("consent-already")
                    .child(labels.already.clone())
            }),
            el("div").slot("consent-actions").children((
                post(
                    "deny",
                    Button::new(labels.deny.clone()).variant(ButtonVariant::Outline),
                ),
                post("approve", Button::primary(labels.allow.clone())),
            )),
        ))
        .into()
}

/// The token the session left for this page's posts, from the request's cookies.
fn csrf_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            let (name, token) = pair.trim().split_once('=')?;
            (name == CSRF_COOKIE).then(|| {
                form_urlencoded::parse(format!("x={token}").as_bytes())
                    .next()
                    .map_or_else(String::new, |(_, token)| token.into_owned())
            })
        })
        .filter(|token| !token.is_empty())
}

/// An OAuth or personal access token may not decide for a person.
fn first_party(token: &Option<OAuthToken>) -> Result<(), AppError> {
    if token.is_some() {
        return Err(AppError::forbidden().with_detail(
            "OAuth and personal access tokens can't manage OAuth access; sign in to the app.",
        ));
    }
    Ok(())
}

/// `?request=<id>`: the authorization request the browser was sent with.
#[derive(Debug, Deserialize)]
struct ConsentQuery {
    request: Uuid,
}

/// The consent page and its two decisions, mounted by `OAuthServerModule` with its `ui` feature.
pub struct ConsentController {
    core: Arc<Core>,
}

impl Injectable for ConsentController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Core>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                core: r.get::<Core>().await?,
            })
        })
    }
}

/// Sends the browser back to the client with `redirect_to`.
fn to_client(redirect_to: String) -> Result<Reply, AppError> {
    let href = Href::parse(&redirect_to).map_err(|_| {
        AppError::bad_request("The client's redirect address is not one a page can follow.")
    })?;
    Ok(Reply::redirect_external(href))
}

impl Controller for ConsentController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let core = self.core.clone();
        let show = move |CurrentPrincipal(me): CurrentPrincipal,
                         token: Option<OAuthToken>,
                         shell: Option<AppShell>,
                         headers: HeaderMap,
                         Query(query): Query<ConsentQuery>| {
            let requests = Requests { core: core.clone() };
            async move {
                first_party(&token)?;
                let view = requests.show(me.user_id, query.request).await?;
                let card = consent_card(
                    &view,
                    csrf_of(&headers).as_deref(),
                    &ConsentLabels::default(),
                );
                let title = ConsentLabels::default()
                    .title
                    .replace("{client}", &view.client.name);
                Ok::<_, AppError>(match shell {
                    Some(shell) => shell.page(title).body(card).to_view(),
                    None => View::new(title).body(card),
                })
            }
        };
        let core = self.core.clone();
        let approve = move |CurrentPrincipal(me): CurrentPrincipal,
                            token: Option<OAuthToken>,
                            uow: Uow,
                            Path(id): Path<Uuid>| {
            let requests = Requests { core: core.clone() };
            async move {
                first_party(&token)?;
                let decision = requests.approve(&uow, me.user_id, id).await?;
                to_client(decision.redirect_to)
            }
        };
        let core = self.core.clone();
        let deny = move |CurrentPrincipal(me): CurrentPrincipal,
                         token: Option<OAuthToken>,
                         uow: Uow,
                         Path(id): Path<Uuid>| {
            let requests = Requests { core: core.clone() };
            async move {
                first_party(&token)?;
                let decision = requests.deny(&uow, me.user_id, id).await?;
                to_client(decision.redirect_to)
            }
        };
        vec![
            RouteDef::on(Method::GET, format!("!{CONSENT_PATH}"), show)
                .authenticated()
                .name("oauth.consent"),
            RouteDef::on(
                Method::POST,
                format!("!{CONSENT_PATH}/{{id}}/approve"),
                approve,
            )
            .authenticated()
            .name("oauth.consent.approve"),
            RouteDef::on(Method::POST, format!("!{CONSENT_PATH}/{{id}}/deny"), deny)
                .authenticated()
                .name("oauth.consent.deny"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::requests::ClientSummary;
    use crate::scope::ScopeInfo;

    fn view(approved: bool) -> AuthorizationRequestView {
        AuthorizationRequestView {
            id: Uuid::nil(),
            client: ClientSummary {
                id: Uuid::nil(),
                name: "Khalsa Books".to_owned(),
                logo_url: None,
                website: Some("https://books.example".to_owned()),
            },
            scopes: vec![
                ScopeInfo {
                    name: "deals:read".to_owned(),
                    description: "See your deals".to_owned(),
                },
                ScopeInfo {
                    name: "email".to_owned(),
                    description: "See your email address".to_owned(),
                },
            ],
            already_approved: approved,
            expires_at: Utc::now(),
        }
    }

    fn html(view: &AuthorizationRequestView, csrf: Option<&str>) -> String {
        navree_ui::render(&consent_card(view, csrf, &ConsentLabels::default()))
    }

    #[test]
    fn the_page_names_the_client_the_scopes_and_the_two_decisions() {
        let page = html(&view(false), Some("tok-1"));
        assert!(page.contains("Authorize Khalsa Books"), "{page}");
        assert!(
            page.contains("deals:read") && page.contains("See your deals"),
            "{page}"
        );
        assert!(page.contains("See your email address"), "{page}");
        assert!(page.contains(r#"href="https://books.example""#), "{page}");
        assert!(
            page.contains(&format!("{CONSENT_PATH}/{}/approve", Uuid::nil()))
                && page.contains(&format!("{CONSENT_PATH}/{}/deny", Uuid::nil())),
            "{page}"
        );
        assert!(
            page.contains(r#"name="_csrf""#) && page.contains("tok-1"),
            "{page}"
        );
        assert!(!page.contains("You allowed these before."), "{page}");
        let again = html(&view(true), None);
        assert!(again.contains("You allowed these before."), "{again}");
        assert!(!again.contains("_csrf"), "{again}");
    }

    #[test]
    fn a_client_cannot_put_markup_or_a_script_address_on_the_page() {
        let mut hostile = view(false);
        hostile.client.name = "<img src=x onerror=alert(1)>".to_owned();
        hostile.client.website = Some("javascript:alert(1)".to_owned());
        let page = html(&hostile, None);
        assert!(!page.contains("<img"), "{page}");
        assert!(!page.contains("javascript:"), "{page}");
    }

    #[test]
    fn the_token_comes_from_the_sessions_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::COOKIE,
            "a=b; XSRF-TOKEN=abc%3D%3D; c=d".parse().unwrap(),
        );
        assert_eq!(csrf_of(&headers).as_deref(), Some("abc=="));
        assert_eq!(csrf_of(&HeaderMap::new()), None);
    }
}
