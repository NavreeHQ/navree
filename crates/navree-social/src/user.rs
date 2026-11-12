//! The values a social sign-in passes around: [`SocialUser`],
//! [`ProviderToken`], [`SocialRedirect`], [`SocialCallback`],
//! [`RedirectOptions`], [`Intent`] and [`LinkedAccount`].

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};
use navree_contracts::Secret;
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// The tokens a provider issued for a user: what
/// [`SocialProvider::user`](crate::SocialProvider::user) reads the profile
/// with, and what `social.store_tokens` keeps (encrypted). Never in debug
/// output.
#[derive(Clone)]
pub struct ProviderToken {
    /// The access token for the provider's API.
    pub access_token: Secret<String>,
    /// The refresh token, when the provider issued one.
    pub refresh_token: Option<Secret<String>>,
    /// When the access token expires, when the provider said.
    pub expires_at: Option<DateTime<Utc>>,
    /// The OpenID Connect ID token, when the provider issued one.
    pub id_token: Option<Secret<String>>,
    /// The scopes the provider granted (when it said).
    pub scopes: Vec<String>,
}

impl ProviderToken {
    /// A token with `access_token` and nothing else.
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: Secret::new(access_token.into()),
            refresh_token: None,
            expires_at: None,
            id_token: None,
            scopes: Vec::new(),
        }
    }

    /// Sets the refresh token.
    pub fn refresh_token(mut self, refresh_token: impl Into<String>) -> Self {
        self.refresh_token = Some(Secret::new(refresh_token.into()));
        self
    }

    /// Sets when the access token expires.
    pub fn expires_at(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Sets the granted scopes.
    pub fn scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }
}

impl fmt::Debug for ProviderToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderToken")
            .field("access_token", &"[redacted]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .field("expires_at", &self.expires_at)
            .field("id_token", &self.id_token.as_ref().map(|_| "[redacted]"))
            .field("scopes", &self.scopes)
            .finish()
    }
}

/// A provider's user, normalized: what [`Social::user`](crate::Social::user)
/// returns and [`Social::sign_in`](crate::Social::sign_in) signs in.
///
/// ```
/// use navree_social::SocialUser;
///
/// let ada = SocialUser::new("github", "583231")
///     .name("Anu Lakhotia")
///     .email(Some("anu@example.com"), true)
///     .nickname("anu");
/// assert_eq!(ada.email.as_deref(), Some("anu@example.com"));
/// assert!(ada.email_verified);
/// ```
#[derive(Clone)]
pub struct SocialUser {
    /// The provider key (`github`, `okta`).
    pub provider: String,
    /// The provider's user id (stable; never the email).
    pub id: String,
    /// The display name, when the provider shared one.
    pub name: Option<String>,
    /// The email address (lower-cased), when the provider shared one.
    pub email: Option<String>,
    /// Whether the provider has verified the email address. Only verified
    /// addresses link accounts or pass `allowed_email_domains`.
    pub email_verified: bool,
    /// The avatar URL.
    pub avatar_url: Option<String>,
    /// The handle (`login`, `username`, `preferred_username`).
    pub nickname: Option<String>,
    /// The provider's tokens (absent for users built in tests).
    pub token: Option<ProviderToken>,
    /// The provider's whole profile (ID token claims and userinfo, or the
    /// API's user object).
    pub raw: Value,
}

impl SocialUser {
    /// A user of `provider` with the provider's user id `id`.
    pub fn new(provider: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            id: id.into(),
            name: None,
            email: None,
            email_verified: false,
            avatar_url: None,
            nickname: None,
            token: None,
            raw: Value::Null,
        }
    }

    /// Sets the display name (an empty one is ignored).
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = non_empty(name.into());
        self
    }

    /// Sets the email address (trimmed and lower-cased) and whether the
    /// provider verified it. `None` or an empty address clears it.
    pub fn email(mut self, email: Option<&str>, verified: bool) -> Self {
        self.email = email.and_then(|e| non_empty(e.trim().to_lowercase()));
        self.email_verified = verified && self.email.is_some();
        self
    }

    /// Sets the avatar URL.
    pub fn avatar_url(mut self, url: impl Into<String>) -> Self {
        self.avatar_url = non_empty(url.into());
        self
    }

    /// Sets the handle.
    pub fn nickname(mut self, nickname: impl Into<String>) -> Self {
        self.nickname = non_empty(nickname.into());
        self
    }

    /// Sets the provider's tokens.
    pub fn token(mut self, token: ProviderToken) -> Self {
        self.token = Some(token);
        self
    }

    /// Sets the provider's whole profile.
    pub fn raw(mut self, raw: Value) -> Self {
        self.raw = raw;
        self
    }

    /// The domain of the email address (`acme.test`).
    pub fn email_domain(&self) -> Option<&str> {
        self.email.as_deref()?.rsplit_once('@').map(|(_, d)| d)
    }
}

impl fmt::Debug for SocialUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SocialUser")
            .field("provider", &self.provider)
            .field("id", &self.id)
            .field("name", &self.name)
            .field("email", &self.email)
            .field("email_verified", &self.email_verified)
            .field("avatar_url", &self.avatar_url)
            .field("nickname", &self.nickname)
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Where to send the browser: `{"url", "state"}` (`GET
/// /api/auth/social/{provider}/redirect`).
#[derive(Clone, Serialize, ToSchema)]
pub struct SocialRedirect {
    /// The provider's authorization URL.
    pub url: String,
    /// The sealed state, also in `url`; the front end keeps it (an httpOnly
    /// cookie) and checks it on the callback page.
    pub state: String,
}

impl fmt::Debug for SocialRedirect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SocialRedirect")
            .field("url", &"[redacted]")
            .field("state", &"[redacted]")
            .finish()
    }
}

/// What the provider sent back to the callback page: `{code, state}`, plus
/// `user` for Sign in with Apple (the JSON Apple posts on the first
/// sign-in, with the name).
#[derive(Clone, Default, Deserialize, ToSchema)]
pub struct SocialCallback {
    /// The authorization code.
    pub code: String,
    /// The state from the redirect.
    pub state: String,
    /// Sign in with Apple's `user` field (a JSON string or object), sent
    /// only on the first sign-in.
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub user: Option<Value>,
}

impl SocialCallback {
    /// A callback with `code` and `state`.
    pub fn new(code: impl Into<String>, state: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            state: state.into(),
            user: None,
        }
    }
}

impl fmt::Debug for SocialCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SocialCallback")
            .field("code", &"[redacted]")
            .field("state", &"[redacted]")
            .field("user", &self.user.as_ref().map(|_| "[present]"))
            .finish()
    }
}

/// The one-time nonce of a sign-in from a native app (`POST
/// /api/auth/social/{provider}/native/nonce`): the app passes `nonce` to
/// the system's sheet (Google: as is; Apple: its SHA-256 in lower-case
/// hex) and posts `state` back with the identity token, within
/// `expires_in` seconds, once.
#[derive(Clone, Serialize, ToSchema)]
#[non_exhaustive]
pub struct NativeNonce {
    /// The nonce for the provider's request.
    pub nonce: String,
    /// The sealed attempt, to post back with the identity token.
    pub state: String,
    /// Seconds until the attempt expires.
    pub expires_in: u64,
}

impl fmt::Debug for NativeNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeNonce")
            .field("nonce", &"[redacted]")
            .field("state", &"[redacted]")
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// What a native app posts after the system's sign-in sheet (`POST
/// /api/auth/social/{provider}/native`): the identity token, the sealed
/// attempt of [`NativeNonce`], and for Sign in with Apple the
/// authorization code and the name Apple hands the app on the first
/// sign-in only.
///
/// ```
/// use navree_social::NativeSignIn;
///
/// let post = NativeSignIn::new("eyJraWQiOi...", "nvs1.AbC")
///     .authorization_code("c3b1f2...")
///     .user(serde_json::json!({ "name": { "firstName": "Simran", "lastName": "Gill" } }));
/// assert!(post.authorization_code.is_some());
/// ```
#[derive(Clone, Default, Deserialize, ToSchema)]
#[non_exhaustive]
pub struct NativeSignIn {
    /// The identity token (a JWT) the provider gave the app.
    pub id_token: String,
    /// The `state` of the attempt's [`NativeNonce`].
    pub state: String,
    /// Sign in with Apple's authorization code: exchanged for a refresh
    /// token the server keeps (encrypted) to revoke when the account is
    /// deleted or Apple disconnected.
    #[serde(default)]
    pub authorization_code: Option<String>,
    /// Sign in with Apple's name, sent by the app on the first sign-in
    /// (`{"name": {"firstName", "lastName"}}`). Unverified: used only as a
    /// display name, never to find or link an account.
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub user: Option<Value>,
}

impl NativeSignIn {
    /// A post with the identity token and the attempt's state.
    pub fn new(id_token: impl Into<String>, state: impl Into<String>) -> Self {
        Self {
            id_token: id_token.into(),
            state: state.into(),
            authorization_code: None,
            user: None,
        }
    }

    /// Sets Sign in with Apple's authorization code.
    pub fn authorization_code(mut self, code: impl Into<String>) -> Self {
        self.authorization_code = Some(code.into());
        self
    }

    /// Sets the name Apple handed the app on the first sign-in.
    pub fn user(mut self, user: Value) -> Self {
        self.user = Some(user);
        self
    }
}

impl fmt::Debug for NativeSignIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeSignIn")
            .field("id_token", &"[redacted]")
            .field("state", &"[redacted]")
            .field(
                "authorization_code",
                &self.authorization_code.as_ref().map(|_| "[redacted]"),
            )
            .field("user", &self.user.as_ref().map(|_| "[present]"))
            .finish()
    }
}

/// What a flow is for: signing in (the default) or connecting a provider
/// to the signed-in user (`?intent=link`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    /// Sign in (or sign up) with the provider.
    #[default]
    SignIn,
    /// Connect the provider to the signed-in user.
    Link,
}

/// Options of [`Social::redirect_with`](crate::Social::redirect_with).
#[derive(Clone, Debug, Default)]
pub struct RedirectOptions {
    /// Scopes requested in addition to the provider's and the configured
    /// ones.
    pub scopes: Vec<String>,
    /// Extra authorization URL parameters (`prompt = consent`).
    pub params: BTreeMap<String, String>,
    /// Where the front end goes after the callback: a relative path
    /// (`/deals`), returned by the callback.
    pub return_to: Option<String>,
    /// Sign-in or link.
    pub intent: Intent,
    /// For [`Intent::Link`]: the signed-in user; the state only links to
    /// this user.
    pub user_id: Option<Uuid>,
}

/// A linked provider account, as `GET /api/auth/social/accounts` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct LinkedAccount {
    /// The provider key (`github`).
    pub provider: String,
    /// The provider's display name (`GitHub`, or the configured `name`).
    pub provider_name: String,
    /// The email address the provider reported.
    pub email: Option<String>,
    /// The name the provider reported.
    pub name: Option<String>,
    /// The avatar URL the provider reported.
    pub avatar_url: Option<String>,
    /// When the account was linked.
    pub linked_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_builder_normalizes_values() {
        let user = SocialUser::new("github", "1")
            .name("  ")
            .email(Some(" Anu@Example.COM "), true)
            .avatar_url("https://a.test/1.png")
            .nickname("anu");
        assert_eq!(user.name, None);
        assert_eq!(user.email.as_deref(), Some("anu@example.com"));
        assert_eq!(user.email_domain(), Some("example.com"));
        assert!(user.email_verified);
        let none = SocialUser::new("github", "1").email(None, true);
        assert!(!none.email_verified, "no address, nothing verified");
    }

    #[test]
    fn tokens_and_codes_stay_out_of_debug_output() {
        let user = SocialUser::new("github", "1").token(
            ProviderToken::new("gho_secret")
                .refresh_token("ghr_secret")
                .scopes(["read:user"]),
        );
        let debug = format!("{user:?}");
        assert!(
            !debug.contains("gho_secret") && !debug.contains("ghr_secret"),
            "{debug}"
        );
        let callback = SocialCallback::new("code-123", "nvs1.state");
        let debug = format!("{callback:?}");
        assert!(
            !debug.contains("code-123") && !debug.contains("nvs1"),
            "{debug}"
        );
    }

    #[test]
    fn callbacks_deserialize_with_an_optional_apple_user() {
        let callback: SocialCallback = serde_json::from_value(json!({
            "code": "c", "state": "s", "user": "{\"name\":{\"firstName\":\"Anu\"}}"
        }))
        .unwrap();
        assert!(callback.user.is_some());
        let callback: SocialCallback =
            serde_json::from_value(json!({ "code": "c", "state": "s" })).unwrap();
        assert!(callback.user.is_none());
    }

    #[test]
    fn intents_are_snake_case() {
        assert_eq!(serde_json::to_value(Intent::SignIn).unwrap(), "sign_in");
        assert_eq!(
            serde_json::from_value::<Intent>(json!("link")).unwrap(),
            Intent::Link
        );
    }
}
