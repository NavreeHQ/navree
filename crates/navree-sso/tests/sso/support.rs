//! Helpers shared by the navree-sso integration tests: app builders, a
//! recording audit sink, connections and domains, and the sign-in steps.

#![allow(dead_code)]

use std::sync::{Arc, Mutex, PoisonError};

use navree_access::{AbilityCtx, AccessModule, AccessOptions, AccessService, Rules};
use navree_auth::{AuthModule, AuthService, ClientMeta, User};
use navree_contracts::{AuditEntry, AuditSink, BoxError, BoxFuture};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::{DatabaseModule, Db, Uow};
use navree_events::EventsModule;
use navree_http_client::{HttpClientModule, HttpClientOptions, SsrfPolicy};
use navree_sso::{
    NewConnection, Provisioning, Sso, SsoConnection, SsoDomain, SsoFake, SsoModule, SsoOwner,
    StaticTxtResolver, TxtResolver,
};
use navree_testing::{TestApp, TestAppBuilder, TestResponse};
use navree_workspace::WorkspaceModule;
use serde_json::Value;

/// The API's public base URL in the tests.
pub const BASE: &str = "https://api.acme.test";

/// A password for users created in tests.
pub const PASSWORD: &str = "correct horse battery";

/// An audit sink that keeps entries in memory.
#[derive(Default)]
pub struct RecordingAudit(pub Mutex<Vec<AuditEntry>>);

impl AuditSink for RecordingAudit {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry);
        Box::pin(async { Ok(()) })
    }
}

impl RecordingAudit {
    /// The actions recorded so far.
    pub fn actions(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|entry| entry.action.clone())
            .collect()
    }
}

/// The world of a test: the app, its DNS and its audit sink.
pub struct World {
    pub app: TestApp,
    pub dns: Arc<StaticTxtResolver>,
    pub audit: Arc<RecordingAudit>,
}

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
}

/// A test app: crypto (fast hashing), a fresh database, auth and SSO read
/// from configuration (`sso.base_url` set), events, access and workspaces
/// with organizations.
pub fn builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("sso.base_url", BASE)
        .config("workspace.organizations.enabled", true)
        .config("workspace.routes", false)
        .config("audit.routes", false)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            super_admin_role: None,
        }))
        .module(AuthModule::from_config())
        .module(WorkspaceModule::from_config())
        .module(HttpClientModule::for_root(HttpClientOptions {
            retries: 0,
            ssrf: SsrfPolicy {
                block_private: true,
                allow_hosts: vec!["127.0.0.1".to_owned()],
            },
            ..HttpClientOptions::default()
        }))
        .module(SsoModule::from_config())
}

/// Boots `builder` with a static DNS and a recording audit sink.
pub async fn world_with(builder: TestAppBuilder) -> World {
    let dns = Arc::new(StaticTxtResolver::new());
    let audit = Arc::new(RecordingAudit::default());
    let app = builder
        .override_value::<dyn TxtResolver>(dns.clone())
        .override_value::<dyn AuditSink>(audit.clone())
        .build()
        .await;
    World { app, dns, audit }
}

/// [`builder`], booted.
pub async fn world() -> World {
    world_with(builder()).await
}

impl World {
    pub async fn sso(&self) -> Arc<Sso> {
        self.app.get::<Sso>().await
    }

    /// Runs `f` in a committed unit of work.
    pub async fn write<T, F, Fut>(&self, f: F) -> T
    where
        F: FnOnce(Uow) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let db = self.app.get::<Db>().await;
        let uow = db.uow(None, None).await;
        let value = uow.scope(f(uow.clone())).await;
        uow.commit().await.expect("commit");
        value
    }

    /// A SAML connection `slug` to `fake` owned by `owner`.
    pub async fn saml_connection(
        &self,
        fake: &SsoFake,
        owner: SsoOwner,
        slug: &str,
        provisioning: Provisioning,
    ) -> SsoConnection {
        let sso = self.sso().await;
        let input = NewConnection::saml(owner, "Acme SSO", fake.metadata_xml())
            .slug(slug)
            .provisioning(provisioning);
        self.write(|uow| async move { sso.create_connection(&uow, input).await })
            .await
            .expect("the connection is created")
    }

    /// Adds `domain` to `connection` and verifies it without DNS.
    pub async fn verified_domain(&self, connection: &SsoConnection, domain: &str) -> SsoDomain {
        let sso = self.sso().await;
        let slug = connection.slug.clone();
        let domain = domain.to_owned();
        self.write(|uow| async move {
            let added = sso.add_domain(&uow, &slug, &domain).await?;
            sso.mark_domain_verified(&uow, added.id).await
        })
        .await
        .expect("the domain is verified")
    }

    /// Creates a user with a password.
    pub async fn create_user(&self, name: &str, email: &str) -> User {
        self.app
            .get::<AuthService>()
            .await
            .create_user(name, email, PASSWORD)
            .await
            .expect("the user is created")
    }

    /// An access token for `user`.
    pub async fn token(&self, user: &User) -> String {
        self.app
            .get::<AuthService>()
            .await
            .issue_tokens(user, ClientMeta::default())
            .await
            .expect("tokens")
            .access_token
    }

    /// A user allowed to manage single sign-on, and their access token.
    pub async fn sso_admin(&self) -> (User, String) {
        let user = self.create_user("Sam Admin", "sam@admin.test").await;
        let access = self.app.get::<AccessService>().await;
        let id = user.id;
        self.write(|uow| async move {
            access.create_role(&uow, "sso-admin", None).await.unwrap();
            access.grant(&uow, "sso-admin", "sso.manage").await.unwrap();
            access.assign_role(&uow, id, "sso-admin").await.unwrap();
        })
        .await;
        let token = self.token(&user).await;
        (user, token)
    }

    /// `GET /api/sso/{slug}/login` as JSON: the identity provider URL.
    pub async fn login_url(&self, slug: &str, return_to: Option<&str>) -> String {
        let mut request = self.app.client().get(&format!("/api/sso/{slug}/login"));
        if let Some(return_to) = return_to {
            request = request.query(&[("return_to", return_to)]);
        }
        request.send().await.assert_status(200).json_value()["url"]
            .as_str()
            .expect("a url")
            .to_owned()
    }

    /// Posts `form` to the assertion consumer service of `slug`.
    pub async fn acs(&self, slug: &str, form: &[(String, String)]) -> TestResponse {
        self.app
            .client()
            .post(&format!("/api/sso/saml/{slug}/acs"))
            .form(form)
            .send()
            .await
    }

    /// A full SAML sign-in with `fake` answering as `name_id` with
    /// `attributes`.
    pub async fn saml_sign_in(
        &self,
        fake: &SsoFake,
        slug: &str,
        name_id: &str,
        attributes: &[(&str, &[&str])],
    ) -> TestResponse {
        let url = self.login_url(slug, None).await;
        let mut response = fake.respond_to(&url).expect("a request").name_id(name_id);
        for (name, values) in attributes {
            response = response.attribute(*name, values.iter().copied());
        }
        self.acs(slug, &response.form()).await
    }
}
