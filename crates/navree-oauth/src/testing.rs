//! Test helpers: [`pkce_pair`] and [`authorize`], which walks the
//! authorization code flow with navree-testing's `TestClient`.
//!
//! ```ignore
//! use navree::oauth::testing::authorize;
//!
//! let tokens = authorize(&app.client(), client_id, &user_token, &["deals:read"]).await;
//! app.client().get("/api/v1/deals").bearer(&tokens.access_token).send().await.assert_status(200);
//! ```

use std::fmt::Display;

use navree_testing::TestClient;

use crate::grants::TokenResponse;
use navree_crypto::random_token;

use crate::token::pkce_challenge;

/// A new PKCE code verifier and its S256 challenge: `(verifier,
/// challenge)`.
pub fn pkce_pair() -> (String, String) {
    let verifier = random_token(64);
    let challenge = pkce_challenge(&verifier);
    (verifier, challenge)
}

/// Walks the authorization code flow for a client with one redirect URI
/// (a public client, or a confidential one with [`AuthorizeFlow::secret`]):
/// `GET /oauth/authorize`, the consent page's `POST
/// /api/oauth/requests/{id}/approve` as the user whose access token is
/// `user_token`, then `POST /oauth/token`. Panics when a step fails.
pub async fn authorize(
    client: &TestClient,
    client_id: impl Display,
    user_token: &str,
    scopes: &[&str],
) -> TokenResponse {
    AuthorizeFlow::new(client_id)
        .run(client, user_token, scopes)
        .await
}

/// The authorization code flow with options; [`authorize`] runs it with
/// the defaults.
#[derive(Clone, Debug)]
pub struct AuthorizeFlow {
    client_id: String,
    secret: Option<String>,
    redirect_uri: Option<String>,
    nonce: Option<String>,
    oauth_path: String,
    api_prefix: String,
}

impl AuthorizeFlow {
    /// The flow for `client_id`.
    pub fn new(client_id: impl Display) -> Self {
        Self {
            client_id: client_id.to_string(),
            secret: None,
            redirect_uri: None,
            nonce: None,
            oauth_path: "/oauth".to_owned(),
            api_prefix: "/api".to_owned(),
        }
    }

    /// Authenticates at the token endpoint with `secret` (a confidential
    /// client).
    pub fn secret(mut self, secret: impl Into<String>) -> Self {
        self.secret = Some(secret.into());
        self
    }

    /// Sends `redirect_uri` (needed when the client has several).
    pub fn redirect_uri(mut self, uri: impl Into<String>) -> Self {
        self.redirect_uri = Some(uri.into());
        self
    }

    /// Sends an OpenID Connect `nonce`.
    pub fn nonce(mut self, nonce: impl Into<String>) -> Self {
        self.nonce = Some(nonce.into());
        self
    }

    /// Where the protocol endpoints are (`oauth.path`, default `/oauth`).
    pub fn oauth_path(mut self, path: impl Into<String>) -> Self {
        self.oauth_path = path.into();
        self
    }

    /// The API prefix (`http.api_prefix`, default `/api`).
    pub fn api_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.api_prefix = prefix.into();
        self
    }

    /// Runs the flow as the user whose access token is `user_token`,
    /// asking for `scopes`.
    pub async fn run(
        &self,
        client: &TestClient,
        user_token: &str,
        scopes: &[&str],
    ) -> TokenResponse {
        let (verifier, challenge) = pkce_pair();
        let scope = scopes.join(" ");
        let mut query = vec![
            ("response_type", "code"),
            ("client_id", self.client_id.as_str()),
            ("scope", scope.as_str()),
            ("state", "navree-test"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ];
        if let Some(uri) = &self.redirect_uri {
            query.push(("redirect_uri", uri));
        }
        if let Some(nonce) = &self.nonce {
            query.push(("nonce", nonce));
        }
        let oauth = self.oauth_path.trim_end_matches('/');
        let response = client
            .get(&format!("{oauth}/authorize"))
            .query(&query)
            .send()
            .await;
        assert_eq!(response.status(), 302, "authorize: {}", response.text());
        let location = response.header("location").expect("a Location").to_owned();
        let request = query_param(&location, "request")
            .unwrap_or_else(|| panic!("authorize redirected to {location} without a request"));

        let api = self.api_prefix.trim_end_matches('/');
        let approved = client
            .post(&format!("{api}/oauth/requests/{request}/approve"))
            .bearer(user_token)
            .send()
            .await;
        assert_eq!(approved.status(), 200, "approve: {}", approved.text());
        let redirect_to = approved.json_value()["redirect_to"]
            .as_str()
            .expect("redirect_to")
            .to_owned();
        let code = query_param(&redirect_to, "code")
            .unwrap_or_else(|| panic!("approve redirected to {redirect_to} without a code"));

        let mut form = vec![
            ("grant_type", "authorization_code".to_owned()),
            ("code", code),
            ("code_verifier", verifier),
            ("client_id", self.client_id.clone()),
        ];
        if let Some(secret) = &self.secret {
            form.push(("client_secret", secret.clone()));
        }
        if let Some(uri) = &self.redirect_uri {
            form.push(("redirect_uri", uri.clone()));
        }
        let tokens = client
            .post(&format!("{oauth}/token"))
            .form(&form)
            .send()
            .await;
        assert_eq!(tokens.status(), 200, "token: {}", tokens.text());
        tokens.json()
    }
}

/// The value of query parameter `name` in `url`.
pub fn query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    let query = query.split('#').next().unwrap_or_default();
    form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::pkce_matches;

    #[test]
    fn pkce_pairs_verify() {
        let (verifier, challenge) = pkce_pair();
        assert!(pkce_matches(&verifier, &challenge));
        assert_ne!(pkce_pair().0, verifier);
    }

    #[test]
    fn query_params_are_read() {
        let url = "https://app.test/cb?code=nvc_a%2Bb&state=s#frag";
        assert_eq!(query_param(url, "code").as_deref(), Some("nvc_a+b"));
        assert_eq!(query_param(url, "state").as_deref(), Some("s"));
        assert_eq!(query_param(url, "nope"), None);
        assert_eq!(query_param("https://app.test/cb", "code"), None);
    }
}
