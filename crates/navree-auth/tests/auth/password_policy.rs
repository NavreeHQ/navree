//! `PasswordSignInPolicy`: modules that keep some accounts off passwords.

use navree_auth::{AuthError, AuthService, ModuleBuilderAuthExt, PasswordSignInPolicy};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use serde_json::json;

use crate::support::{self, PASSWORD};

/// Refuses passwords for `@corp.test` addresses.
struct CorpOnlySso;

impl Injectable for CorpOnlySso {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(CorpOnlySso) })
    }
}

impl PasswordSignInPolicy for CorpOnlySso {
    fn check<'a>(&'a self, email: &'a str) -> BoxFuture<'a, Result<(), AuthError>> {
        Box::pin(async move {
            if email.ends_with("@corp.test") {
                return Err(AuthError::PasswordSignInRefused {
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
        m.password_sign_in_policy::<CorpOnlySso>();
    }
}

#[tokio::test]
async fn a_policy_refuses_login_and_registration_for_its_accounts() {
    let app = support::builder().module(PolicyModule).build().await;
    support::create_user(&app, "ada@corp.test").await;
    support::create_user(&app, "grace@example.com").await;

    // The check runs on the lower-cased email, before the password.
    app.client()
        .post("/api/auth/login")
        .json(&json!({ "email": "Ada@Corp.test", "password": PASSWORD }))
        .send()
        .await
        .assert_error(403, "Sign in with Corp SSO")
        .assert_error_code(403, "sso_required");
    app.client()
        .post("/api/auth/login")
        .json(&json!({ "email": "grace@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(200);
    app.client()
        .post("/api/auth/register")
        .json(&json!({ "name": "Linus", "email": "linus@corp.test", "password": PASSWORD }))
        .send()
        .await
        .assert_error_code(403, "sso_required");

    let auth = app.get::<AuthService>().await;
    assert!(matches!(
        auth.check_password_sign_in("ADA@corp.test").await,
        Err(AuthError::PasswordSignInRefused { .. })
    ));
    assert!(
        auth.check_password_sign_in("grace@example.com")
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn without_a_policy_every_account_may_use_a_password() {
    let app = support::app().await;
    let auth = app.get::<AuthService>().await;
    assert!(auth.check_password_sign_in("ada@corp.test").await.is_ok());
}
