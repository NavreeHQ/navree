//! The passkey page: `{sign-in}/passkey`, mounted when
//! `auth.passkeys.enabled` is on. The page holds one button and a small
//! script (`{sign-in}/passkey.js`, served from this site, so the page's
//! content security policy allows it) that runs the WebAuthn ceremony against
//! two JSON routes: `POST {sign-in}/passkey/options` and
//! `POST {sign-in}/passkey/verify`.
//!
//! A native app does the same ceremony in the system's own sheet, started by
//! the `passkey` device call the sign-in page gives a client that can. Its two
//! routes stand beside the page's:
//!
//! - `POST {sign-in}/passkey/native/options` answers the same options
//!   (`{"publicKey": {...}}`) and has the session remember the challenge as
//!   the one it asked for;
//! - `POST {sign-in}/passkey/native` takes the call's result as a form
//!   (`credential`, the call's ticket as `_call`, and `outcome`). The ticket
//!   must be the one sealed for this session, this call and this route, and
//!   it is good once; the assertion must answer the challenge this session
//!   asked for; and then it is checked by
//!   [`AuthService::login_with_passkey_session`](crate::AuthService::login_with_passkey_session),
//!   as the page's own `verify` route checks a browser's. The answer is where
//!   the app goes next, as after the sign-in form.

use std::time::Duration;

use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use http::{HeaderValue, Method};
use navree_contracts::TicketCheck;
use navree_http::extract::Query;
use navree_http::{ApiMeta, AppError, Form, IntoResponse, Json, Response, RouteDef};
use navree_session::Session;
use navree_ui::html::{SafeHtml, escape_into};
use navree_ui::pages::ReturnTo;
use navree_ui::{Node, Reply, Stack, Text, el};
use navree_validation::{Valid, Validate};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    PASSKEY_CALL, PagesController, Paths, api, call_holder, link, page, signed_in_redirect,
    signed_in_target,
};
use crate::{AuthError, ClientMeta, PasskeyLoginOptionsInput, SessionSignIn};

/// The script of the page.
const SCRIPT: &str = include_str!("passkey.js");

/// How many ceremonies one client may start per minute.
const CEREMONIES_PER_MINUTE: u32 = 30;

/// The longest assertion a native app's call may post, in bytes (one is about a
/// kilobyte).
const MAX_ASSERTION_LEN: usize = 16 * 1024;

/// The outcome of a device call that produced its result, as the protocol posts it.
const OUTCOME_OK: &str = "ok";

/// The outcome of a device call the person closed the sheet of.
const OUTCOME_CANCELLED: &str = "cancelled";

/// What a native app's `passkey` call posts back, as a form: the assertion as
/// WebAuthn's `toJSON()` writes it (JSON text), the ticket the call carried, and
/// how the call went (`ok`, or why there is no assertion). Nothing in it is
/// trusted before the ticket and the assertion are checked, and it is never
/// written to a log.
#[derive(Default, Deserialize)]
struct PasskeyCallResult {
    #[serde(default)]
    credential: String,
    #[serde(default, rename = "_call")]
    ticket: String,
    #[serde(default)]
    outcome: String,
}

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
            .name("auth.pages.passkey")
            .api(api("The passkey sign-in page", &[])),
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
            .name("auth.pages.passkey.script")
            .api(api("The passkey page's script", &[])),
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
            .name("auth.pages.passkey.options")
            .api(ApiMeta {
                description: Some(
                    "JSON for the page's script: the WebAuthn request options as `{publicKey}`."
                        .to_owned(),
                ),
                ..api("Starts signing in with a passkey", &[400, 403, 429])
            }),
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
                                signed_in_target(&service, &session, &paths, input.next.as_deref())
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
            .name("auth.pages.passkey.verify")
            .api(ApiMeta {
                description: Some(
                    "JSON for the page's script: the assertion the browser made. Answers \
                     `{status, next}`, `status` being `signed_in` or `two_factor`."
                        .to_owned(),
                ),
                ..api("Signs in with a passkey", &[400, 401, 403, 429])
            }),
        );
        routes.extend(self.passkey_call_routes());
        routes
    }

    /// The two routes of the `passkey` device call a native app starts from the
    /// sign-in page: see the module's words.
    fn passkey_call_routes(&self) -> Vec<RouteDef> {
        let paths = self.paths.clone();
        let absolute = |path: &str| format!("!{path}");
        let then = format!("{}/native", paths.passkey);
        let mut routes = Vec::new();

        // POST passkey/native/options
        let service = self.service.clone();
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&format!("{then}/options")),
                move |session: Session| {
                    let service = service.clone();
                    async move {
                        let options = service
                            .passkey_call_options(&session)
                            .await
                            .map_err(AuthError::into_app_error)?;
                        Ok::<_, AppError>(Json(json!({ "publicKey": options })))
                    }
                },
            )
            .public()
            .throttle(CEREMONIES_PER_MINUTE, Duration::from_secs(60))
            .name("auth.pages.passkey.native.options")
            .api(ApiMeta {
                description: Some(
                    "JSON for a native app's `passkey` call: the WebAuthn request options as \
                     `{publicKey}`. The session remembers the challenge as the one it asked \
                     for."
                        .to_owned(),
                ),
                ..api(
                    "Starts signing in with a passkey in a native app",
                    &[403, 429],
                )
            }),
        );

        // POST passkey/native
        let (service, tickets, at, route) = (
            self.service.clone(),
            self.tickets.clone(),
            paths.clone(),
            then.clone(),
        );
        routes.push(
            RouteDef::on(
                Method::POST,
                absolute(&then),
                move |session: Session,
                      meta: ClientMeta,
                      Query(query): Query<NextQuery>,
                      Form(result): Form<PasskeyCallResult>| {
                    let (service, tickets, paths, route) =
                        (service.clone(), tickets.clone(), at.clone(), route.clone());
                    async move {
                        let next = query.next.as_deref();
                        // The ticket first: sealed for this session, this call and this
                        // route, and good once. A result without it is nobody's.
                        let check = TicketCheck::new(call_holder(&session), PASSKEY_CALL)
                            .at("POST", &route);
                        tickets
                            .redeem(&result.ticket, &check)
                            .await
                            .map_err(|error| AuthError::from(error).into_app_error())?;
                        if result.outcome != OUTCOME_OK {
                            // No assertion came back. A sheet the person closed needs no
                            // words; anything else is said on the sign-in page.
                            let back = if result.outcome == OUTCOME_CANCELLED {
                                paths.with_next(&paths.sign_in, next)
                            } else {
                                paths.sign_in_with(next, "passkey-unused")
                            };
                            return Ok(Reply::redirect(back).into_response());
                        }
                        // An assertion that does not read is refused where every other is.
                        let credential = Some(result.credential.as_str())
                            .filter(|text| text.len() <= MAX_ASSERTION_LEN)
                            .and_then(|text| serde_json::from_str::<Value>(text).ok())
                            .unwrap_or(Value::Null);
                        let outcome = service
                            .login_with_passkey_call_session(&session, &credential, false, &meta)
                            .await;
                        match outcome {
                            Ok(SessionSignIn::SignedIn(_)) => {
                                Ok(signed_in_redirect(&service, &session, &paths, next))
                            }
                            Ok(SessionSignIn::TwoFactorRequired) => {
                                Ok(Reply::redirect(paths.with_next(&paths.two_factor, next))
                                    .into_response())
                            }
                            Err(AuthError::PasskeyRefused(_)) => {
                                Ok(Reply::redirect(paths.sign_in_with(next, "passkey-failed"))
                                    .into_response())
                            }
                            Err(error) => Err::<Response, AppError>(error.into_app_error()),
                        }
                    }
                },
            )
            .public()
            .throttle(CEREMONIES_PER_MINUTE, Duration::from_secs(60))
            .name("auth.pages.passkey.native")
            .api(ApiMeta {
                description: Some(
                    "The result of a native app's `passkey` call, as a form: `credential` \
                     (the assertion), `_call` (the call's ticket) and `outcome`. Goes on to \
                     `next`, or to the second factor (303); an assertion that is refused, or \
                     a call that brought none, goes back to the sign-in page (303). A ticket \
                     that is not this session's, or was used, is refused (422)."
                        .to_owned(),
                ),
                ..api(
                    "Signs a native app in with a passkey",
                    &[303, 403, 422, 429],
                )
            }),
        );
        routes
    }
}
