//! [`AuthorizeUrl`]: the provider's authorization URL, built in parts.

use crate::SocialError;

/// The authorization URL of a flow. Navree fills in the client id, the
/// redirect URI, the state, the PKCE challenge, the nonce and the extra
/// scopes and parameters; a [`SocialProvider`](crate::SocialProvider) adds
/// its endpoint and default scopes in
/// [`authorize_url`](crate::SocialProvider::authorize_url):
///
/// ```
/// use navree_social::AuthorizeUrl;
///
/// let url = AuthorizeUrl::default()
///     .endpoint("https://accounts.zoho.com/oauth/v2/auth")
///     .scopes(["AaaServer.profile.Read"])
///     .param("access_type", "offline");
/// assert_eq!(url.endpoint_url(), Some("https://accounts.zoho.com/oauth/v2/auth"));
/// assert_eq!(url.scope_list(), ["AaaServer.profile.Read"]);
/// ```
#[derive(Clone, Debug, Default)]
pub struct AuthorizeUrl {
    endpoint: Option<String>,
    scopes: Vec<String>,
    extra_scopes: Vec<String>,
    separator: Option<String>,
    core: Vec<(String, String)>,
    params: Vec<(String, String)>,
}

impl AuthorizeUrl {
    /// Sets the authorization endpoint.
    pub fn endpoint(mut self, url: impl Into<String>) -> Self {
        self.endpoint = Some(url.into());
        self
    }

    /// Adds the provider's scopes (requested before the extra ones).
    pub fn scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scopes.extend(scopes.into_iter().map(Into::into));
        self
    }

    /// Adds a query parameter (a later value for the same name wins).
    pub fn param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let name = name.into();
        self.params.retain(|(existing, _)| *existing != name);
        self.params.push((name, value.into()));
        self
    }

    /// Joins the scopes with `separator` instead of a space.
    pub fn scope_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = Some(separator.into());
        self
    }

    /// The endpoint, once set.
    pub fn endpoint_url(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// Every scope requested, the provider's first, without duplicates.
    pub fn scope_list(&self) -> Vec<String> {
        let mut all: Vec<String> = Vec::new();
        for scope in self.scopes.iter().chain(&self.extra_scopes) {
            if !scope.is_empty() && !all.contains(scope) {
                all.push(scope.clone());
            }
        }
        all
    }

    /// The value of query parameter `name`, if set.
    pub fn param_value(&self, name: &str) -> Option<&str> {
        self.core
            .iter()
            .chain(&self.params)
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Navree's own parameters, in URL order.
    pub(crate) fn core_param(mut self, name: &str, value: impl Into<String>) -> Self {
        self.core.push((name.to_owned(), value.into()));
        self
    }

    /// Scopes from the configuration and the redirect options.
    pub(crate) fn extra_scopes(mut self, scopes: impl IntoIterator<Item = String>) -> Self {
        self.extra_scopes.extend(scopes);
        self
    }

    /// The full URL: the endpoint, then `response_type`, Navree's
    /// parameters, `scope`, and the extra parameters. Parameters already in
    /// the endpoint's query string are kept.
    pub(crate) fn build(&self, provider: &str) -> Result<String, SocialError> {
        let endpoint = self.endpoint.as_deref().ok_or_else(|| {
            SocialError::Config(format!(
                "provider `{provider}` has no authorization endpoint"
            ))
        })?;
        let mut url = url::Url::parse(endpoint).map_err(|_| {
            SocialError::Config(format!(
                "provider `{provider}`: the authorization endpoint is not a URL"
            ))
        })?;
        let scopes = self.scope_list();
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("response_type", "code");
            let mut core = self.core.iter();
            // client_id and redirect_uri first, then the scope, then the rest.
            for (name, value) in core.by_ref().take(2) {
                query.append_pair(name, value);
            }
            if !scopes.is_empty() {
                query.append_pair(
                    "scope",
                    &scopes.join(self.separator.as_deref().unwrap_or(" ")),
                );
            }
            for (name, value) in core.chain(&self.params) {
                query.append_pair(name, value);
            }
        }
        Ok(url.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_url_has_the_documented_order() {
        let url = AuthorizeUrl::default()
            .core_param("client_id", "Iv1.8a61f9b3a7aba766")
            .core_param("redirect_uri", "https://app.acme.test/auth/callback/github")
            .core_param("state", "nvs1.x")
            .core_param("code_challenge", "E9M")
            .core_param("code_challenge_method", "S256")
            .extra_scopes(["user:email".to_owned(), "repo".to_owned()])
            .endpoint("https://github.com/login/oauth/authorize")
            .scopes(["read:user", "user:email"])
            .param("prompt", "consent")
            .build("github")
            .unwrap();
        assert_eq!(
            url,
            "https://github.com/login/oauth/authorize?response_type=code&client_id=Iv1.8a61f9b3a7aba766\
             &redirect_uri=https%3A%2F%2Fapp.acme.test%2Fauth%2Fcallback%2Fgithub\
             &scope=read%3Auser+user%3Aemail+repo&state=nvs1.x&code_challenge=E9M\
             &code_challenge_method=S256&prompt=consent"
        );
    }

    #[test]
    fn a_missing_endpoint_is_a_configuration_error() {
        assert!(matches!(
            AuthorizeUrl::default().build("zoho"),
            Err(SocialError::Config(_))
        ));
    }

    #[test]
    fn params_replace_and_separators_apply() {
        let url = AuthorizeUrl::default()
            .endpoint("https://idp.test/auth?tenant=1")
            .scopes(["a", "b"])
            .scope_separator(",")
            .param("prompt", "login")
            .param("prompt", "consent");
        assert_eq!(url.param_value("prompt"), Some("consent"));
        assert_eq!(
            url.build("x").unwrap(),
            "https://idp.test/auth?tenant=1&response_type=code&scope=a%2Cb&prompt=consent"
        );
    }
}
