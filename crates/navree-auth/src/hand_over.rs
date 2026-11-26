//! Handing a browser sign-in over to a native app (feature `session`).
//!
//! A native app signs a person in through the system's sign-in session
//! (`ASWebAuthenticationSession` on Apple systems, a Custom Tab or Auth Tab
//! on Android, the person's own browser on Windows), which shows the site's
//! own sign-in pages with every way in the site offers. That session keeps
//! its own cookies, so the app is still signed out when it ends. The
//! hand-over is an authorization code with a proof of possession (PKCE, S256
//! only):
//!
//! 1. The app makes a random `code_verifier` and opens the system's sign-in
//!    session at `GET /auth/app/sign-in?platform=apple|android|windows&app=<its
//!    id>&code_challenge=<base64url SHA-256 of the
//!    verifier>&code_challenge_method=S256&state=<its own random value>`.
//!    The app's id is its bundle id (`apple`), its package name (`android`),
//!    or, on `windows`, its package family name when it is installed from a
//!    package (in whatever case it writes the letters) and the id it was
//!    built with when it is not. The app must be
//!    one of `auth.apps` with a `callback`. The session remembers the
//!    request and goes to `GET /auth/app/finish`.
//! 2. `GET /auth/app/finish` sends a browser that is not signed in to the
//!    sign-in page with `next` set back to itself and the hand-over's own
//!    one-time `attempt`; the person signs in there by any way and in as
//!    many steps as it takes. A sign-in that comes back with that `attempt`
//!    is answered `303` to the app's configured callback with a one-time
//!    `code` (60 seconds) and the app's `state`; the browser is then signed
//!    out, unless `auth.apps.keep_browser_signed_in` is on. A browser that
//!    was already signed in when the hand-over began is asked first, on a
//!    small page of the framework's own (no page toolkit needed) that names
//!    the app and the person ("com.example.orders wants to sign in as Simran
//!    Kaur"), with Continue and Cancel, posted to `POST
//!    /auth/app/confirm` with the session's CSRF token (masked anew on every
//!    page) and a one-time value of this hand-over. Continue hands that
//!    sign-in over and keeps the browser signed in (an `https` callback), or
//!    signs the browser out and asks the person to sign in again (a callback
//!    that is not `https`). Cancel ends the hand-over and sends the browser
//!    to the callback with `error=access_denied` and the `state`. For an app
//!    whose callback is not `https`, a sign-in that comes back with the
//!    `attempt` gets that page too, in place of the code ("The Windows app
//!    ExampleOrders_8wekyb3d8bbwe wants to sign in as Simran Kaur"): the
//!    code leaves only when the person continues, and it is that sign-in
//!    which is handed over, no other the browser has by then. The `attempt`
//!    and the confirmation's value each work once, also for requests sent at
//!    the same time: each is held in the cache the codes are kept in, and
//!    the first request that takes it from there uses it up.
//! 3. The app posts `{platform, app, code, code_verifier}` to `POST
//!    {api}/auth/app/exchange` from its own HTTP stack. The code works once,
//!    for that app, with the verifier whose SHA-256 the session was started
//!    with, and the sign-in policies are asked about it
//!    (`SignInMethod::AppHandOver`). The answer signs the app's own session
//!    cookie in as a new session of the same person, so the app and the
//!    browser end separately. That session keeps the time the person last
//!    authenticated in the browser session it came from. How long it lasts
//!    depends on how the hand-over finished:
//!    - after a sign-in made for this hand-over (it came back with the
//!      `attempt`), it lasts as a "remember me" sign-in made at that time
//!      would: the person has just signed in for the app;
//!    - after a browser that was already signed in was confirmed on the
//!      framework's page, it ends when that browser session was due to end
//!      as the hand-over finished, and never later than a "remember me"
//!      sign-in made at that time would. Signing the browser out afterwards
//!      does not end the app's session.
//!
//!    The code is worth only what its browser session still is: a confirmed
//!    hand-over's code is refused once that browser session has ended
//!    (signed out, ended from another device, revoked by a password change,
//!    a reset, signing out everywhere or offboarding). A code from a sign-in
//!    made for the hand-over is refused once that sign-in has ended, unless
//!    the hand-over itself signed the browser out after issuing the code.
//!
//! The code reaches only the app the operating system opens for the
//! callback (a verified universal link or App Link), and only the app that
//! made the verifier can use it: a code planted through a link (another
//! person's, or the attacker's own) fails at the exchange. Only a sign-in
//! made for this hand-over (it came back with the `attempt`), or one the
//! person confirmed on the framework's page, is handed over: a sign-in made
//! for anything else while the hand-over waited (the person signing in to
//! the site in another tab, or a sign-in another site posted) is refused,
//! and the hand-over ends. Opening the start address, which any site can
//! do, signs nobody out and hands nothing over. An app whose callback is a
//! custom scheme (`custom_scheme`, which another app could claim) never
//! gets a sign-in the browser already had: the person signs in again. Nor
//! does it get a sign-in without the person's word. Whoever holds the
//! scheme can open the start address itself, with a challenge of its own,
//! and the proof of possession then proves nothing: the code would go to it
//! with the verifier it made. So the page names the app and the system the
//! settings list it under, says "Continue only if you opened it yourself",
//! and the code is issued on Continue alone. A Windows app is always such
//! an app: its callback is a link on its own scheme, which any program on
//! the machine can register, and nothing proves to the site which program
//! asked. The answers of the finish and the confirmation are `no-store` and
//! `no-referrer`.
//!
//! Only the browser's own navigation finishes a hand-over. The finish
//! answers with a redirect to the app's callback, which reaches the app only
//! when the browser itself follows it; a script's `fetch` would take the
//! one-time `attempt` and the code with it, and the app would never hear of
//! either. So a request for the finish address that says it is anything else
//! (navree-ui's runtime or a native client asking, a fetch by a script, a
//! page loaded ahead of a click, a frame) changes nothing and is answered
//! `403` with a small page that is none of navree-ui's. The runtime shows
//! only a page that says it is the app's and loads any other for real, at
//! the address the answer came from: a fetch of its own that reaches the
//! finish address ends as the browser's own navigation to it. The sign-in
//! pages (`AuthPages`) do not let it come to that: they tell the runtime to
//! leave for the finish address, which it does without a fetch.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use http::header::{CACHE_CONTROL, REFERRER_POLICY, X_FRAME_OPTIONS};
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use navree_contracts::AuditKind;
use navree_crypto::{constant_time_eq, random_token, sha256_hex};
use navree_http::response::{Html, Redirect};
use navree_http::{
    ApiMeta, AppError, Controller, Form, IntoResponse, Json, Query, Response, RouteDef, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_openapi::ToSchema;
use navree_session::Session;
use navree_validation::{Valid, Validate};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::apps::HandOverApp;
use crate::controller::api;
use crate::mail::escape_html;
use crate::passkeys::base64_url;
use crate::passkeys::webauthn::sha256;
use crate::service::entry;
use crate::{
    AppsOptions, AuthError, AuthOptions, AuthService, ClientMeta, SessionUser, SignInAttempt,
    SignInMethod, User, UserResource,
};

/// Where an app starts the hand-over (outside the API prefix: it is opened
/// in the system's sign-in session).
pub const APP_SIGN_IN_PATH: &str = "/auth/app/sign-in";

/// Where the browser goes once it is signed in, to be sent on to the app.
pub const APP_SIGN_IN_FINISH_PATH: &str = "/auth/app/finish";

/// Where the confirmation page of a browser that was already signed in
/// posts the person's answer.
pub(crate) const APP_SIGN_IN_CONFIRM_PATH: &str = "/auth/app/confirm";

/// How long a hand-over code can be exchanged, in seconds.
pub const APP_CODE_TTL_SECS: u64 = 60;

/// How long a started hand-over waits for the person to sign in, in seconds.
const PENDING_TTL_SECS: i64 = 15 * 60;

/// The session key of a started hand-over.
const PENDING_KEY: &str = "_auth_app";

/// The longest `state` an app may pass, in bytes.
const MAX_STATE_LEN: usize = 512;

/// The request headers of navree-ui's browser runtime and of a native
/// client: each asks with its own HTTP stack, never as the browser's
/// navigation.
const ASKED_BY_CODE: [&str; 2] = ["x-navree-ui", "x-navree-native"];

/// The request headers of a page loaded ahead of a click, or for a preview,
/// which the person may never open: `Sec-Purpose` of today's browsers, and
/// the three older ones (`Purpose`, `X-Purpose`, `X-Moz`).
const AHEAD_OF_A_CLICK: [&str; 4] = ["sec-purpose", "purpose", "x-purpose", "x-moz"];

/// Whether a request with these `headers` is the browser's own navigation to
/// a page of its own: not navree-ui's runtime or a native client asking, not
/// a page loaded ahead of a click, and, where the browser says how it asks
/// (`Sec-Fetch-Mode`, `Sec-Fetch-Dest`), a navigation to a document, so not a
/// script's fetch and not a frame. A browser that says nothing of it is
/// taken at its word.
fn opened_by_the_browser(headers: &HeaderMap) -> bool {
    let says = |name: &str, wanted: &str| {
        headers
            .get(name)
            .is_none_or(|value| value.as_bytes().eq_ignore_ascii_case(wanted.as_bytes()))
    };
    !ASKED_BY_CODE
        .iter()
        .chain(&AHEAD_OF_A_CLICK)
        .any(|name| headers.contains_key(*name))
        && says("sec-fetch-mode", "navigate")
        && says("sec-fetch-dest", "document")
}

/// The answer to a request for the finish address that was not the browser's
/// own navigation: nothing of the hand-over is read or changed. It is `403`
/// with a page of the framework's own, not one of navree-ui's: the runtime
/// of navree-ui shows no such page, and sends the browser to the address the
/// answer came from instead, whether it asked for that address itself or a
/// post or a link of its own was sent on there. Never cached, never sent on
/// as a referrer, never framed.
fn not_a_navigation() -> Response {
    let mut response = Html(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"referrer\" content=\"no-referrer\">\n\
         <title>Opened by the browser only</title>\n</head>\n\
         <body>\n<p>This address is only opened by the browser itself.</p>\n</body>\n</html>\n",
    )
    .into_response();
    *response.status_mut() = StatusCode::FORBIDDEN;
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
}

/// What an app asks for at [`APP_SIGN_IN_PATH`].
#[derive(Clone, Default, Deserialize)]
pub(crate) struct StartRequest {
    #[serde(default)]
    pub(crate) platform: Option<String>,
    #[serde(default)]
    pub(crate) app: Option<String>,
    #[serde(default)]
    pub(crate) code_challenge: Option<String>,
    #[serde(default)]
    pub(crate) code_challenge_method: Option<String>,
    #[serde(default)]
    pub(crate) state: Option<String>,
    #[serde(default)]
    pub(crate) redirect_uri: Option<String>,
}

/// A started hand-over, kept in the sign-in session's session.
#[derive(Clone, Serialize, Deserialize)]
struct Pending {
    platform: String,
    app: String,
    callback: String,
    challenge: String,
    state: Option<String>,
    /// Whether the browser was signed in before the hand-over began.
    was_signed_in: bool,
    /// When it began (Unix seconds).
    started_at: i64,
    /// The random value the sign-in page comes back with (`?attempt=` on
    /// the finish address). Only a sign-in that came back with it is handed
    /// over without asking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attempt: Option<String>,
    /// While the person has not answered the confirmation page: the
    /// one-time value its form posts back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confirm: Option<String>,
    /// While the page asks after a sign-in made for this hand-over (an app
    /// whose callback is not `https`): the browser's server-side session of
    /// that sign-in. Continue hands that sign-in over, and no other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asked_after: Option<Uuid>,
}

/// The query of [`APP_SIGN_IN_FINISH_PATH`]: the hand-over's `attempt`, when
/// the sign-in page sent the browser back.
#[derive(Clone, Default, Deserialize)]
pub(crate) struct FinishRequest {
    #[serde(default)]
    pub(crate) attempt: Option<String>,
}

/// The form of the confirmation page (`POST /auth/app/confirm`): the
/// hand-over's one-time value and the button pressed (`continue` or
/// `cancel`). The session layer has taken out and checked `_csrf`.
#[derive(Clone, Default, Deserialize)]
pub(crate) struct ConfirmRequest {
    #[serde(default)]
    pub(crate) confirm: Option<String>,
    #[serde(default)]
    pub(crate) decision: Option<String>,
}

/// An issued code, kept in the cache under its SHA-256.
#[derive(Serialize, Deserialize)]
struct Code {
    user_id: Uuid,
    platform: String,
    app: String,
    challenge: String,
    expires_at: i64,
    /// When the user last authenticated in the browser session the code
    /// came from (Unix seconds): the app's session keeps it.
    #[serde(default)]
    auth_time: Option<i64>,
    /// How the hand-over finished.
    #[serde(default)]
    finished: Option<Finished>,
    /// The browser's server-side session the code came from.
    #[serde(default)]
    session_id: Option<Uuid>,
    /// For a confirmed hand-over: when that browser session ends (Unix
    /// seconds). The app's session ends no later.
    #[serde(default)]
    session_ends_at: Option<i64>,
    /// For a sign-in made for the hand-over: the hand-over signed the
    /// browser out right after issuing the code, so that sign-in ended by
    /// the hand-over's own doing.
    #[serde(default)]
    browser_signed_out: bool,
}

/// How a hand-over finished: it decides how long the app's session lasts
/// and what the exchange asks of the browser session the code came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Finished {
    /// The person signed in for this hand-over: the sign-in came back with
    /// the hand-over's `attempt`.
    SignedIn,
    /// The browser was signed in before the hand-over began, and the person
    /// confirmed it on the framework's page.
    Confirmed,
}

/// The browser sign-in a code is issued from.
struct Source {
    /// The browser's signed-in user and its server-side session.
    user: SessionUser,
    /// When that server-side session ends (Unix seconds).
    ends_at: i64,
    /// How the hand-over finished.
    finished: Finished,
}

/// Where [`AuthService::finish_app_sign_in`] and
/// [`AuthService::confirm_app_sign_in`] send the browser.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Finish {
    /// Not signed in yet: to the sign-in page, which comes back here (with
    /// the hand-over's `attempt`, when it has one).
    SignIn(Option<String>),
    /// To the app's callback, with the code (or `error`) and the state.
    App(String),
    /// The page that asks the person first: a browser that was signed in
    /// when the hand-over began, and any sign-in that is about to leave for
    /// an app whose callback is not `https`.
    Confirm(ConfirmPage),
}

/// What the confirmation page says and posts back.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ConfirmPage {
    /// The app's id, as the settings write it: its bundle id, its package
    /// name, or the id of a Windows build.
    app: String,
    /// The system the settings list the app under (`Apple`, `Android`,
    /// `Windows`). The page names it with an app whose callback is not
    /// `https`: nothing proves which program asked, so the person is told
    /// what the settings say the id is.
    system: &'static str,
    /// The signed-in person's name.
    name: String,
    /// The signed-in person's email.
    email: String,
    /// What continuing does.
    asks: Asks,
    /// The hand-over's one-time value.
    token: String,
}

/// What the confirmation page asks the person to continue with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asks {
    /// The browser was signed in before the hand-over began, and the app's
    /// callback is `https`: that sign-in is handed over, and the browser
    /// stays signed in.
    HandOver,
    /// The browser was signed in before the hand-over began, and the app's
    /// callback is not `https`: the browser is signed out, and the person
    /// signs in again for the app.
    SignInAgain,
    /// The person signed in for this hand-over, and the app's callback is
    /// not `https`: that sign-in is handed over, and the browser stays
    /// signed in afterwards or not.
    AfterSignIn { stays_signed_in: bool },
}

/// The system an app of `platform` is for, as the confirmation page names
/// it.
fn system_of(platform: &str) -> &'static str {
    match platform {
        "apple" => "Apple",
        "android" => "Android",
        "windows" => "Windows",
        _ => "",
    }
}

fn refused(reason: impl Into<String>) -> AuthError {
    AuthError::HandOverRefused(reason.into())
}

fn code_key(code: &str) -> String {
    format!("navree-auth:app-code:{}", sha256_hex(code.as_bytes()))
}

/// The cache key that holds a one-time value of a hand-over (its `attempt`,
/// or the value of its confirmation page) until a request uses it.
fn once_key(value: &str) -> String {
    format!("navree-auth:app-once:{}", sha256_hex(value.as_bytes()))
}

/// A PKCE challenge: 43 base64url characters (a SHA-256).
fn is_challenge(challenge: &str) -> bool {
    challenge.len() == 43
        && challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A PKCE verifier (RFC 7636): 43 to 128 unreserved characters.
fn is_verifier(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

/// The S256 challenge of `verifier`.
pub(crate) fn s256(verifier: &str) -> String {
    base64_url(&sha256(verifier.as_bytes()))
}

/// Whether an app's callback is an `https` link (else a custom scheme,
/// which another app could claim): its scheme read as the configuration
/// check reads it, in any case (`HTTPS://` is `https`).
fn is_https(callback: &str) -> bool {
    crate::apps::is_https_url(callback)
}

/// The app's `callback` with `pair` (the code, or the error) and the app's
/// `state` added to its query.
fn callback_with(
    callback: &str,
    pair: (&str, &str),
    state: Option<&str>,
) -> Result<String, AuthError> {
    let mut url =
        url::Url::parse(callback).map_err(|_| refused("the app's callback is not a URL"))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair(pair.0, pair.1);
        if let Some(state) = state {
            query.append_pair("state", state);
        }
    }
    Ok(url.into())
}

impl AuthService {
    /// Starts handing a sign-in over to an app (`GET /auth/app/sign-in`):
    /// the app is one of `apps` with a callback, the challenge is S256, a
    /// `redirect_uri`, when given, is that callback. The request is kept in
    /// the browser's session with a one-time `attempt` (and, for a browser
    /// that is signed in, the one-time value of the confirmation page); the
    /// browser goes on to [`APP_SIGN_IN_FINISH_PATH`]. Nothing else changes:
    /// any site can open this address, so it never signs the browser out.
    /// Refusals are [`AuthError::HandOverRefused`] (400 `invalid_hand_over`).
    pub(crate) async fn start_app_sign_in(
        &self,
        session: &Session,
        apps: &AppsOptions,
        request: &StartRequest,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let result = self.check_start(apps, request);
        let app = match result {
            Ok(app) => app,
            Err(error) => {
                self.hand_over_refused(None, &error, meta).await;
                return Err(error);
            }
        };
        // A browser that is already signed in is neither signed out nor
        // handed over from here: the finish asks the person first. Any
        // other sign-in must come back with `attempt`.
        let was_signed_in = self.live_user(session).await?.is_some();
        let attempt = random_token(32);
        let confirm = was_signed_in.then(|| random_token(32));
        self.hold_once(&attempt).await?;
        if let Some(confirm) = &confirm {
            self.hold_once(confirm).await?;
        }
        self.keep_pending(
            session,
            &Pending {
                platform: app.platform.to_owned(),
                app: app.id,
                callback: app.callback,
                challenge: request.code_challenge.clone().unwrap_or_default(),
                state: request.state.clone(),
                was_signed_in,
                started_at: self.clock.now().as_secs(),
                attempt: Some(attempt),
                confirm,
                asked_after: None,
            },
        )
    }

    /// Keeps `pending` in the browser's session.
    fn keep_pending(&self, session: &Session, pending: &Pending) -> Result<(), AuthError> {
        session
            .insert(PENDING_KEY, pending)
            .map_err(|error| AuthError::Session(error.to_string()))
    }

    /// Holds the one-time `value` of a hand-over in the cache for as long
    /// as the hand-over may wait.
    async fn hold_once(&self, value: &str) -> Result<(), AuthError> {
        let ttl = u64::try_from(PENDING_TTL_SECS).unwrap_or(15 * 60) + 5;
        self.cache
            .put(&once_key(value), &true, Duration::from_secs(ttl))
            .await?;
        Ok(())
    }

    /// Uses the one-time `value` of a hand-over up: `false` when another
    /// request used it first (two at the same time find one `true`
    /// between them), or it is no longer held.
    async fn use_once(&self, value: &str) -> Result<bool, AuthError> {
        Ok(self.cache.forget(&once_key(value)).await?)
    }

    /// The browser's signed-in user and when its server-side session ends
    /// (Unix seconds), when its sign-in is live.
    async fn live_user(&self, session: &Session) -> Result<Option<(SessionUser, i64)>, AuthError> {
        match self.session_user(session) {
            Some(user) => Ok(self
                .live_session(user.user_id, user.session_id)
                .await?
                .map(|ends_at| (user, ends_at))),
            None => Ok(None),
        }
    }

    /// The hand-over waiting in the browser's session, unless there is none
    /// or it waited too long (it then ends): refused, and audited.
    async fn pending(&self, session: &Session, meta: &ClientMeta) -> Result<Pending, AuthError> {
        let Some(pending) = session.get::<Pending>(PENDING_KEY) else {
            let error = refused("no app sign-in was started in this browser");
            self.hand_over_refused(None, &error, meta).await;
            return Err(error);
        };
        if self.clock.now().as_secs() - pending.started_at > PENDING_TTL_SECS {
            return Err(self
                .end_hand_over(
                    session,
                    &pending.app,
                    "the app sign-in waited too long",
                    meta,
                )
                .await);
        }
        Ok(pending)
    }

    /// Ends the waiting hand-over of `app`, refused for `reason` (audited).
    async fn end_hand_over(
        &self,
        session: &Session,
        app: &str,
        reason: &str,
        meta: &ClientMeta,
    ) -> AuthError {
        session.remove(PENDING_KEY);
        let error = refused(reason);
        self.hand_over_refused(Some(app), &error, meta).await;
        error
    }

    fn check_start(
        &self,
        apps: &AppsOptions,
        request: &StartRequest,
    ) -> Result<HandOverApp, AuthError> {
        let (Some(platform), Some(id)) = (request.platform.as_deref(), request.app.as_deref())
        else {
            return Err(refused("no platform or app"));
        };
        let app = apps
            .hand_over_app(platform, id)
            .ok_or_else(|| refused("not an app of auth.apps with a callback"))?;
        if request
            .redirect_uri
            .as_deref()
            .is_some_and(|uri| uri != app.callback)
        {
            return Err(refused("redirect_uri is not the app's callback"));
        }
        match request.code_challenge.as_deref() {
            None => return Err(refused("no code challenge")),
            Some(challenge) if !is_challenge(challenge) => {
                return Err(refused("malformed code challenge"));
            }
            Some(_) => {}
        }
        if request.code_challenge_method.as_deref() != Some("S256") {
            return Err(refused("the code challenge method must be S256"));
        }
        if request.state.as_deref().is_some_and(|state| {
            state.len() > MAX_STATE_LEN || state.chars().any(|c| c.is_control())
        }) {
            return Err(refused("malformed state"));
        }
        Ok(app)
    }

    /// Finishes a started hand-over (`GET /auth/app/finish`): a browser not
    /// signed in yet goes to the sign-in page ([`Finish::SignIn`]); a sign-in
    /// that came back with the hand-over's `attempt` gets a one-time code
    /// for an app with an `https` callback and goes there ([`Finish::App`]),
    /// once (the attempt is used up, also by two requests sent at the same
    /// time). For an app whose callback is not `https` the person is asked
    /// first, on the page that names the app ([`Finish::Confirm`]): any
    /// program can claim a scheme and start a hand-over itself, with a
    /// challenge of its own, so the code leaves only when the person says
    /// they opened the app. A browser that was signed in when the hand-over
    /// began is asked first too. Each page is answered by
    /// [`confirm_app_sign_in`](AuthService::confirm_app_sign_in). Any other
    /// sign-in (made for something else while the hand-over waited) ends
    /// the hand-over, refused. An impersonation is never handed over.
    pub(crate) async fn finish_app_sign_in(
        &self,
        session: &Session,
        attempt: Option<&str>,
        keep_browser_signed_in: bool,
        meta: &ClientMeta,
    ) -> Result<Finish, AuthError> {
        let pending = self.pending(session, meta).await?;
        let Some((signed_in, ends_at)) = self.live_user(session).await? else {
            return Ok(Finish::SignIn(pending.attempt.clone()));
        };
        if signed_in.impersonator.is_some() {
            return Err(self
                .end_hand_over(
                    session,
                    &pending.app,
                    "an impersonation is not handed over",
                    meta,
                )
                .await);
        }
        let made_for_this = match (pending.attempt.as_deref(), attempt) {
            (Some(expected), Some(given)) => {
                constant_time_eq(given.as_bytes(), expected.as_bytes())
            }
            _ => false,
        };
        if made_for_this {
            // The attempt works once, also for two requests at the same
            // time: the first to take it from the cache uses it up.
            let used = match pending.attempt.as_deref() {
                Some(attempt) => self.use_once(attempt).await?,
                None => false,
            };
            if !used {
                return Err(self
                    .end_hand_over(
                        session,
                        &pending.app,
                        "the app sign-in's attempt was already used",
                        meta,
                    )
                    .await);
            }
            if !is_https(&pending.callback) {
                // Any program can claim a scheme, open the start address
                // itself and hold the verifier: the code would go to it. The
                // person is told which app asks, and the code leaves only
                // when they continue.
                let mut pending = pending;
                let token = random_token(32);
                self.hold_once(&token).await?;
                pending.attempt = None;
                pending.confirm = Some(token);
                pending.asked_after = Some(signed_in.session_id);
                self.keep_pending(session, &pending)?;
                return self
                    .ask(session, &pending, &signed_in, keep_browser_signed_in, meta)
                    .await;
            }
            let source = Source {
                user: signed_in,
                ends_at,
                finished: Finished::SignedIn,
            };
            return self
                .issue_code(session, pending, source, keep_browser_signed_in, meta)
                .await;
        }
        // The person says first: the browser was signed in before the
        // hand-over began, or the page of a sign-in made for it is opened
        // again before it is answered.
        if pending.confirm.is_some() {
            return self
                .ask(session, &pending, &signed_in, keep_browser_signed_in, meta)
                .await;
        }
        // A sign-in that did not come back from the hand-over's own sign-in
        // page, nor was confirmed, is never handed over.
        Err(self
            .end_hand_over(
                session,
                &pending.app,
                "the sign-in was not made for this app sign-in",
                meta,
            )
            .await)
    }

    /// The page that asks the person before `pending` goes on, for the
    /// browser sign-in `signed_in`. What continuing does follows from the
    /// app's callback and from whether the page comes after a sign-in made
    /// for the hand-over. A page that came after such a sign-in is shown to
    /// that sign-in alone: for any other the hand-over ends, refused.
    async fn ask(
        &self,
        session: &Session,
        pending: &Pending,
        signed_in: &SessionUser,
        keep_browser_signed_in: bool,
        meta: &ClientMeta,
    ) -> Result<Finish, AuthError> {
        let not_for_this = "the sign-in was not made for this app sign-in";
        let Some(token) = pending.confirm.clone() else {
            return Err(self
                .end_hand_over(session, &pending.app, not_for_this, meta)
                .await);
        };
        if pending
            .asked_after
            .is_some_and(|sign_in| sign_in != signed_in.session_id)
        {
            return Err(self
                .end_hand_over(session, &pending.app, not_for_this, meta)
                .await);
        }
        let Some(user) = self.find_user(signed_in.user_id).await? else {
            return Err(self
                .end_hand_over(session, &pending.app, "the user no longer exists", meta)
                .await);
        };
        let asks = match pending.asked_after {
            Some(_) => Asks::AfterSignIn {
                stays_signed_in: pending.was_signed_in || keep_browser_signed_in,
            },
            None if is_https(&pending.callback) => Asks::HandOver,
            None => Asks::SignInAgain,
        };
        Ok(Finish::Confirm(ConfirmPage {
            app: pending.app.clone(),
            system: system_of(&pending.platform),
            name: user.name,
            email: user.email,
            asks,
            token,
        }))
    }

    /// Answers the confirmation page (`POST /auth/app/confirm`) with the
    /// hand-over's one-time value `token`. Continue (`proceed`) on the page
    /// of a browser that was signed in before hands that sign-in over to an
    /// app with an `https` callback (the browser stays signed in), or signs
    /// the browser out and sends it to the sign-in page with the hand-over's
    /// `attempt` for an app whose callback is not `https` (and for a browser
    /// no longer signed in). Continue on the page that came after a sign-in
    /// made for the hand-over hands that sign-in over, and no other: when
    /// the browser is no longer signed in with it, the hand-over ends,
    /// refused. Cancel ends the hand-over and sends the browser to the
    /// callback with `error=access_denied` and the `state`; the browser
    /// stays as it is. A value that is not the waiting hand-over's, or one
    /// another request used already (two answers sent at the same time get
    /// one code between them), ends it, refused.
    pub(crate) async fn confirm_app_sign_in(
        &self,
        session: &Session,
        token: Option<&str>,
        proceed: bool,
        keep_browser_signed_in: bool,
        meta: &ClientMeta,
    ) -> Result<Finish, AuthError> {
        let mut pending = self.pending(session, meta).await?;
        let answered = match (pending.confirm.as_deref(), token) {
            (Some(expected), Some(given)) => {
                constant_time_eq(given.as_bytes(), expected.as_bytes())
            }
            _ => false,
        };
        if !answered {
            return Err(self
                .end_hand_over(
                    session,
                    &pending.app,
                    "the confirmation is not this app sign-in's",
                    meta,
                )
                .await);
        }
        // The value works once, also for two requests at the same time: the
        // first to take it from the cache uses it up.
        let used = match pending.confirm.as_deref() {
            Some(value) => self.use_once(value).await?,
            None => false,
        };
        if !used {
            return Err(self
                .end_hand_over(
                    session,
                    &pending.app,
                    "the confirmation was already used",
                    meta,
                )
                .await);
        }
        pending.confirm = None;
        let asked_after = pending.asked_after.take();
        if !proceed {
            session.remove(PENDING_KEY);
            tracing::info!(target: "navree::auth", app = %pending.app, "an app sign-in was cancelled");
            let callback = callback_with(
                &pending.callback,
                ("error", "access_denied"),
                pending.state.as_deref(),
            )?;
            return Ok(Finish::App(callback));
        }
        let signed_in = self.live_user(session).await?;
        if let Some(asked_after) = asked_after {
            // The page came after a sign-in made for this hand-over: that
            // sign-in is handed over, and no other the browser has by now.
            return match signed_in {
                Some((user, ends_at)) if user.session_id == asked_after => {
                    let source = Source {
                        user,
                        ends_at,
                        finished: Finished::SignedIn,
                    };
                    self.issue_code(session, pending, source, keep_browser_signed_in, meta)
                        .await
                }
                _ => Err(self
                    .end_hand_over(
                        session,
                        &pending.app,
                        "the sign-in made for this app sign-in has ended",
                        meta,
                    )
                    .await),
            };
        }
        if let Some((user, ends_at)) = signed_in
            && is_https(&pending.callback)
        {
            let source = Source {
                user,
                ends_at,
                finished: Finished::Confirmed,
            };
            return self
                .issue_code(session, pending, source, keep_browser_signed_in, meta)
                .await;
        }
        // Another app can claim a custom scheme: such an app never gets the
        // sign-in the browser had. The person signs in again, for this
        // hand-over.
        if signed_in.is_some() {
            self.sign_out_session(session, meta).await?;
        }
        pending.was_signed_in = false;
        self.keep_pending(session, &pending)?;
        Ok(Finish::SignIn(pending.attempt))
    }

    /// Issues the one-time code of `pending` for the browser sign-in
    /// `source` and ends the hand-over: the browser goes to the app's
    /// callback, signed out first unless it was signed in before or
    /// `keep_browser_signed_in` says otherwise. The code records how the
    /// hand-over finished, the browser's server-side session and, for a
    /// confirmed hand-over, when that session ends.
    async fn issue_code(
        &self,
        session: &Session,
        pending: Pending,
        source: Source,
        keep_browser_signed_in: bool,
        meta: &ClientMeta,
    ) -> Result<Finish, AuthError> {
        let signed_in = source.user;
        if signed_in.impersonator.is_some() {
            return Err(self
                .end_hand_over(
                    session,
                    &pending.app,
                    "an impersonation is not handed over",
                    meta,
                )
                .await);
        }
        let signs_out = !pending.was_signed_in && !keep_browser_signed_in;
        let confirmed = source.finished == Finished::Confirmed;
        let now = self.clock.now();
        let code = random_token(43);
        self.cache
            .put(
                &code_key(&code),
                &Code {
                    user_id: signed_in.user_id,
                    platform: pending.platform.clone(),
                    app: pending.app.clone(),
                    challenge: pending.challenge.clone(),
                    expires_at: now.as_secs() + i64::try_from(APP_CODE_TTL_SECS).unwrap_or(60),
                    auth_time: Some(signed_in.auth_time),
                    finished: Some(source.finished),
                    session_id: Some(signed_in.session_id),
                    session_ends_at: confirmed.then_some(source.ends_at),
                    browser_signed_out: !confirmed && signs_out,
                },
                Duration::from_secs(APP_CODE_TTL_SECS + 5),
            )
            .await?;
        session.remove(PENDING_KEY);
        if signs_out {
            self.sign_out_session(session, meta).await?;
        }
        let callback = callback_with(
            &pending.callback,
            ("code", code.as_str()),
            pending.state.as_deref(),
        )?;
        Ok(Finish::App(callback))
    }

    /// Exchanges a hand-over `code` for a session of the app's own (`POST
    /// /auth/app/exchange`): the code works once (the first attempt uses it
    /// up, right or wrong), within [`APP_CODE_TTL_SECS`] seconds, for the
    /// app it was issued to, and with the verifier of the challenge the
    /// hand-over was started with, while the browser session it came from
    /// still allows it ([`code_source`](AuthService::code_source)). The
    /// [`SignInPolicy`](crate::SignInPolicy)s are asked about
    /// [`SignInMethod::AppHandOver`] as for any sign-in. The app's session is
    /// signed in as a new session of the same person, kept like "remember
    /// me", that keeps the time the person last authenticated in the browser
    /// session the code came from. After a sign-in made for the hand-over it
    /// ends when a "remember me" sign-in made then would; after a confirmed
    /// hand-over it ends when the browser session was due to end as the
    /// hand-over finished, and never later than that
    /// ([`start_handed_over_session`](AuthService::start_handed_over_session)).
    /// Audited as `auth.login` with `metadata.method = "app_hand_over"`;
    /// refusals as `auth.app_sign_in_refused` with the reason.
    pub(crate) async fn exchange_app_code(
        &self,
        session: &Session,
        platform: &str,
        app: &str,
        code: &str,
        verifier: &str,
        meta: &ClientMeta,
    ) -> Result<User, AuthError> {
        let result = async {
            let (user, issued) = self.check_code(platform, app, code, verifier).await?;
            let (auth_time, ends_by) = self.code_source(&issued).await?;
            let method = SignInMethod::AppHandOver {
                platform: platform.to_owned(),
                app: app.to_owned(),
            };
            self.check_sign_in(&SignInAttempt::of(&user, method), meta)
                .await?;
            self.start_handed_over_session(session, &user, auth_time, ends_by, meta)
                .await?;
            Ok::<_, AuthError>(user)
        }
        .await;
        match result {
            Ok(user) => {
                self.audit(
                    entry(AuditKind::Auth, "auth.login", meta)
                        .actor(user.id)
                        .metadata(json!({
                            "via": "session",
                            "method": "app_hand_over",
                            "platform": platform,
                            "app": app,
                        })),
                )
                .await;
                Ok(user)
            }
            Err(error) => {
                self.hand_over_refused(Some(app), &error, meta).await;
                Err(error)
            }
        }
    }

    /// The user of a valid `code`, and the code as it was issued.
    async fn check_code(
        &self,
        platform: &str,
        app: &str,
        code: &str,
        verifier: &str,
    ) -> Result<(User, Code), AuthError> {
        if code.is_empty() || code.len() > 128 {
            return Err(refused("malformed code"));
        }
        let key = code_key(code);
        let issued: Code = self
            .cache
            .get(&key)
            .await?
            .ok_or_else(|| refused("unknown or expired code"))?;
        if !self.cache.forget(&key).await? {
            return Err(refused("code already used"));
        }
        if self.clock.now().as_secs() >= issued.expires_at {
            return Err(refused("code expired"));
        }
        if issued.platform != platform || issued.app != app {
            return Err(refused("code of another app"));
        }
        if !is_verifier(verifier)
            || !constant_time_eq(s256(verifier).as_bytes(), issued.challenge.as_bytes())
        {
            return Err(refused("wrong code verifier"));
        }
        let user = self
            .find_user(issued.user_id)
            .await?
            .ok_or_else(|| refused("the user no longer exists"))?;
        Ok((user, issued))
    }

    /// What the browser sign-in an `issued` code came from still allows:
    /// the time the app's session keeps (when the person last authenticated
    /// there), and, for a confirmed hand-over, the end of that browser
    /// session, which the app's session never outlasts. A confirmed
    /// hand-over's browser session must still be live. A sign-in made for
    /// the hand-over must be too, unless the hand-over itself signed the
    /// browser out after issuing the code (nothing else records when a
    /// person's credentials were revoked: signing out everywhere, a password
    /// change, a reset and offboarding each revoke the live sessions only).
    async fn code_source(&self, issued: &Code) -> Result<(i64, Option<i64>), AuthError> {
        let (Some(auth_time), Some(finished), Some(session_id)) =
            (issued.auth_time, issued.finished, issued.session_id)
        else {
            return Err(refused("the code names no sign-in"));
        };
        let live = self
            .live_session(issued.user_id, session_id)
            .await?
            .is_some();
        match finished {
            Finished::SignedIn => {
                if !live && !issued.browser_signed_out {
                    return Err(refused("the sign-in the code came from has ended"));
                }
                Ok((auth_time, None))
            }
            Finished::Confirmed => {
                let Some(ends_at) = issued.session_ends_at else {
                    return Err(refused("the code names no end of its sign-in"));
                };
                if !live {
                    return Err(refused("the browser session the code came from has ended"));
                }
                Ok((auth_time, Some(ends_at)))
            }
        }
    }

    async fn hand_over_refused(&self, app: Option<&str>, error: &AuthError, meta: &ClientMeta) {
        let AuthError::HandOverRefused(reason) = error else {
            return;
        };
        tracing::warn!(
            target: "navree::security",
            app = app.unwrap_or("-"),
            %reason,
            "an app sign-in hand-over was refused"
        );
        self.audit(
            entry(AuditKind::Security, "auth.app_sign_in_refused", meta)
                .metadata(json!({ "app": app, "reason": reason })),
        )
        .await;
    }
}

/// `POST /auth/app/exchange`: the code the callback carried and the
/// verifier the app made. `Debug` shows neither.
#[derive(Deserialize, Validate, ToSchema)]
#[non_exhaustive]
pub struct AppExchangeInput {
    /// `apple`, `android` or `windows`.
    #[validate(length(min = 1, max = 16))]
    pub platform: String,
    /// The app's id, as it started the hand-over: its bundle id, its package
    /// name, or the id a Windows build names itself by.
    #[validate(length(min = 1, max = 255))]
    pub app: String,
    /// The one-time code from the callback.
    #[validate(length(min = 1, max = 128))]
    pub code: String,
    /// The PKCE verifier whose S256 challenge started the hand-over.
    #[validate(length(min = 43, max = 128))]
    pub code_verifier: String,
}

impl fmt::Debug for AppExchangeInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppExchangeInput")
            .field("platform", &self.platform)
            .field("app", &self.app)
            .field("code", &"[redacted]")
            .field("code_verifier", &"[redacted]")
            .finish()
    }
}

/// The hand-over routes, mounted when navree-session's layer is in the app
/// and an app of `auth.apps` has a callback.
pub(crate) struct AppSignInController {
    service: Arc<AuthService>,
    options: Arc<AuthOptions>,
    apps: Arc<AppsOptions>,
    sessions: bool,
    /// The sign-in page (`ui.sign_in`, else `/sign-in`).
    sign_in: String,
    /// `http.api_prefix`, which the exchange route is under.
    api_prefix: String,
}

impl Injectable for AppSignInController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<AuthService>(),
            ProviderKey::of::<AuthOptions>(),
            ProviderKey::of::<AppsOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let app = r.app();
            let sessions = app
                .try_get::<navree_session::SessionOptions>()
                .await
                .is_some();
            let sign_in = app
                .try_get::<navree_config::Config>()
                .await
                .and_then(|config| config.get_string("ui.sign_in"))
                .filter(|path| {
                    path.starts_with('/')
                        && !path.starts_with("//")
                        && !path.contains(['\\', '?', '#'])
                })
                .unwrap_or_else(|| "/sign-in".to_owned());
            let api_prefix = match app.try_get::<navree_http::HttpOptions>().await {
                Some(http) => http.api_prefix.clone(),
                None => String::new(),
            };
            Ok(AppSignInController {
                api_prefix,
                service: r.get::<AuthService>().await?,
                options: r.get::<AuthOptions>().await?,
                apps: r.get::<AppsOptions>().await?,
                sessions,
                sign_in,
            })
        })
    }
}

impl Controller for AppSignInController {
    fn prefix(&self) -> &'static str {
        "!/"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.sessions || !self.apps.has_hand_over() {
            return Vec::new();
        }
        let mut routes = Vec::new();

        let service = self.service.clone();
        let apps = self.apps.clone();
        let start =
            move |session: Session, meta: ClientMeta, Query(request): Query<StartRequest>| {
                let service = service.clone();
                let apps = apps.clone();
                async move {
                    service
                        .start_app_sign_in(&session, &apps, &request, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    Ok::<_, AppError>(Redirect::to(APP_SIGN_IN_FINISH_PATH).into_response())
                }
            };
        routes.push(
            RouteDef::on(Method::GET, format!("!{APP_SIGN_IN_PATH}"), start)
                .public()
                // Any site can send a browser here, and each start is kept until it is
                // finished or runs out.
                .throttle(STARTS_PER_MINUTE, Duration::from_secs(60))
                .name("auth.app.sign_in")
                .api(ApiMeta {
                    description: Some(
                        "Opened by a native app in the system's sign-in session, with \
                         `platform`, `app`, `code_challenge`, `code_challenge_method=S256` and \
                         `state`. Goes on to the sign-in page and, once the person is signed \
                         in, to the app's callback with a one-time code."
                            .to_owned(),
                    ),
                    ..api("Start handing a sign-in over to an app", &[303, 400, 429])
                }),
        );

        let service = self.service.clone();
        let keep = self.apps.keep_browser_signed_in;
        let sign_in = self.sign_in.clone();
        let finish = move |session: Session,
                           meta: ClientMeta,
                           headers: HeaderMap,
                           Query(request): Query<FinishRequest>| {
            let service = service.clone();
            let sign_in = sign_in.clone();
            async move {
                // Only the browser's own navigation finishes a hand-over: the
                // redirect to the app reaches the app only when the browser
                // follows it. Anything else changes nothing.
                if !opened_by_the_browser(&headers) {
                    return Ok::<_, AppError>(not_a_navigation());
                }
                let finish = service
                    .finish_app_sign_in(&session, request.attempt.as_deref(), keep, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(finish_response(finish, &sign_in, &session))
            }
        };
        routes.push(
            RouteDef::on(Method::GET, format!("!{APP_SIGN_IN_FINISH_PATH}"), finish)
                .public()
                .name("auth.app.finish")
                .api(ApiMeta {
                    description: Some(
                        "Sends a browser that is not signed in to the sign-in page, and a \
                         sign-in made for this hand-over on to the app's callback with a \
                         one-time code. A browser that was signed in when the hand-over began \
                         gets a page that asks the person first (200, HTML), and so does a \
                         sign-in made for an app whose callback is not https. Only the browser's \
                         own navigation is answered so: a fetch by a script, a page loaded \
                         ahead of a click or a frame changes nothing and gets 403."
                            .to_owned(),
                    ),
                    ..api(
                        "Send a signed-in browser on to the app with a one-time code",
                        &[200, 303, 400, 403],
                    )
                }),
        );

        let service = self.service.clone();
        let sign_in = self.sign_in.clone();
        let confirm = move |session: Session,
                            meta: ClientMeta,
                            Form(input): Form<ConfirmRequest>| {
            let service = service.clone();
            let sign_in = sign_in.clone();
            async move {
                let proceed = input.decision.as_deref() == Some("continue");
                let finish = service
                    .confirm_app_sign_in(&session, input.confirm.as_deref(), proceed, keep, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(finish_response(finish, &sign_in, &session))
            }
        };
        routes.push(
            RouteDef::on(
                Method::POST,
                format!("!{APP_SIGN_IN_CONFIRM_PATH}"),
                confirm,
            )
            .public()
            .name("auth.app.confirm")
            .api(ApiMeta {
                description: Some(
                    "The answer of the page that asks a signed-in browser before its \
                         sign-in goes to an app: a form with the session's CSRF token \
                         (`_csrf`), the hand-over's one-time `confirm` value and `decision` \
                         (`continue` or `cancel`)."
                        .to_owned(),
                ),
                ..api(
                    "Answer the question of an app sign-in hand-over",
                    &[303, 400, 403],
                )
            }),
        );

        let service = self.service.clone();
        let apps = self.apps.clone();
        let exchange_path = format!(
            "!/{}/{}/app/exchange",
            self.api_prefix.trim_matches('/'),
            self.options.routes_prefix.trim_matches('/')
        )
        .replace("//", "/");
        let exchange =
            move |session: Session,
                  meta: ClientMeta,
                  Valid(Json(input)): Valid<Json<AppExchangeInput>>| {
                let service = service.clone();
                let apps = apps.clone();
                async move {
                    // The app is named as the settings write it, as it was
                    // when the hand-over started: a Windows build may write
                    // its package family name in another case. An id that is
                    // no app's stays as it came, and its code is refused.
                    let app = apps
                        .hand_over_app(&input.platform, &input.app)
                        .map_or_else(|| input.app.clone(), |app| app.id);
                    let user = service
                        .exchange_app_code(
                            &session,
                            &input.platform,
                            &app,
                            &input.code,
                            &input.code_verifier,
                            &meta,
                        )
                        .await
                        .map_err(AuthError::into_app_error)?;
                    let user = service
                        .present(&user)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    Ok::<_, AppError>(Json(user).into_response())
                }
            };
        routes.push(
            RouteDef::on(Method::POST, exchange_path, exchange)
                .public()
                .throttle(EXCHANGES_PER_MINUTE, Duration::from_secs(60))
                .name("auth.app.exchange")
                .api(ApiMeta {
                    description: Some(
                        "Exchanges the one-time code of an app sign-in hand-over and the app's \
                         PKCE verifier for a session of the app's own: the session cookie of \
                         this answer is signed in. The request carries the CSRF token of the \
                         session as `X-XSRF-TOKEN`."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<AppExchangeInput>),
                    response: Some(schema_fn::<UserResource>),
                    ..api("Sign an app in with a hand-over code", &[400, 403, 429])
                }),
        );

        // A browser the system did not hand to the app lands on the
        // callback with the code in its address: an empty page that keeps
        // nothing and sends nothing on, when the app turns it on
        // (`auth.apps.callback_page`, off by default).
        let own = [
            APP_SIGN_IN_PATH,
            APP_SIGN_IN_FINISH_PATH,
            crate::APPLE_ASSOCIATION_PATH,
            crate::ANDROID_ASSOCIATION_PATH,
            self.sign_in.as_str(),
        ];
        for path in self.apps.callback_page_paths() {
            if own.contains(&path.as_str()) {
                continue;
            }
            routes.push(
                RouteDef::on(Method::GET, format!("!{path}"), || async {
                    callback_page()
                })
                .public(),
            );
        }
        routes
    }
}

/// The page of an app's `https` callback, for a browser the system did not
/// hand to the app: empty, never cached, never sent on as a referrer.
fn callback_page() -> Response {
    let mut response = Html(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"referrer\" content=\"no-referrer\">\n<title></title>\n</head>\n\
         <body></body>\n</html>\n",
    )
    .into_response();
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
}

/// Exchanges one client may attempt per minute.
const EXCHANGES_PER_MINUTE: u32 = 30;

/// Hand-overs one client may start per minute.
const STARTS_PER_MINUTE: u32 = 30;

/// The answer of the finish and of the confirmation: a redirect to the
/// sign-in page or to the app's callback, or the confirmation page. The
/// code can be in the address and the page holds a one-time value: never
/// cached, never sent on as a referrer, never framed.
fn finish_response(finish: Finish, sign_in: &str, session: &Session) -> Response {
    let mut response = match finish {
        Finish::SignIn(attempt) => {
            Redirect::to(&sign_in_href(sign_in, attempt.as_deref())).into_response()
        }
        Finish::App(callback) => Redirect::to(&callback).into_response(),
        // Masked anew on every page, as every page of the framework writes
        // it: the page never carries the session's token as it is.
        Finish::Confirm(page) => {
            let csrf = navree_http::csrf::page_token(&session.csrf_token());
            Html(confirm_page(&page, &csrf)).into_response()
        }
    };
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
}

/// The confirmation page: plain HTML of the framework's own, so it works in
/// an app without a page toolkit. Its form posts `csrf` (the session's CSRF
/// token as a page writes it, masked), the hand-over's one-time value and
/// the button pressed to
/// `POST /auth/app/confirm`.
fn confirm_page(page: &ConfirmPage, csrf: &str) -> String {
    let app = escape_html(&page.app);
    let person = if page.name.trim().is_empty() {
        escape_html(&page.email)
    } else {
        format!("{} ({})", escape_html(&page.name), escape_html(&page.email))
    };
    // An app whose callback is not `https` is named with its system, as the
    // settings list it: "the Windows app ExampleOrders_8wekyb3d8bbwe".
    let named = |the: &str| match page.system {
        "" => app.clone(),
        system => format!("{the} {system} app {app}"),
    };
    let only_if = format!("Continue only if you opened {app} yourself.");
    let (title, text) = match page.asks {
        Asks::HandOver => (
            format!("{app} wants to sign in as {person}"),
            format!(
                "{app} gets a sign-in of its own as {person}. You stay signed in here. {only_if}"
            ),
        ),
        Asks::SignInAgain => {
            let the_app = named("the");
            (
                format!("Sign in again to continue to {the_app}"),
                format!(
                    "You are signed in here as {person}. To continue to {the_app}, you are \
                     signed out here and sign in again. {only_if}"
                ),
            )
        }
        Asks::AfterSignIn { stays_signed_in } => {
            let the_app = named("The");
            let here = if stays_signed_in {
                "You stay signed in here."
            } else {
                "You are signed out here."
            };
            (
                format!("{the_app} wants to sign in as {person}"),
                format!("{the_app} gets a sign-in of its own as {person}. {here} {only_if}"),
            )
        }
    };
    let csrf = escape_html(csrf);
    let token = escape_html(&page.token);
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"referrer\" content=\"no-referrer\">\n<title>{title}</title>\n</head>\n\
         <body>\n<main>\n<h1>{title}</h1>\n<p>{text}</p>\n\
         <form method=\"post\" action=\"{APP_SIGN_IN_CONFIRM_PATH}\">\n\
         <input type=\"hidden\" name=\"_csrf\" value=\"{csrf}\">\n\
         <input type=\"hidden\" name=\"confirm\" value=\"{token}\">\n\
         <button type=\"submit\" name=\"decision\" value=\"continue\">Continue</button>\n\
         <button type=\"submit\" name=\"decision\" value=\"cancel\">Cancel</button>\n\
         </form>\n</main>\n</body>\n</html>\n"
    )
}

/// The sign-in page `sign_in` with `next` set to the finish address, carrying
/// the hand-over's `attempt` when it has one.
fn sign_in_href(sign_in: &str, attempt: Option<&str>) -> String {
    let next = match attempt {
        Some(attempt) => format!(
            "{APP_SIGN_IN_FINISH_PATH}?attempt={}",
            form_urlencoded::byte_serialize(attempt.as_bytes()).collect::<String>()
        ),
        None => APP_SIGN_IN_FINISH_PATH.to_owned(),
    };
    format!(
        "{sign_in}?next={}",
        form_urlencoded::byte_serialize(next.as_bytes()).collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use http::header::CONTENT_TYPE;

    use super::*;

    #[test]
    fn the_sign_in_page_comes_back_to_the_finish_address_with_the_attempt() {
        assert_eq!(
            sign_in_href("/sign-in", None),
            "/sign-in?next=%2Fauth%2Fapp%2Ffinish"
        );
        assert_eq!(
            sign_in_href("/login", Some("a-b_c")),
            "/login?next=%2Fauth%2Fapp%2Ffinish%3Fattempt%3Da-b_c"
        );
    }

    fn page(asks: Asks, system: &'static str, app: &str) -> String {
        confirm_page(
            &ConfirmPage {
                app: app.to_owned(),
                system,
                name: "Simran Kaur".to_owned(),
                email: "simran@example.in".to_owned(),
                asks,
                token: "one-time".to_owned(),
            },
            "masked",
        )
    }

    // What each page says: the app, the person, what continuing does with the
    // browser, and that only whoever opened the app should continue. An app
    // whose callback is not https is named with the system the settings list
    // it under, since its id alone need not say what asks.
    #[test]
    fn the_confirmation_page_names_the_app_and_says_who_should_continue() {
        const FAMILY: &str = "CharkhaSupply_8wekyb3d8bbwe";
        const PERSON: &str = "Simran Kaur (simran@example.in)";

        let https = page(Asks::HandOver, "Apple", "com.example.orders");
        assert!(
            https.contains(&format!(
                "<h1>com.example.orders wants to sign in as {PERSON}</h1>"
            )),
            "{https}"
        );
        assert!(
            https.contains(
                "You stay signed in here. Continue only if you opened com.example.orders yourself."
            ),
            "{https}"
        );
        assert!(!https.contains("Apple app"), "{https}");

        let again = page(Asks::SignInAgain, "Windows", FAMILY);
        assert!(
            again.contains(&format!(
                "<h1>Sign in again to continue to the Windows app {FAMILY}</h1>"
            )),
            "{again}"
        );
        assert!(
            again.contains(&format!(
                "To continue to the Windows app {FAMILY}, you are signed out here and sign in \
                 again. Continue only if you opened {FAMILY} yourself."
            )),
            "{again}"
        );

        let after = page(
            Asks::AfterSignIn {
                stays_signed_in: false,
            },
            "Windows",
            FAMILY,
        );
        assert!(
            after.contains(&format!(
                "<h1>The Windows app {FAMILY} wants to sign in as {PERSON}</h1>"
            )),
            "{after}"
        );
        assert!(
            after.contains(&format!(
                "The Windows app {FAMILY} gets a sign-in of its own as {PERSON}. You are signed \
                 out here. Continue only if you opened {FAMILY} yourself."
            )),
            "{after}"
        );
        let kept = page(
            Asks::AfterSignIn {
                stays_signed_in: true,
            },
            "Android",
            "com.example.legacy",
        );
        assert!(
            kept.contains(&format!(
                "The Android app com.example.legacy gets a sign-in of its own as {PERSON}. You \
                 stay signed in here. Continue only if you opened com.example.legacy yourself."
            )),
            "{kept}"
        );

        // Every page posts the same form, and runs no script.
        for html in [&https, &again, &after, &kept] {
            assert!(html.contains("action=\"/auth/app/confirm\""), "{html}");
            assert!(
                html.contains("name=\"confirm\" value=\"one-time\""),
                "{html}"
            );
            assert!(html.contains("name=\"_csrf\" value=\"masked\""), "{html}");
            assert!(
                html.contains("value=\"continue\"") && html.contains("value=\"cancel\""),
                "{html}"
            );
            assert!(!html.contains("<script"), "{html}");
        }

        // What a page writes of the app and of the person is text.
        let odd = confirm_page(
            &ConfirmPage {
                app: "<b>orders</b>".to_owned(),
                system: "Windows",
                name: "<i>Simran</i>".to_owned(),
                email: "simran@example.in".to_owned(),
                asks: Asks::SignInAgain,
                token: "\"><i>".to_owned(),
            },
            "masked",
        );
        assert!(!odd.contains("<b>") && !odd.contains("<i>"), "{odd}");

        // The systems the settings list apps under; an app of any other is
        // named by its id alone.
        assert_eq!(system_of("apple"), "Apple");
        assert_eq!(system_of("android"), "Android");
        assert_eq!(system_of("windows"), "Windows");
        assert_eq!(system_of("linux"), "");
        assert!(
            page(Asks::SignInAgain, "", "x.y")
                .contains("<h1>Sign in again to continue to x.y</h1>")
        );
    }

    #[test]
    fn only_the_browsers_own_navigation_opens_the_finish_address() {
        let asked = |pairs: &[(&'static str, &'static str)]| {
            let mut headers = HeaderMap::new();
            for (name, value) in pairs {
                headers.insert(*name, HeaderValue::from_static(value));
            }
            opened_by_the_browser(&headers)
        };
        // A browser that says how it asks, in any case, and one that says nothing.
        assert!(asked(&[]));
        assert!(asked(&[
            ("sec-fetch-mode", "navigate"),
            ("sec-fetch-dest", "document"),
            ("sec-fetch-site", "cross-site"),
        ]));
        assert!(asked(&[
            ("sec-fetch-mode", "Navigate"),
            ("sec-fetch-dest", "DOCUMENT")
        ]));
        for not_the_browser in [
            // navree-ui's runtime and a native client, whatever else they say.
            &[("x-navree-ui", "1")][..],
            &[
                ("x-navree-ui", "1"),
                ("sec-fetch-mode", "navigate"),
                ("sec-fetch-dest", "document"),
            ],
            &[("x-navree-native", "1")],
            // A script's fetch, with or without the runtime's header.
            &[("sec-fetch-mode", "cors"), ("sec-fetch-dest", "empty")],
            &[("sec-fetch-mode", "same-origin")],
            &[("sec-fetch-mode", "no-cors"), ("sec-fetch-dest", "image")],
            // A frame, which navigates but is no page of the browser's own.
            &[("sec-fetch-mode", "navigate"), ("sec-fetch-dest", "iframe")],
            // A page loaded ahead of a click.
            &[
                ("sec-purpose", "prefetch"),
                ("sec-fetch-mode", "navigate"),
                ("sec-fetch-dest", "document"),
            ],
            &[("sec-purpose", "prefetch;prerender")],
            &[("purpose", "prefetch")],
            &[("x-moz", "prefetch")],
            // A page loaded for a preview, which says it is a navigation.
            &[("x-purpose", "preview")],
            &[
                ("x-purpose", "preview"),
                ("sec-fetch-mode", "navigate"),
                ("sec-fetch-dest", "document"),
            ],
        ] {
            assert!(!asked(not_the_browser), "{not_the_browser:?}");
        }
    }

    // What navree-ui's runtime needs of it to go to the address for real: HTML
    // that does not say it is a page of the app. Nothing sends the browser on
    // from it, and nothing keeps it.
    #[test]
    fn the_answer_to_anything_else_is_a_page_no_runtime_shows_and_nothing_keeps() {
        let response = not_a_navigation();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let headers = response.headers();
        assert!(
            headers[CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
        assert!(headers.get("x-navree-page").is_none());
        assert!(headers.get(http::header::LOCATION).is_none());
        assert_eq!(headers[CACHE_CONTROL], "no-store");
        assert_eq!(headers[REFERRER_POLICY], "no-referrer");
        assert_eq!(headers[X_FRAME_OPTIONS], "DENY");
    }

    #[test]
    fn a_callback_is_https_by_its_scheme_in_any_case() {
        for https in [
            "https://example.com/app/signed-in",
            "HTTPS://example.com/app/signed-in",
            "Https://Example.com/app/signed-in",
        ] {
            assert!(is_https(https), "{https}");
        }
        for other in [
            "com.example.orders:/signed-in",
            "httpsx://example.com/app",
            "/app/signed-in",
        ] {
            assert!(!is_https(other), "{other}");
        }
    }

    #[test]
    fn pkce_values_are_read_strictly() {
        // RFC 7636, appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert!(is_verifier(verifier));
        assert_eq!(
            s256(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(is_challenge(&s256(verifier)));
        for bad in ["short", &"a".repeat(129), &format!("{}!", &verifier[..43])] {
            assert!(!is_verifier(bad), "{bad}");
        }
        for bad in [
            "",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-c",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw+cM",
        ] {
            assert!(!is_challenge(bad), "{bad}");
        }
    }
}
