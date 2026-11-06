//! The passkey page: `{sign-in}/passkey`, mounted when
//! `auth.passkeys.enabled` is on. The page holds one button and a small
//! script (`{sign-in}/passkey.js`, served from this site, so the page's
//! content security policy allows it) that runs the WebAuthn ceremony against
//! two JSON routes: `POST {sign-in}/passkey/options` and
//! `POST {sign-in}/passkey/verify`.

use std::time::Duration;

use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use http::{HeaderValue, Method};
use navree_http::extract::Query;
use navree_http::{AppError, IntoResponse, Json, RouteDef};
use navree_session::Session;
use navree_ui::html::{SafeHtml, escape_into};
use navree_ui::pages::ReturnTo;
use navree_ui::{Node, Stack, Text, el};
use navree_validation::{Valid, Validate};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{PagesController, Paths, link, page, signed_in_target};
use crate::{AuthError, ClientMeta, PasskeyLoginOptionsInput, SessionSignIn};

/// The script of the page.
const SCRIPT: &str = include_str!("passkey.js");

/// How many ceremonies one client may start per minute.
const CEREMONIES_PER_MINUTE: u32 = 30;

/// `?next=`.
#[derive(Debug, Default, Deserialize)]
struct NextQuery {
    #[serde(default)]
    next: Option<String>,
}

/// `POST {sign-in}/passkey/verify`: the assertion the browser made, and the
/// page to go to afterwards.
#[derive(Debug, Deserialize, Validate)]
struct PasskeySignIn {
    /// The `PublicKeyCredential` JSON.
    #[validate(skip)]
    credential: Value,
    /// "Remember me".
    #[validate(skip)]
    #[serde(default)]
    remember: bool,
    /// Where the sign-in page was going.
    #[validate(length(max = 2048))]
    #[serde(default)]
    next: Option<String>,
}

fn passkey_page(paths: &Paths, next: Option<&str>) -> navree_ui::View {
    let next = next
        .and_then(|next| ReturnTo::parse_for(next, &paths.sign_in))
        .map(|next| next.as_str().to_owned())
        .unwrap_or_default();
    let mut script = String::from("<script src=\"");
    escape_into(&mut script, &format!("{}.js", paths.passkey));
    script.push_str("\" defer></script>");
    page(
        paths,
        "Sign in with a passkey",
        "Use the passkey on this device, or one from your phone or security key.",
        Stack::gap(4)
            .children((
                el("div")
                    .slot("alert")
                    .attr("data-tone", "danger")
                    .attr("role", "alert")
                    .attr("id", "passkey-error")
                    .flag("hidden", true),
                el("button")
                    .slot("button")
                    .attr("data-variant", "primary")
                    .attr("type", "button")
                    .attr("id", "passkey-sign-in")
                    .attr("data-base", paths.passkey.clone())
                    .attr("data-next", next)
                    .child("Use a passkey"),
                el("noscript").child(Text::muted(
                    "Passkeys need JavaScript. Sign in with your password instead.",
                )),
                Node::from(SafeHtml::trusted(script)),
                link("Sign in with a password", &paths.sign_in),
            ))
            .into(),
    )
}

impl PagesController {
    /// The routes of the passkey page, when `auth.passkeys.enabled`.
    pub(super) fn passkey_routes(&self) -> Vec<RouteDef> {
        let paths = self.paths.clone();
        let absolute = |path: &str| format!("!{path}");
        let mut routes = Vec::new();

        // GET passkey
        let at = paths.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                absolute(&paths.passkey),
                move |session: Session, Query(query): Query<NextQuery>| {
                    let paths = at.clone();
                    async move {
                        // Starts the session, so the page's script finds the
                        // XSRF-TOKEN cookie.
                        session.csrf_token();
                        passkey_page(&paths, query.next.as_deref()).into_response()
                    }
                },
            )
            .public()
            .page()
            .name("auth.pages.passkey"),
        );

        // GET passkey.js
        routes.push(
            RouteDef::on(
                Method::GET,
                absolute(&format!("{}.js", paths.passkey)),
                || async {
                    let mut response = SCRIPT.into_response();
                    let headers = response.headers_mut();
                    headers.insert(
                        CONTENT_TYPE,
                        HeaderValue::from_static("text/javascript; charset=utf-8"),
                    );
                    headers.insert(
                        CACHE_CONTROL,
                        HeaderValue::from_static("public, max-age=3600"),
                    );
                    response
                },
            )
            .public()
            .name("auth.pages.passkey.script"),
        );

        // POST passkey/options
        let service = self.service.clone();
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&format!("{}/options", paths.passkey)),
                move |Valid(Json(input)): Valid<Json<PasskeyLoginOptionsInput>>| {
                    let service = service.clone();
                    async move {
                        let options = service
                            .passkey_login_options(input.email.as_deref())
                            .await
                            .map_err(AuthError::into_app_error)?;
                        Ok::<_, AppError>(Json(json!({ "publicKey": options })))
                    }
                },
            )
            .public()
            .throttle(CEREMONIES_PER_MINUTE, Duration::from_secs(60))
            .name("auth.pages.passkey.options"),
        );

        // POST passkey/verify
        let (service, at) = (self.service.clone(), paths.clone());
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&format!("{}/verify", paths.passkey)),
                move |session: Session,
                      meta: ClientMeta,
                      Valid(Json(input)): Valid<Json<PasskeySignIn>>| {
                    let (service, paths) = (service.clone(), at.clone());
                    async move {
                        let outcome = service
                            .login_with_passkey_session(
                                &session,
                                &input.credential,
                                input.remember,
                                &meta,
                            )
                            .await
                            .map_err(AuthError::into_app_error)?;
                        let (status, next) = match outcome {
                            SessionSignIn::SignedIn(_) => (
                                "signed_in",
                                signed_in_target(
                                    &service,
                                    &session,
                                    &paths,
                                    input.next.as_deref(),
                                )
                                .as_str()
                                .to_owned(),
                            ),
                            SessionSignIn::TwoFactorRequired => (
                                "two_factor",
                                paths
                                    .with_next(&paths.two_factor, input.next.as_deref())
                                    .as_str()
                                    .to_owned(),
                            ),
                        };
                        Ok::<_, AppError>(Json(json!({ "status": status, "next": next })))
                    }
                },
            )
            .public()
            .throttle(CEREMONIES_PER_MINUTE, Duration::from_secs(60))
            .name("auth.pages.passkey.verify"),
        );
        routes
    }
}
