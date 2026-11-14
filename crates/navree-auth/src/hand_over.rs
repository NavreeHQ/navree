//! Handing a browser sign-in over to a native app (feature `session`).
//!
//! A native app signs a person in through the system's sign-in session
//! (`ASWebAuthenticationSession` on Apple systems, a Custom Tab or Auth Tab
//! on Android), which shows the site's own sign-in pages with every way in
//! the site offers. That session keeps its own cookies, so the app is still
//! signed out when it ends. The hand-over is an authorization code with a
//! proof of possession (PKCE, S256 only):
//!
//! 1. The app makes a random `code_verifier` and opens the system's sign-in
//!    session at `GET /auth/app/sign-in?platform=apple|android&app=<bundle
//!    id or package>&code_challenge=<base64url SHA-256 of the
//!    verifier>&code_challenge_method=S256&state=<its own random value>`.
//!    The app must be one of `auth.apps` with a `callback`. The session
//!    remembers the request and goes to `GET /auth/app/finish`.
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
//!    signs the browser out and asks the person to sign in again (a custom
//!    scheme). Cancel ends the hand-over and sends the browser to the
//!    callback with `error=access_denied` and the `state`. The `attempt` and
//!    the confirmation's value each work once, also for requests sent at the
//!    same time: each is held in the cache the codes are kept in, and the
//!    first request that takes it from there uses it up.
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
//! gets a sign-in the browser already had: the person signs in again. The
//! answers of the finish and the confirmation are `no-store` and
//! `no-referrer`.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use http::header::{CACHE_CONTROL, REFERRER_POLICY, X_FRAME_OPTIONS};
use http::{HeaderValue, Method};
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
    /// While a browser that was signed in at the start has not answered the
    /// confirmation page: the one-time value its form posts back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confirm: Option<String>,
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
    /// The browser was signed in when the hand-over began: the page that
    /// asks the person first.
    Confirm(ConfirmPage),
}

/// What the confirmation page says and posts back.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ConfirmPage {
    /// The app's bundle id or package name.
    app: String,
    /// The signed-in person's name.
    name: String,
    /// The signed-in person's email.
    email: String,
    /// An app with a custom-scheme callback: continuing signs the browser
    /// out, and the person signs in again.
    sign_in_again: bool,
    /// The hand-over's one-time value.
    token: String,
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
                started_at: self.clock.now().timestamp(),
                attempt: Some(attempt),
                confirm,
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
        if self.clock.now().timestamp() - pending.started_at > PENDING_TTL_SECS {
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
    /// for the app and goes to its callback ([`Finish::App`]), once (the
    /// attempt is used up, also by two requests sent at the same time); a
    /// browser
    /// that was signed in when the hand-over began is asked first
    /// ([`Finish::Confirm`], answered by
    /// [`confirm_app_sign_in`](AuthService::confirm_app_sign_in)). Any
    /// other sign-in (made for something else while the hand-over waited)
    /// ends the hand-over, refused. An impersonation is never handed over.
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
            let source = Source {
                user: signed_in,
                ends_at,
                finished: Finished::SignedIn,
            };
            return self
                .issue_code(session, pending, source, keep_browser_signed_in, meta)
                .await;
        }
        // Signed in before the hand-over began: the person says first.
        if let Some(token) = pending.confirm.clone() {
            let Some(user) = self.find_user(signed_in.user_id).await? else {
                return Err(self
                    .end_hand_over(session, &pending.app, "the user no longer exists", meta)
                    .await);
            };
            return Ok(Finish::Confirm(ConfirmPage {
                app: pending.app.clone(),
                name: user.name,
                email: user.email,
                sign_in_again: !is_https(&pending.callback),
                token,
            }));
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

    /// Answers the confirmation page (`POST /auth/app/confirm`) with the
    /// hand-over's one-time value `token`: Continue (`proceed`) hands the
    /// browser's sign-in over to an app with an `https` callback (the
    /// browser stays signed in), or signs the browser out and sends it to
    /// the sign-in page with the hand-over's `attempt` for an app with a
    /// custom scheme (and for a browser no longer signed in). Cancel ends
    /// the hand-over and sends the browser to the callback with
    /// `error=access_denied` and the `state`. A value that is not the
    /// waiting hand-over's, or one another request used already (two
    /// answers sent at the same time get one code between them), ends it,
    /// refused.
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
                    expires_at: now.timestamp() + i64::try_from(APP_CODE_TTL_SECS).unwrap_or(60),
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
        if self.clock.now().timestamp() >= issued.expires_at {
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
    /// `apple` or `android`.
    #[validate(length(min = 1, max = 16))]
    pub platform: String,
    /// The app's bundle id or package name.
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
        let finish =
            move |session: Session, meta: ClientMeta, Query(request): Query<FinishRequest>| {
                let service = service.clone();
                let sign_in = sign_in.clone();
                async move {
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
                         gets a page that asks the person first (200, HTML)."
                            .to_owned(),
                    ),
                    ..api(
                        "Send a signed-in browser on to the app with a one-time code",
                        &[200, 303, 400],
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
                async move {
                    let user = service
                        .exchange_app_code(
                            &session,
                            &input.platform,
                            &input.app,
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
    let (title, text) = if page.sign_in_again {
        (
            format!("Sign in again to continue to {app}"),
            format!(
                "You are signed in here as {person}. To continue to {app}, you are signed out \
                 here and sign in again."
            ),
        )
    } else {
        (
            format!("{app} wants to sign in as {person}"),
            format!(
                "{app} gets a sign-in of its own as {person}. You stay signed in here. Continue \
                 only if you opened {app} yourself."
            ),
        )
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
