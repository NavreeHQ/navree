//! The examples of `docs/security/single-sign-on`, through the facade with
//! the `sso` feature (in `full`): the testing example as written, the
//! onboarding function, the workspace controller (macros) and the static
//! DNS resolver.
#![cfg(feature = "sqlite")]
#![cfg(feature = "sso")]

use navree::prelude::*;
use navree::sso::{
    NewConnection, Provisioning, RoleRule, Sso, SsoConnection, SsoDomain, SsoFake, SsoOwner,
    StaticTxtResolver, TxtResolver,
};
use navree::testing::TestApp;
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// "Testing": the page's test, as written.

#[tokio::test]
async fn alice_signs_in_with_her_company_identity_provider() -> Result<(), BoxError> {
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("sso.base_url", "https://api.acme.test")
        .config("sso.management_routes", false) // no AccessModule here
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(SsoModule::from_config())
        .build()
        .await;

    // A connection to the fake identity provider, with a verified domain.
    let idp = SsoFake::new();
    let sso = app.get::<Sso>().await;
    let uow = app.get::<navree::db::Db>().await.uow(None, None).await;
    let connection = NewConnection::saml(SsoOwner::App, "Acme", idp.metadata_xml()).slug("acme");
    sso.create_connection(&uow, connection).await?;
    let domain = sso.add_domain(&uow, "acme", "acme.test").await?;
    sso.mark_domain_verified(&uow, domain.id).await?;
    uow.commit().await?;

    // The login route sends the browser to the identity provider...
    let login = app.client().get("/api/sso/acme/login").send().await;
    let url = login.json_value()["url"].as_str().unwrap().to_owned();

    // ...which posts its signed answer to the assertion consumer service.
    let answer = idp
        .respond_to(&url)?
        .name_id("alice@acme.test")
        .attribute("groups", ["Engineering"]);
    app.client()
        .post("/api/sso/saml/acme/acs")
        .form(&answer.form())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("alice@acme.test"));
    Ok(())
}

// ---------------------------------------------------------------------------
// "Using the Sso service": the onboarding function, as written.

/// Connects a workspace to its company's identity provider and returns
/// the TXT record its administrator must publish.
pub async fn connect_workspace(
    sso: &Sso,
    uow: &Uow,
    workspace: Uuid,
    metadata_xml: String,
    domain: &str,
) -> Result<SsoDomain, BoxError> {
    let connection = sso
        .create_connection(
            uow,
            NewConnection::saml(SsoOwner::Workspace(workspace), "Acme Okta", metadata_xml)
                .provisioning(Provisioning {
                    roles: vec![RoleRule {
                        group: "Acme Admins".to_owned(),
                        role: "admin".to_owned(),
                    }],
                    ..Provisioning::default()
                }),
        )
        .await?;
    Ok(sso.add_domain(uow, &connection.slug, domain).await?)
}

// The workspace administrator's controller, as written.

/// The workspace's single sign-on settings.
#[injectable]
pub struct WorkspaceSsoController {
    sso: Arc<Sso>,
}

#[controller("/workspace/sso", version = "1")]
#[guard(InWorkspace::guard())]
impl WorkspaceSsoController {
    #[get("/")]
    #[permissions("sso.manage")]
    async fn index(&self, workspace: CurrentWorkspace) -> Result<Json<Vec<SsoConnection>>> {
        let owner = SsoOwner::Workspace(workspace.id());
        Ok(Json(self.sso.connections(Some(owner)).await?))
    }
}

#[module(controllers = [WorkspaceSsoController])]
struct SettingsModule;

#[tokio::test]
async fn a_workspace_connects_and_its_admin_lists_it() -> Result<(), BoxError> {
    let dns = Arc::new(StaticTxtResolver::new());
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("sso.base_url", "https://api.acme.test")
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(AuthModule::from_config())
        .module(WorkspaceModule::from_config())
        .module(SsoModule::from_config())
        .module(SettingsModule)
        .override_value::<dyn TxtResolver>(dns.clone())
        .build()
        .await;

    let auth = app.get::<AuthService>().await;
    let olive = auth
        .create_user("Olive", "olive@acme.test", "correct horse battery")
        .await?;
    let sso = app.get::<Sso>().await;
    let workspaces = app.get::<navree::workspace::Workspaces>().await;
    let db = app.get::<navree::db::Db>().await;
    let uow = db.uow(None, None).await;
    let workspace = workspaces
        .create(
            &uow,
            olive.id,
            navree::workspace::NewWorkspace::named("Acme"),
        )
        .await?;
    let domain = connect_workspace(
        &sso,
        &uow,
        workspace.id,
        SsoFake::new().metadata_xml(),
        "acme.test",
    )
    .await?;
    uow.commit().await?;

    // "Testing": the static resolver answers the TXT record.
    dns.add("acme.test", &domain.txt_value);
    let uow = db.uow(None, None).await;
    assert!(sso.verify_domain(&uow, domain.id).await?.verified);
    uow.commit().await?;

    let token = auth
        .issue_tokens(&olive, navree::auth::ClientMeta::default())
        .await?
        .access_token;
    app.client()
        .get("/api/v1/workspace/sso")
        .header("x-workspace", workspace.id.to_string())
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("0.name", json!("Acme Okta"))
        .assert_json_path("0.provisioning.roles.0.role", json!("admin"));
    Ok(())
}
