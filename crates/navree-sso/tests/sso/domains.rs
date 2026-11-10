//! Verified domains, discovery, password enforcement and the management
//! routes.

use navree_sso::{Provisioning, SsoError, SsoFake, SsoOwner};
use serde_json::json;

use crate::support::{self, PASSWORD};

#[tokio::test]
async fn a_domain_is_verified_by_its_txt_record() {
    let world = support::world().await;
    let fake = SsoFake::new();
    world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;
    let sso = world.sso().await;
    let added = {
        let sso = sso.clone();
        world
            .write(|uow| async move { sso.add_domain(&uow, "acme", " Acme.Test. ").await })
            .await
            .unwrap()
    };
    assert_eq!(added.domain, "acme.test");
    assert!(!added.verified);
    assert_eq!(added.txt_name, "acme.test");
    assert!(added.txt_value.starts_with("navree-sso-verification="));

    // No record yet.
    let id = added.id;
    let missing = {
        let sso = sso.clone();
        world
            .write(|uow| async move { sso.verify_domain(&uow, id).await })
            .await
    };
    assert!(matches!(missing, Err(SsoError::DomainNotVerified(_))));
    // Another value does not count; the right one does.
    world.dns.add("acme.test", "navree-sso-verification=wrong");
    world.dns.add("acme.test", "v=spf1 -all");
    world.dns.add("acme.test", &added.txt_value);
    let verified = {
        let sso = sso.clone();
        world
            .write(|uow| async move { sso.verify_domain(&uow, id).await })
            .await
            .unwrap()
    };
    assert!(verified.verified);
    assert_eq!(sso.domains("acme").await.unwrap().len(), 1);

    // Another connection cannot claim it once verified.
    world
        .saml_connection(&fake, SsoOwner::App, "other", Provisioning::default())
        .await;
    let taken = {
        let sso = sso.clone();
        world
            .write(|uow| async move {
                let added = sso.add_domain(&uow, "other", "acme.test").await?;
                sso.mark_domain_verified(&uow, added.id).await
            })
            .await
    };
    assert!(matches!(taken, Err(SsoError::DomainTaken(_))));
    let bad = {
        let sso = sso.clone();
        world
            .write(|uow| async move { sso.add_domain(&uow, "acme", "not a domain").await })
            .await
    };
    assert!(matches!(bad, Err(SsoError::Invalid(_))));
}

#[tokio::test]
async fn discovery_and_password_enforcement() {
    let world =
        support::world_with(support::builder().config("sso.enforce_for_verified_domains", true))
            .await;
    let fake = SsoFake::new();
    let connection = world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;
    world.create_verified_user("Ann", "ann@acme.test").await;
    world.create_user("Ola", "ola@other.test").await;

    let discover = |email: &str| {
        let client = world.app.client();
        let email = email.to_owned();
        async move {
            client
                .post("/api/sso/discover")
                .json(&json!({ "email": email }))
                .send()
                .await
                .assert_status(200)
                .json_value()
        }
    };
    // Before verification nothing routes, and passwords work.
    assert_eq!(discover("ann@acme.test").await["sso"], false);
    let login = |email: &str| {
        let client = world.app.client();
        let email = email.to_owned();
        async move {
            client
                .post("/api/auth/login")
                .json(&json!({ "email": email, "password": PASSWORD }))
                .send()
                .await
        }
    };
    login("ann@acme.test").await.assert_status(200);

    world.verified_domain(&connection, "acme.test").await;
    let found = discover("Ann@ACME.test").await;
    assert_eq!(found["sso"], true);
    assert_eq!(found["connection"], "acme");
    assert_eq!(found["protocol"], "saml");
    assert_eq!(found["password_allowed"], false);
    assert_eq!(
        found["login_url"],
        format!("{}/api/sso/acme/login", support::BASE)
    );
    assert_eq!(discover("ola@other.test").await["password_allowed"], true);

    // Passwords are refused for the domain, everywhere else they work.
    login("ann@acme.test")
        .await
        .assert_error(403, "Sign in with Acme SSO")
        .assert_error_code(403, "sso_required");
    login("ola@other.test").await.assert_status(200);
    world
        .app
        .client()
        .post("/api/auth/register")
        .json(&json!({ "name": "New", "email": "new@acme.test", "password": PASSWORD }))
        .send()
        .await
        .assert_error_code(403, "sso_required");
    // Single sign-on still works for them.
    world
        .saml_sign_in(&fake, "acme", "ann@acme.test", &[])
        .await
        .assert_status(200);

    // A connection can opt out of enforcement.
    let sso = world.sso().await;
    world
        .write(|uow| async move {
            sso.update_connection(
                &uow,
                "acme",
                navree_sso::ConnectionUpdate {
                    enforce: Some(false),
                    ..Default::default()
                },
            )
            .await
        })
        .await
        .unwrap();
    login("ann@acme.test").await.assert_status(200);
    assert_eq!(discover("ann@acme.test").await["password_allowed"], true);
}

#[tokio::test]
async fn management_routes_need_the_permission() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let (_admin, token) = world.sso_admin().await;
    let someone = world.create_user("Joe", "joe@example.test").await;
    let joe = world.token(&someone).await;
    let input = json!({
        "owner": { "type": "app" },
        "name": "Acme Okta",
        "settings": { "protocol": "saml", "metadata_xml": fake.metadata_xml() },
    });

    world
        .app
        .client()
        .post("/api/sso/connections")
        .json(&input)
        .send()
        .await
        .assert_status(401);
    world
        .app
        .client()
        .post("/api/sso/connections")
        .bearer(&joe)
        .json(&input)
        .send()
        .await
        .assert_status(403);

    let created = world
        .app
        .client()
        .post("/api/sso/connections")
        .bearer(&token)
        .json(&input)
        .send()
        .await
        .assert_status(201)
        .assert_json_path("__type", json!("SsoConnection"))
        .assert_json_path("slug", json!("acme-okta"))
        .assert_json_path("protocol", json!("saml"))
        .assert_json_path(
            "service_provider.acs_url",
            json!("https://api.acme.test/api/sso/saml/acme-okta/acs"),
        )
        .json_value();
    let id = created["id"].as_str().unwrap().to_owned();

    // The same name again gets the next slug; a taken slug is refused.
    world
        .app
        .client()
        .post("/api/sso/connections")
        .bearer(&token)
        .json(&input)
        .send()
        .await
        .assert_status(201)
        .assert_json_path("slug", json!("acme-okta-2"));
    let mut taken = input.clone();
    taken["slug"] = json!("acme-okta");
    world
        .app
        .client()
        .post("/api/sso/connections")
        .bearer(&token)
        .json(&taken)
        .send()
        .await
        .assert_validation_error("slug");
    let mut broken = input.clone();
    broken["settings"]["metadata_xml"] = json!("<nope/>");
    world
        .app
        .client()
        .post("/api/sso/connections")
        .bearer(&token)
        .json(&broken)
        .send()
        .await
        .assert_validation_error("settings.metadata_xml");

    world
        .app
        .client()
        .get("/api/sso/connections?owner_type=app")
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("0.id", json!(id));
    world
        .app
        .client()
        .patch(&format!("/api/sso/connections/{id}"))
        .bearer(&token)
        .json(&json!({ "name": "Acme Okta (prod)", "enforce": true }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Acme Okta (prod)"))
        .assert_json_path("enforce", json!(true));

    let domain = world
        .app
        .client()
        .post(&format!("/api/sso/connections/{id}/domains"))
        .bearer(&token)
        .json(&json!({ "domain": "acme.test" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("verified", json!(false))
        .json_value();
    let domain_id = domain["id"].as_str().unwrap().to_owned();
    world
        .app
        .client()
        .post(&format!("/api/sso/domains/{domain_id}/verify"))
        .bearer(&token)
        .send()
        .await
        .assert_error_code(400, "domain_not_verified");
    world
        .dns
        .add("acme.test", domain["txt_value"].as_str().unwrap());
    world
        .app
        .client()
        .post(&format!("/api/sso/domains/{domain_id}/verify"))
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("verified", json!(true));
    world
        .app
        .client()
        .get(&format!("/api/sso/connections/{id}/domains"))
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("0.domain", json!("acme.test"));
    world
        .app
        .client()
        .delete(&format!("/api/sso/domains/{domain_id}"))
        .bearer(&token)
        .send()
        .await
        .assert_status(204);
    world
        .app
        .client()
        .delete(&format!("/api/sso/connections/{id}"))
        .bearer(&token)
        .send()
        .await
        .assert_status(204);
    world
        .app
        .client()
        .get(&format!("/api/sso/connections/{id}"))
        .bearer(&token)
        .send()
        .await
        .assert_error_code(404, "sso_connection_not_found");
}

#[tokio::test]
async fn client_secrets_are_encrypted_and_never_returned() {
    let world = support::world().await;
    let (_admin, token) = world.sso_admin().await;
    let created = world
        .app
        .client()
        .post("/api/sso/connections")
        .bearer(&token)
        .json(&json!({
            "owner": { "type": "app" },
            "name": "Acme Entra",
            "settings": {
                "protocol": "oidc",
                "issuer": "https://login.acme.test/v2.0",
                "client_id": "c1",
                "client_secret": "super-secret-value",
            },
        }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("has_client_secret", json!(true))
        .assert_json_path(
            "service_provider.redirect_uri",
            json!("https://api.acme.test/api/sso/oidc/acme-entra/callback"),
        );
    assert!(!created.text().contains("super-secret-value"));
    let id = created.json_value()["id"].as_str().unwrap().to_owned();
    let db = world.app.get::<navree_db::Db>().await;
    use navree_orm::Model as _;
    let row = navree_sso::sso_connection::Model::find(&db, uuid::Uuid::parse_str(&id).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(!row.settings.contains("super-secret-value"));
    // Changing other settings keeps the secret.
    world
        .app
        .client()
        .patch(&format!("/api/sso/connections/{id}"))
        .bearer(&token)
        .json(&json!({
            "settings": { "protocol": "oidc", "issuer": "https://login.acme.test/v2.0", "client_id": "c2" },
        }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("settings.client_id", json!("c2"))
        .assert_json_path("has_client_secret", json!(true));
    // A connection keeps its protocol, and owners must exist.
    world
        .app
        .client()
        .patch(&format!("/api/sso/connections/{id}"))
        .bearer(&token)
        .json(&json!({ "settings": { "protocol": "saml", "metadata_url": "https://idp.test/m" } }))
        .send()
        .await
        .assert_validation_error("settings.protocol");
    world
        .app
        .client()
        .post("/api/sso/connections")
        .bearer(&token)
        .json(&json!({
            "owner": { "type": "workspace", "id": uuid::Uuid::now_v7() },
            "name": "Nobody's",
            "settings": { "protocol": "oidc", "issuer": "https://login.acme.test", "client_id": "c" },
        }))
        .send()
        .await
        .assert_validation_error("owner");
}
