//! The real provider calls: OAuth 2.0 code exchange and refresh, OpenID
//! Connect discovery and ID token verification, and the profile requests of
//! each built-in provider.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, TimeDelta, Utc};
use navree_contracts::Secret;
use navree_crypto::jose::{JwkSet, Jws};
use navree_http_client::{HttpClientError, Response};
use serde_json::{Map, Value, json};

use crate::provider::{
    Endpoints, Issuer, Kind, MICROSOFT_CONSUMERS_TENANT, ProfileSource, ProviderEntry,
};
use crate::runtime::Runtime;
use crate::state::StatePayload;
use crate::{AuthorizeUrl, ProviderToken, SocialCallback, SocialError, SocialUser};

/// Clock skew allowed on ID token times, in seconds.
const LEEWAY_SECS: i64 = 60;

/// Lifetime of Sign in with Apple's client secret JWT, in seconds.
const APPLE_SECRET_TTL_SECS: i64 = 300;

/// An outgoing request failed: the provider is unreachable.
fn unavailable(entry: &ProviderEntry) -> impl FnOnce(HttpClientError) -> SocialError + '_ {
    move |error| SocialError::ProviderUnavailable {
        provider: entry.name.clone(),
        message: error.to_string(),
    }
}

fn refused(entry: &ProviderEntry, message: impl Into<String>) -> SocialError {
    SocialError::Provider {
        provider: entry.name.clone(),
        message: message.into(),
    }
}

fn invalid_id_token(entry: &ProviderEntry, reason: impl Into<String>) -> SocialError {
    SocialError::InvalidIdToken {
        provider: entry.name.clone(),
        reason: reason.into(),
    }
}

/// The provider's endpoints; an `oidc` provider's discovery document is
/// read once (and again after a failure).
pub(crate) async fn endpoints(
    rt: &Runtime,
    entry: &ProviderEntry,
) -> Result<Endpoints, SocialError> {
    if entry.kind != Kind::Oidc {
        return Ok(entry.static_endpoints());
    }
    let discovery = entry
        .discovery
        .get_or_try_init(|| async {
            let response = rt
                .http
                .get(entry.discovery_url())
                .header("accept", "application/json")
                .send()
                .await
                .map_err(unavailable(entry))?;
            if !response.is_success() {
                return Err(SocialError::ProviderUnavailable {
                    provider: entry.name.clone(),
                    message: format!("discovery answered {}", response.status().as_u16()),
                });
            }
            let discovery: crate::provider::Discovery =
                response.json().map_err(|_| SocialError::ProviderUnavailable {
                    provider: entry.name.clone(),
                    message: "the discovery document is not valid".to_owned(),
                })?;
            let configured = entry.config.issuer.as_deref().unwrap_or_default();
            if discovery.issuer.trim_end_matches('/') != configured.trim_end_matches('/') {
                return Err(SocialError::Config(format!(
                    "social.providers.{}.issuer is `{configured}` but the discovery document names `{}`",
                    entry.key, discovery.issuer
                )));
            }
            Ok(discovery)
        })
        .await?;
    Ok(entry.endpoints_from(discovery))
}

/// Adds a built-in provider's endpoint, scopes and fixed parameters, or
/// lets a custom provider do it.
pub(crate) fn decorate(
    entry: &ProviderEntry,
    endpoints: &Endpoints,
    url: AuthorizeUrl,
) -> AuthorizeUrl {
    if let Some(custom) = &entry.custom {
        return custom.authorize_url(&entry.config, url);
    }
    let mut url = url
        .endpoint(endpoints.authorize.clone())
        .scopes(entry.kind.default_scopes().iter().copied());
    match entry.kind {
        Kind::Google => {
            if let Some(domain) = &entry.config.hosted_domain {
                url = url.param("hd", domain.clone());
            }
        }
        // Apple sends the name and email only with a form post.
        Kind::Apple => url = url.param("response_mode", "form_post"),
        Kind::Slack => {
            if let Some(team) = &entry.config.team {
                url = url.param("team", team.clone());
            }
        }
        _ => {}
    }
    url
}

/// The client secret to send: Apple's freshly signed JWT, or the
/// configured one.
fn client_secret(rt: &Runtime, entry: &ProviderEntry) -> Result<Option<String>, SocialError> {
    if let Some(key) = &entry.apple_key {
        let now = rt.now().timestamp();
        let audience = entry
            .config
            .url
            .as_deref()
            .map_or("https://appleid.apple.com", |u| u.trim_end_matches('/'));
        let claims = json!({
            "iss": entry.config.team_id,
            "iat": now,
            "exp": now + APPLE_SECRET_TTL_SECS,
            "aud": audience,
            "sub": entry.config.client_id,
        });
        // `{"alg":"ES256","kid":"<key_id>"}`, as Apple expects.
        return key
            .sign_json(&key.header(), &claims)
            .map(Some)
            .map_err(|error| {
                SocialError::Config(format!("cannot sign Apple's client secret: {error}"))
            });
    }
    Ok(entry.config.secret().map(str::to_owned))
}

/// POSTs `form` to the token endpoint with the client's credentials.
async fn token_request(
    rt: &Runtime,
    entry: &ProviderEntry,
    endpoints: &Endpoints,
    mut form: Vec<(&'static str, String)>,
) -> Result<Response, SocialError> {
    let mut request = rt
        .http
        .post(&endpoints.token)
        .header("accept", "application/json");
    form.push(("client_id", entry.config.client_id.clone()));
    if let Some(secret) = client_secret(rt, entry)? {
        if endpoints.basic_auth {
            let encode =
                |v: &str| form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>();
            let credentials = format!("{}:{}", encode(&entry.config.client_id), encode(&secret));
            request = request.header(
                "authorization",
                format!("Basic {}", STANDARD.encode(credentials)),
            );
        } else {
            form.push(("client_secret", secret));
        }
    }
    request.form(&form).send().await.map_err(unavailable(entry))
}

/// Exchanges the authorization code for tokens.
pub(crate) async fn exchange(
    rt: &Runtime,
    entry: &ProviderEntry,
    code: &str,
    state: &StatePayload,
) -> Result<ProviderToken, SocialError> {
    let endpoints = endpoints(rt, entry).await?;
    let mut form = vec![
        ("grant_type", "authorization_code".to_owned()),
        ("code", code.to_owned()),
        ("redirect_uri", rt.redirect_uri(&entry.key)),
    ];
    if entry.config.pkce {
        form.push(("code_verifier", state.verifier.clone()));
    }
    let response = token_request(rt, entry, &endpoints, form).await?;
    let body = read_token_response(entry, &response)?;
    parse_token(entry, &body, rt.now(), None)
}

/// Exchanges a refresh token for a new access token:
/// [`SocialError::Reauthorize`] when the provider refuses it.
pub(crate) async fn refresh(
    rt: &Runtime,
    entry: &ProviderEntry,
    refresh_token: &str,
) -> Result<ProviderToken, SocialError> {
    let endpoints = endpoints(rt, entry).await?;
    let form = vec![
        ("grant_type", "refresh_token".to_owned()),
        ("refresh_token", refresh_token.to_owned()),
    ];
    let response = token_request(rt, entry, &endpoints, form).await?;
    let body = read_token_response(entry, &response).map_err(|error| match error {
        SocialError::Provider { provider, .. } => SocialError::Reauthorize { provider },
        other => other,
    })?;
    parse_token(entry, &body, rt.now(), Some(refresh_token))
}

/// The token response's JSON, or the provider's refusal.
fn read_token_response(entry: &ProviderEntry, response: &Response) -> Result<Value, SocialError> {
    let status = response.status();
    if status.is_server_error() {
        return Err(SocialError::ProviderUnavailable {
            provider: entry.name.clone(),
            message: format!("the token endpoint answered {}", status.as_u16()),
        });
    }
    let body: Value = response.json().unwrap_or(Value::Null);
    // Some providers answer 200 with an error (GitHub) or `ok: false` (Slack).
    if let Some(error) = body.get("error").and_then(Value::as_str) {
        return Err(refused(entry, error));
    }
    if body.get("ok") == Some(&Value::Bool(false)) {
        return Err(refused(entry, "the token request was refused"));
    }
    if !status.is_success() {
        return Err(refused(
            entry,
            format!("the token endpoint answered {}", status.as_u16()),
        ));
    }
    Ok(body)
}

fn parse_token(
    entry: &ProviderEntry,
    body: &Value,
    now: DateTime<Utc>,
    previous_refresh: Option<&str>,
) -> Result<ProviderToken, SocialError> {
    let access = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| refused(entry, "no access token in the token response"))?;
    let mut token = ProviderToken::new(access);
    let expires_in = match body.get("expires_in") {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    };
    if let Some(secs) = expires_in.filter(|s| *s > 0) {
        token.expires_at = now.checked_add_signed(TimeDelta::seconds(secs));
    }
    match body.get("refresh_token").and_then(Value::as_str) {
        Some(refresh) if !refresh.is_empty() => token = token.refresh_token(refresh),
        // A refresh that returns no new refresh token keeps the old one.
        _ => {
            if let Some(previous) = previous_refresh {
                token = token.refresh_token(previous);
            }
        }
    }
    if let Some(id_token) = body.get("id_token").and_then(Value::as_str) {
        token.id_token = Some(Secret::new(id_token.to_owned()));
    }
    if let Some(scope) = body.get("scope").and_then(Value::as_str) {
        token.scopes = scope
            .split([' ', ','])
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
    }
    Ok(token)
}

/// GETs `url` with the access token: the JSON body, or the provider's
/// refusal.
async fn get_json(
    rt: &Runtime,
    entry: &ProviderEntry,
    url: &str,
    token: &ProviderToken,
    accept: &str,
) -> Result<Value, SocialError> {
    let response = rt
        .http
        .get(url)
        .bearer(token.access_token.expose())
        .header("accept", accept)
        .send()
        .await
        .map_err(unavailable(entry))?;
    if response.status().is_server_error() {
        return Err(SocialError::ProviderUnavailable {
            provider: entry.name.clone(),
            message: format!(
                "the profile request answered {}",
                response.status().as_u16()
            ),
        });
    }
    if !response.is_success() {
        return Err(refused(
            entry,
            format!(
                "the profile request answered {}",
                response.status().as_u16()
            ),
        ));
    }
    response
        .json()
        .map_err(|_| refused(entry, "the profile is not JSON"))
}

/// Reads the user of a token: verifies the ID token, or calls the profile
/// API, then applies the provider's own rules (Google Workspace domain,
/// Microsoft tenant, Slack workspace).
pub(crate) async fn profile(
    rt: &Runtime,
    entry: &ProviderEntry,
    token: ProviderToken,
    state: &StatePayload,
    callback: &SocialCallback,
) -> Result<SocialUser, SocialError> {
    let endpoints = endpoints(rt, entry).await?;
    let user = match entry.kind.profile_source() {
        ProfileSource::IdToken => {
            let id_token = token
                .id_token
                .as_ref()
                .ok_or_else(|| refused(entry, "no ID token in the token response"))?;
            let mut claims =
                verify_id_token(rt, entry, &endpoints, id_token.expose(), &state.nonce).await?;
            if claims.get("email").is_none()
                && let Some(userinfo) = &endpoints.userinfo
            {
                let info = get_json(rt, entry, userinfo, &token, "application/json").await?;
                merge_userinfo(&mut claims, &info);
            }
            id_token_user(entry, claims, callback)?
        }
        ProfileSource::UserInfo => {
            let url = endpoints.userinfo.as_deref().ok_or_else(|| {
                SocialError::Config(format!("{} has no userinfo endpoint", entry.name))
            })?;
            let info = get_json(rt, entry, url, &token, "application/json").await?;
            if info.get("ok") == Some(&Value::Bool(false)) {
                return Err(refused(entry, "the profile request was refused"));
            }
            if entry.kind == Kind::Slack
                && let Some(team) = &entry.config.team
                && info
                    .get("https://slack.com/team_id")
                    .and_then(Value::as_str)
                    != Some(team)
            {
                return Err(SocialError::NotAllowed(format!(
                    "Sign in with an account of the {} Slack workspace",
                    team
                )));
            }
            standard_user(entry, info)?
        }
        ProfileSource::GitHub => github_user(rt, entry, &endpoints, &token).await?,
        ProfileSource::Discord => {
            let url = endpoints.userinfo.as_deref().unwrap_or_default();
            discord_user(
                entry,
                get_json(rt, entry, url, &token, "application/json").await?,
            )?
        }
        ProfileSource::X => {
            let mut fields = "profile_image_url".to_owned();
            if token.scopes.iter().any(|s| s == "users.email") {
                fields.push_str(",confirmed_email");
            }
            let base = endpoints.userinfo.as_deref().unwrap_or_default();
            let url = format!("{base}?user.fields={fields}");
            x_user(
                entry,
                get_json(rt, entry, &url, &token, "application/json").await?,
            )?
        }
        ProfileSource::Custom => {
            let custom = entry
                .custom
                .as_ref()
                .ok_or_else(|| SocialError::Config(format!("{} is not registered", entry.key)))?;
            custom.user(&entry.config, token.clone()).await?
        }
    };
    let mut user = user;
    user.provider = entry.key.clone();
    if user.token.is_none() {
        user.token = Some(token);
    }
    Ok(user)
}

/// Verifies an ID token: signature (against the provider's key set),
/// issuer, audience, expiry and nonce. Returns the claims.
pub(crate) async fn verify_id_token(
    rt: &Runtime,
    entry: &ProviderEntry,
    endpoints: &Endpoints,
    id_token: &str,
    nonce: &str,
) -> Result<Value, SocialError> {
    let jws = Jws::parse(id_token).map_err(|e| invalid_id_token(entry, e.to_string()))?;
    let claims: Map<String, Value> = jws.claims().map_err(|_| {
        invalid_id_token(
            entry,
            "malformed token: the claims are not a JSON object".to_owned(),
        )
    })?;
    let jwks_url = endpoints
        .jwks
        .as_deref()
        .ok_or_else(|| SocialError::Config(format!("{} has no key set", entry.name)))?;
    let keys = rt
        .jwks
        .get(jwks_url, jws.header().kid.as_deref(), || async {
            let response = rt
                .http
                .get(jwks_url)
                .header("accept", "application/json")
                .send()
                .await
                .map_err(unavailable(entry))?;
            if !response.is_success() {
                return Err(SocialError::ProviderUnavailable {
                    provider: entry.name.clone(),
                    message: format!("the key set answered {}", response.status().as_u16()),
                });
            }
            response
                .json::<JwkSet>()
                .map_err(|_| SocialError::ProviderUnavailable {
                    provider: entry.name.clone(),
                    message: "the key set is not valid".to_owned(),
                })
        })
        .await?;
    // RS256/384/512, PS256/384/512, ES256, ES384 or EdDSA: a key set never
    // verifies `none` or an HMAC token.
    jws.verify_with(&keys)
        .map_err(|e| invalid_id_token(entry, e.to_string()))?;
    let claims = Value::Object(claims);
    check_claims(
        entry,
        &endpoints.issuer,
        &claims,
        nonce,
        rt.now().timestamp(),
    )?;
    Ok(claims)
}

/// The issuer, audience, time and nonce checks of an ID token.
pub(crate) fn check_claims(
    entry: &ProviderEntry,
    issuer: &Issuer,
    claims: &Value,
    nonce: &str,
    now: i64,
) -> Result<(), SocialError> {
    let iss = claims
        .get("iss")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let issuer_ok = match issuer {
        Issuer::Exact(allowed) => allowed.iter().any(|a| a == iss),
        Issuer::MicrosoftTenant(base) => claims
            .get("tid")
            .and_then(Value::as_str)
            .is_some_and(|tid| iss == format!("{base}/{tid}/v2.0")),
    };
    if !issuer_ok {
        return Err(invalid_id_token(
            entry,
            format!("unexpected issuer `{iss}`"),
        ));
    }
    let client_id = entry.config.client_id.as_str();
    let audience_ok = match claims.get("aud") {
        Some(Value::String(aud)) => aud == client_id,
        Some(Value::Array(auds)) => {
            auds.iter().any(|a| a.as_str() == Some(client_id))
                && (auds.len() == 1 || claims.get("azp").and_then(Value::as_str) == Some(client_id))
        }
        _ => false,
    };
    if !audience_ok {
        return Err(invalid_id_token(entry, "the token is for another client"));
    }
    let exp = claims.get("exp").and_then(Value::as_i64).unwrap_or(0);
    if exp + LEEWAY_SECS <= now {
        return Err(invalid_id_token(entry, "the token has expired"));
    }
    if let Some(nbf) = claims.get("nbf").and_then(Value::as_i64)
        && nbf - LEEWAY_SECS > now
    {
        return Err(invalid_id_token(entry, "the token is not valid yet"));
    }
    if claims.get("nonce").and_then(Value::as_str) != Some(nonce) {
        return Err(invalid_id_token(entry, "the nonce does not match"));
    }
    Ok(())
}

/// Adds the userinfo members the ID token lacks (only for the same `sub`).
fn merge_userinfo(claims: &mut Value, info: &Value) {
    if info.get("sub") != claims.get("sub") {
        return;
    }
    if let (Some(claims), Some(info)) = (claims.as_object_mut(), info.as_object()) {
        for (key, value) in info {
            claims.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
}

/// `true`, or the string `"true"` (Apple).
fn truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "true",
        _ => false,
    }
}

fn str_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

/// A user from standard OpenID Connect claims (`sub`, `name`, `email`,
/// `email_verified`, `picture`, `preferred_username`).
pub(crate) fn standard_user(
    entry: &ProviderEntry,
    claims: Value,
) -> Result<SocialUser, SocialError> {
    let sub = str_of(&claims, "sub").ok_or_else(|| refused(entry, "the profile has no `sub`"))?;
    let name = str_of(&claims, "name").map(str::to_owned).or_else(|| {
        let parts: Vec<&str> = [
            str_of(&claims, "given_name"),
            str_of(&claims, "family_name"),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    });
    let mut user = SocialUser::new(entry.key.clone(), sub).email(
        str_of(&claims, "email"),
        truthy(claims.get("email_verified")),
    );
    if let Some(name) = name {
        user = user.name(name);
    }
    if let Some(picture) = str_of(&claims, "picture") {
        user = user.avatar_url(picture);
    }
    if let Some(nickname) = str_of(&claims, "preferred_username").or(str_of(&claims, "nickname")) {
        user = user.nickname(nickname);
    }
    Ok(user.raw(claims))
}

/// The user of a verified ID token, with each provider's rules.
fn id_token_user(
    entry: &ProviderEntry,
    claims: Value,
    callback: &SocialCallback,
) -> Result<SocialUser, SocialError> {
    match entry.kind {
        Kind::Google => {
            if let Some(domain) = &entry.config.hosted_domain
                && str_of(&claims, "hd") != Some(domain.as_str())
            {
                return Err(SocialError::NotAllowed(format!(
                    "Sign in with a {domain} Google account"
                )));
            }
            standard_user(entry, claims)
        }
        Kind::Microsoft => {
            let tid = str_of(&claims, "tid").unwrap_or_default().to_owned();
            match entry.config.tenant.as_deref().unwrap_or("common") {
                "common" => {}
                "organizations" if tid == MICROSOFT_CONSUMERS_TENANT => {
                    return Err(SocialError::NotAllowed(
                        "Sign in with a work or school Microsoft account".to_owned(),
                    ));
                }
                "consumers" if tid != MICROSOFT_CONSUMERS_TENANT => {
                    return Err(SocialError::NotAllowed(
                        "Sign in with a personal Microsoft account".to_owned(),
                    ));
                }
                "organizations" | "consumers" => {}
                tenant => {
                    if uuid::Uuid::parse_str(tenant).is_ok() && !tid.eq_ignore_ascii_case(tenant) {
                        return Err(SocialError::NotAllowed(
                            "Sign in with an account of your organization".to_owned(),
                        ));
                    }
                }
            }
            // Microsoft's `email` claim is set by the account's owner or
            // tenant admin; it counts as verified only with `xms_edov`.
            let verified = truthy(claims.get("xms_edov"));
            let mut user = standard_user(entry, claims)?;
            user.email_verified = verified && user.email.is_some();
            Ok(user)
        }
        Kind::Apple => {
            let mut user = standard_user(entry, claims)?;
            if user.name.is_none()
                && let Some(name) = apple_name(callback.user.as_ref())
            {
                user = user.name(name);
            }
            Ok(user)
        }
        _ => standard_user(entry, claims),
    }
}

/// The name in Apple's first-sign-in `user` field (a JSON string or
/// object: `{"name": {"firstName", "lastName"}, "email"}`).
pub(crate) fn apple_name(user: Option<&Value>) -> Option<String> {
    let parsed;
    let user = match user? {
        Value::String(raw) => {
            parsed = serde_json::from_str::<Value>(raw).ok()?;
            &parsed
        }
        other => other,
    };
    let name = user.get("name")?;
    let parts: Vec<&str> = [str_of(name, "firstName"), str_of(name, "lastName")]
        .into_iter()
        .flatten()
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// GitHub: `/user` plus the primary verified address from `/user/emails`.
async fn github_user(
    rt: &Runtime,
    entry: &ProviderEntry,
    endpoints: &Endpoints,
    token: &ProviderToken,
) -> Result<SocialUser, SocialError> {
    let accept = "application/vnd.github+json";
    let user_url = endpoints.userinfo.as_deref().unwrap_or_default();
    let profile = get_json(rt, entry, user_url, token, accept).await?;
    let emails = match get_json(rt, entry, &format!("{user_url}/emails"), token, accept).await {
        Ok(Value::Array(emails)) => emails,
        // Without the `user:email` scope the list is refused; fall back
        // to the public (unverified) address.
        Ok(_) | Err(SocialError::Provider { .. }) => Vec::new(),
        Err(error) => return Err(error),
    };
    github_profile(entry, profile, &emails)
}

pub(crate) fn github_profile(
    entry: &ProviderEntry,
    mut profile: Value,
    emails: &[Value],
) -> Result<SocialUser, SocialError> {
    let id = match profile.get("id") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => return Err(refused(entry, "the profile has no `id`")),
    };
    let verified = |e: &&Value| e.get("verified") == Some(&Value::Bool(true));
    let primary = |e: &&Value| e.get("primary") == Some(&Value::Bool(true));
    let (email, email_verified) = match emails
        .iter()
        .find(|e| primary(e) && verified(e))
        .or_else(|| emails.iter().find(verified))
    {
        Some(found) => (str_of(found, "email").map(str::to_owned), true),
        None => (str_of(&profile, "email").map(str::to_owned), false),
    };
    let login = str_of(&profile, "login").map(str::to_owned);
    let name = str_of(&profile, "name")
        .map(str::to_owned)
        .or(login.clone());
    let mut user = SocialUser::new(entry.key.clone(), id).email(email.as_deref(), email_verified);
    if let Some(name) = name {
        user = user.name(name);
    }
    if let Some(login) = login {
        user = user.nickname(login);
    }
    if let Some(avatar) = str_of(&profile, "avatar_url") {
        user = user.avatar_url(avatar);
    }
    if let Some(object) = profile.as_object_mut() {
        object.insert("emails".to_owned(), Value::Array(emails.to_vec()));
    }
    Ok(user.raw(profile))
}

/// Discord: `users/@me` (`verified` is the email's status).
pub(crate) fn discord_user(
    entry: &ProviderEntry,
    profile: Value,
) -> Result<SocialUser, SocialError> {
    let id = str_of(&profile, "id").ok_or_else(|| refused(entry, "the profile has no `id`"))?;
    let mut user = SocialUser::new(entry.key.clone(), id)
        .email(str_of(&profile, "email"), truthy(profile.get("verified")));
    if let Some(name) = str_of(&profile, "global_name").or(str_of(&profile, "username")) {
        user = user.name(name);
    }
    if let Some(username) = str_of(&profile, "username") {
        user = user.nickname(username);
    }
    if let Some(avatar) = str_of(&profile, "avatar") {
        user = user.avatar_url(format!(
            "https://cdn.discordapp.com/avatars/{id}/{avatar}.png"
        ));
    }
    Ok(user.raw(profile))
}

/// X: `users/me` (`data`); `confirmed_email` with the `users.email` scope.
pub(crate) fn x_user(entry: &ProviderEntry, body: Value) -> Result<SocialUser, SocialError> {
    let data = body
        .get("data")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let id = str_of(&data, "id").ok_or_else(|| refused(entry, "the profile has no `id`"))?;
    let email = str_of(&data, "confirmed_email");
    let mut user = SocialUser::new(entry.key.clone(), id).email(email, email.is_some());
    if let Some(name) = str_of(&data, "name") {
        user = user.name(name);
    }
    if let Some(username) = str_of(&data, "username") {
        user = user.nickname(username);
    }
    if let Some(avatar) = str_of(&data, "profile_image_url") {
        user = user.avatar_url(avatar);
    }
    Ok(user.raw(data))
}

#[cfg(test)]
mod tests {
    use crate::ProviderConfig;

    use super::*;

    fn entry(kind: &str, f: impl FnOnce(&mut ProviderConfig)) -> ProviderEntry {
        let mut config = ProviderConfig {
            key: kind.to_owned(),
            client_id: "client".to_owned(),
            ..ProviderConfig::default()
        };
        f(&mut config);
        ProviderEntry::new(config, None).unwrap()
    }

    fn claims(extra: Value) -> Value {
        let mut claims = json!({
            "iss": "https://accounts.google.com",
            "aud": "client",
            "sub": "1",
            "exp": 2_000,
            "nonce": "n",
        });
        for (k, v) in extra.as_object().unwrap() {
            claims[k] = v.clone();
        }
        claims
    }

    #[test]
    fn id_token_claims_are_checked() {
        let google = entry("google", |_| {});
        let issuer = google.static_endpoints().issuer;
        let check = |c: &Value, now| check_claims(&google, &issuer, c, "n", now);
        assert!(check(&claims(json!({})), 1_000).is_ok());
        assert!(check(&claims(json!({ "iss": "accounts.google.com" })), 1_000).is_ok());
        assert!(check(&claims(json!({ "iss": "https://evil.test" })), 1_000).is_err());
        assert!(check(&claims(json!({ "aud": "other" })), 1_000).is_err());
        assert!(check(&claims(json!({ "aud": ["client"] })), 1_000).is_ok());
        assert!(
            check(&claims(json!({ "aud": ["client", "other"] })), 1_000).is_err(),
            "several audiences need azp"
        );
        assert!(
            check(
                &claims(json!({ "aud": ["client", "other"], "azp": "client" })),
                1_000
            )
            .is_ok()
        );
        assert!(
            check(&claims(json!({})), 2_000 + LEEWAY_SECS).is_err(),
            "expired"
        );
        assert!(
            check(&claims(json!({ "nbf": 1_500 })), 1_000).is_err(),
            "not yet valid"
        );
        assert!(check(&claims(json!({ "nonce": "other" })), 1_000).is_err());
        assert!(check(&claims(json!({ "nonce": null })), 1_000).is_err());
    }

    #[test]
    fn microsoft_issuers_carry_the_tenant() {
        let microsoft = entry("microsoft", |_| {});
        let issuer = microsoft.static_endpoints().issuer;
        let tid = "72f988bf-86f1-41af-91ab-2d7cd011db47";
        let good = claims(json!({
            "iss": format!("https://login.microsoftonline.com/{tid}/v2.0"),
            "tid": tid,
        }));
        assert!(check_claims(&microsoft, &issuer, &good, "n", 1_000).is_ok());
        let other = claims(json!({
            "iss": format!("https://login.microsoftonline.com/{tid}/v2.0"),
            "tid": MICROSOFT_CONSUMERS_TENANT,
        }));
        assert!(check_claims(&microsoft, &issuer, &other, "n", 1_000).is_err());
    }

    #[test]
    fn microsoft_tenants_and_unverified_emails() {
        let organizations = entry("microsoft", |c| c.tenant = Some("organizations".into()));
        let personal =
            json!({ "sub": "1", "tid": MICROSOFT_CONSUMERS_TENANT, "email": "a@outlook.com" });
        assert!(matches!(
            id_token_user(&organizations, personal.clone(), &SocialCallback::default()),
            Err(SocialError::NotAllowed(_))
        ));
        let common = entry("microsoft", |_| {});
        let user = id_token_user(&common, personal, &SocialCallback::default()).unwrap();
        assert_eq!(user.email.as_deref(), Some("a@outlook.com"));
        assert!(
            !user.email_verified,
            "Microsoft emails are not verified without xms_edov"
        );
        let edov = json!({ "sub": "1", "tid": "t", "email": "a@acme.test", "xms_edov": true, "email_verified": false });
        assert!(
            id_token_user(&common, edov, &SocialCallback::default())
                .unwrap()
                .email_verified
        );
        let one_tenant = entry("microsoft", |c| {
            c.tenant = Some("72f988bf-86f1-41af-91ab-2d7cd011db47".into())
        });
        let stranger = json!({ "sub": "1", "tid": "00000000-0000-0000-0000-000000000001" });
        assert!(id_token_user(&one_tenant, stranger, &SocialCallback::default()).is_err());
    }

    #[test]
    fn google_workspace_domains_are_enforced() {
        let google = entry("google", |c| c.hosted_domain = Some("acme.test".into()));
        let outsider = json!({ "sub": "1", "email": "a@gmail.com", "email_verified": true });
        assert!(matches!(
            id_token_user(&google, outsider, &SocialCallback::default()),
            Err(SocialError::NotAllowed(_))
        ));
        let member = json!({ "sub": "1", "hd": "acme.test", "email": "a@acme.test", "email_verified": true, "name": "Anu", "picture": "https://p/1" });
        let user = id_token_user(&google, member, &SocialCallback::default()).unwrap();
        assert_eq!(user.name.as_deref(), Some("Anu"));
        assert_eq!(user.avatar_url.as_deref(), Some("https://p/1"));
        assert!(user.email_verified);
    }

    #[test]
    fn apple_names_come_from_the_first_callback() {
        assert_eq!(
            apple_name(Some(&json!(
                "{\"name\":{\"firstName\":\"Anu\",\"lastName\":\"Lakhotia\"}}"
            ))),
            Some("Anu Lakhotia".to_owned())
        );
        assert_eq!(
            apple_name(Some(&json!({ "name": { "firstName": "Anu" } }))),
            Some("Anu".to_owned())
        );
        assert_eq!(apple_name(Some(&json!("not json"))), None);
        assert_eq!(apple_name(None), None);
        let apple_claims = json!({ "sub": "001.abc", "email": "x@privaterelay.appleid.com", "email_verified": "true" });
        let callback = SocialCallback {
            user: Some(json!({ "name": { "firstName": "Anu", "lastName": "Lakhotia" } })),
            ..SocialCallback::default()
        };
        let apple = entry("apple", |c| {
            c.team_id = Some("T".into());
            c.key_id = Some("K".into());
            c.private_key = Some(Secret::new(
                include_str!("../tests/fixtures/apple-authkey.p8").to_owned(),
            ));
        });
        let user = id_token_user(&apple, apple_claims, &callback).unwrap();
        assert_eq!(user.name.as_deref(), Some("Anu Lakhotia"));
        assert!(user.email_verified, "Apple's \"true\" string counts");
    }

    #[test]
    fn github_prefers_the_primary_verified_email() {
        let github = entry("github", |_| {});
        let profile = json!({ "id": 583231, "login": "anu", "name": null, "email": "public@anu.test", "avatar_url": "https://a/1" });
        let emails = [
            json!({ "email": "old@anu.test", "primary": false, "verified": true }),
            json!({ "email": "anu@example.com", "primary": true, "verified": true }),
        ];
        let user = github_profile(&github, profile.clone(), &emails).unwrap();
        assert_eq!(user.id, "583231");
        assert_eq!(
            user.name.as_deref(),
            Some("anu"),
            "the login when there is no name"
        );
        assert_eq!(user.email.as_deref(), Some("anu@example.com"));
        assert!(user.email_verified);
        let unverified =
            [json!({ "email": "anu@example.com", "primary": true, "verified": false })];
        let user = github_profile(&github, profile.clone(), &unverified).unwrap();
        assert_eq!(user.email.as_deref(), Some("public@anu.test"));
        assert!(!user.email_verified);
    }

    #[test]
    fn discord_and_x_profiles() {
        let discord = entry("discord", |_| {});
        let user = discord_user(
            &discord,
            json!({ "id": "80351110224678912", "username": "nelly", "global_name": "Nelly", "email": "nelly@discord.test", "verified": true, "avatar": "8342729096ea3675442027381ff50dfe" }),
        )
        .unwrap();
        assert_eq!(user.name.as_deref(), Some("Nelly"));
        assert!(user.email_verified);
        assert_eq!(
            user.avatar_url.as_deref(),
            Some(
                "https://cdn.discordapp.com/avatars/80351110224678912/8342729096ea3675442027381ff50dfe.png"
            )
        );
        let x = entry("x", |_| {});
        let user = x_user(
            &x,
            json!({ "data": { "id": "2244994945", "name": "X Dev", "username": "xdevelopers" } }),
        )
        .unwrap();
        assert_eq!(user.nickname.as_deref(), Some("xdevelopers"));
        assert_eq!(user.email, None);
    }

    #[test]
    fn token_responses_parse_numbers_strings_and_scopes() {
        let github = entry("github", |_| {});
        let now = DateTime::<Utc>::from_timestamp(1_000, 0).unwrap();
        let token = parse_token(
            &github,
            &json!({ "access_token": "a", "expires_in": "3600", "scope": "read:user,user:email" }),
            now,
            Some("old-refresh"),
        )
        .unwrap();
        assert_eq!(token.expires_at.unwrap().timestamp(), 4_600);
        assert_eq!(token.scopes, ["read:user", "user:email"]);
        assert_eq!(token.refresh_token.unwrap().expose(), "old-refresh");
        assert!(parse_token(&github, &json!({}), now, None).is_err());
    }
}
