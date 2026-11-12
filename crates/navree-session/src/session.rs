//! [`Session`]: the current request's session, and [`SessionError`].

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use http::request::Parts;
use navree_contracts::SessionRecord;
use navree_http::AppError;
use navree_http::extract::{FromRequestParts, OptionalFromRequestParts};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// The length of session ids (base62 characters, about 238 bits).
pub(crate) const ID_LEN: usize = 40;

/// The session key holding the CSRF token.
pub(crate) const TOKEN_KEY: &str = "_token";

/// The session key holding this session's own idle lifetime in minutes
/// ([`Session::set_lifetime`]); absent for the configured one.
pub(crate) const LIFETIME_KEY: &str = "_lifetime";

/// The longest lifetime a session may set itself (about a year, as
/// `session.lifetime_minutes` allows).
pub(crate) const MAX_LIFETIME_MINUTES: u64 = 366 * 24 * 60;

/// Whether `key` is kept for the session's own use.
fn is_reserved(key: &str) -> bool {
    key == TOKEN_KEY || key == LIFETIME_KEY
}

/// The length of CSRF tokens (base62 characters).
pub(crate) const TOKEN_LEN: usize = 40;

/// A new random session id.
fn new_id() -> String {
    navree_crypto::random_token(ID_LEN)
}

/// A new random CSRF token.
fn new_token() -> String {
    navree_crypto::random_token(TOKEN_LEN)
}

/// The session of the current request: an extractor, and a cheap handle
/// (clones share the same session).
///
/// The session layer loads it before the handler runs and, after the
/// response, saves it when it changed and sets the session cookie. Values
/// are stored as JSON.
///
/// ```no_run
/// use navree_session::Session;
///
/// async fn visit(session: Session) -> navree_http::Result<String> {
///     let visits = session.get::<u32>("visits").unwrap_or(0) + 1;
///     session.insert("visits", &visits)?;
///     Ok(format!("visit {visits}"))
/// }
/// ```
///
/// `Debug` shows the keys only: ids and values can be credentials.
#[derive(Clone)]
pub struct Session {
    state: Arc<Mutex<State>>,
}

/// What the session layer knows about a session. No `Debug`: it holds the
/// id and the values.
pub(crate) struct State {
    /// The current id.
    pub(crate) id: String,
    /// The values.
    pub(crate) data: Map<String, Value>,
    /// The id the record was loaded under; `None` for a new session.
    pub(crate) stored_id: Option<String>,
    /// When the loaded record expires.
    pub(crate) expires_at: Option<DateTime<Utc>>,
    /// Something changed since the session was loaded.
    pub(crate) dirty: bool,
    /// [`Session::csrf_token`] was called: the response carries the
    /// `XSRF-TOKEN` cookie.
    pub(crate) token_requested: bool,
}

impl State {
    /// The CSRF token, if the session has one.
    pub(crate) fn token(&self) -> Option<&str> {
        self.data
            .get(TOKEN_KEY)
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
    }

    /// The lifetime this session set itself, in minutes
    /// ([`Session::set_lifetime`]), if any.
    pub(crate) fn lifetime_minutes(&self) -> Option<u64> {
        self.data
            .get(LIFETIME_KEY)
            .and_then(Value::as_u64)
            .filter(|minutes| (1..=MAX_LIFETIME_MINUTES).contains(minutes))
    }
}

impl Session {
    /// A new, empty session that no request carries and nothing saves
    /// (for tests and background code that call functions taking a
    /// `&Session`). Requests get theirs from the session layer.
    pub fn detached() -> Session {
        Session::from_state(State {
            id: new_id(),
            data: Map::new(),
            stored_id: None,
            expires_at: None,
            dirty: false,
            token_requested: false,
        })
    }

    /// A session loaded from the record stored under `id`.
    pub(crate) fn loaded(id: String, record: SessionRecord) -> Session {
        Session::from_state(State {
            stored_id: Some(id.clone()),
            id,
            data: record.data,
            expires_at: record.expires_at,
            dirty: false,
            token_requested: false,
        })
    }

    fn from_state(state: State) -> Session {
        Session {
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether the request came with this session's cookie: `false` for a session the
    /// request started (no cookie, or one for a session that has ended). A form that a
    /// page of this site drew always comes back with one, since drawing it started the
    /// session; a post with none came from somewhere else.
    pub fn is_resumed(&self) -> bool {
        self.lock().stored_id.is_some()
    }

    /// The session id. It is a credential: never log it or send it to the
    /// client other than in the encrypted session cookie.
    pub fn id(&self) -> String {
        self.lock().id.clone()
    }

    /// The value stored under `key`, or `None` when there is none or it is
    /// not a `T`.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let value = self.lock().data.get(key)?.clone();
        serde_json::from_value(value).ok()
    }

    /// Stores `value` under `key`, replacing any previous value.
    ///
    /// Fails when `value` cannot be represented as JSON, and for the keys
    /// the session keeps for itself: `_token` (the CSRF token) and
    /// `_lifetime` (see [`set_lifetime`](Session::set_lifetime)).
    pub fn insert<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> Result<(), SessionError> {
        if is_reserved(key) {
            return Err(SessionError::ReservedKey {
                key: key.to_owned(),
            });
        }
        let value = serde_json::to_value(value).map_err(|error| SessionError::Serialize {
            key: key.to_owned(),
            reason: error.to_string(),
        })?;
        let mut state = self.lock();
        state.data.insert(key.to_owned(), value);
        state.dirty = true;
        Ok(())
    }

    /// Removes the value under `key`, returning it. The CSRF token
    /// (`_token`) and the lifetime (`_lifetime`) cannot be removed this
    /// way.
    pub fn remove(&self, key: &str) -> Option<Value> {
        if is_reserved(key) {
            return None;
        }
        let mut state = self.lock();
        let removed = state.data.remove(key);
        if removed.is_some() {
            state.dirty = true;
        }
        removed
    }

    /// Whether a value is stored under `key`.
    pub fn contains(&self, key: &str) -> bool {
        self.lock().data.contains_key(key)
    }

    /// Gives the session a new id and a new CSRF token, keeping its values;
    /// the old record is deleted when the response is sent. Call it on
    /// login and on privilege changes, against session fixation.
    pub fn regenerate(&self) {
        let mut state = self.lock();
        state.id = new_id();
        if state.token().is_some() {
            state
                .data
                .insert(TOKEN_KEY.to_owned(), Value::String(new_token()));
        }
        state.dirty = true;
    }

    /// Gives the session a new id and a new CSRF token and removes every
    /// value; the old record is deleted when the response is sent (logout).
    pub fn invalidate(&self) {
        let mut state = self.lock();
        state.id = new_id();
        state.data.clear();
        state
            .data
            .insert(TOKEN_KEY.to_owned(), Value::String(new_token()));
        state.dirty = true;
    }

    /// Makes this session live `lifetime` when idle, in place of
    /// `session.lifetime_minutes`, and its cookies last as long: what
    /// "remember me" sets at sign-in. Rounded up to whole minutes, at
    /// least one and at most about a year. It is stored with the session, so
    /// it holds on every later request; [`invalidate`](Session::invalidate)
    /// and [`flush`](Session::flush) drop it, and [`clear_lifetime`](Session::clear_lifetime)
    /// goes back to the configured one.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use navree_session::Session;
    ///
    /// let session = Session::detached();
    /// session.set_lifetime(Duration::from_secs(30 * 24 * 3600));
    /// assert_eq!(session.lifetime(), Some(Duration::from_secs(30 * 24 * 3600)));
    /// ```
    pub fn set_lifetime(&self, lifetime: std::time::Duration) {
        let minutes = lifetime
            .as_secs()
            .div_ceil(60)
            .clamp(1, MAX_LIFETIME_MINUTES);
        let mut state = self.lock();
        state
            .data
            .insert(LIFETIME_KEY.to_owned(), Value::from(minutes));
        state.dirty = true;
    }

    /// The lifetime set with [`set_lifetime`](Session::set_lifetime), if
    /// any (`None`: the configured `session.lifetime_minutes`).
    pub fn lifetime(&self) -> Option<std::time::Duration> {
        self.lock()
            .lifetime_minutes()
            .map(|minutes| std::time::Duration::from_secs(minutes * 60))
    }

    /// Goes back to the configured lifetime (`session.lifetime_minutes`).
    pub fn clear_lifetime(&self) {
        let mut state = self.lock();
        if state.data.remove(LIFETIME_KEY).is_some() {
            state.dirty = true;
        }
    }

    /// Removes every value except the CSRF token, keeping the id.
    pub fn flush(&self) {
        let mut state = self.lock();
        let token = state.data.remove(TOKEN_KEY);
        state.data.clear();
        if let Some(token) = token {
            state.data.insert(TOKEN_KEY.to_owned(), token);
        }
        state.dirty = true;
    }

    /// The CSRF token of the session (40 characters), created on first use.
    /// The response also carries it in the `XSRF-TOKEN` cookie.
    pub fn csrf_token(&self) -> String {
        let mut state = self.lock();
        state.token_requested = true;
        if let Some(token) = state.token() {
            return token.to_owned();
        }
        let token = new_token();
        state
            .data
            .insert(TOKEN_KEY.to_owned(), Value::String(token.clone()));
        state.dirty = true;
        token
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock();
        f.debug_struct("Session")
            .field("keys", &state.data.keys().collect::<Vec<_>>())
            .field("dirty", &state.dirty)
            .finish_non_exhaustive()
    }
}

/// The session of the request; a 500 error when `SessionModule` is not in
/// the app.
impl FromRequestParts for Session {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<Session>().cloned().ok_or_else(|| {
            AppError::internal(
                "no session in the request: add SessionModule to the app (its layer loads one per request)",
            )
        })
    }
}

/// `Option<Session>`: `None` outside the session layer.
impl OptionalFromRequestParts for Session {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Option<Self>, Self::Rejection> {
        Ok(parts.extensions.get::<Session>().cloned())
    }
}

/// A session operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// A value could not be converted to JSON.
    #[error("the session value for `{key}` cannot be stored as JSON: {reason}")]
    Serialize {
        /// The key.
        key: String,
        /// Why (the serializer's message).
        reason: String,
    },
    /// The key is reserved for the session's own use.
    #[error(
        "the session key `{key}` is reserved (it holds the CSRF token or the session's lifetime)"
    )]
    ReservedKey {
        /// The key.
        key: String,
    },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use navree_http::handler::Endpoint;
    use serde_json::json;

    use super::*;

    #[test]
    fn handlers_on_the_session_are_native() {
        async fn handler(_: Session, _: Option<Session>) {}
        let _ = Endpoint::new(handler);
    }

    #[test]
    fn values_roundtrip_as_json() {
        let session = Session::detached();
        assert_eq!(session.id().len(), ID_LEN);
        session.insert("n", &7u32).unwrap();
        session.insert("s", "text").unwrap();
        assert_eq!(session.get::<u32>("n"), Some(7));
        assert_eq!(session.get::<String>("s").as_deref(), Some("text"));
        assert_eq!(session.get::<String>("n"), None, "not a string");
        assert_eq!(session.get::<u32>("missing"), None);
        assert!(session.contains("n"));
        assert!(!session.contains("missing"));
        assert!(session.lock().dirty);
    }

    #[test]
    fn clones_share_the_session() {
        let session = Session::detached();
        let clone = session.clone();
        clone.insert("k", &1).unwrap();
        clone.regenerate();
        assert_eq!(session.get::<i32>("k"), Some(1));
        assert_eq!(session.id(), clone.id());
    }

    #[test]
    fn values_that_are_not_json_are_rejected() {
        let session = Session::detached();
        let mut map = BTreeMap::new();
        map.insert((1, 2), "tuple keys are not JSON object keys");
        let error = session.insert("map", &map).unwrap_err();
        assert!(matches!(error, SessionError::Serialize { ref key, .. } if key == "map"));
        assert!(!session.contains("map"));
        assert!(!session.lock().dirty);
    }

    #[test]
    fn the_token_key_is_reserved() {
        let session = Session::detached();
        let token = session.csrf_token();
        let error = session.insert(TOKEN_KEY, "forged").unwrap_err();
        assert!(matches!(error, SessionError::ReservedKey { .. }));
        assert_eq!(session.remove(TOKEN_KEY), None);
        assert_eq!(session.csrf_token(), token);
    }

    #[test]
    fn the_csrf_token_is_created_once() {
        let session = Session::detached();
        assert!(!session.lock().dirty);
        let token = session.csrf_token();
        assert_eq!(token.len(), TOKEN_LEN);
        assert!(token.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert!(session.lock().dirty && session.lock().token_requested);
        assert_eq!(session.csrf_token(), token);
        assert_ne!(Session::detached().csrf_token(), token);
    }

    #[test]
    fn regenerate_keeps_values_and_rotates_an_existing_token() {
        let session = Session::detached();
        let id = session.id();
        session.insert("k", &1).unwrap();
        session.regenerate();
        assert_ne!(session.id(), id);
        assert_eq!(session.get::<i32>("k"), Some(1));
        assert_eq!(session.lock().token(), None, "no token is created");

        let token = session.csrf_token();
        session.regenerate();
        assert_ne!(session.csrf_token(), token);
    }

    #[test]
    fn invalidate_empties_the_session_with_a_fresh_token() {
        let session = Session::detached();
        let id = session.id();
        let token = session.csrf_token();
        session.insert("k", &1).unwrap();
        session.invalidate();
        assert_ne!(session.id(), id);
        assert!(!session.contains("k"));
        let fresh = session
            .lock()
            .token()
            .map(str::to_owned)
            .expect("a fresh token");
        assert_ne!(fresh, token);
        assert_eq!(session.lock().data.len(), 1, "only the token");
    }

    // "remember me" is a lifetime the session keeps for itself.
    #[test]
    fn a_session_keeps_its_own_lifetime_until_it_is_cleared() {
        use std::time::Duration;

        let session = Session::detached();
        assert_eq!(session.lifetime(), None);
        session.set_lifetime(Duration::from_secs(30 * 24 * 3600));
        assert_eq!(
            session.lifetime(),
            Some(Duration::from_secs(30 * 24 * 3600))
        );
        assert!(session.lock().dirty);
        // Rounded up to whole minutes, at least one, at most about a year.
        session.set_lifetime(Duration::from_secs(61));
        assert_eq!(session.lifetime(), Some(Duration::from_secs(120)));
        session.set_lifetime(Duration::ZERO);
        assert_eq!(session.lifetime(), Some(Duration::from_secs(60)));
        session.set_lifetime(Duration::from_secs(u64::MAX));
        assert_eq!(
            session.lifetime(),
            Some(Duration::from_secs(MAX_LIFETIME_MINUTES * 60))
        );
        // The key is the session's own: not written, not removed, by hand.
        assert!(matches!(
            session.insert(LIFETIME_KEY, &5),
            Err(SessionError::ReservedKey { .. })
        ));
        assert_eq!(session.remove(LIFETIME_KEY), None);
        assert!(session.lifetime().is_some());
        // Regenerating keeps it (signing in); invalidating drops it (signing out).
        session.regenerate();
        assert!(session.lifetime().is_some());
        session.clear_lifetime();
        assert_eq!(session.lifetime(), None);
        session.set_lifetime(Duration::from_secs(3600));
        session.invalidate();
        assert_eq!(session.lifetime(), None);
    }

    #[test]
    fn flush_keeps_the_id_and_the_token() {
        let session = Session::detached();
        let id = session.id();
        let token = session.csrf_token();
        session.insert("a", &1).unwrap();
        session.insert("b", &2).unwrap();
        session.flush();
        assert_eq!(session.id(), id);
        assert!(!session.contains("a") && !session.contains("b"));
        assert_eq!(session.csrf_token(), token);
    }

    #[test]
    fn remove_only_dirties_when_something_was_removed() {
        let session = Session::loaded("id".into(), SessionRecord::default());
        assert_eq!(session.remove("missing"), None);
        assert!(!session.lock().dirty);
        session.insert("k", &json!({"a": 1})).unwrap();
        assert_eq!(session.remove("k"), Some(json!({"a": 1})));
    }

    #[test]
    fn debug_shows_keys_only() {
        let session = Session::detached();
        session.insert("secret_key_name", "hunter2").unwrap();
        let token = session.csrf_token();
        let debug = format!("{session:?}");
        assert!(debug.contains("secret_key_name"), "{debug}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(!debug.contains(&token), "{debug}");
        assert!(!debug.contains(&session.id()), "{debug}");
    }
}
