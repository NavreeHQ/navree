//! The session layer ([`LayerSlot::Session`]): loads the session, runs the
//! request, saves the session and sets the cookies.

use std::borrow::Cow;
use std::sync::Arc;

use chrono::TimeDelta;
use cookie::Cookie;
use http::header::{
    AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HeaderName, ORIGIN, SET_COOKIE, USER_AGENT,
};
use http::{HeaderMap, HeaderValue, Method};
use navree_contracts::{
    AuditEntry, AuditKind, AuditSink, Clock, RequestId, SessionRecord, SessionStore, SystemClock,
};
use navree_crypto::Encrypter;
use navree_glob::Glob;
use navree_http::body::Body;
use navree_http::csrf::{TokenChecked, token_matches};
use navree_http::extract::Request;
use navree_http::middleware::{Next, State, from_fn_with_state};
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, ClientIp, GlobalLayer, HttpOptions, LayerSlot};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, ProviderKey, Resolver,
};
use percent_encoding::percent_decode_str;
use serde_json::json;
use tokio::sync::OnceCell;

use crate::multipart;
use crate::session::Session;
use crate::{SameSite, SessionOptions, XSRF_COOKIE};

/// The header carrying the CSRF token as is.
static X_CSRF_TOKEN: HeaderName = HeaderName::from_static(navree_http::csrf::CSRF_HEADER);

/// At most this many same-name session cookies of a request are tried.
const MAX_SESSION_COOKIES: usize = 4;

/// Set by browsers: `cross-site`, `same-site`, `same-origin` or `none`.
static SEC_FETCH_SITE: HeaderName = HeaderName::from_static("sec-fetch-site");

/// The header carrying the URL-encoded `XSRF-TOKEN` cookie value.
static X_XSRF_TOKEN: HeaderName = HeaderName::from_static(navree_http::csrf::XSRF_HEADER);

/// What the session layer needs, resolved once at boot.
pub(crate) struct SessionManager {
    options: Arc<SessionOptions>,
    /// The session cookie's name ([`SessionOptions::cookie_name`]).
    cookie_name: String,
    store: Arc<dyn SessionStore>,
    encrypter: Arc<Encrypter>,
    /// The app's `dyn Clock`, else the system clock.
    clock: Arc<dyn Clock>,
    /// `csrf_except`, compiled.
    except: Vec<Glob>,
    /// `http.cors.allowed_origins`: front ends trusted to write cross-site.
    allowed_origins: Vec<String>,
    /// The paths whose posts are checked by their origin instead of the token
    /// ([`navree_http::csrf::SameOriginOnly`]).
    same_origin_only: Vec<&'static str>,
}

impl Injectable for SessionManager {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<SessionOptions>(),
            ProviderKey::of::<dyn SessionStore>(),
            ProviderKey::of::<Encrypter>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let options = r.get::<SessionOptions>().await?;
            let except = options
                .csrf_except
                .iter()
                .map(|pattern| {
                    let pattern = pattern.trim();
                    if pattern.starts_with('/') {
                        Glob::new(pattern)
                    } else {
                        Glob::new(&format!("/{pattern}"))
                    }
                })
                .collect();
            let encrypter = r.get::<Encrypter>().await?;
            if encrypter.current_kid().is_none() {
                tracing::warn!(
                    target: "navree::session",
                    "no application key is configured (`app.key` / APP_KEY): requests that \
                     write a session will fail until one is set"
                );
            }
            let allowed_origins = r
                .app()
                .try_get::<HttpOptions>()
                .await
                .map(|http| {
                    http.cors
                        .allowed_origins
                        .iter()
                        .map(|origin| origin.trim().trim_end_matches('/').to_owned())
                        .collect()
                })
                .unwrap_or_default();
            let same_origin_only = r
                .app()
                .contributions::<navree_http::csrf::SameOriginOnly>()
                .into_iter()
                .map(|contributed| contributed.value.0)
                .collect();
            Ok(SessionManager {
                same_origin_only,
                cookie_name: options.cookie_name(),
                options,
                store: r.get::<dyn SessionStore>().await?,
                encrypter,
                clock: clock_of(&r).await,
                except,
                allowed_origins,
            })
        })
    }
}

/// The app's `dyn Clock`, or the system clock.
pub(crate) async fn clock_of(r: &Resolver) -> Arc<dyn Clock> {
    match r.app().try_get::<dyn Clock>().await {
        Some(clock) => clock,
        None => Arc::new(SystemClock),
    }
}

/// `minutes` as a duration.
fn minutes(minutes: u64) -> TimeDelta {
    TimeDelta::minutes(i64::try_from(minutes).unwrap_or(i64::MAX))
}

/// How long after a write a request extends the session again: a minute,
/// or half the lifetime when that is shorter.
fn touch_interval(lifetime: TimeDelta) -> TimeDelta {
    TimeDelta::minutes(1).min(lifetime / 2)
}

/// The [`LayerSlot::Session`] layer.
pub(crate) fn session_layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::Session, "session", |ctx, router| {
        // A weaker check than the token: say at boot which paths have it.
        let paths: Vec<&'static str> = ctx
            .contributions::<navree_http::csrf::SameOriginOnly>()
            .into_iter()
            .map(|contributed| contributed.value.0)
            .collect();
        if !paths.is_empty() {
            tracing::info!(
                target: "navree::security",
                ?paths,
                "these paths are checked by their origin in place of the CSRF token"
            );
        }
        let manager: Arc<OnceCell<Arc<SessionManager>>> = Arc::new(OnceCell::new());
        Ok(router.layer(from_fn_with_state(manager, handle)))
    })
}

async fn handle(
    State(manager): State<Arc<OnceCell<Arc<SessionManager>>>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(ctx) = request.extensions().get::<AppContext>().cloned() else {
        return AppError::internal("the session layer runs outside the navree pipeline")
            .into_response();
    };
    let manager = match manager
        .get_or_try_init(|| ctx.get::<SessionManager>())
        .await
    {
        Ok(manager) => manager.clone(),
        Err(error) => return AppError::internal(error).into_response(),
    };
    manager.run(request, next).await
}

impl SessionManager {
    async fn run(&self, mut request: Request, next: Next) -> Response {
        // Only this layer says a token matched: whatever was there before it is not its word.
        request.extensions_mut().remove::<TokenChecked>();
        let cookies = session_cookies(request.headers(), &self.cookie_name);
        let session = match self.start(&cookies).await {
            Ok(session) => session,
            Err(error) => return AppError::internal(error).into_response(),
        };
        if self.needs_csrf_check(&request) {
            // Whether the session's token was sent and matched: not when the path is
            // checked by its origin, nor when no session cookie came.
            let mut token_matched = false;
            let checked = if self.same_origin_only.contains(&request.uri().path()) {
                check_same_origin(request.headers(), request.uri(), !cookies.is_empty())
            } else if cookies.is_empty() {
                self.check_cross_site(request.headers())
            } else {
                let compared = match check_csrf(&request, &session) {
                    // A plain HTML form cannot set a header: its body may carry the token.
                    Err(CsrfFailure::Missing) => match FormBody::of(request.headers()) {
                        Some(form) => {
                            let taken = match form {
                                FormBody::Urlencoded => take_form_token(request, &session).await,
                                FormBody::Multipart(boundary) => {
                                    take_multipart_token(request, &boundary, &session).await
                                }
                            };
                            match taken {
                                Ok(checked) => {
                                    request = checked;
                                    Ok(())
                                }
                                Err(refused) => {
                                    let (returned, failure) = *refused;
                                    reject(&returned, failure).await;
                                    return AppError::csrf_mismatch().into_response();
                                }
                            }
                        }
                        None => Err(CsrfFailure::Missing),
                    },
                    other => other,
                };
                token_matched = compared.is_ok();
                compared
            };
            if let Err(failure) = checked {
                reject(&request, failure).await;
                return AppError::csrf_mismatch().into_response();
            }
            // On the request the handler gets: after a form's field was taken out, the
            // request rebuilt from its body.
            if token_matched {
                request.extensions_mut().insert(TokenChecked);
            }
        }
        request.extensions_mut().insert(session.clone());
        let mut response = next.run(request).await;
        if let Err(error) = self.finish(&session, response.headers_mut()).await {
            return AppError::internal(error).into_response();
        }
        response
    }

    /// The session the first live session cookie names (several cookies
    /// can share the name, e.g. host-only and domain cookies), or a new one
    /// when no cookie decrypts to a live record. Expired records are
    /// deleted. A new session always gets a new id: an id chosen by the
    /// client is never adopted.
    async fn start(&self, cookies: &[String]) -> Result<Session, BoxError> {
        let ids = cookies
            .iter()
            .take(MAX_SESSION_COOKIES)
            .filter_map(|value| self.encrypter.decrypt_string(value).ok());
        for id in ids {
            let Some(record) = self.store.load(&id).await? else {
                continue;
            };
            if record.expires_at.is_some_and(|at| at <= self.clock.now()) {
                self.store.delete(&id).await?;
                continue;
            }
            return Ok(Session::loaded(id, record));
        }
        Ok(Session::detached())
    }

    /// Saves the session when it changed (or, at most once a minute, to
    /// extend it), deletes the record it replaced and sets the cookies. A
    /// session that set its own lifetime ([`Session::set_lifetime`], what
    /// "remember me" does) lives and keeps its cookies that long instead of
    /// `session.lifetime_minutes`.
    async fn finish(&self, session: &Session, headers: &mut HeaderMap) -> Result<(), BoxError> {
        let now = self.clock.now();
        let (save, replaced, xsrf, lifetime) = {
            let state = session.lock();
            let lifetime = state
                .lifetime_minutes()
                .map_or_else(|| self.options.lifetime(), minutes);
            let touch = state.stored_id.as_deref() == Some(state.id.as_str())
                && state
                    .expires_at
                    .is_some_and(|at| at - now < lifetime - touch_interval(lifetime));
            let save = (state.dirty || touch).then(|| {
                let record = SessionRecord {
                    data: state.data.clone(),
                    expires_at: Some(now + lifetime),
                };
                (state.id.clone(), record)
            });
            let replaced = state.stored_id.clone().filter(|stored| *stored != state.id);
            let xsrf = state
                .token()
                .filter(|_| save.is_some() || state.token_requested)
                .map(str::to_owned);
            (save, replaced, xsrf, lifetime)
        };
        if let Some((id, record)) = save {
            // Encrypt first: without an app key nothing is stored.
            let value = self.encrypter.encrypt_string(&id)?;
            let cookie = self.cookie(&self.cookie_name, value, true, lifetime);
            self.store.save(&id, record).await?;
            headers.append(SET_COOKIE, HeaderValue::from_str(&cookie.to_string())?);
        }
        if let Some(old) = replaced {
            self.store.delete(&old).await?;
        }
        if let Some(token) = xsrf {
            let cookie = self.cookie(XSRF_COOKIE, token, false, lifetime);
            headers.append(SET_COOKIE, HeaderValue::from_str(&cookie.to_string())?);
        }
        Ok(())
    }

    /// A browser request without a session cookie has no token to send, so
    /// it passes unless the browser marks it cross-site
    /// (`Sec-Fetch-Site: cross-site`, which pages cannot forge) and its
    /// `Origin` is not an allowed front end (`http.cors.allowed_origins`):
    /// a cross-site form must not, for example, sign the victim in to the
    /// attacker's account (login CSRF).
    fn check_cross_site(&self, headers: &HeaderMap) -> Result<(), CsrfFailure> {
        let cross_site = headers
            .get(&SEC_FETCH_SITE)
            .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"cross-site"));
        if !cross_site {
            return Ok(());
        }
        let origin = headers
            .get(ORIGIN)
            .and_then(|value| value.to_str().ok())
            .map(|origin| origin.trim().trim_end_matches('/'));
        match origin {
            Some(origin) if self.allowed_origins.iter().any(|allowed| allowed == origin) => Ok(()),
            _ => Err(CsrfFailure::CrossSite),
        }
    }

    /// Whether `request` is subject to CSRF protection: a state-changing
    /// method, no bearer token, and a path not in `csrf_except`.
    fn needs_csrf_check(&self, request: &Request) -> bool {
        let safe = matches!(
            *request.method(),
            Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
        );
        if safe || has_bearer_token(request.headers()) {
            return false;
        }
        let path = request.uri().path();
        !self.except.iter().any(|pattern| pattern.matches(path))
    }

    /// A cookie with the configured attributes, kept for `lifetime`.
    fn cookie(
        &self,
        name: &str,
        value: String,
        http_only: bool,
        lifetime: TimeDelta,
    ) -> Cookie<'static> {
        let options = &self.options;
        let mut cookie = Cookie::build((name.to_owned(), value))
            .path("/")
            .http_only(http_only)
            .secure(options.secure)
            .same_site(match options.same_site {
                SameSite::Strict => cookie::SameSite::Strict,
                SameSite::Lax => cookie::SameSite::Lax,
                SameSite::None => cookie::SameSite::None,
            })
            .max_age(cookie::time::Duration::seconds(lifetime.num_seconds()));
        if let Some(domain) = &options.domain {
            cookie = cookie.domain(domain.clone());
        }
        cookie.build()
    }
}

/// The check of a request to a [`navree_http::csrf::SameOriginOnly`] path, in place of the
/// token: the browser's `Sec-Fetch-Site` must say `same-origin`; a browser that doesn't
/// send it must send an `Origin` naming this host ([`navree_http::csrf::origin_is_host`]);
/// a request that sends neither passes only without a session cookie.
fn check_same_origin(
    headers: &HeaderMap,
    uri: &http::Uri,
    has_session: bool,
) -> Result<(), CsrfFailure> {
    if let Some(site) = headers.get(&SEC_FETCH_SITE) {
        return if site.as_bytes().eq_ignore_ascii_case(b"same-origin") {
            Ok(())
        } else {
            Err(CsrfFailure::CrossSite)
        };
    }
    let Some(origin) = headers.get(ORIGIN) else {
        return if has_session {
            Err(CsrfFailure::Missing)
        } else {
            Ok(())
        };
    };
    let same = origin
        .to_str()
        .is_ok_and(|origin| navree_http::csrf::origin_is_host(origin, headers, uri));
    if same {
        Ok(())
    } else {
        Err(CsrfFailure::CrossSite)
    }
}

/// Whether the request authenticates with `Authorization: Bearer <token>`.
fn has_bearer_token(headers: &HeaderMap) -> bool {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().split_once(' '))
        .is_some_and(|(scheme, token)| {
            scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()
        })
}

/// Why a request failed the CSRF check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CsrfFailure {
    /// The request sent no `X-CSRF-TOKEN` or `X-XSRF-TOKEN` header.
    Missing,
    /// The tokens sent are not the session's.
    Mismatch,
    /// A cross-site browser request from an origin that is not allowed,
    /// without a session.
    CrossSite,
}

impl CsrfFailure {
    fn as_str(self) -> &'static str {
        match self {
            CsrfFailure::Missing => "missing",
            CsrfFailure::Mismatch => "mismatch",
            CsrfFailure::CrossSite => "cross_site",
        }
    }
}

/// The field a plain HTML form carries the CSRF token in (navree-http's
/// name, which navree-ui's forms use too).
pub const CSRF_FIELD: &str = navree_http::csrf::CSRF_FIELD;

/// The most of a urlencoded body read to find the token field. A larger body
/// has to send the header (a script can).
const FORM_TOKEN_BODY_MAX: usize = 256 * 1024;

/// A body a plain HTML form sends, which may carry the token.
enum FormBody {
    /// `application/x-www-form-urlencoded`.
    Urlencoded,
    /// `multipart/form-data` (a form with a file field), with its boundary.
    Multipart(String),
}

impl FormBody {
    /// The kind of form the request's body is, if it is one.
    fn of(headers: &HeaderMap) -> Option<FormBody> {
        if is_urlencoded(headers) {
            return Some(FormBody::Urlencoded);
        }
        multipart::form_boundary(headers).map(FormBody::Multipart)
    }
}

/// Whether `given` (trimmed) stands for the session's CSRF token: the token, or
/// a mask of it as a page carries it ([`navree_http::csrf::token_matches`]),
/// compared in constant time. A session without a token has none to match.
fn is_session_token(session: &Session, given: &str) -> bool {
    let state = session.lock();
    state
        .token()
        .is_some_and(|expected| token_matches(given, expected))
}

/// Whether the request body is `application/x-www-form-urlencoded`.
fn is_urlencoded(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|kind| {
            kind.trim()
                .eq_ignore_ascii_case("application/x-www-form-urlencoded")
        })
}

/// Reads a urlencoded body, takes its one `_csrf` field out, and checks that
/// field against the session's token. On success the request goes on with the
/// body it would have had without the field; on failure it comes back (with an
/// empty body) beside the reason.
///
/// Nothing but the exact field name counts, in the body only. Two fields of
/// the name are refused whichever is right, so no parser downstream can read a
/// different one than this does.
async fn take_form_token(
    request: Request,
    session: &Session,
) -> Result<Request, Box<(Request, CsrfFailure)>> {
    let limit = navree_http::body::BodyLimit::of(request.extensions()).min(FORM_TOKEN_BODY_MAX);
    let (mut parts, body) = request.into_parts();
    let fail = |parts: http::request::Parts, failure| {
        Err(Box::new((
            Request::from_parts(parts, Body::empty()),
            failure,
        )))
    };
    let Ok(bytes) = navree_http::body::read_body(body, limit).await else {
        return fail(parts, CsrfFailure::Missing);
    };
    let mut kept: Vec<&[u8]> = Vec::new();
    let mut found: Vec<String> = Vec::new();
    for segment in bytes.split(|byte| *byte == b'&') {
        match form_urlencoded::parse(segment).next() {
            Some((name, value)) if name == CSRF_FIELD => found.push(value.into_owned()),
            _ => kept.push(segment),
        }
    }
    let [given] = found.as_slice() else {
        return fail(
            parts,
            if found.is_empty() {
                CsrfFailure::Missing
            } else {
                CsrfFailure::Mismatch
            },
        );
    };
    if !is_session_token(session, given) {
        return fail(parts, CsrfFailure::Mismatch);
    }
    let rest = kept.join(&b'&');
    parts
        .headers
        .insert(CONTENT_LENGTH, HeaderValue::from(rest.len()));
    Ok(Request::from_parts(parts, Body::from(rest)))
}

/// Reads a multipart body's first part, which has to be the `_csrf` field and
/// end within its first [`multipart::TOKEN_PART_MAX`] bytes, and checks it
/// against the session's token. On success the request goes on with its body
/// as it came, the field included, its files still unread; on failure it comes
/// back (with an empty body) beside the reason.
///
/// A form with a file field puts the token first (navree-ui's forms do), so
/// nothing of a file is read or held to find it. A first part that is anything
/// else, or a body that does not read one way, is refused as missing the token
/// (see [`multipart`]).
async fn take_multipart_token(
    request: Request,
    boundary: &str,
    session: &Session,
) -> Result<Request, Box<(Request, CsrfFailure)>> {
    let limit =
        navree_http::body::BodyLimit::of(request.extensions()).min(multipart::TOKEN_PART_MAX);
    let (parts, body) = request.into_parts();
    let fail = |parts: http::request::Parts, failure| {
        Err(Box::new((
            Request::from_parts(parts, Body::empty()),
            failure,
        )))
    };
    let Some((given, body)) = multipart::first_part_token(body, boundary, limit).await else {
        return fail(parts, CsrfFailure::Missing);
    };
    let matches = std::str::from_utf8(&given).is_ok_and(|given| is_session_token(session, given));
    if !matches {
        return fail(parts, CsrfFailure::Mismatch);
    }
    Ok(Request::from_parts(parts, body))
}

/// Whether `X-CSRF-TOKEN`, or the URL-decoded `X-XSRF-TOKEN`, stands for the
/// session's CSRF token: equals it, or is a mask of it (compared in constant
/// time).
fn check_csrf(request: &Request, session: &Session) -> Result<(), CsrfFailure> {
    let headers = request.headers();
    let plain = headers
        .get_all(&X_CSRF_TOKEN)
        .iter()
        .map(|value| value.to_str().map(Cow::Borrowed).ok());
    let decoded = headers.get_all(&X_XSRF_TOKEN).iter().map(|value| {
        value
            .to_str()
            .ok()
            .and_then(|value| percent_decode_str(value).decode_utf8().ok())
    });
    let given: Vec<Option<Cow<'_, str>>> = plain.chain(decoded).collect();
    if given.is_empty() {
        return Err(CsrfFailure::Missing);
    }
    let state = session.lock();
    let expected = state.token().ok_or(CsrfFailure::Mismatch)?;
    let matches = given
        .iter()
        .flatten()
        .any(|given| token_matches(given, expected));
    if matches {
        Ok(())
    } else {
        Err(CsrfFailure::Mismatch)
    }
}

/// Logs a failed CSRF check (target `navree::security`) and records it in the
/// audit trail when the app has a `dyn AuditSink`. Tokens are never logged.
fn reject(request: &Request, failure: CsrfFailure) -> impl Future<Output = ()> + Send + 'static {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let ip = request.extensions().get::<ClientIp>().map(|ip| ip.0);
    tracing::warn!(
        target: "navree::security",
        method,
        path,
        reason = failure.as_str(),
        ip = ip.map(tracing::field::display),
        "CSRF token mismatch"
    );
    let ctx = request.extensions().get::<AppContext>().cloned();
    let mut entry = AuditEntry::new(AuditKind::Security, "csrf.mismatch").metadata(json!({
        "method": method,
        "path": path,
        "reason": failure.as_str(),
    }));
    if let Some(ip) = ip {
        entry = entry.ip(ip.to_string());
    }
    if let Some(request_id) = request.extensions().get::<RequestId>() {
        entry = entry.request_id(request_id.as_str());
    }
    if let Some(user_agent) = request
        .headers()
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok())
    {
        entry = entry.user_agent(user_agent);
    }
    async move {
        let Some(ctx) = ctx else {
            return;
        };
        let Some(sink) = ctx.try_get::<dyn AuditSink>().await else {
            return;
        };
        if let Err(error) = sink.record(entry).await {
            tracing::warn!(target: "navree::security", %error, "could not audit a CSRF failure");
        }
    }
}

/// The values of every cookie `name` in the request's `Cookie` headers, in
/// order.
fn session_cookies(headers: &HeaderMap, name: &str) -> Vec<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(Cookie::split_parse)
        .filter_map(Result::ok)
        .filter(|cookie| cookie.name() == name)
        .map(|cookie| cookie.value().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(*name, HeaderValue::from_static(value));
        }
        headers
    }

    // The origin check of a same-origin-only path: the browser's word, else the Origin
    // against the host the request went to (Host, else HTTP/2's :authority), else nothing
    // only without a session.
    #[test]
    fn a_same_origin_only_path_reads_the_browsers_word_then_the_origin() {
        let path = http::Uri::from_static("/_navree/ui/appearance");
        let h2 = http::Uri::from_static("https://app.example/_navree/ui/appearance");
        let check = |pairs: &[(&'static str, &'static str)], uri: &http::Uri, session| {
            check_same_origin(&headers(pairs), uri, session)
        };
        assert_eq!(
            check(&[("sec-fetch-site", "same-origin")], &path, true),
            Ok(())
        );
        for site in ["same-site", "cross-site", "none"] {
            assert_eq!(
                check(&[("sec-fetch-site", site)], &path, true),
                Err(CsrfFailure::CrossSite),
                "{site}"
            );
        }
        // Neither header: only a client with no session to ride on.
        assert_eq!(check(&[], &path, false), Ok(()));
        assert_eq!(check(&[], &path, true), Err(CsrfFailure::Missing));
        // The Origin against Host, case and default ports aside.
        for (origin, host) in [
            ("https://app.example", "app.example"),
            ("https://APP.example", "app.example:443"),
            ("http://app.example:80", "app.example"),
            ("http://127.0.0.1:8080", "127.0.0.1:8080"),
            ("https://[::1]:8443", "[::1]:8443"),
        ] {
            let mut sent = headers(&[]);
            sent.insert("origin", HeaderValue::from_str(origin).unwrap());
            sent.insert("host", HeaderValue::from_str(host).unwrap());
            assert_eq!(
                check_same_origin(&sent, &path, true),
                Ok(()),
                "{origin} {host}"
            );
        }
        for (origin, host) in [
            ("https://evil.example", "app.example"),
            ("https://app.example.evil.example", "app.example"),
            ("https://app.example:8443", "app.example"),
            ("http://app.example:8080", "app.example:80"),
            ("null", "app.example"),
            ("ftp://app.example", "app.example"),
            ("https://user@app.example", "app.example"),
            ("https://app.example/x", "app.example"),
            ("https://app.example", "app.example:443x"),
        ] {
            let mut sent = headers(&[]);
            sent.insert("origin", HeaderValue::from_str(origin).unwrap());
            sent.insert("host", HeaderValue::from_str(host).unwrap());
            assert_eq!(
                check_same_origin(&sent, &path, true),
                Err(CsrfFailure::CrossSite),
                "{origin} {host}"
            );
        }
        // HTTP/2 and HTTP/3 may send no Host: the URI's authority stands in.
        assert_eq!(
            check(&[("origin", "https://app.example")], &h2, true),
            Ok(())
        );
        assert_eq!(
            check(&[("origin", "https://evil.example")], &h2, true),
            Err(CsrfFailure::CrossSite)
        );
        // With neither, nothing to compare with.
        assert_eq!(
            check(&[("origin", "https://app.example")], &path, true),
            Err(CsrfFailure::CrossSite)
        );
    }

    #[test]
    fn session_cookies_are_found_among_others() {
        let found = session_cookies(
            &headers(&[
                ("cookie", "theme=dark; XSRF-TOKEN=abc; navree_session=first"),
                ("cookie", "navree_session=v1.k.n.c; other=1"),
            ]),
            "navree_session",
        );
        assert_eq!(found, ["first", "v1.k.n.c"]);
        assert!(
            session_cookies(
                &headers(&[("cookie", "navree_session_x=1")]),
                "navree_session"
            )
            .is_empty()
        );
        assert_eq!(
            session_cookies(&headers(&[("cookie", "navree_session=")]), "navree_session"),
            [""],
            "an empty cookie is still a cookie"
        );
        assert!(session_cookies(&HeaderMap::new(), "navree_session").is_empty());
    }

    #[test]
    fn only_bearer_tokens_count() {
        assert!(has_bearer_token(&headers(&[(
            "authorization",
            "Bearer 1|abc"
        )])));
        assert!(has_bearer_token(&headers(&[(
            "authorization",
            "bearer abc"
        )])));
        assert!(!has_bearer_token(&headers(&[("authorization", "Bearer ")])));
        assert!(!has_bearer_token(&headers(&[("authorization", "Bearer")])));
        assert!(!has_bearer_token(&headers(&[(
            "authorization",
            "Basic dTpw"
        )])));
        assert!(!has_bearer_token(&HeaderMap::new()));
    }

    #[test]
    fn the_touch_interval_is_a_minute_or_half_the_lifetime() {
        assert_eq!(
            touch_interval(TimeDelta::minutes(120)),
            TimeDelta::minutes(1)
        );
        assert_eq!(
            touch_interval(TimeDelta::minutes(1)),
            TimeDelta::seconds(30)
        );
    }
}
