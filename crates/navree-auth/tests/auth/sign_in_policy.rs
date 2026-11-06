//! `SignInPolicy`: one decision that every sign-in flow asks before it
//! issues a session, tokens or a two-factor challenge.

use std::sync::{Arc, Mutex, PoisonError};

use navree_auth::{
    AuthError, AuthService, ClientMeta, ModuleBuilderAuthExt, SignInAttempt, SignInAudience,
    SignInMethod, SignInPolicy,
};
use navree_contracts::AuditSink;
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_testing::TestApp;
use serde_json::json;

use crate::support::{
    self, PASSWORD, RecordingSink, enable_two_factor, grant_role, tokens, totp_code,
};

/// What the policy saw, and whether it enforces.
#[derive(Default)]
struct Seen(Mutex<Vec<SignInAttempt>>);

impl Seen {
    fn attempts(&self) -> Vec<SignInAttempt> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// `@corp.test` signs in through the `corp` connection only.
struct CorpOnlySso {
    seen: Arc<Seen>,
}

impl Injectable for CorpOnlySso {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Seen>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(CorpOnlySso {
                seen: r.get::<Seen>().await?,
            })
        })
    }
}

impl SignInPolicy for CorpOnlySso {
    fn check<'a>(&'a self, attempt: &'a SignInAttempt) -> BoxFuture<'a, Result<(), AuthError>> {
        Box::pin(async move {
            self.seen
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(attempt.clone());
            let through_corp = attempt.method
                == SignInMethod::Sso {
                    connection: "corp".to_owned(),
                };
            if attempt.domain() == Some("corp.test") && !through_corp {
                return Err(AuthError::SignInRefused {
                    code: "sso_required".to_owned(),
                    message: "Sign in with Corp SSO".to_owned(),
                });
            }
            Ok(())
        })
    }
}

struct PolicyModule(Arc<Seen>);

impl Module for PolicyModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc(self.0.clone()).sign_in_policy::<CorpOnlySso>();
    }
}

struct Setup {
    app: TestApp,
    seen: Arc<Seen>,
    sink: Arc<RecordingSink>,
}

async fn setup(
    configure: impl FnOnce(navree_testing::TestAppBuilder) -> navree_testing::TestAppBuilder,
) -> Setup {
    let seen = Arc::new(Seen::default());
    let sink = Arc::new(RecordingSink::default());
    let app = configure(support::builder())
        .module(PolicyModule(seen.clone()))
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    Setup { app, seen, sink }
}

/// Another spelling of the domain (a trailing dot, full-width letters, the
/// ideographic full stop) is the same domain to a policy.
#[tokio::test]
async fn a_policy_sees_one_spelling_of_the_domain() {
    let s = setup(|b| b).await;
    let auth = s.app.get::<AuthService>().await;
    // Emails from an identity provider or a social provider reach the
    // service as they were sent (the login form is stricter).
    for email in [
        "anu@CORP.test.",
        "anu@\u{ff43}orp.test",
        "anu@corp\u{3002}test",
    ] {
        let error = auth
            .attempt_login(email, "wrong", &ClientMeta::default())
            .await
            .unwrap_err();
        assert!(
            matches!(&error, AuthError::SignInRefused { code, .. } if code == "sso_required"),
            "{email}: {error:?}"
        );
    }
    assert!(
        s.seen
            .attempts()
            .iter()
            .all(|attempt| attempt.domain() == Some("corp.test"))
    );
}

#[tokio::test]
async fn a_policy_refuses_login_and_registration_for_its_accounts() {
    let s = setup(|b| b).await;
    support::create_user(&s.app, "anu@corp.test").await;
    support::create_user(&s.app, "gauri@example.com").await;

    // The check runs on the lower-cased email, before the password.
    s.app
        .client()
        .post("/api/auth/login")
        .json(&json!({ "email": "Anu@Corp.test", "password": "wrong" }))
        .send()
        .await
        .assert_error(403, "Sign in with Corp SSO")
        .assert_error_code(403, "sso_required");
    s.app
        .client()
        .post("/api/auth/login")
        .json(&json!({ "email": "gauri@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(200);
    s.app
        .client()
        .post("/api/auth/register")
        .json(&json!({ "name": "Laksh", "email": "laksh@corp.test", "password": PASSWORD }))
        .send()
        .await
        .assert_error_code(403, "sso_required");

    let auth = s.app.get::<AuthService>().await;
    assert!(matches!(
        auth.check_password_sign_in("ANU@corp.test").await,
        Err(AuthError::SignInRefused { .. })
    ));
    assert!(
        auth.check_password_sign_in("gauri@example.com")
            .await
            .is_ok()
    );

    let refused = s.sink.with_action("auth.login_refused");
    assert_eq!(
        refused.len(),
        1,
        "the login refusal only (register and checks don't audit)"
    );
    assert_eq!(
        refused[0].metadata,
        json!({
            "method": "password",
            "email": "anu@corp.test",
            "code": "sso_required",
            "audience": "app",
        })
    );
    let first = &s.seen.attempts()[0];
    assert_eq!(first.email, "anu@corp.test");
    assert_eq!(first.method, SignInMethod::Password);
    assert_eq!(first.audience, SignInAudience::App);
}

#[tokio::test]
async fn sign_in_asks_the_policy_with_the_method_of_other_crates() {
    let s = setup(|b| b).await;
    let ada = support::create_user(&s.app, "anu@corp.test").await;
    let auth = s.app.get::<AuthService>().await;
    let meta = ClientMeta::default();

    let refused = auth
        .sign_in(&ada, SignInMethod::social("github"), &meta)
        .await;
    assert!(matches!(
        refused,
        Err(AuthError::SignInRefused { ref code, .. }) if code == "sso_required"
    ));
    let audited = s.sink.with_action("auth.login_refused");
    assert_eq!(audited[0].metadata["method"], "social");
    assert_eq!(audited[0].metadata["provider"], "github");
    assert_eq!(audited[0].actor_id, Some(ada.id));

    // Another connection is refused too; the domain's own goes on.
    assert!(
        auth.sign_in(&ada, SignInMethod::sso("other"), &meta)
            .await
            .is_err()
    );
    let tokens = auth
        .sign_in(&ada, SignInMethod::sso("corp"), &meta)
        .await
        .unwrap();
    s.app
        .client()
        .get("/api/auth/me")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(200);

    // `issue_tokens` asks nothing: tests and seeders.
    let before = s.seen.attempts().len();
    auth.issue_tokens(&ada, meta.clone()).await.unwrap();
    assert_eq!(s.seen.attempts().len(), before);
}

#[tokio::test]
async fn the_second_step_of_a_two_factor_sign_in_asks_again_with_the_first_method() {
    let clock = support::clock();
    let s = setup(|b| b.override_value::<dyn navree_contracts::Clock>(clock.clone())).await;
    let grace = support::create_user(&s.app, "gauri@example.com").await;
    let secret = enable_two_factor(&s.app, &clock, &grace).await;
    let auth = s.app.get::<AuthService>().await;

    // A challenge handed out by another crate's flow carries its method.
    let challenge = auth
        .two_factor_challenge(&grace, &SignInMethod::social("gitlab"))
        .unwrap();
    s.app
        .client()
        .post("/api/auth/2fa/verify")
        .json(&json!({ "challenge_token": challenge, "code": totp_code(&secret, &clock) }))
        .send()
        .await
        .assert_status(200);
    let last = s.seen.attempts().pop().unwrap();
    assert_eq!(last.method, SignInMethod::social("gitlab"));
    assert_eq!(last.user_id, Some(grace.id));
}

#[tokio::test]
async fn platform_administrators_break_glass_and_it_is_audited() {
    let clock = support::clock();
    let s = setup(|b| {
        b.override_value::<dyn navree_contracts::Clock>(clock.clone())
            .module(navree_access::AccessModule::for_root(
                navree_access::AccessOptions::default(),
            ))
            .config("auth.platform.enabled", true)
    })
    .await;
    let admin = support::create_user(&s.app, "root@corp.test").await;
    grant_role(&s.app, admin.id, "platform-admin", &[]).await;
    let secret = enable_two_factor(&s.app, &clock, &admin).await;
    let client = s.app.client();

    // The app refuses them like anyone of the domain.
    client
        .post("/api/auth/login")
        .json(&json!({ "email": "root@corp.test", "password": PASSWORD }))
        .send()
        .await
        .assert_error_code(403, "sso_required");

    // The platform console lets them in (break-glass) and says so.
    let challenge = client
        .post("/api/auth/platform/login")
        .json(&json!({ "email": "root@corp.test", "password": PASSWORD }))
        .send()
        .await
        .assert_status(200)
        .json_value()["challenge_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = client
        .post("/api/auth/platform/2fa/verify")
        .json(&json!({ "challenge_token": challenge, "code": totp_code(&secret, &clock) }))
        .send()
        .await
        .assert_status(200);
    tokens(&response);
    let broken = s.sink.with_action("auth.break_glass");
    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].actor_id, Some(admin.id));
    assert_eq!(broken[0].metadata["refused_code"], "sso_required");
    assert_eq!(broken[0].metadata["flow"], "platform");
    let platform = s
        .seen
        .attempts()
        .into_iter()
        .filter(|attempt| attempt.audience == SignInAudience::Platform)
        .count();
    assert_eq!(platform, 2, "both steps ask");
}

#[tokio::test]
async fn without_break_glass_platform_sign_in_obeys_the_policy() {
    let clock = support::clock();
    let s = setup(|b| {
        b.override_value::<dyn navree_contracts::Clock>(clock.clone())
            .module(navree_access::AccessModule::for_root(
                navree_access::AccessOptions::default(),
            ))
            .config("auth.platform.enabled", true)
            .config("auth.platform.break_glass", false)
    })
    .await;
    let admin = support::create_user(&s.app, "root@corp.test").await;
    grant_role(&s.app, admin.id, "platform-admin", &[]).await;
    enable_two_factor(&s.app, &clock, &admin).await;
    s.app
        .client()
        .post("/api/auth/platform/login")
        .json(&json!({ "email": "root@corp.test", "password": PASSWORD }))
        .send()
        .await
        .assert_error_code(403, "sso_required");
    assert!(s.sink.with_action("auth.break_glass").is_empty());
    let refused = s.sink.with_action("auth.login_refused");
    assert_eq!(refused.last().unwrap().metadata["audience"], "platform");
}

#[tokio::test]
async fn without_a_policy_every_account_may_sign_in() {
    let app = support::app().await;
    let auth = app.get::<AuthService>().await;
    assert!(auth.check_password_sign_in("anu@corp.test").await.is_ok());
    let attempt = SignInAttempt::new("anu@corp.test", SignInMethod::Passkey);
    assert!(
        auth.check_sign_in(&attempt, &ClientMeta::default())
            .await
            .is_ok()
    );
}
