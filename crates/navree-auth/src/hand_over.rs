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
//!    sign-in page with `next` set back to itself; the person signs in there
//!    by any way and in as many steps as it takes. Once the browser is
//!    signed in, it answers `303` to the app's configured callback with a
//!    one-time `code` (60 seconds) and the app's `state`. The browser is
//!    then signed out, unless it was signed in before the hand-over began or
//!    `auth.apps.keep_browser_signed_in` is on.
//! 3. The app posts `{platform, app, code, code_verifier}` to `POST
//!    {api}/auth/app/exchange` from its own HTTP stack. The code works once,
//!    for that app, with the verifier whose SHA-256 the session was started
//!    with, and the sign-in policies are asked about it
//!    (`SignInMethod::AppHandOver`). The answer signs the app's own session
//!    cookie in as a new session of the same person, so the app and the
//!    browser end separately. That session keeps the time the person last
//!    authenticated in the browser session it came from, and ends when a
//!    "remember me" sign-in made at that time would: a hand-over never
//!    makes a sign-in younger or longer-lived than the one it came from.
//!
//! The code reaches only the app the operating system opens for the
//! callback (a verified universal link or App Link), and only the app that
//! made the verifier can use it: a code planted through a link (another
//! person's, or the attacker's own) fails at the exchange. An app whose
//! callback is a custom scheme (`custom_scheme`, which another app could
//! claim) never gets a sign-in the browser already had: the browser is
//! signed out at the start and the person signs in afresh, on a sign-in
//! page whose `next` is the finish address with the hand-over's own
//! one-time `attempt`. The finish hands such an app only a sign-in that came
//! back on that address; a sign-in made for anything else while the
//! hand-over waited (the person signing in to the site in another tab) is
//! refused, and the hand-over ends. The redirect that carries the code is
//! `no-store` and `no-referrer`.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use http::header::{CACHE_CONTROL, REFERRER_POLICY};
use http::{HeaderValue, Method};
use navree_contracts::AuditKind;
use navree_crypto::{constant_time_eq, random_token, sha256_hex};
use navree_http::response::Redirect;
use navree_http::{ApiMeta, AppError, Controller, IntoResponse, Json, Query, RouteDef, schema_fn};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_openapi::ToSchema;
use navree_session::Session;
use navree_validation::{Valid, Validate};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::apps::HandOverApp;
use crate::controller::api;
use crate::passkeys::base64_url;
use crate::passkeys::webauthn::sha256;
use crate::service::entry;
use crate::{
    AppsOptions, AuthError, AuthOptions, AuthService, ClientMeta, SignInAttempt, SignInMethod, User,
    UserResource,
};

/// Where an app starts the hand-over (outside the API prefix: it is opened
/// in the system's sign-in session).
pub const APP_SIGN_IN_PATH: &str = "/auth/app/sign-in";

/// Where the browser goes once it is signed in, to be sent on to the app.
pub const APP_SIGN_IN_FINISH_PATH: &str = "/auth/app/finish";

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
    /// For an app whose callback is a custom scheme: the random value the
    /// sign-in page comes back with (`?attempt=` on the finish address).
    /// Only a sign-in that came back with it is handed over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attempt: Option<String>,
}

/// The query of [`APP_SIGN_IN_FINISH_PATH`]: the hand-over's `attempt`, when
/// the sign-in page sent the browser back.
#[derive(Clone, Default, Deserialize)]
pub(crate) struct FinishRequest {
    #[serde(default)]
    pub(crate) attempt: Option<String>,
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
}

/// Where [`AuthService::finish_app_sign_in`] sends the browser.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Finish {
    /// Not signed in yet: to the sign-in page, which comes back here (with
    /// the hand-over's `attempt`, when it has one).
    SignIn(Option<String>),
    /// To the app's callback, with the code and the state.
    App(String),
}

fn refused(reason: impl Into<String>) -> AuthError {
    AuthError::HandOverRefused(reason.into())
}

fn code_key(code: &str) -> String {
    format!("navree-auth:app-code:{}", sha256_hex(code.as_bytes()))
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

impl AuthService {
    /// Starts handing a sign-in over to an app (`GET /auth/app/sign-in`):
    /// the app is one of `apps` with a callback, the challenge is S256, a
    /// `redirect_uri`, when given, is that callback. The request is kept in
    /// the browser's session; the browser goes on to
    /// [`APP_SIGN_IN_FINISH_PATH`]. Refusals are
    /// [`AuthError::HandOverRefused`] (400 `invalid_hand_over`).
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
        let mut was_signed_in = match self.session_user(session) {
            Some(user) => self
                .live_session(user.user_id, user.session_id)
                .await?
                .is_some(),
            None => false,
        };
        // Any app on the device can claim a custom scheme, and the app that
        // started the hand-over holds the verifier: never hand such an app a
        // sign-in the browser already had, or one it makes for anything
        // else while the hand-over waits. The person signs in afresh, and
        // the sign-in page comes back with `attempt`.
        let custom_scheme = !app.callback.starts_with("https://");
        if was_signed_in && custom_scheme {
            self.sign_out_session(session, meta).await?;
            was_signed_in = false;
        }
        session
            .insert(
                PENDING_KEY,
                &Pending {
                    platform: app.platform.to_owned(),
                    app: app.id,
                    callback: app.callback,
                    challenge: request.code_challenge.clone().unwrap_or_default(),
                    state: request.state.clone(),
                    was_signed_in,
                    started_at: self.clock.now().timestamp(),
                    attempt: custom_scheme.then(|| random_token(32)),
                },
            )
            .map_err(|error| AuthError::Session(error.to_string()))?;
        Ok(())
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
    /// signed in yet goes to the sign-in page ([`Finish::SignIn`]); a signed-in
    /// one gets a one-time code for the app and goes to its callback
    /// ([`Finish::App`]), signed out first unless it was signed in before
    /// or `keep_browser_signed_in` says otherwise. An impersonation is never
    /// handed over. For an app whose callback is a custom scheme, the
    /// sign-in must come back with the hand-over's `attempt`: any other
    /// sign-in ends the hand-over, refused.
    pub(crate) async fn finish_app_sign_in(
        &self,
        session: &Session,
        attempt: Option<&str>,
        keep_browser_signed_in: bool,
        meta: &ClientMeta,
    ) -> Result<Finish, AuthError> {
        let Some(pending) = session.get::<Pending>(PENDING_KEY) else {
            let error = refused("no app sign-in was started in this browser");
            self.hand_over_refused(None, &error, meta).await;
            return Err(error);
        };
        let now = self.clock.now();
        if now.timestamp() - pending.started_at > PENDING_TTL_SECS {
            session.remove(PENDING_KEY);
            let error = refused("the app sign-in waited too long");
            self.hand_over_refused(Some(&pending.app), &error, meta)
                .await;
            return Err(error);
        }
        let signed_in = match self.session_user(session) {
            Some(user) => self
                .live_session(user.user_id, user.session_id)
                .await?
                .map(|_| user),
            None => None,
        };
        let Some(signed_in) = signed_in else {
            return Ok(Finish::SignIn(pending.attempt.clone()));
        };
        // A sign-in that did not come back from the hand-over's own sign-in
        // page is not handed to an app another app could stand in for.
        if let Some(expected) = &pending.attempt
            && !attempt
                .is_some_and(|attempt| constant_time_eq(attempt.as_bytes(), expected.as_bytes()))
        {
            session.remove(PENDING_KEY);
            let error = refused("the sign-in was not made for this app sign-in");
            self.hand_over_refused(Some(&pending.app), &error, meta)
                .await;
            return Err(error);
        }
        if signed_in.impersonator.is_some() {
            session.remove(PENDING_KEY);
            let error = refused("an impersonation is not handed over");
            self.hand_over_refused(Some(&pending.app), &error, meta)
                .await;
            return Err(error);
        }
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
                },
                Duration::from_secs(APP_CODE_TTL_SECS + 5),
            )
            .await?;
        session.remove(PENDING_KEY);
        if !pending.was_signed_in && !keep_browser_signed_in {
            self.sign_out_session(session, meta).await?;
        }
        let mut callback = url::Url::parse(&pending.callback)
            .map_err(|_| refused("the app's callback is not a URL"))?;
        {
            let mut query = callback.query_pairs_mut();
            query.append_pair("code", &code);
            if let Some(state) = &pending.state {
                query.append_pair("state", state);
            }
        }
        Ok(Finish::App(callback.into()))
    }

    /// Exchanges a hand-over `code` for a session of the app's own (`POST
    /// /auth/app/exchange`): the code works once (the first attempt uses it
    /// up, right or wrong), within [`APP_CODE_TTL_SECS`] seconds, for the
    /// app it was issued to, and with the verifier of the challenge the
    /// hand-over was started with. The [`SignInPolicy`](crate::SignInPolicy)s
    /// are asked about [`SignInMethod::AppHandOver`] as for any sign-in. The
    /// app's session is signed in as a new session of the same person, kept
    /// like "remember me", that keeps the time the person last authenticated
    /// in the browser session the code came from and ends when a "remember
    /// me" sign-in made then would
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
            let (user, auth_time) = self.check_code(platform, app, code, verifier).await?;
            let method = SignInMethod::AppHandOver {
                platform: platform.to_owned(),
                app: app.to_owned(),
            };
            self.check_sign_in(&SignInAttempt::of(&user, method), meta)
                .await?;
            self.start_handed_over_session(session, &user, auth_time, meta)
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

    /// The user of a valid `code` and the time they last authenticated in
    /// the browser session it came from.
    async fn check_code(
        &self,
        platform: &str,
        app: &str,
        code: &str,
        verifier: &str,
    ) -> Result<(User, i64), AuthError> {
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
        let auth_time = issued
            .auth_time
            .ok_or_else(|| refused("the code names no sign-in time"))?;
        let user = self
            .find_user(issued.user_id)
            .await?
            .ok_or_else(|| refused("the user no longer exists"))?;
        Ok((user, auth_time))
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
                .name("auth.app.sign_in")
                .api(ApiMeta {
                    description: Some(
                        "Opened by a native app in the system's sign-in session, with \
                         `platform`, `app`, `code_challenge`, `code_challenge_method=S256` and \
                         `state`. Goes on to the sign-in page and, once the person is signed \
                         in, to the app's callback with a one-time code."
                            .to_owned(),
                    ),
                    ..api("Start handing a sign-in over to an app", &[303, 400])
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
                    let target = match service
                        .finish_app_sign_in(&session, request.attempt.as_deref(), keep, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?
                    {
                        Finish::SignIn(attempt) => sign_in_href(&sign_in, attempt.as_deref()),
                        Finish::App(callback) => callback,
                    };
                    // The code is in the address: never cached, never sent on as
                    // a referrer.
                    let mut response = Redirect::to(&target).into_response();
                    let headers = response.headers_mut();
                    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
                    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
                    Ok::<_, AppError>(response)
                }
            };
        routes.push(
            RouteDef::on(Method::GET, format!("!{APP_SIGN_IN_FINISH_PATH}"), finish)
                .public()
                .name("auth.app.finish")
                .api(api(
                    "Send a signed-in browser on to the app with a one-time code",
                    &[303, 400],
                )),
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
        routes
    }
}

/// Exchanges one client may attempt per minute.
const EXCHANGES_PER_MINUTE: u32 = 30;

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
