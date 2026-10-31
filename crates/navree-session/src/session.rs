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
    /// Fails when `value` cannot be represented as JSON, and for the key
    /// `_token`, which holds the CSRF token.
    pub fn insert<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> Result<(), SessionError> {
        if key == TOKEN_KEY {
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
    /// (`_token`) cannot be removed.
    pub fn remove(&self, key: &str) -> Option<Value> {
        if key == TOKEN_KEY {
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
    #[error("the session key `{key}` is reserved (it holds the CSRF token)")]
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
