//! `AuthService::revoke_credentials_of_domain` and `revoke_all_credentials`:
//! the sign-in sessions end, and modules that registered a
//! `CredentialRevoker` are asked.

use std::sync::{Arc, Mutex, PoisonError};

use navree_auth::{AuthError, AuthService};
use navree_contracts::{
    BoxError, BoxFuture, CredentialRevoker, ModuleBuilderRevocationExt, RevokedByKind,
};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use serde_json::json;
use uuid::Uuid;

use crate::support::{self, login, tokens};

/// Remembers the users it was asked about; fails when told to.
struct Recording {
    asked: Arc<Mutex<Vec<Vec<Uuid>>>>,
    fail: bool,
    name: &'static str,
}

impl CredentialRevoker for Recording {
    fn name(&self) -> &'static str {
        self.name
    }

    fn revoke_for_users<'a>(
        &'a self,
        users: &'a [Uuid],
    ) -> BoxFuture<'a, Result<RevokedByKind, BoxError>> {
        Box::pin(async move {
            self.asked
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(users.to_vec());
            if self.fail {
                return Err("the store is down".into());
            }
            Ok(RevokedByKind::from([("api_keys", users.len() as u64)]))
        })
    }
}

/// Shared by the two revokers of [`Revokers`].
#[derive(Default)]
struct Asked(Arc<Mutex<Vec<Vec<Uuid>>>>);

struct Failing(Recording);
struct Working(Recording);

macro_rules! revoker {
    ($type:ident, $fail:expr, $name:expr) => {
        impl Injectable for $type {
            fn dependencies() -> Vec<navree_kernel::ProviderKey> {
                vec![navree_kernel::ProviderKey::of::<Asked>()]
            }

            fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
                Box::pin(async move {
                    Ok($type(Recording {
                        asked: r.get::<Asked>().await?.0.clone(),
                        fail: $fail,
                        name: $name,
                    }))
                })
            }
        }

        impl CredentialRevoker for $type {
            fn name(&self) -> &'static str {
                self.0.name()
            }

            fn revoke_for_users<'a>(
                &'a self,
                users: &'a [Uuid],
            ) -> BoxFuture<'a, Result<RevokedByKind, BoxError>> {
                self.0.revoke_for_users(users)
            }
        }
    };
}

revoker!(Failing, true, "failing");
revoker!(Working, false, "working");

struct Revokers(Arc<Asked>);

impl Module for Revokers {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc(self.0.clone())
            .credential_revoker::<Failing>()
            .credential_revoker::<Working>();
    }
}

async fn refresh(app: &navree_testing::TestApp, token: &str) -> u16 {
    app.client()
        .post("/api/auth/refresh")
        .json(&json!({ "refresh_token": token }))
        .send()
        .await
        .status()
        .as_u16()
}

#[tokio::test]
async fn a_domains_sessions_end_whatever_the_spelling_of_the_email_or_the_domain() {
    let app = support::app().await;
    let auth = app.get::<AuthService>().await;
    let agra = support::create_user(&app, "aru@acme.test").await;
    let idn = support::create_user(&app, "ina@bücher.example").await;
    let other = support::create_user(&app, "bal@other.test").await;
    let mut sessions = Vec::new();
    for email in ["aru@acme.test", "bal@other.test"] {
        let (_, refresh) = tokens(&login(&app.client(), email, support::PASSWORD).await);
        sessions.push(refresh);
    }
    // The IDN account signs in through the service (the login form takes
    // ASCII addresses only).
    let pair = auth
        .issue_tokens(&idn, navree_auth::ClientMeta::default())
        .await
        .unwrap();

    let report = auth
        .revoke_credentials_of_domain("ACME.test.")
        .await
        .unwrap();
    assert_eq!((report.users, report.revoked("refresh_tokens")), (1, 1));
    assert_eq!(refresh(&app, &sessions[0]).await, 401);
    assert_eq!(refresh(&app, &sessions[1]).await, 200, "another domain");

    // An international domain is found by its ASCII form and its own.
    let report = auth
        .revoke_credentials_of_domain("xn--bcher-kva.example")
        .await
        .unwrap();
    assert_eq!(report.users, 1);
    assert_eq!(report.revoked("refresh_tokens"), 1);
    assert_eq!(refresh(&app, &pair.refresh_token).await, 401);
    let report = auth
        .revoke_credentials_of_domain("BÜCHER.example")
        .await
        .unwrap();
    assert_eq!(report.users, 1, "the account is still there");
    assert_eq!(report.total(), 0, "nothing left to revoke");

    for blank in ["", " ", "."] {
        let error = auth.revoke_credentials_of_domain(blank).await.unwrap_err();
        assert!(matches!(error, AuthError::Invalid { .. }), "{blank:?}");
    }
    let _ = (agra, other);
}

#[tokio::test]
async fn every_registered_revoker_is_asked_even_when_one_fails() {
    let asked = Arc::new(Asked::default());
    let app = support::builder()
        .module(Revokers(asked.clone()))
        .build()
        .await;
    let auth = app.get::<AuthService>().await;
    let ann = support::create_user(&app, "aru@acme.test").await;
    let ned = support::create_user(&app, "ned@acme.test").await;
    let (_, session) = tokens(&login(&app.client(), "aru@acme.test", support::PASSWORD).await);

    let error = auth
        .revoke_credentials_of_domain("acme.test")
        .await
        .unwrap_err();
    assert!(
        matches!(
            &error,
            AuthError::Revocation {
                module: "failing",
                ..
            }
        ),
        "{error:?}"
    );
    // The sessions were revoked and the other revoker ran.
    assert_eq!(refresh(&app, &session).await, 401);
    {
        let asked = asked.0.lock().unwrap_or_else(PoisonError::into_inner);
        assert_eq!(asked.len(), 2, "both revokers were asked: {asked:?}");
        let mut users = asked[0].clone();
        users.sort();
        let mut expected = vec![ann.id, ned.id];
        expected.sort();
        assert_eq!(users, expected);
    }

    let report = auth.revoke_all_credentials(ann.id).await;
    assert!(matches!(report, Err(AuthError::Revocation { .. })));
}
