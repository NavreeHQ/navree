//! The module: configuration, routes, the OpenAPI document and metadata
//! fetched by URL.

use navree_api::ApiModule;
use navree_sso::{
    NewConnection, Provisioning, SamlSettings, Sso, SsoFake, SsoModule, SsoOptions, SsoOwner,
};
use navree_testing::TestApp;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support;

#[tokio::test]
async fn options_come_from_the_sso_section() {
    let world = support::world_with(
        support::builder()
            .config("sso.routes_prefix", "/single-sign-on")
            .config("sso.saml.clock_skew_secs", 30)
            .config("sso.oidc.scopes", "openid, email"),
    )
    .await;
    let sso = world.app.get::<Sso>().await;
    assert_eq!(sso.options().routes_prefix, "/single-sign-on");
    assert_eq!(sso.options().saml.clock_skew_secs, 30);
    assert_eq!(sso.options().oidc.scopes, ["openid", "email"]);
    world
        .app
        .client()
        .post("/api/single-sign-on/discover")
        .json(&serde_json::json!({ "email": "a@b.test" }))
        .send()
        .await
        .assert_status(200);
}

/// An app with auth and single sign-on only (no access module).
fn minimal() -> navree_testing::TestAppBuilder {
    crate::servers::database(TestApp::builder())
        .config("app.key", navree_crypto::generate_key())
        .module(navree_crypto::CryptoModule::from_config())
        .module(navree_db::DatabaseModule::from_config())
        .module(navree_auth::AuthModule::from_config())
}

#[tokio::test]
#[should_panic(expected = "sso.base_url")]
async fn invalid_options_fail_the_boot() {
    minimal()
        .module(SsoModule::for_root(SsoOptions {
            base_url: Some("not a url".to_owned()),
            management_routes: false,
            ..SsoOptions::default()
        }))
        .build()
        .await;
}

#[tokio::test]
#[should_panic(expected = "declares permissions:sso.manage but no `dyn RouteAuthorizer`")]
async fn the_management_routes_need_an_authorizer() {
    minimal().module(SsoModule::from_config()).build().await;
}

#[tokio::test]
async fn an_app_without_access_turns_the_management_routes_off() {
    let app = minimal()
        .config("sso.management_routes", false)
        .module(SsoModule::from_config())
        .build()
        .await;
    app.client()
        .post("/api/sso/discover")
        .json(&serde_json::json!({ "email": "a@b.test" }))
        .send()
        .await
        .assert_status(200);
    app.client()
        .get("/api/sso/connections")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn routes_can_be_turned_off() {
    let world = support::world_with(support::builder().config("sso.routes", false)).await;
    world
        .app
        .client()
        .post("/api/sso/discover")
        .json(&serde_json::json!({ "email": "a@b.test" }))
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn the_openapi_document_lists_the_routes() {
    let world = support::world_with(support::builder().module(ApiModule)).await;
    let document = world
        .app
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let paths = document["paths"].as_object().unwrap();
    for route in [
        "/api/sso/discover",
        "/api/sso/{connection}/login",
        "/api/sso/saml/{connection}/acs",
        "/api/sso/saml/{connection}/metadata",
        "/api/sso/oidc/{connection}/callback",
        "/api/sso/exchange",
        "/api/sso/connections",
        "/api/sso/connections/{id}",
        "/api/sso/connections/{id}/domains",
        "/api/sso/domains/{id}/verify",
        "/api/sso/domains/{id}",
    ] {
        assert!(paths.contains_key(route), "{route}");
    }
    let schemas = document["components"]["schemas"].as_object().unwrap();
    for schema in [
        "SsoConnection",
        "SsoDomain",
        "SsoDiscovery",
        "NewConnection",
    ] {
        assert!(schemas.contains_key(schema), "{schema}");
    }
}

#[tokio::test]
async fn metadata_by_url_is_fetched_and_kept() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/metadata"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fake.metadata_xml()))
        .expect(1)
        .mount(&server)
        .await;
    let sso = world.sso().await;
    let input = NewConnection {
        settings: navree_sso::ConnectionSettings::Saml(SamlSettings {
            metadata_url: Some(format!("{}/metadata", server.uri())),
            ..SamlSettings::default()
        }),
        ..NewConnection::saml(SsoOwner::App, "Acme", "")
    }
    .slug("acme")
    .provisioning(Provisioning::default());
    let connection = world
        .write(|uow| async move { sso.create_connection(&uow, input).await })
        .await
        .unwrap();
    world.verified_domain(&connection, "acme.test").await;
    // Two sign-ins, one metadata fetch (`expect(1)`, checked on drop).
    for _ in 0..2 {
        world
            .saml_sign_in(&fake, "acme", "amy@acme.test", &[])
            .await
            .assert_status(200);
    }

    // Plain HTTP to another host is refused.
    let sso = world.sso().await;
    let insecure = NewConnection {
        settings: navree_sso::ConnectionSettings::Saml(SamlSettings {
            metadata_url: Some("http://idp.acme.test/metadata".to_owned()),
            ..SamlSettings::default()
        }),
        ..NewConnection::saml(SsoOwner::App, "Plain", "")
    };
    let refused = world
        .write(|uow| async move { sso.create_connection(&uow, insecure).await })
        .await;
    assert!(matches!(refused, Err(navree_sso::SsoError::Invalid(_))));
}
