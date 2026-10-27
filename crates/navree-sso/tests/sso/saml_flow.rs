//! SAML sign-in through the routes: login, the assertion consumer
//! service, linking and just-in-time users, replays and states.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_auth::{AuthService, user};
use navree_db::Db;
use navree_sso::{Provisioning, SsoFake, SsoOwner};
use sea_orm::EntityTrait;
use serde_json::json;

use crate::support::{self, BASE};

#[tokio::test]
async fn the_login_route_redirects_to_the_identity_provider() {
    let world = support::world().await;
    let fake = SsoFake::new();
    world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;

    // A browser gets a redirect...
    let response = world
        .app
        .client()
        .get("/api/sso/acme/login")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(302);
    let location = response.header("location").unwrap().to_owned();
    assert!(location.starts_with("https://idp.sso-fake.test/sso?SAMLRequest="));
    assert_eq!(response.header("cache-control"), Some("no-store"));
    // ...a front end the URL.
    let url = world.login_url("acme", Some("/deals")).await;
    let request = fake.respond_to(&url).unwrap();
    assert_eq!(request.acs_url(), format!("{BASE}/api/sso/saml/acme/acs"));

    world
        .app
        .client()
        .get("/api/sso/acme/login?return_to=https://evil.test")
        .send()
        .await
        .assert_error_code(400, "invalid_return_to");
    world
        .app
        .client()
        .get("/api/sso/nope/login")
        .send()
        .await
        .assert_error_code(404, "sso_connection_not_found");
}

#[tokio::test]
async fn a_new_user_from_a_verified_domain_is_created_and_signed_in() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let connection = world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;
    world.verified_domain(&connection, "acme.test").await;

    let url = world.login_url("acme", Some("/deals")).await;
    let response = fake
        .respond_to(&url)
        .unwrap()
        .name_id("Alice@Acme.test")
        .attribute("firstName", ["Alice"])
        .attribute("lastName", ["Admin"]);
    let body = world
        .acs("acme", &response.form())
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("alice@acme.test"))
        .assert_json_path("user.name", json!("Alice Admin"))
        .assert_json_path("token_type", json!("Bearer"))
        .assert_json_path("return_to", json!("/deals"))
        .json_value();
    // The tokens work.
    world
        .app
        .client()
        .get("/api/auth/me")
        .bearer(body["access_token"].as_str().unwrap())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email", json!("alice@acme.test"));
    // The user has no password and a verified email.
    let auth = world.app.get::<AuthService>().await;
    let user = auth
        .find_by_email("alice@acme.test")
        .await
        .unwrap()
        .unwrap();
    assert!(user.email_verified_at.is_some());
    assert!(auth.validate_user("alice@acme.test", "").await.is_err());
    assert!(world.audit.actions().contains(&"sso.login".to_owned()));
    assert!(world.audit.actions().contains(&"auth.register".to_owned()));

    // The same assertion again is a replay; a fresh sign-in finds the user.
    world
        .acs("acme", &response.form())
        .await
        .assert_error_code(400, "invalid_state");
    let again = response.clone().relay_state(None);
    world.acs("acme", &again.form()).await.assert_status(400);
    world
        .saml_sign_in(&fake, "acme", "alice@acme.test", &[])
        .await
        .assert_status(200)
        .assert_json_path("user.id", json!(user.id));
    let users = user::Entity::find()
        .all(world.app.get::<Db>().await.conn())
        .await
        .unwrap();
    assert_eq!(users.len(), 1);
}

#[tokio::test]
async fn an_assertion_works_once() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let connection = world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;
    world.verified_domain(&connection, "acme.test").await;
    let first = world.login_url("acme", None).await;
    let second = world.login_url("acme", None).await;
    let assertion_id = "_the_same_assertion";
    let response = fake
        .respond_to(&first)
        .unwrap()
        .name_id("bob@acme.test")
        .assertion_id(assertion_id);
    world.acs("acme", &response.form()).await.assert_status(200);
    // The same assertion id answering another (valid) request.
    let replayed = fake
        .respond_to(&second)
        .unwrap()
        .name_id("bob@acme.test")
        .assertion_id(assertion_id);
    world
        .acs("acme", &replayed.form())
        .await
        .assert_error_code(400, "assertion_replayed");
}

#[tokio::test]
async fn emails_outside_verified_domains_are_not_trusted() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let connection = world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;
    world.verified_domain(&connection, "acme.test").await;
    world.create_user("Victim", "victim@other.test").await;
    world.create_user("Ann", "ann@acme.test").await;

    // An identity provider claiming someone else's address takes nothing
    // over, and cannot squat an address outside its domains.
    world
        .saml_sign_in(&fake, "acme", "victim@other.test", &[])
        .await
        .assert_error_code(409, "email_taken");
    world
        .saml_sign_in(&fake, "acme", "new@other.test", &[])
        .await
        .assert_error_code(403, "unverified_domain");
    // An existing user of a verified domain is linked.
    let ann = world
        .saml_sign_in(&fake, "acme", "ann@acme.test", &[])
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(ann["user"]["name"], "Ann");
}

#[tokio::test]
async fn provisioning_switches() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let off = world
        .saml_connection(
            &fake,
            SsoOwner::App,
            "closed",
            Provisioning {
                jit: false,
                ..Provisioning::default()
            },
        )
        .await;
    world.verified_domain(&off, "closed.test").await;
    world
        .saml_sign_in(&fake, "closed", "new@closed.test", &[])
        .await
        .assert_error_code(403, "sso_provisioning_disabled");

    world
        .saml_connection(
            &fake,
            SsoOwner::App,
            "open",
            Provisioning {
                allow_unverified_emails: true,
                ..Provisioning::default()
            },
        )
        .await;
    let body = world
        .saml_sign_in(&fake, "open", "anyone@anywhere.test", &[])
        .await
        .assert_status(200)
        .json_value();
    // Created, but its email is not marked verified.
    assert_eq!(body["user"]["email_verified_at"], serde_json::Value::Null);
    world
        .saml_sign_in(&fake, "open", "missing-email", &[])
        .await
        .assert_error_code(400, "missing_email");
}

#[tokio::test]
async fn idp_initiated_sign_in_needs_the_switch() {
    let fake = SsoFake::new();
    for allowed in [false, true] {
        let world =
            support::world_with(support::builder().config("sso.saml.allow_idp_initiated", allowed))
                .await;
        let connection = world
            .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
            .await;
        world.verified_domain(&connection, "acme.test").await;
        let sp = navree_sso::saml::ServiceProvider {
            entity_id: connection.service_provider.entity_id.clone().unwrap(),
            acs_url: connection.service_provider.acs_url.clone().unwrap(),
        };
        let response = fake
            .unsolicited(&sp, chrono::Utc::now())
            .name_id("ida@acme.test")
            .relay_state(Some("/inbox".to_owned()));
        let result = world.acs("acme", &response.form()).await;
        if allowed {
            result
                .assert_status(200)
                .assert_json_path("return_to", json!("/inbox"));
        } else {
            result.assert_error_code(400, "invalid_saml_response");
        }
    }
}

#[tokio::test]
async fn refusals_from_the_identity_provider_and_bad_responses() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let connection = world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;
    world.verified_domain(&connection, "acme.test").await;

    let url = world.login_url("acme", None).await;
    let refused = fake.respond_to(&url).unwrap().failure(
        "urn:oasis:names:tc:SAML:2.0:status:Responder",
        Some("User is not assigned to this application"),
    );
    world
        .acs("acme", &refused.form())
        .await
        .assert_error_code(403, "sso_refused");

    // A response changed after signing.
    let url = world.login_url("acme", None).await;
    let mut form = fake
        .respond_to(&url)
        .unwrap()
        .name_id("eve@acme.test")
        .form();
    let xml = String::from_utf8(STANDARD.decode(&form[0].1).unwrap()).unwrap();
    form[0].1 = STANDARD.encode(xml.replace("eve@acme.test", "admin@acme.test"));
    world
        .acs("acme", &form)
        .await
        .assert_error_code(400, "invalid_saml_response");

    // Not base64, no form at all.
    world
        .acs("acme", &[("SAMLResponse".to_owned(), "%%%".to_owned())])
        .await
        .assert_error_code(400, "invalid_saml_response");
}

#[tokio::test]
async fn the_metadata_route_describes_the_service_provider() {
    let world = support::world().await;
    let fake = SsoFake::new();
    world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;
    let response = world
        .app
        .client()
        .get("/api/sso/saml/acme/metadata")
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        response.header("content-type"),
        Some("application/samlmetadata+xml")
    );
    let xml = response.text();
    assert!(xml.contains(&format!("entityID=\"{BASE}/api/sso/saml/acme/metadata\"")));
    assert!(xml.contains(&format!("Location=\"{BASE}/api/sso/saml/acme/acs\"")));
}

#[tokio::test]
async fn a_complete_url_gets_a_one_time_code_or_the_error() {
    let world = support::world_with(
        support::builder().config("sso.complete_url", "https://app.acme.test/auth/sso"),
    )
    .await;
    let fake = SsoFake::new();
    let connection = world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;
    world.verified_domain(&connection, "acme.test").await;

    let response = world
        .saml_sign_in(&fake, "acme", "carol@acme.test", &[])
        .await
        .assert_status(303);
    let location = url::Url::parse(response.header("location").unwrap()).unwrap();
    assert_eq!(location.path(), "/auth/sso");
    let code = location
        .query_pairs()
        .find(|(k, _)| k == "code")
        .unwrap()
        .1
        .into_owned();
    world
        .app
        .client()
        .post("/api/sso/exchange")
        .json(&json!({ "code": code }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("carol@acme.test"));
    // A code works once.
    world
        .app
        .client()
        .post("/api/sso/exchange")
        .json(&json!({ "code": code }))
        .send()
        .await
        .assert_error_code(400, "invalid_sso_code");

    // A refusal goes back to the front end too.
    let response = world
        .saml_sign_in(&fake, "acme", "dan@elsewhere.test", &[])
        .await
        .assert_status(303);
    let location = url::Url::parse(response.header("location").unwrap()).unwrap();
    let query: std::collections::HashMap<String, String> =
        location.query_pairs().into_owned().collect();
    assert_eq!(query["error"], "unverified_domain");
    assert!(query["message"].contains("elsewhere.test"));
    // Nothing was written by the refused sign-in.
    let auth = world.app.get::<AuthService>().await;
    assert!(
        auth.find_by_email("dan@elsewhere.test")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_disabled_connection_refuses_sign_in() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let connection = world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;
    let sso = world.sso().await;
    let id = connection.id.to_string();
    world
        .write(|uow| async move {
            sso.update_connection(
                &uow,
                &id,
                navree_sso::ConnectionUpdate {
                    enabled: Some(false),
                    ..Default::default()
                },
            )
            .await
        })
        .await
        .unwrap();
    world
        .app
        .client()
        .get("/api/sso/acme/login")
        .send()
        .await
        .assert_error_code(403, "sso_connection_disabled");
}
