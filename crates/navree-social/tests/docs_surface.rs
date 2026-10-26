//! Every public item `docs/packages/social-sign-in` names (types, fields,
//! methods, configuration keys, events and fake helpers), so a rename
//! fails the build or this test. The routes are exercised in
//! `tests/social`; the page's examples run in the facade's
//! `tests/social_docs.rs`.

use std::collections::BTreeMap;

use chrono::Utc;
use navree_auth::{ClientMeta, LoginOutcome, User};
use navree_config::{Config, ConfigSources};
use navree_contracts::Secret;
use navree_db::Uow;
use navree_events::Event;
use navree_social::{
    AuthorizeUrl, Intent, LinkByEmail, LinkedAccount, ProviderConfig, ProviderToken,
    RedirectOptions, SOCIAL_MIGRATIONS, Social, SocialAccountLinked, SocialAccountUnlinked,
    SocialCallback, SocialError, SocialModule, SocialOptions, SocialRedirect, SocialUser,
    TwoFactorPolicy,
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn the_documented_values_have_their_fields() {
    let SocialUser {
        provider,
        id,
        name,
        email,
        email_verified,
        avatar_url,
        nickname,
        token,
        raw,
    } = SocialUser::new("github", "583231")
        .name("Ada Lovelace")
        .email(Some("ada@example.com"), true)
        .avatar_url("https://avatars.test/ada")
        .nickname("ada")
        .token(ProviderToken::new("gho"))
        .raw(json!({ "login": "ada" }));
    assert_eq!((provider.as_str(), id.as_str()), ("github", "583231"));
    assert_eq!(name.as_deref(), Some("Ada Lovelace"));
    assert_eq!(email.as_deref(), Some("ada@example.com"));
    assert!(email_verified);
    assert!(avatar_url.is_some() && nickname.is_some() && token.is_some());
    assert_eq!(raw["login"], "ada");

    let ProviderToken {
        access_token,
        refresh_token,
        expires_at,
        id_token,
        scopes,
    } = ProviderToken::new("a")
        .refresh_token("r")
        .expires_at(Utc::now())
        .scopes(["s"]);
    let _: (
        &Secret<String>,
        Option<Secret<String>>,
        Option<Secret<String>>,
    ) = (&access_token, refresh_token, id_token);
    assert!(expires_at.is_some());
    assert_eq!(scopes, ["s"]);

    let RedirectOptions {
        scopes,
        params,
        return_to,
        intent,
        user_id,
    } = RedirectOptions::default();
    let _: (
        Vec<String>,
        BTreeMap<String, String>,
        Option<String>,
        Option<Uuid>,
    ) = (scopes, params, return_to, user_id);
    assert_eq!(intent, Intent::SignIn);
    let _ = Intent::Link;

    let _ = SocialRedirect {
        url: String::new(),
        state: String::new(),
    };
    let _ = SocialCallback {
        code: String::new(),
        state: String::new(),
        user: None,
    };
    let account = LinkedAccount {
        provider: "github".into(),
        provider_name: "GitHub".into(),
        email: None,
        name: None,
        avatar_url: None,
        linked_at: Utc::now(),
    };
    let listed = serde_json::to_value(&account).unwrap();
    for key in [
        "provider",
        "provider_name",
        "email",
        "name",
        "avatar_url",
        "linked_at",
    ] {
        assert!(listed.get(key).is_some(), "{key}");
    }
}

#[test]
fn the_events_have_their_documented_names() {
    assert_eq!(SocialAccountLinked::NAME, "social.linked");
    assert_eq!(SocialAccountUnlinked::NAME, "social.unlinked");
    let _ = SocialAccountLinked {
        user_id: Uuid::nil(),
        provider: String::new(),
        provider_user_id: String::new(),
    };
    assert_eq!(SOCIAL_MIGRATIONS, "social");
}

#[test]
fn every_reference_key_is_read() {
    let config = Config::load(&ConfigSources::new().set(
        "social",
        json!({
            "redirect_base": "https://app.acme.test/auth/callback",
            "routes": false,
            "registration": true,
            "link_by_email": "never",
            "allowed_email_domains": ["acme.test"],
            "two_factor": "skip",
            "state_ttl_secs": 300,
            "store_tokens": true,
            "providers": {
                "google": {
                    "type": "google", "name": "Google Workspace", "client_id": "g", "client_secret": "s",
                    "scopes": ["https://www.googleapis.com/auth/calendar.readonly"],
                    "params": { "access_type": "offline", "prompt": "consent" },
                    "pkce": false, "allowed_email_domains": ["acme.test"], "hosted_domain": "acme.test",
                    "authorize_url": "https://idp.test/a", "token_url": "https://idp.test/t",
                    "userinfo_url": "https://idp.test/u", "jwks_url": "https://idp.test/k",
                },
                "microsoft": { "client_id": "m", "tenant": "organizations" },
                "slack": { "client_id": "s", "team": "T0123" },
                "gitlab": { "client_id": "gl", "url": "https://gitlab.acme.test" },
                "github": { "client_id": "gh", "url": "https://git.acme.test", "api_url": "https://git.acme.test/api/v3" },
                "apple": {
                    "client_id": "com.acme.web", "team_id": "T", "key_id": "K",
                    "private_key": include_str!("fixtures/apple-authkey.p8"),
                },
                "okta": { "type": "oidc", "issuer": "https://acme.okta.com", "client_id": "o" },
                "zoho": { "type": "custom", "client_id": "z", "token_url": "https://accounts.zoho.com/oauth/v2/token" },
                "linkedin": { "client_id": "l" },
                "discord": { "client_id": "d" },
                "x": { "client_id": "x" },
            },
        }),
    ))
    .unwrap();
    let options: SocialOptions = config.typed().unwrap();
    assert!(!options.routes);
    assert_eq!(options.registration, Some(true));
    assert_eq!(options.link_by_email, LinkByEmail::Never);
    assert_eq!(options.two_factor, TwoFactorPolicy::Skip);
    assert_eq!(options.allowed_email_domains, ["acme.test"]);
    assert_eq!(options.state_ttl_secs, 300);
    assert!(options.store_tokens);
    assert_eq!(options.providers.len(), 11);
    let google: ProviderConfig = options.provider("google").unwrap();
    assert_eq!(google.kind(), "google");
    assert_eq!(google.name.as_deref(), Some("Google Workspace"));
    assert_eq!(google.secret(), Some("s"));
    assert!(!google.pkce);
    assert_eq!(google.params["access_type"], "offline");
    assert_eq!(google.hosted_domain.as_deref(), Some("acme.test"));
    assert!(google.authorize_url.is_some() && google.token_url.is_some());
    assert!(google.userinfo_url.is_some() && google.jwks_url.is_some());
    assert_eq!(
        options.provider("microsoft").unwrap().tenant.as_deref(),
        Some("organizations")
    );
    assert_eq!(
        options.provider("slack").unwrap().team.as_deref(),
        Some("T0123")
    );
    assert!(options.provider("github").unwrap().api_url.is_some());
    let apple = options.provider("apple").unwrap();
    assert!(apple.team_id.is_some() && apple.key_id.is_some() && apple.private_key.is_some());
    assert_eq!(
        options.provider("okta").unwrap().issuer.as_deref(),
        Some("https://acme.okta.com")
    );
}

#[test]
fn the_module_and_the_fake_have_their_documented_constructors() {
    let _ = SocialModule::from_config();
    let _ = SocialModule::for_root(SocialOptions::default());
    let _ = SocialModule;
    let (social, fake) = Social::fake();
    assert!(social.is_fake());
    fake.on_code("github", "c", SocialUser::new("github", "1"))
        .on_code_error("github", "d", "access_denied")
        .on_refresh("google", ProviderToken::new("ya29"));
    assert!(fake.exchanged().is_empty());
    let url = AuthorizeUrl::default()
        .endpoint("https://idp.test/auth")
        .scopes(["a"])
        .param("prompt", "consent")
        .scope_separator(" ");
    assert_eq!(url.endpoint_url(), Some("https://idp.test/auth"));
    assert_eq!(url.param_value("prompt"), Some("consent"));
    let _ = [
        SocialError::Reauthorize {
            provider: String::new(),
        },
        SocialError::NoToken {
            provider: String::new(),
        },
        SocialError::NotLinked {
            provider: String::new(),
        },
        SocialError::InvalidState,
    ];
}

/// The `Social` methods of the page's table, with their documented
/// argument and result types (compiled, not run).
#[allow(dead_code)]
async fn the_service_methods(
    social: &Social,
    uow: &Uow,
    meta: &ClientMeta,
) -> Result<(), SocialError> {
    let _: SocialRedirect = social.redirect("github").await?;
    let _: SocialRedirect = social
        .redirect_with(
            "github",
            RedirectOptions {
                return_to: Some("/deals".into()),
                ..RedirectOptions::default()
            },
        )
        .await?;
    let user: SocialUser = social.user("github", &SocialCallback::default()).await?;
    let _: LoginOutcome = social.sign_in(&user, meta).await?;
    let _: LinkedAccount = social.link(uow, Uuid::nil(), &user).await?;
    social.unlink(uow, Uuid::nil(), "github").await?;
    let _: Vec<LinkedAccount> = social.accounts(Uuid::nil()).await?;
    let _: Option<User> = social.find_user("github", "583231").await?;
    let _: Secret<String> = social.token(Uuid::nil(), "google").await?;
    let _: Vec<String> = social.providers()?;
    let _: String = social.provider_name("github")?;
    Ok(())
}
