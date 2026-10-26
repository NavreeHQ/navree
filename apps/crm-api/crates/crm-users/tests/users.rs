//! `UsersModule`: `/api/v1/me`, `/api/v1/users` and roles on the auth user
//! resource (spec §7).

use crm_testkit::{Role, TestCrm};
use crm_users::UsersModule;
use serde_json::{Value, json};
use uuid::Uuid;

async fn crm() -> TestCrm {
    TestCrm::builder().module(UsersModule).build().await
}

fn new_user(email: &str, role: &str) -> Value {
    json!({
        "name": "Nina Novak",
        "email": email,
        "password": "a long password",
        "role": role,
    })
}

#[tokio::test]
async fn me_returns_roles() {
    let crm = crm().await;
    for (role, roles) in [
        (Role::Admin, json!(["admin"])),
        (Role::Manager, json!(["manager"])),
        (Role::Rep1, json!(["rep"])),
    ] {
        let client = crm.login_as(role).await;
        let body = client
            .get("/api/v1/me")
            .send()
            .await
            .assert_status(200)
            .json_value();
        assert_eq!(
            body,
            json!({
                "id": crm.user_id(role),
                "name": role.name(),
                "email": role.email(),
                "roles": roles,
            })
        );
    }
    crm.client()
        .get("/api/v1/me")
        .send()
        .await
        .assert_error(401, "Unauthorized");
    crm.shutdown().await;
}

#[tokio::test]
async fn admin_can_create_user_with_role() {
    let crm = crm().await;
    let admin = crm.login_as(Role::Admin).await;
    let response = admin
        .post("/api/v1/users")
        .json(&new_user(" Nina@Navis.test ", "manager"))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("__type", json!("User"))
        .assert_json_path("name", json!("Nina Novak"))
        .assert_json_path("email", json!("nina@navis.test"))
        .assert_json_path("roles", json!(["manager"]))
        .assert_json_path("email_verified_at", Value::Null);
    let body = response.json_value();
    let id: Uuid = serde_json::from_value(body["id"].clone()).expect("a UUID id");
    assert!(body["created_at"].is_string(), "{body}");
    assert_eq!(
        response.header("location"),
        Some(format!("/api/v1/users/{id}").as_str())
    );

    // The new account signs in with its password and has its role.
    let login = crm
        .client()
        .post("/api/auth/login")
        .json(&json!({ "email": "nina@navis.test", "password": "a long password" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.roles", json!(["manager"]))
        .json_value();
    let token = login["access_token"].as_str().expect("access_token");
    crm.client()
        .get("/api/v1/me")
        .bearer(token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("roles", json!(["manager"]));
    crm.shutdown().await;
}

#[tokio::test]
async fn rep_cannot_create_user() {
    let crm = crm().await;
    for role in [Role::Rep1, Role::Manager] {
        crm.login_as(role)
            .await
            .post("/api/v1/users")
            .json(&new_user("eve@navis.test", "admin"))
            .send()
            .await
            .assert_error(403, "Forbidden");
    }
    crm.client()
        .post("/api/v1/users")
        .json(&new_user("eve@navis.test", "admin"))
        .send()
        .await
        .assert_status(401);
    // Nothing was created.
    let admin = crm.login_as(Role::Admin).await;
    admin
        .get("/api/v1/users")
        .query(&[("search", "eve@")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(0));
    crm.shutdown().await;
}

#[tokio::test]
async fn auth_me_includes_roles() {
    let crm = crm().await;
    let manager = crm.login_as(Role::Manager).await;
    manager
        .get("/api/auth/me")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("__type", json!("User"))
        .assert_json_path("email", json!("manager@navis.test"))
        .assert_json_path("roles", json!(["manager"]));
    // The login response's `user` too.
    crm.client()
        .post("/api/auth/login")
        .json(&json!({ "email": "rep2@navis.test", "password": "password123" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user.roles", json!(["rep"]));
    crm.shutdown().await;
}

#[tokio::test]
async fn creating_a_user_validates_the_input() {
    let crm = crm().await;
    let admin = crm.login_as(Role::Admin).await;
    admin
        .post("/api/v1/users")
        .json(&json!({ "name": "", "email": "not-an-email", "password": "a long password", "role": "rep" }))
        .send()
        .await
        .assert_validation_error("name")
        .assert_validation_error("email");
    admin
        .post("/api/v1/users")
        .json(&new_user("nina@navis.test", "owner"))
        .send()
        .await
        .assert_validation_error("role");
    admin
        .post("/api/v1/users")
        .json(&new_user("rep1@navis.test", "rep"))
        .send()
        .await
        .assert_validation_error("email");
    admin
        .post("/api/v1/users")
        .json(&json!({ "name": "Nina", "email": "nina@navis.test", "password": "short", "role": "rep" }))
        .send()
        .await
        .assert_validation_error("password");
    crm.shutdown().await;
}

#[tokio::test]
async fn any_signed_in_user_lists_and_searches_users() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep2).await;
    let page = rep
        .get("/api/v1/users")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(page["meta"]["total"], json!(4));
    assert_eq!(page["meta"]["current_page"], json!(1));
    assert_eq!(page["meta"]["per_page"], json!(15));
    let names: Vec<&str> = page["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|user| user["name"].as_str().expect("name"))
        .collect();
    assert_eq!(
        names,
        ["Dana Whitfield", "Marcus Reyes", "Priya Shah", "Tom Becker"]
    );
    assert_eq!(page["data"][2]["roles"], json!(["rep"]));
    assert_eq!(page["data"][0]["__type"], json!("User"));

    for search in ["priya", "REP1@"] {
        let page = rep
            .get("/api/v1/users")
            .query(&[("search", search)])
            .send()
            .await
            .assert_status(200)
            .json_value();
        assert_eq!(page["meta"]["total"], json!(1), "search {search}: {page}");
        assert_eq!(page["data"][0]["email"], json!("rep1@navis.test"));
    }
    let second = rep
        .get("/api/v1/users")
        .query(&[("per_page", "3"), ("page", "2")])
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(second["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(second["meta"]["last_page"], json!(2));
    crm.client()
        .get("/api/v1/users")
        .send()
        .await
        .assert_status(401);
    crm.shutdown().await;
}

#[tokio::test]
async fn admin_updates_name_and_role() {
    let crm = crm().await;
    let admin = crm.login_as(Role::Admin).await;
    let tom = crm.user_id(Role::Rep2);
    admin
        .patch(&format!("/api/v1/users/{tom}"))
        .json(&json!({ "name": "Thomas Becker", "role": "manager" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(tom))
        .assert_json_path("name", json!("Thomas Becker"))
        .assert_json_path("roles", json!(["manager"]));
    // The new role takes effect at once.
    let promoted = crm.login_as(Role::Rep2).await;
    promoted
        .get("/api/v1/me")
        .send()
        .await
        .assert_json_path("roles", json!(["manager"]))
        .assert_json_path("name", json!("Thomas Becker"));
    let rules = promoted.get("/api/me/abilities").send().await.json_value();
    assert!(
        rules
            .as_array()
            .is_some_and(|rules| rules.contains(&json!({ "action": "manage", "subject": "Deal" }))),
        "{rules:#}"
    );
    // Only the name: the role stays.
    admin
        .patch(&format!("/api/v1/users/{tom}"))
        .json(&json!({ "name": "Tom Becker" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("roles", json!(["manager"]));

    let rep = crm.login_as(Role::Rep1).await;
    rep.patch(&format!("/api/v1/users/{tom}"))
        .json(&json!({ "role": "rep" }))
        .send()
        .await
        .assert_error(403, "Forbidden");
    admin
        .patch(&format!("/api/v1/users/{}", Uuid::now_v7()))
        .json(&json!({ "name": "Nobody" }))
        .send()
        .await
        .assert_status(404);
    admin
        .patch(&format!("/api/v1/users/{tom}"))
        .json(&json!({ "role": "owner" }))
        .send()
        .await
        .assert_validation_error("role");
    crm.shutdown().await;
}

#[tokio::test]
async fn user_routes_are_documented_with_their_schemas() {
    let crm = crm().await;
    let doc = crm
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let schema = |path: &str, method: &str, status: &str| {
        doc["paths"][path][method]["responses"][status]["content"]["application/json"]["schema"]
            ["$ref"]
            .clone()
    };
    assert_eq!(
        schema("/api/v1/users", "get", "200"),
        json!("#/components/schemas/UserPage")
    );
    assert_eq!(
        schema("/api/v1/users", "post", "201"),
        json!("#/components/schemas/User")
    );
    assert_eq!(
        schema("/api/v1/users/{id}", "patch", "200"),
        json!("#/components/schemas/User")
    );
    assert_eq!(
        schema("/api/v1/me", "get", "200"),
        json!("#/components/schemas/Me")
    );
    let page = &doc["components"]["schemas"]["UserPage"];
    assert_eq!(
        page["properties"]["data"]["items"]["$ref"],
        json!("#/components/schemas/User")
    );
    let mut meta: Vec<&str> = page["properties"]["meta"]["required"]
        .as_array()
        .expect("meta is documented inline")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    meta.sort_unstable();
    assert_eq!(meta, ["current_page", "last_page", "per_page", "total"]);
    assert_eq!(page["required"], json!(["data", "meta"]), "{page:#}");
    crm.shutdown().await;
}

/// User search ignores case for non-ASCII letters too.
#[tokio::test]
async fn user_search_ignores_case_beyond_ascii() {
    let crm = crm().await;
    let admin = crm.login_as(Role::Admin).await;
    let mut input = new_user("ozlem@navis.test", "rep");
    input["name"] = json!("Özlem Çelik");
    admin
        .post("/api/v1/users")
        .json(&input)
        .send()
        .await
        .assert_status(201);
    for term in ["Özlem", "özlem", "ÇELIK", "çelik"] {
        let page = admin
            .get("/api/v1/users")
            .query(&[("search", term)])
            .send()
            .await
            .assert_status(200)
            .json_value();
        assert_eq!(page["meta"]["total"], json!(1), "search {term}: {page}");
        assert_eq!(page["data"][0]["name"], json!("Özlem Çelik"));
    }
    crm.shutdown().await;
}

#[tokio::test]
async fn user_search_takes_wildcards_literally() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    for term in ["_", "%", "\\"] {
        rep.get("/api/v1/users")
            .query(&[("search", term)])
            .send()
            .await
            .assert_status(200)
            .assert_json_path("meta.total", json!(0));
    }
    crm.shutdown().await;
}
