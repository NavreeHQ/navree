//! Social sign-in asks navree-auth's sign-in policies: a domain that must
//! sign in elsewhere (single sign-on enforced, say) is refused with 403
//! `sso_required`, before any account is created or linked.

use chrono::Utc;
use navree_auth::{
    AuthError, AuthService, JwtService, ModuleBuilderAuthExt, SignInAttempt, SignInMethod,
    SignInPolicy, TokenType, User,
};
use navree_db::Db;
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::Model as _;
use navree_social::{Social, SocialUser};
use navree_testing::TestApp;
use serde_json::json;

use crate::support::{create_user, fake_app, sign_in};

/// `@corp.test` signs in through single sign-on only.
struct CorpOnlySso;

impl Injectable for CorpOnlySso {
    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(CorpOnlySso) })
    }
}

impl SignInPolicy for CorpOnlySso {
    fn check<'a>(&'a self, attempt: &'a SignInAttempt) -> BoxFuture<'a, Result<(), AuthError>> {
        Box::pin(async move {
            let sso = matches!(attempt.method, SignInMethod::Sso { .. });
            if attempt.domain() == Some("corp.test") && !sso {
                return Err(AuthError::SignInRefused {
                    code: "sso_required".to_owned(),
                    message: "Sign in with Corp SSO".to_owned(),
                });
            }
            Ok(())
        })
    }
}

struct PolicyModule;

impl Module for PolicyModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.sign_in_policy::<CorpOnlySso>();
    }
}

fn github(id: &str, email: Option<&str>) -> SocialUser {
    SocialUser::new("github", id)
        .name("Someone")
        .email(email, true)
}

async fn two_factor_on(app: &TestApp, id: uuid::Uuid) {
    let db = app.get::<Db>().await;
    User::query()
        .where_key(id)
        .update(&*db, [User::TWO_FACTOR_CONFIRMED_AT.set(Some(Utc::now()))])
        .await
        .unwrap();
}

#[tokio::test]
async fn social_sign_in_is_refused_for_an_enforced_domain() {
    let (app, fake) = fake_app(|b| b.module(PolicyModule)).await;
    let auth = app.get::<AuthService>().await;

    // A new person of the domain: refused, and no account is created.
    fake.on_code("github", "new", github("1", Some("bob@corp.test")));
    sign_in(&app, "github", "new")
        .await
        .assert_error(403, "Sign in with Corp SSO")
        .assert_error_code(403, "sso_required");
    assert!(auth.find_by_email("bob@corp.test").await.unwrap().is_none());

    // An existing account of the domain, by its verified email.
    let ada = create_user(&app, "Anu", "anu@corp.test").await;
    let db = app.get::<Db>().await;
    User::query()
        .where_key(ada.id)
        .update(&*db, [User::EMAIL_VERIFIED_AT.set(Some(Utc::now()))])
        .await
        .unwrap();
    fake.on_code("github", "anu", github("2", Some("anu@corp.test")));
    sign_in(&app, "github", "anu")
        .await
        .assert_error_code(403, "sso_required");

    // A linked account whose provider address is elsewhere: the account's
    // own email decides.
    let social = app.get::<Social>().await;
    let uow = db.uow(None, None).await;
    social
        .link(&uow, ada.id, &github("3", Some("anu@home.test")))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    fake.on_code("github", "linked", github("3", Some("anu@home.test")));
    sign_in(&app, "github", "linked")
        .await
        .assert_error_code(403, "sso_required");

    // Other domains are unaffected.
    fake.on_code("github", "gauri", github("4", Some("gauri@example.com")));
    sign_in(&app, "github", "gauri")
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("gauri@example.com"));
}

#[tokio::test]
async fn no_two_factor_challenge_is_handed_out_for_an_enforced_domain() {
    let (app, fake) = fake_app(|b| b.module(PolicyModule)).await;
    let social = app.get::<Social>().await;
    let db = app.get::<Db>().await;

    // Linked before the domain came to sign in elsewhere.
    let ada = create_user(&app, "Anu", "anu@corp.test").await;
    let uow = db.uow(None, None).await;
    social.link(&uow, ada.id, &github("1", None)).await.unwrap();
    uow.commit().await.unwrap();
    two_factor_on(&app, ada.id).await;
    fake.on_code("github", "anu", github("1", None));
    sign_in(&app, "github", "anu")
        .await
        .assert_error_code(403, "sso_required");

    // Elsewhere the challenge carries the provider for the second step.
    let grace = create_user(&app, "Gauri", "gauri@example.com").await;
    let uow = db.uow(None, None).await;
    social
        .link(&uow, grace.id, &github("2", None))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    two_factor_on(&app, grace.id).await;
    fake.on_code("github", "gauri", github("2", None));
    let body = sign_in(&app, "github", "gauri")
        .await
        .assert_status(200)
        .json_value();
    let challenge = body["challenge_token"].as_str().unwrap();
    let claims = app
        .get::<JwtService>()
        .await
        .verify(challenge, TokenType::TwoFactorChallenge)
        .unwrap();
    assert_eq!(claims.sign_in, Some(SignInMethod::social("github")));
}
