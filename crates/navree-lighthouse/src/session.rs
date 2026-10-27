//! The dashboard's sign-in session: an encrypted, httpOnly,
//! `SameSite=Strict` cookie scoped to the dashboard's path, holding the
//! user, the expiry and the CSRF token.

use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use cookie::{Cookie, SameSite};
use http::HeaderMap;
use http::header::COOKIE;
use navree_crypto::{Encrypter, constant_time_eq, random_token};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The session cookie.
pub(crate) const SESSION_COOKIE: &str = "lighthouse_session";
/// The sign-in form's CSRF cookie (before there is a session).
pub(crate) const LOGIN_CSRF_COOKIE: &str = "lighthouse_csrf";
/// The header carrying the CSRF token of a signed-in dashboard.
pub(crate) const CSRF_HEADER: &str = "x-csrf-token";
/// How long a sign-in lasts.
pub(crate) const SESSION_HOURS: i64 = 12;

/// What the cookie holds.
#[derive(Serialize, Deserialize)]
struct SessionData {
    /// The user.
    u: Uuid,
    /// Expiry (Unix seconds).
    e: i64,
    /// The CSRF token.
    c: String,
}

/// A signed-in dashboard session (a request extension).
#[derive(Clone, Debug)]
pub(crate) struct DashboardSession {
    pub(crate) user_id: Uuid,
    pub(crate) csrf: String,
}

impl DashboardSession {
    /// Whether `token` is this session's CSRF token.
    pub(crate) fn csrf_matches(&self, token: &str) -> bool {
        constant_time_eq(self.csrf.as_bytes(), token.as_bytes())
    }
}

/// Reads and writes session cookies.
#[derive(Clone)]
pub(crate) struct Sessions {
    encrypter: Arc<Encrypter>,
    path: String,
    secure: bool,
}

impl Sessions {
    pub(crate) fn new(encrypter: Arc<Encrypter>, path: &str, secure: bool) -> Sessions {
        Sessions {
            encrypter,
            path: path.to_owned(),
            secure,
        }
    }

    /// The session of a request, if its cookie is valid and unexpired.
    pub(crate) fn read(&self, headers: &HeaderMap) -> Option<DashboardSession> {
        let value = cookie_value(headers, SESSION_COOKIE)?;
        let data: SessionData = self.encrypter.decrypt_json(&value).ok()?;
        (data.e > Utc::now().timestamp()).then_some(DashboardSession {
            user_id: data.u,
            csrf: data.c,
        })
    }

    /// A `Set-Cookie` value starting a session for `user_id`.
    pub(crate) fn start(&self, user_id: Uuid) -> Option<String> {
        let data = SessionData {
            u: user_id,
            e: (Utc::now() + TimeDelta::hours(SESSION_HOURS)).timestamp(),
            c: random_token(32),
        };
        let value = self.encrypter.encrypt_json(&data).ok()?;
        Some(self.cookie(SESSION_COOKIE, value, Some(SESSION_HOURS * 3600)))
    }

    /// A `Set-Cookie` value ending the session.
    pub(crate) fn end(&self) -> String {
        self.cookie(SESSION_COOKIE, String::new(), Some(0))
    }

    /// A `Set-Cookie` value for the sign-in form's CSRF token.
    pub(crate) fn login_csrf(&self, token: &str) -> String {
        self.cookie(LOGIN_CSRF_COOKIE, token.to_owned(), Some(3600))
    }

    fn cookie(&self, name: &'static str, value: String, max_age: Option<i64>) -> String {
        let mut cookie = Cookie::build((name, value))
            .path(self.path.clone())
            .http_only(true)
            .same_site(SameSite::Strict)
            .secure(self.secure);
        if let Some(seconds) = max_age {
            cookie = cookie.max_age(cookie::time::Duration::seconds(seconds));
        }
        cookie.build().to_string()
    }
}

/// The value of cookie `name` in a request's headers.
pub(crate) fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| Cookie::split_parse(value.to_owned()).filter_map(Result::ok))
        .find(|cookie| cookie.name() == name)
        .map(|cookie| cookie.value().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;
    use navree_crypto::{AppKey, generate_key};

    use super::*;

    fn sessions() -> Sessions {
        let key = AppKey::parse(&generate_key()).unwrap();
        Sessions::new(Arc::new(Encrypter::new(key)), "/_lighthouse", true)
    }

    fn headers_with(set_cookie: &str) -> HeaderMap {
        let pair = set_cookie.split(';').next().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_str(pair).unwrap());
        headers
    }

    #[test]
    fn a_session_round_trips_through_its_cookie() {
        let sessions = sessions();
        let user = Uuid::now_v7();
        let set_cookie = sessions.start(user).unwrap();
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));
        assert!(set_cookie.contains("Path=/_lighthouse"));
        assert!(set_cookie.contains("Secure"));
        let session = sessions.read(&headers_with(&set_cookie)).unwrap();
        assert_eq!(session.user_id, user);
        assert!(session.csrf_matches(&session.csrf.clone()));
        assert!(!session.csrf_matches("nope"));
    }

    #[test]
    fn tampered_foreign_and_ended_cookies_are_no_session() {
        let sessions = sessions();
        let set_cookie = sessions.start(Uuid::now_v7()).unwrap();
        let mut tampered = set_cookie.clone();
        tampered.insert(30, 'x');
        assert!(sessions.read(&headers_with(&tampered)).is_none());
        let other = self::sessions();
        assert!(
            other.read(&headers_with(&set_cookie)).is_none(),
            "another key"
        );
        assert!(sessions.read(&headers_with(&sessions.end())).is_none());
        assert!(sessions.end().contains("Max-Age=0"));
    }
}
