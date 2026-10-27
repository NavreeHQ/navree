//! SCIM 2.0 compliance cases (RFC 7643 and RFC 7644) over HTTP: discovery,
//! the bearer token, users and groups as Microsoft Entra ID and Okta
//! provision them, filters, paging, projections, PATCH, errors, and
//! connections kept apart.

use navree_directory::{
    ConnectionStatus, ConnectionUpdate, DirectoryUserDeactivated, DirectoryUserUpdated, Owner,
};
use navree_testing::TestResponse;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{Harness, harness};

const USER: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
const GROUP: &str = "urn:ietf:params:scim:schemas:core:2.0:Group";
const ENTERPRISE: &str = "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User";
const PATCH: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";
const ERROR: &str = "urn:ietf:params:scim:api:messages:2.0:Error";

/// A harness with a SCIM connection; its token.
async fn scim() -> (Harness, String, Uuid) {
    let h = harness(|b| b.config("app.url", "https://app.acme.test")).await;
    let created = h.scim(Owner::App).await;
    let token = created.scim_token.unwrap().expose().clone();
    (h, token, created.connection.id)
}

/// What Microsoft Entra ID posts for a new user.
fn entra_user(external_id: &str, user_name: &str) -> Value {
    json!({
        "schemas": [USER, ENTERPRISE],
        "externalId": external_id,
        "userName": user_name,
        "active": true,
        "displayName": "Ada Lovelace",
        "password": "Initial!Passw0rd",
        "name": { "formatted": "Ada Lovelace", "familyName": "Lovelace", "givenName": "Ada" },
        "emails": [{ "primary": true, "type": "work", "value": user_name }],
        "title": "Principal engineer",
        (ENTERPRISE): { "employeeNumber": "1815", "department": "Engineering" }
    })
}

async fn post(h: &Harness, token: &str, path: &str, body: &Value) -> TestResponse {
    h.app
        .client()
        .post(path)
        .bearer(token)
        .header("content-type", "application/scim+json")
        .body(serde_json::to_vec(body).unwrap())
        .send()
        .await
}

async fn patch(h: &Harness, token: &str, path: &str, operations: Value) -> TestResponse {
    h.app
        .client()
        .patch(path)
        .bearer(token)
        .json(&json!({ "schemas": [PATCH], "Operations": operations }))
        .send()
        .await
}

async fn get(h: &Harness, token: &str, path: &str) -> TestResponse {
    h.app.client().get(path).bearer(token).send().await
}

fn assert_scim_error(response: &TestResponse, status: u16, scim_type: Option<&str>) {
    assert_eq!(response.status().as_u16(), status, "{}", response.text());
    assert_eq!(
        response.header("content-type"),
        Some("application/scim+json"),
        "{}",
        response.text()
    );
    let body = response.json_value();
    assert_eq!(body["schemas"], json!([ERROR]));
    assert_eq!(body["status"], json!(status.to_string()));
    match scim_type {
        Some(kind) => assert_eq!(body["scimType"], json!(kind), "{body}"),
        None => assert!(body.get("scimType").is_none(), "{body}"),
    }
}

#[tokio::test]
async fn discovery_describes_the_server_without_a_token() {
    let (h, _, _) = scim().await;
    let config = h.app.client().get("/scim/v2/ServiceProviderConfig").send().await;
    let config = config.assert_status(200);
    assert_eq!(config.header("content-type"), Some("application/scim+json"));
    let body = config.json_value();
    assert_eq!(body["patch"]["supported"], json!(true));
    assert_eq!(body["bulk"]["supported"], json!(false));
    assert_eq!(body["filter"], json!({ "supported": true, "maxResults": 200 }));
    assert_eq!(body["authenticationSchemes"][0]["type"], json!("oauthbearertoken"));
    assert_eq!(
        body["meta"]["location"],
        json!("https://app.acme.test/scim/v2/ServiceProviderConfig")
    );
    let types = h.app.client().get("/scim/v2/ResourceTypes").send().await;
    types
        .assert_status(200)
        .assert_json_path("totalResults", json!(2))
        .assert_json_path("Resources.0.endpoint", json!("/Users"));
    h.app
        .client()
        .get("/scim/v2/ResourceTypes/Group")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("schema", json!(GROUP));
    let schemas = h.app.client().get("/scim/v2/Schemas").send().await;
    schemas.assert_status(200).assert_json_path("totalResults", json!(3));
    h.app
        .client()
        .get(&format!("/scim/v2/Schemas/{ENTERPRISE}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("EnterpriseUser"));
    let missing = h.app.client().get("/scim/v2/Schemas/urn:nope").send().await;
    assert_scim_error(&missing, 404, None);
}

#[tokio::test]
async fn requests_carry_the_connections_token() {
    let (h, token, id) = scim().await;
    let none = h.app.client().get("/scim/v2/Users").send().await;
    assert_scim_error(&none, 401, None);
    assert_eq!(none.header("www-authenticate"), Some("Bearer realm=\"SCIM\""));
    let wrong = get(&h, "nvscim_wrong", "/scim/v2/Users").await;
    assert_scim_error(&wrong, 401, None);
    let basic = h
        .app
        .client()
        .get("/scim/v2/Users")
        .header("authorization", format!("Basic {token}"))
        .send()
        .await;
    assert_scim_error(&basic, 401, None);
    get(&h, &token, "/scim/v2/Users").await.assert_status(200);

    // A paused connection refuses.
    let directory = h.directory().await;
    directory
        .update_connection(
            id,
            ConnectionUpdate {
                status: Some(ConnectionStatus::Paused),
                ..ConnectionUpdate::default()
            },
        )
        .await
        .unwrap();
    assert_scim_error(&get(&h, &token, "/scim/v2/Users").await, 403, None);
    directory
        .update_connection(
            id,
            ConnectionUpdate {
                status: Some(ConnectionStatus::Active),
                ..ConnectionUpdate::default()
            },
        )
        .await
        .unwrap();

    // A new token replaces the old one.
    let fresh = directory.rotate_scim_token(id).await.unwrap();
    assert_scim_error(&get(&h, &token, "/scim/v2/Users").await, 401, None);
    get(&h, fresh.expose(), "/scim/v2/Users").await.assert_status(200);
    let shown = directory.connection(id).await.unwrap();
    assert!(fresh.expose().starts_with(shown.scim_token_hint.as_deref().unwrap()));
}

#[tokio::test]
async fn users_are_created_read_filtered_and_paged() {
    let (h, token, _) = scim().await;
    let created = post(&h, &token, "/scim/v2/Users", &entra_user("8f6c9a1e", "ada@acme.test")).await;
    let created = created.assert_status(201);
    assert_eq!(created.header("content-type"), Some("application/scim+json"));
    let ada = created.json_value();
    let id = ada["id"].as_str().unwrap().to_owned();
    Uuid::parse_str(&id).unwrap();
    assert_eq!(
        created.header("location"),
        Some(format!("https://app.acme.test/scim/v2/Users/{id}").as_str())
    );
    assert_eq!(ada["schemas"], json!([USER, ENTERPRISE]));
    assert_eq!(ada["userName"], json!("ada@acme.test"));
    assert_eq!(ada["externalId"], json!("8f6c9a1e"));
    assert_eq!(ada[ENTERPRISE]["employeeNumber"], json!("1815"));
    assert_eq!(ada["groups"], json!([]));
    assert_eq!(ada["meta"]["resourceType"], json!("User"));
    assert!(ada["meta"]["version"].as_str().unwrap().starts_with("W/\""));
    assert!(ada.get("password").is_none(), "passwords are never kept");
    assert_eq!(h.log.calls(), vec!["user:create:8f6c9a1e"]);

    let read = get(&h, &token, &format!("/scim/v2/Users/{id}")).await;
    let read = read.assert_status(200);
    assert_eq!(read.json_value(), ada);

    // A user without an externalId is keyed by its SCIM id.
    let grace = post(
        &h,
        &token,
        "/scim/v2/Users",
        &json!({ "schemas": [USER], "userName": "grace@acme.test", "name": { "familyName": "Hopper" } }),
    )
    .await;
    let grace = grace.assert_status(201);
    let grace_id = grace.json_value()["id"].as_str().unwrap().to_owned();
    assert!(grace.json_value().get("externalId").is_none());
    let directory = h.directory().await;
    let connection = directory.all_connections().await.unwrap()[0].id;
    assert!(directory.user(connection, &grace_id).await.unwrap().is_some());

    // Filters: indexed equality (case-insensitive userName), and the rest.
    let by_name = get(&h, &token, "/scim/v2/Users?filter=userName%20eq%20%22ADA%40acme.test%22").await;
    by_name
        .assert_status(200)
        .assert_json_path("schemas", json!(["urn:ietf:params:scim:api:messages:2.0:ListResponse"]))
        .assert_json_path("totalResults", json!(1))
        .assert_json_path("Resources.0.id", json!(id));
    get(&h, &token, "/scim/v2/Users?filter=externalId%20eq%20%228f6c9a1e%22")
        .await
        .assert_json_path("totalResults", json!(1));
    get(&h, &token, "/scim/v2/Users?filter=externalId%20eq%20%228F6C9A1E%22")
        .await
        .assert_json_path("totalResults", json!(0));
    get(&h, &token, "/scim/v2/Users?filter=name.familyName%20sw%20%22hop%22")
        .await
        .assert_json_path("totalResults", json!(1))
        .assert_json_path("Resources.0.userName", json!("grace@acme.test"));
    get(
        &h,
        &token,
        &format!("/scim/v2/Users?filter={}", urlencode(&format!("{ENTERPRISE}:department eq \"Engineering\""))),
    )
    .await
    .assert_json_path("totalResults", json!(1));

    // Paging: startIndex is 1-based; count 0 gives the total only.
    let page = get(&h, &token, "/scim/v2/Users?startIndex=2&count=1").await;
    page.assert_status(200)
        .assert_json_path("totalResults", json!(2))
        .assert_json_path("startIndex", json!(2))
        .assert_json_path("itemsPerPage", json!(1))
        .assert_json_path("Resources.0.userName", json!("grace@acme.test"));
    let count = get(&h, &token, "/scim/v2/Users?count=0").await;
    count
        .assert_json_path("totalResults", json!(2))
        .assert_json_path("Resources", json!([]));
    get(&h, &token, "/scim/v2/Users?startIndex=-4&count=50")
        .await
        .assert_json_path("startIndex", json!(1))
        .assert_json_path("itemsPerPage", json!(2));

    // Projections.
    let only = get(&h, &token, &format!("/scim/v2/Users/{id}?attributes=userName,name.givenName")).await;
    assert_eq!(
        only.json_value(),
        json!({ "schemas": [USER, ENTERPRISE], "id": id, "userName": "ada@acme.test", "name": { "givenName": "Ada" } })
    );
    let without = get(&h, &token, &format!("/scim/v2/Users/{id}?excludedAttributes=emails,meta")).await;
    let without = without.json_value();
    assert!(without.get("emails").is_none() && without.get("meta").is_none());
    assert_eq!(without["title"], json!("Principal engineer"));
}

fn urlencode(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

#[tokio::test]
async fn bad_requests_get_scim_errors() {
    let (h, token, _) = scim().await;
    post(&h, &token, "/scim/v2/Users", &entra_user("e1", "ada@acme.test"))
        .await
        .assert_status(201);
    let taken = post(&h, &token, "/scim/v2/Users", &entra_user("e2", "ADA@acme.test")).await;
    assert_scim_error(&taken, 409, Some("uniqueness"));
    let same_external = post(&h, &token, "/scim/v2/Users", &entra_user("e1", "other@acme.test")).await;
    assert_scim_error(&same_external, 409, Some("uniqueness"));
    let nameless = post(&h, &token, "/scim/v2/Users", &json!({ "schemas": [USER] })).await;
    assert_scim_error(&nameless, 400, Some("invalidValue"));
    assert_eq!(nameless.json_value()["detail"], json!("userName is required"));
    let garbage = h
        .app
        .client()
        .post("/scim/v2/Users")
        .bearer(&token)
        .header("content-type", "application/scim+json")
        .body("{not json")
        .send()
        .await;
    assert_scim_error(&garbage, 400, Some("invalidSyntax"));
    let filter = get(&h, &token, "/scim/v2/Users?filter=userName%20is%20%22x%22").await;
    assert_scim_error(&filter, 400, Some("invalidFilter"));
    assert_eq!(filter.json_value()["detail"], json!("`is` is not a filter operator"));
    let count = get(&h, &token, "/scim/v2/Users?count=many").await;
    assert_scim_error(&count, 400, Some("invalidValue"));
    let missing = get(&h, &token, &format!("/scim/v2/Users/{}", Uuid::now_v7())).await;
    assert_scim_error(&missing, 404, None);
    assert_scim_error(&get(&h, &token, "/scim/v2/Users/not-a-uuid").await, 404, None);
    // Nothing of a refused request is kept.
    get(&h, &token, "/scim/v2/Users")
        .await
        .assert_json_path("totalResults", json!(1));
}

#[tokio::test]
async fn users_are_patched_the_way_identity_providers_send_it() {
    let (h, token, _) = scim().await;
    let ada = post(&h, &token, "/scim/v2/Users", &entra_user("8f6c9a1e", "ada@acme.test")).await;
    let path = format!("/scim/v2/Users/{}", ada.json_value()["id"].as_str().unwrap());
    h.log.clear();

    // Microsoft Entra ID disables with a string boolean.
    let disabled = patch(&h, &token, &path, json!([{ "op": "Replace", "path": "active", "value": "False" }])).await;
    disabled.assert_status(200).assert_json_path("active", json!("False"));
    assert_eq!(h.log.calls(), vec!["user:deactivate:8f6c9a1e"]);
    let event = &h.inbox.of::<DirectoryUserDeactivated>()[0];
    assert!(!event.user.active && !event.deleted);
    assert_eq!(event.run_id, None);

    // Okta enables with a path-less object.
    let enabled = patch(&h, &token, &path, json!([{ "op": "replace", "value": { "active": true } }])).await;
    enabled.assert_status(200).assert_json_path("active", json!(true));
    assert_eq!(h.log.calls()[1], "user:reactivate:8f6c9a1e");
    assert!(h.inbox.of::<DirectoryUserUpdated>()[0].reactivated);

    let changed = patch(
        &h,
        &token,
        &path,
        json!([
            { "op": "Replace", "path": "emails[type eq \"work\"].value", "value": "lovelace@acme.test" },
            { "op": "Add", "path": "phoneNumbers[type eq \"mobile\"].value", "value": "+44 7700 900123" },
            { "op": "Add", "path": format!("{ENTERPRISE}:department"), "value": "Research" },
            { "op": "Remove", "path": "title" }
        ]),
    )
    .await;
    let changed = changed.assert_status(200);
    let user = changed.json_value();
    assert_eq!(user["emails"][0]["value"], json!("lovelace@acme.test"));
    assert_eq!(user["phoneNumbers"], json!([{ "type": "mobile", "value": "+44 7700 900123" }]));
    assert_eq!(user[ENTERPRISE]["department"], json!("Research"));
    assert!(user.get("title").is_none());
    let updated = h.inbox.of::<DirectoryUserUpdated>();
    let last = updated.last().unwrap();
    assert_eq!(last.changed, vec!["department", "emails", "phone_numbers", "title"]);
    assert_eq!(last.user.primary_email(), Some("lovelace@acme.test"));

    // A PATCH that changes nothing calls nobody.
    h.log.clear();
    patch(&h, &token, &path, json!([{ "op": "replace", "path": "active", "value": true }]))
        .await
        .assert_status(200);
    assert!(h.log.calls().is_empty());

    let bad = patch(&h, &token, &path, json!([{ "op": "replace", "path": "emails[", "value": 1 }])).await;
    assert_scim_error(&bad, 400, Some("invalidPath"));
    let no_target = patch(&h, &token, &path, json!([{ "op": "remove" }])).await;
    assert_scim_error(&no_target, 400, Some("noTarget"));
}

#[tokio::test]
async fn put_replaces_and_delete_removes() {
    let (h, token, _) = scim().await;
    let ada = post(&h, &token, "/scim/v2/Users", &entra_user("8f6c9a1e", "ada@acme.test")).await;
    let id = ada.json_value()["id"].as_str().unwrap().to_owned();
    let path = format!("/scim/v2/Users/{id}");
    h.log.clear();

    let mut replacement = entra_user("8f6c9a1e", "ada@acme.test");
    replacement["title"] = json!("Countess");
    replacement["id"] = json!("ignored");
    let replaced = h
        .app
        .client()
        .put(&path)
        .bearer(&token)
        .json(&replacement)
        .send()
        .await;
    replaced
        .assert_status(200)
        .assert_json_path("id", json!(id))
        .assert_json_path("title", json!("Countess"));
    assert_eq!(h.log.calls(), vec!["user:update:8f6c9a1e"]);

    let deleted = h.app.client().delete(&path).bearer(&token).send().await;
    let deleted = deleted.assert_status(204);
    assert!(deleted.bytes().is_empty());
    assert_eq!(h.log.calls()[1], "user:deactivate:8f6c9a1e (deleted)");
    assert!(h.inbox.of::<DirectoryUserDeactivated>()[0].deleted);
    assert_scim_error(&get(&h, &token, &path).await, 404, None);
    assert_scim_error(&h.app.client().delete(&path).bearer(&token).send().await, 404, None);
    // The user name is free again.
    post(&h, &token, "/scim/v2/Users", &entra_user("8f6c9a1e", "ada@acme.test"))
        .await
        .assert_status(201);
}

#[tokio::test]
async fn groups_carry_their_members() {
    let (h, token, _) = scim().await;
    let ada = post(&h, &token, "/scim/v2/Users", &entra_user("e-ada", "ada@acme.test")).await;
    let ada_id = ada.json_value()["id"].as_str().unwrap().to_owned();
    let grace = post(&h, &token, "/scim/v2/Users", &entra_user("e-grace", "grace@acme.test")).await;
    let grace_id = grace.json_value()["id"].as_str().unwrap().to_owned();
    h.log.clear();

    let created = post(
        &h,
        &token,
        "/scim/v2/Groups",
        &json!({
            "schemas": [GROUP],
            "externalId": "grp-eng",
            "displayName": "Engineering",
            "members": [{ "value": ada_id }, { "value": Uuid::now_v7().to_string() }]
        }),
    )
    .await;
    let created = created.assert_status(201);
    let group = created.json_value();
    let group_id = group["id"].as_str().unwrap().to_owned();
    assert_eq!(group["schemas"], json!([GROUP]));
    assert_eq!(group["displayName"], json!("Engineering"));
    assert_eq!(group["members"][0]["value"], json!(ada_id));
    assert_eq!(group["members"][0]["display"], json!("Ada Lovelace"));
    assert_eq!(group["members"][0]["type"], json!("User"));
    assert_eq!(group["members"].as_array().unwrap().len(), 1, "unknown members are left out");
    assert_eq!(h.log.calls(), vec!["group:create:grp-eng:+local-e-ada-"]);

    // The user shows the group.
    get(&h, &token, &format!("/scim/v2/Users/{ada_id}"))
        .await
        .assert_json_path("groups.0.value", json!(group_id))
        .assert_json_path("groups.0.display", json!("Engineering"));

    let path = format!("/scim/v2/Groups/{group_id}");
    // Microsoft Entra ID adds and removes members with value lists.
    patch(&h, &token, &path, json!([{ "op": "Add", "path": "members", "value": [{ "value": grace_id }] }]))
        .await
        .assert_status(200);
    assert_eq!(h.log.calls()[1], "group:update:grp-eng:+local-e-grace-");
    patch(&h, &token, &path, json!([{ "op": "Remove", "path": "members", "value": [{ "value": ada_id }] }]))
        .await
        .assert_status(200);
    assert_eq!(h.log.calls()[2], "group:update:grp-eng:+-e-ada");
    // Okta removes with a value filter and renames with replace.
    let renamed = patch(
        &h,
        &token,
        &path,
        json!([
            { "op": "remove", "path": format!("members[value eq \"{grace_id}\"]") },
            { "op": "replace", "value": { "displayName": "Research" } }
        ]),
    )
    .await;
    renamed
        .assert_status(200)
        .assert_json_path("displayName", json!("Research"))
        .assert_json_path("members", json!([]));

    // Lists, filters and excluded members.
    let list = get(&h, &token, "/scim/v2/Groups?filter=displayName%20eq%20%22research%22&excludedAttributes=members").await;
    let list = list.assert_status(200).assert_json_path("totalResults", json!(1));
    assert!(list.json_value()["Resources"][0].get("members").is_none());
    get(&h, &token, "/scim/v2/Groups?filter=displayName%20co%20%22sear%22")
        .await
        .assert_json_path("totalResults", json!(1));

    // PUT replaces the members.
    h.app
        .client()
        .put(&path)
        .bearer(&token)
        .json(&json!({ "schemas": [GROUP], "displayName": "Research", "externalId": "grp-eng", "members": [{ "value": ada_id }, { "value": grace_id }] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("members.1.value", json!(grace_id));

    h.app
        .client()
        .delete(&path)
        .bearer(&token)
        .send()
        .await
        .assert_status(204);
    assert_eq!(h.log.calls().last().unwrap(), "group:delete:grp-eng");
    assert_scim_error(&get(&h, &token, &path).await, 404, None);
    get(&h, &token, &format!("/scim/v2/Users/{ada_id}"))
        .await
        .assert_json_path("groups", json!([]));
}

#[tokio::test]
async fn connections_are_kept_apart() {
    let (h, token, _) = scim().await;
    let other = h.scim(Owner::Workspace(Uuid::now_v7())).await;
    let other_token = other.scim_token.unwrap().expose().clone();
    let ada = post(&h, &token, "/scim/v2/Users", &entra_user("e1", "ada@acme.test")).await;
    let id = ada.json_value()["id"].as_str().unwrap().to_owned();
    // The same person in another tenant is another user.
    post(&h, &other_token, "/scim/v2/Users", &entra_user("e1", "ada@acme.test"))
        .await
        .assert_status(201);
    assert_scim_error(&get(&h, &other_token, &format!("/scim/v2/Users/{id}")).await, 404, None);
    get(&h, &other_token, "/scim/v2/Users")
        .await
        .assert_json_path("totalResults", json!(1));
    // A deletion reaches only its own connection.
    let connection = h.directory().await.connection(other.connection.id).await.unwrap();
    assert_eq!(connection.owner(), other.connection.owner());
}
