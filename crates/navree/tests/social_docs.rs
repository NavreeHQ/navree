//! The examples of `docs/packages/social-sign-in`, through the facade with
//! the `social` feature (`cargo test -p navree --features social`): the
//! testing example as written, the custom controller (macros) and the
//! custom provider.
#![cfg(feature = "social")]

use navree::auth::{ClientMeta, LoginOutcome};
use navree::prelude::*;
use navree::social::{Social, SocialCallback, SocialRedirect, SocialUser};
use navree::testing::TestApp;
use serde_json::json;

// ---------------------------------------------------------------------------
// "Testing": the page's test, as written.

#[tokio::test]
async fn a_new_github_user_gets_an_account_and_tokens() {
    let (social, fake) = Social::fake();
    fake.on_code(
        "github",
        "code-ada",
        SocialUser::new("github", "583231")
            .name("Ada Lovelace")
            .email(Some("ada@example.com"), true),
    );

    let app = TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config(
            "social.redirect_base",
            "https://app.acme.test/auth/callback",
        )
        .config("social.providers.github.client_id", "test")
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(SocialModule::from_config())
        .override_value(Arc::new(social))
        .build()
        .await;

    let redirect = app
        .client()
        .get("/api/auth/social/github/redirect")
        .send()
        .await
        .assert_status(200);
    let state = redirect.json_value()["state"].as_str().unwrap().to_owned();

    app.client()
        .post("/api/auth/social/github/callback")
        .json(&json!({ "code": "code-ada", "state": state }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("ada@example.com"));

    // The state works once.
    app.client()
        .post("/api/auth/social/github/callback")
        .json(&json!({ "code": "code-ada", "state": state }))
        .send()
        .await
        .assert_error(400, "Invalid or expired state");
}

// ---------------------------------------------------------------------------
// "Using the Social service": the page's controller, as written.

/// The page's single sign-on controller.
#[injectable]
pub struct SsoController {
    social: Arc<Social>,
}

#[controller("/sso", version = "1")]
impl SsoController {
    /// `GET /api/v1/sso/okta`: where to send the browser.
    #[get("/okta")]
    #[public]
    async fn redirect(&self) -> Result<Json<SocialRedirect>> {
        Ok(Json(self.social.redirect("okta").await?))
    }

    /// `POST /api/v1/sso/okta`: `{code, state}` from the callback page.
    #[post("/okta")]
    #[public]
    async fn callback(
        &self,
        meta: ClientMeta,
        Json(callback): Json<SocialCallback>,
    ) -> Result<Json<serde_json::Value>> {
        let profile = self.social.user("okta", &callback).await?;
        let groups = profile.raw["groups"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !groups.iter().any(|g| g == "crm-users") {
            return Err(AppError::forbidden().with_detail("Ask IT for access to the CRM"));
        }
        match self.social.sign_in(&profile, &meta).await? {
            LoginOutcome::Authenticated { user, tokens } => Ok(Json(serde_json::json!({
                "user_id": user.id,
                "access_token": tokens.access_token,
                "refresh_token": tokens.refresh_token,
            }))),
            LoginOutcome::TwoFactorRequired {
                challenge_token, ..
            } => Ok(Json(serde_json::json!({
                "two_factor_required": true,
                "challenge_token": challenge_token,
            }))),
        }
    }
}

/// Mounts [`SsoController`].
#[module(controllers = [SsoController])]
pub struct SsoModule;

#[tokio::test]
async fn the_sso_controller_signs_in_crm_users_only() {
    let (social, fake) = Social::fake();
    let okta = |id: &str, groups: serde_json::Value| {
        SocialUser::new("okta", id)
            .name("Ada Lovelace")
            .email(Some(&format!("{id}@acme.test")), true)
            .raw(json!({ "sub": id, "groups": groups }))
    };
    fake.on_code("okta", "crm", okta("ada", json!(["crm-users"])));
    fake.on_code("okta", "other", okta("grace", json!(["finance"])));
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("social.redirect_base", "https://app.acme.test/auth/callback")
        .config(
            "social.providers.okta",
            json!({ "type": "oidc", "name": "Okta", "issuer": "https://acme.okta.com", "client_id": "0oa8f2kq1xYz3AbC4d5e" }),
        )
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(SocialModule::from_config())
        .module(SsoModule)
        .override_value(Arc::new(social))
        .build()
        .await;

    let state = |app: &TestApp| {
        let client = app.client();
        async move {
            client
                .get("/api/v1/sso/okta")
                .send()
                .await
                .assert_status(200)
                .json_value()["state"]
                .as_str()
                .unwrap()
                .to_owned()
        }
    };
    let ada = state(&app).await;
    let body = app
        .client()
        .post("/api/v1/sso/okta")
        .json(&json!({ "code": "crm", "state": ada }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(body["access_token"].is_string());

    let grace = state(&app).await;
    app.client()
        .post("/api/v1/sso/okta")
        .json(&json!({ "code": "other", "state": grace }))
        .send()
        .await
        .assert_error(403, "Ask IT for access to the CRM");

    // A SocialError returned with `?` keeps its answer.
    app.client()
        .post("/api/v1/sso/okta")
        .json(&json!({ "code": "crm", "state": "nvs1.forged" }))
        .send()
        .await
        .assert_error(400, "Invalid or expired state");
}

// ---------------------------------------------------------------------------
// "Your own provider": the page's provider, as written.

mod zoho {
    use navree::BoxFuture;
    use navree::prelude::*;
    use navree::social::{
        AuthorizeUrl, ProviderConfig, ProviderToken, SocialError, SocialProvider, SocialUser,
    };

    #[injectable]
    pub struct Zoho {
        http: Arc<navree::http_client::HttpClient>,
    }

    impl SocialProvider for Zoho {
        fn key(&self) -> &'static str {
            "zoho"
        }

        fn authorize_url(&self, _config: &ProviderConfig, url: AuthorizeUrl) -> AuthorizeUrl {
            url.endpoint("https://accounts.zoho.com/oauth/v2/auth")
                .scopes(["AaaServer.profile.Read"])
        }

        fn user<'a>(
            &'a self,
            _config: &'a ProviderConfig,
            token: ProviderToken,
        ) -> BoxFuture<'a, Result<SocialUser, SocialError>> {
            Box::pin(async move {
                let profile: serde_json::Value = self
                    .http
                    .get("https://accounts.zoho.com/oauth/user/info")
                    .bearer(token.access_token.expose())
                    .send()
                    .await?
                    .json()?;
                Ok(SocialUser::new("zoho", profile["ZUID"].to_string())
                    .name(profile["Display_Name"].as_str().unwrap_or_default())
                    .email(profile["Email"].as_str(), true)
                    .token(token)
                    .raw(profile))
            })
        }
    }
}

#[tokio::test]
async fn a_custom_provider_registers_with_for_feature() {
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("social.redirect_base", "https://app.acme.test/auth/callback")
        .config(
            "social.providers.zoho",
            json!({ "type": "custom", "client_id": "1000.ABC", "token_url": "https://accounts.zoho.com/oauth/v2/token" }),
        )
        .module(CryptoModule::from_config())
        .module(HttpClientModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(SocialModule::from_config())
        .module(SocialModule::for_feature::<zoho::Zoho>())
        .build()
        .await;
    let url = app
        .client()
        .get("/api/auth/social/zoho/redirect")
        .send()
        .await
        .assert_status(200)
        .json_value()["url"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(url.starts_with(
        "https://accounts.zoho.com/oauth/v2/auth?response_type=code&client_id=1000.ABC"
    ));
    assert!(url.contains("scope=AaaServer.profile.Read"));
}

// ---------------------------------------------------------------------------
// "Calling the provider's API later": compiles against the real types.

#[allow(dead_code)]
async fn calendar_events(
    social: &Social,
    http: &navree::http_client::HttpClient,
    user: &navree::auth::User,
) -> Result<serde_json::Value, BoxError> {
    let google = social.token(user.id, "google").await?; // Secret<String>
    let events = http
        .get("https://www.googleapis.com/calendar/v3/calendars/primary/events")
        .bearer(google.expose())
        .send()
        .await?;
    Ok(events.json()?)
}
