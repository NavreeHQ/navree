//! The source conformance suite: one directory, served by Microsoft Graph,
//! Google Workspace and Okta (wiremock, through real navree-integrations
//! connections), by an Active Directory and an OpenLDAP fixture server,
//! and by the fake. Every source must give the same users and groups.
//! Then what only some sources do: Graph's delta rounds and resyncs,
//! paged LDAP searches, binds and LDAPS.

use std::sync::Arc;

use navree_directory::{
    Directory, DirectoryGroup, DirectoryUser, NewConnection, Owner, RunStatus, SyncOptions,
};
use navree_http_client::{HttpClientModule, HttpClientOptions, SsrfPolicy};
use navree_integrations::{
    ConnectCallback, ConnectOptions, IntegrationsModule, NewApiKey, NewServiceAccount,
    ServiceAccountKey,
};
use navree_testing::TestAppBuilder;
use serde_json::{Value, json};
use uuid::Uuid;
use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::ldap_server::{Entry, LdapServer};
use crate::support::{Harness, harness, real};

const ADA: &str = "6e7b768e-07e2-4810-8459-485f84f8f204";
const GRACE: &str = "8a3c9f2e-1b4d-4e5f-9a6b-7c8d9e0f1a2b";
const ALAN: &str = "2f1e0d9c-8b7a-4c6d-5e4f-3a2b1c0d9e8f";
const ENG: &str = "c0ffee00-1111-4222-8333-444455556666";
const OPS: &str = "deadbeef-7777-4888-9999-aaaabbbbcccc";
const TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

/// A user as every source must give it: id, sign-in name, active, name,
/// email, title, department.
type UserCore = (
    String,
    String,
    bool,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);
/// A group: id, name, members.
type GroupCore = (String, String, Vec<String>);

fn expected() -> (Vec<UserCore>, Vec<GroupCore>) {
    let user = |id: &str, name: &str, active: bool, full: &str, title: Option<&str>, dept: &str| {
        (
            id.to_owned(),
            name.to_owned(),
            active,
            full.to_owned(),
            Some(name.to_owned()),
            title.map(str::to_owned),
            Some(dept.to_owned()),
        )
    };
    (
        vec![
            user(
                ADA,
                "ada@acme.test",
                true,
                "Ada Lovelace",
                Some("Principal engineer"),
                "Engineering",
            ),
            user(
                ALAN,
                "alan@acme.test",
                true,
                "Alan Turing",
                None,
                "Research",
            ),
            user(
                GRACE,
                "grace@acme.test",
                false,
                "Grace Hopper",
                Some("Rear admiral"),
                "Navy",
            ),
        ],
        vec![
            (
                ENG.to_owned(),
                "Engineering".to_owned(),
                vec![ADA.to_owned(), GRACE.to_owned()],
            ),
            (
                OPS.to_owned(),
                "Operations".to_owned(),
                vec![ALAN.to_owned()],
            ),
        ],
    )
}

/// What the directory holds for connection `id`, as compared.
async fn synced(directory: &Directory, id: Uuid) -> (Vec<UserCore>, Vec<GroupCore>) {
    let users = directory
        .users(id)
        .await
        .unwrap()
        .into_iter()
        .map(|u| {
            let user = u.user;
            (
                user.external_id.clone(),
                user.user_name.clone(),
                user.active,
                user.full_name(),
                user.primary_email().map(str::to_owned),
                user.title.clone(),
                user.department.clone(),
            )
        })
        .collect();
    let groups = directory
        .groups(id)
        .await
        .unwrap()
        .into_iter()
        .map(|g| {
            let mut members = g.group.members;
            members.sort();
            (g.group.external_id, g.group.display_name, members)
        })
        .collect();
    (users, groups)
}

/// Syncs connection `id` and checks it holds the expected directory.
async fn sync_and_compare(h: &Harness, id: Uuid) {
    let directory = h.directory().await;
    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded, "{run:?}");
    let (users, groups) = synced(&directory, id).await;
    let (mut want_users, mut want_groups) = expected();
    want_users.sort_by(|a, b| a.1.cmp(&b.1));
    want_groups.sort_by(|a, b| a.1.cmp(&b.1));
    for group in &mut want_groups {
        group.2.sort();
    }
    assert_eq!(users, want_users);
    assert_eq!(groups, want_groups);
}

/// The HTTP client of the real-source tests: loopback allowed (the mock
/// servers), no retries.
fn loopback(builder: TestAppBuilder) -> TestAppBuilder {
    builder
        .module(HttpClientModule::for_root(HttpClientOptions {
            retries: 0,
            ssrf: SsrfPolicy {
                block_private: true,
                allow_hosts: vec!["127.0.0.1".to_owned()],
            },
            ..HttpClientOptions::default()
        }))
        .module(IntegrationsModule::from_config())
        .config("integrations.routes", false)
        .config(
            "integrations.redirect_base",
            "https://app.acme.test/settings/integrations",
        )
}

fn ok(body: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

// -- the fake --------------------------------------------------------------------

#[tokio::test]
async fn the_fake_gives_the_directory() {
    let h = harness(|b| b).await;
    h.fake()
        .user(
            DirectoryUser::new(ADA, "ada@acme.test")
                .name("Ada", "Lovelace")
                .email("ada@acme.test")
                .title("Principal engineer")
                .department("Engineering"),
        )
        .user(
            DirectoryUser::new(GRACE, "grace@acme.test")
                .name("Grace", "Hopper")
                .email("grace@acme.test")
                .title("Rear admiral")
                .department("Navy")
                .inactive(),
        )
        .user(
            DirectoryUser::new(ALAN, "alan@acme.test")
                .name("Alan", "Turing")
                .email("alan@acme.test")
                .department("Research"),
        )
        .group(DirectoryGroup::new(ENG, "Engineering").members([ADA, GRACE]))
        .group(DirectoryGroup::new(OPS, "Operations").members([ALAN]));
    let id = h.connection("ldap").await;
    sync_and_compare(&h, id).await;
}

// -- Microsoft Graph ------------------------------------------------------------

fn graph_user(
    id: &str,
    upn: &str,
    given: &str,
    family: &str,
    enabled: bool,
    title: Option<&str>,
    dept: &str,
) -> Value {
    json!({
        "id": id,
        "userPrincipalName": upn,
        "displayName": format!("{given} {family}"),
        "givenName": given,
        "surname": family,
        "mail": upn,
        "accountEnabled": enabled,
        "jobTitle": title,
        "department": dept,
        "businessPhones": [],
        "usageLocation": "GB"
    })
}

fn graph_users() -> [Value; 3] {
    [
        graph_user(
            ADA,
            "ada@acme.test",
            "Ada",
            "Lovelace",
            true,
            Some("Principal engineer"),
            "Engineering",
        ),
        graph_user(
            GRACE,
            "grace@acme.test",
            "Grace",
            "Hopper",
            false,
            Some("Rear admiral"),
            "Navy",
        ),
        graph_user(
            ALAN,
            "alan@acme.test",
            "Alan",
            "Turing",
            true,
            None,
            "Research",
        ),
    ]
}

/// The first delta round of users and groups, in two pages each.
async fn mount_graph_snapshot(mock: &MockServer) {
    let base = mock.uri();
    let [ada, grace, alan] = graph_users();
    Mock::given(method("GET"))
        .and(path("/v1.0/users/delta"))
        .and(query_param_is_missing("$skiptoken"))
        .and(query_param_is_missing("$deltatoken"))
        .and(header("authorization", "Bearer graph-token"))
        .respond_with(ok(json!({
            "value": [ada, grace],
            "@odata.nextLink": format!("{base}/v1.0/users/delta?$skiptoken=users-2")
        })))
        .mount(mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1.0/users/delta"))
        .and(query_param("$skiptoken", "users-2"))
        .respond_with(ok(json!({
            "value": [alan],
            "@odata.deltaLink": format!("{base}/v1.0/users/delta?$deltatoken=users-d1")
        })))
        .mount(mock)
        .await;
    let user = |id: &str| json!({ "@odata.type": "#microsoft.graph.user", "id": id });
    Mock::given(method("GET"))
        .and(path("/v1.0/groups/delta"))
        .and(query_param_is_missing("$skiptoken"))
        .and(query_param_is_missing("$deltatoken"))
        .respond_with(ok(json!({
            "value": [{ "id": ENG, "displayName": "Engineering", "members@delta": [user(ADA)] }],
            "@odata.nextLink": format!("{base}/v1.0/groups/delta?$skiptoken=groups-2")
        })))
        .mount(mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1.0/groups/delta"))
        .and(query_param("$skiptoken", "groups-2"))
        .respond_with(ok(json!({
            "value": [
                { "id": ENG, "members@delta": [user(GRACE)] },
                { "id": OPS, "displayName": "Operations", "description": "Runs things",
                  "members@delta": [user(ALAN), { "@odata.type": "#microsoft.graph.group", "id": ENG }] }
            ],
            "@odata.deltaLink": format!("{base}/v1.0/groups/delta?$deltatoken=groups-d1")
        })))
        .mount(mock)
        .await;
}

/// An app connected to Microsoft 365 (admin consent, scope set
/// `directory`) on `mock`, and a `microsoft` directory connection.
async fn graph(mock: &MockServer) -> (Harness, Uuid) {
    let uri = mock.uri();
    Mock::given(method("POST"))
        .and(path(format!("/{TENANT}/oauth2/v2.0/token")))
        .respond_with(ok(
            json!({ "token_type": "Bearer", "expires_in": 3599, "access_token": "graph-token" }),
        ))
        .mount(mock)
        .await;
    let h = real(move |b| {
        loopback(b).config(
            "integrations.providers.microsoft",
            json!({
                "client_id": "0f6c1d2e-8a3b-4c5d-9e7f-1a2b3c4d5e6f",
                "client_secret": "ms-secret",
                "authority_url": uri,
                "api_url": format!("{uri}/v1.0")
            }),
        )
    })
    .await;
    let integrations = h.app.get::<navree_integrations::Integrations>().await;
    let state = integrations
        .connect_url(
            Owner::App,
            "microsoft",
            ConnectOptions::default().scope_set("directory"),
        )
        .await
        .unwrap()
        .state;
    integrations
        .complete(
            Owner::App,
            "microsoft",
            &ConnectCallback::admin_consent(state, TENANT),
            None,
        )
        .await
        .unwrap();
    let id = h.connection("microsoft").await;
    (h, id)
}

#[tokio::test]
async fn microsoft_graph_gives_the_directory() {
    let mock = MockServer::start().await;
    mount_graph_snapshot(&mock).await;
    let (h, id) = graph(&mock).await;
    sync_and_compare(&h, id).await;
    let ada = h.directory().await.user(id, ADA).await.unwrap().unwrap();
    assert_eq!(ada.user.attributes["usageLocation"], json!("GB"));
}

#[tokio::test]
async fn microsoft_graph_delta_rounds_bring_only_changes_and_resync_when_expired() {
    let mock = MockServer::start().await;
    mount_graph_snapshot(&mock).await;
    let (h, id) = graph(&mock).await;
    let directory = h.directory().await;
    directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    h.log.clear();

    // Round two: Ada's title changed (Graph sends only the changed
    // property), Alan was deleted, Grace left Engineering.
    let base = mock.uri();
    Mock::given(method("GET"))
        .and(path("/v1.0/users/delta"))
        .and(query_param("$deltatoken", "users-d1"))
        .respond_with(ok(json!({
            "value": [
                { "id": ADA, "jobTitle": "Chief engineer" },
                { "id": ALAN, "@removed": { "reason": "changed" } }
            ],
            "@odata.deltaLink": format!("{base}/v1.0/users/delta?$deltatoken=users-d2")
        })))
        .mount(&mock)
        .await;
    let mut ada = graph_users()[0].clone();
    ada["jobTitle"] = json!("Chief engineer");
    Mock::given(method("GET"))
        .and(path("/v1.0/users"))
        .and(query_param("$filter", format!("id in ('{ADA}')").as_str()))
        .respond_with(ok(json!({ "value": [ada] })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1.0/groups/delta"))
        .and(query_param("$deltatoken", "groups-d1"))
        .respond_with(ok(json!({
            "value": [{ "id": ENG, "members@delta": [
                { "@odata.type": "#microsoft.graph.user", "id": GRACE, "@removed": { "reason": "deleted" } }
            ] }],
            "@odata.deltaLink": format!("{base}/v1.0/groups/delta?$deltatoken=groups-d2")
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1.0/groups"))
        .and(query_param("$filter", format!("id in ('{ENG}')").as_str()))
        .respond_with(ok(
            json!({ "value": [{ "id": ENG, "displayName": "Engineering" }] }),
        ))
        .mount(&mock)
        .await;
    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded, "{run:?}");
    assert_eq!(run.complete, Some(false));
    assert_eq!(
        h.log.calls(),
        vec![
            format!("user:update:{ADA}"),
            format!("group:update:{ENG}:+-{GRACE}"),
            format!("user:deactivate:{ALAN}"),
        ]
    );
    let ada = directory.user(id, ADA).await.unwrap().unwrap();
    assert_eq!(ada.user.title.as_deref(), Some("Chief engineer"));
    assert_eq!(
        ada.user.department.as_deref(),
        Some("Engineering"),
        "re-read in full"
    );

    // Round three: the delta link expired (410), so the source starts over
    // with a snapshot, which brings Alan back.
    Mock::given(method("GET"))
        .and(path("/v1.0/users/delta"))
        .and(query_param("$deltatoken", "users-d2"))
        .respond_with(ResponseTemplate::new(410).set_body_json(json!({
            "error": { "code": "syncStateNotFound", "message": "The sync state is no longer valid" }
        })))
        .mount(&mock)
        .await;
    h.log.clear();
    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.complete, Some(true));
    assert_eq!(
        h.log.calls(),
        vec![
            format!("user:update:{ADA}"),
            format!("user:reactivate:{ALAN}"),
            format!("group:update:{ENG}:+local-{GRACE}-"),
        ]
    );
}

#[tokio::test]
async fn a_graph_refusal_fails_the_run_with_graphs_message() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1.0/users/delta"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": { "code": "Authorization_RequestDenied", "message": "Insufficient privileges to complete the operation." }
        })))
        .mount(&mock)
        .await;
    let (h, id) = graph(&mock).await;
    let run = h
        .directory()
        .await
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(
        run.error.as_deref(),
        Some(
            "Microsoft Graph could not be read: 403: Insufficient privileges to complete the operation."
        )
    );
}

// -- Google Workspace -----------------------------------------------------------

fn google_user(
    id: &str,
    email: &str,
    given: &str,
    family: &str,
    suspended: bool,
    title: Option<&str>,
    dept: &str,
) -> Value {
    let mut organization = json!({ "department": dept, "primary": true });
    if let Some(title) = title {
        organization["title"] = json!(title);
    }
    json!({
        "id": id,
        "primaryEmail": email,
        "name": { "givenName": given, "familyName": family, "fullName": format!("{given} {family}") },
        "suspended": suspended,
        "organizations": [organization],
        "orgUnitPath": "/"
    })
}

#[tokio::test]
async fn google_workspace_gives_the_directory() {
    let mock = MockServer::start().await;
    let uri = mock.uri();
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ok(
            json!({ "token_type": "Bearer", "expires_in": 3599, "access_token": "google-token" }),
        ))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/google/admin/directory/v1/users"))
        .and(query_param("customer", "my_customer"))
        .and(query_param_is_missing("pageToken"))
        .and(header("authorization", "Bearer google-token"))
        .respond_with(ok(json!({
            "users": [
                google_user(ADA, "ada@acme.test", "Ada", "Lovelace", false, Some("Principal engineer"), "Engineering"),
                google_user(GRACE, "grace@acme.test", "Grace", "Hopper", true, Some("Rear admiral"), "Navy")
            ],
            "nextPageToken": "page-2"
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/google/admin/directory/v1/users"))
        .and(query_param("pageToken", "page-2"))
        .respond_with(ok(json!({
            "users": [google_user(ALAN, "alan@acme.test", "Alan", "Turing", false, None, "Research")]
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/google/admin/directory/v1/groups"))
        .respond_with(ok(json!({ "groups": [
            { "id": ENG, "email": "eng@acme.test", "name": "Engineering" },
            { "id": OPS, "email": "ops@acme.test", "name": "Operations" }
        ] })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/google/admin/directory/v1/groups/{ENG}/members"
        )))
        .respond_with(ok(json!({ "members": [
            { "id": ADA, "type": "USER" }, { "id": GRACE, "type": "USER" }
        ] })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/google/admin/directory/v1/groups/{OPS}/members"
        )))
        .respond_with(ok(json!({ "members": [
            { "id": ALAN, "type": "USER" }, { "id": ENG, "type": "GROUP" }
        ] })))
        .mount(&mock)
        .await;
    let h = real(move |b| {
        loopback(b).config(
            "integrations.providers.google",
            json!({ "token_url": format!("{uri}/token"), "api_url": format!("{uri}/google") }),
        )
    })
    .await;
    let key = ServiceAccountKey::from_value(&json!({
        "type": "service_account",
        "project_id": "acme-prod",
        "private_key_id": "4f2a9c",
        "private_key": include_str!("../fixtures/rsa-2048.pem"),
        "client_email": "navree@acme-prod.iam.gserviceaccount.com",
        "client_id": "109318734729811",
        "token_uri": "https://oauth2.googleapis.com/token"
    }))
    .unwrap();
    h.app
        .get::<navree_integrations::Integrations>()
        .await
        .connect_service_account(
            Owner::App,
            "google",
            NewServiceAccount {
                scope_set: "directory".to_owned(),
                key,
                subject: Some("admin@acme.test".to_owned()),
                connected_by: None,
            },
        )
        .await
        .unwrap();
    let id = h
        .directory()
        .await
        .create_connection(
            Owner::App,
            NewConnection::new("Acme Google Workspace", "google")
                .settings(json!({ "admin_email": "admin@acme.test" })),
        )
        .await
        .unwrap()
        .connection
        .id;
    sync_and_compare(&h, id).await;
}

// -- Okta -------------------------------------------------------------------------

fn okta_user(
    id: &str,
    login: &str,
    first: &str,
    last: &str,
    status: &str,
    title: Option<&str>,
    dept: &str,
) -> Value {
    json!({
        "id": id,
        "status": status,
        "profile": {
            "login": login, "email": login, "firstName": first, "lastName": last,
            "displayName": format!("{first} {last}"), "title": title, "department": dept
        }
    })
}

#[tokio::test]
async fn okta_gives_the_directory() {
    let mock = MockServer::start().await;
    let uri = mock.uri();
    Mock::given(method("GET"))
        .and(path("/okta/api/v1/users"))
        .and(query_param_is_missing("after"))
        .and(header("authorization", "SSWS 00okta-api-token"))
        .respond_with(
            ok(json!([
                okta_user(
                    ADA,
                    "ada@acme.test",
                    "Ada",
                    "Lovelace",
                    "ACTIVE",
                    Some("Principal engineer"),
                    "Engineering"
                ),
                okta_user(
                    GRACE,
                    "grace@acme.test",
                    "Grace",
                    "Hopper",
                    "SUSPENDED",
                    Some("Rear admiral"),
                    "Navy"
                )
            ]))
            .append_header(
                "link",
                format!("<{uri}/okta/api/v1/users?limit=200>; rel=\"self\"").as_str(),
            )
            .append_header(
                "link",
                format!("<{uri}/okta/api/v1/users?after=p2&limit=200>; rel=\"next\"").as_str(),
            ),
        )
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/okta/api/v1/users"))
        .and(query_param("after", "p2"))
        .respond_with(ok(json!([okta_user(
            ALAN,
            "alan@acme.test",
            "Alan",
            "Turing",
            "ACTIVE",
            None,
            "Research"
        )])))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/okta/api/v1/groups"))
        .respond_with(ok(json!([
            { "id": ENG, "type": "OKTA_GROUP", "profile": { "name": "Engineering" } },
            { "id": OPS, "type": "OKTA_GROUP", "profile": { "name": "Operations" } }
        ])))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/okta/api/v1/groups/{ENG}/users")))
        .respond_with(ok(json!([{ "id": ADA }, { "id": GRACE }])))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/okta/api/v1/groups/{OPS}/users")))
        .respond_with(ok(json!([{ "id": ALAN }])))
        .mount(&mock)
        .await;
    let h = real(|b| {
        loopback(b).config(
            "integrations.providers.okta",
            json!({ "type": "api_key", "name": "Okta", "scheme": "SSWS" }),
        )
    })
    .await;
    h.app
        .get::<navree_integrations::Integrations>()
        .await
        .connect_api_key(
            Owner::App,
            "okta",
            NewApiKey {
                scope_set: "directory".to_owned(),
                key: "00okta-api-token".to_owned(),
                api_url: Some(format!("{}/okta", mock.uri())),
                fields: Default::default(),
                connected_by: None,
            },
        )
        .await
        .unwrap();
    let id = h.connection("okta").await;
    sync_and_compare(&h, id).await;
}

#[tokio::test]
async fn a_pull_source_without_its_integration_connection_fails_the_run() {
    let h = real(|b| {
        loopback(b).config(
            "integrations.providers.okta",
            json!({ "type": "api_key", "name": "Okta", "scheme": "SSWS" }),
        )
    })
    .await;
    let id = h.connection("okta").await;
    let run = h
        .directory()
        .await
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("Okta is not connected"));
}

// -- LDAP and Active Directory -------------------------------------------------

/// The directory as Active Directory holds it.
fn active_directory() -> Vec<Entry> {
    let guid = |id: &str| Uuid::parse_str(id).unwrap().to_bytes_le().to_vec();
    let person = |dn: &str, id: &str, upn: &str, given: &str, family: &str, uac: &str| {
        Entry::new(dn)
            .attr(
                "objectClass",
                &["top", "person", "organizationalPerson", "user"],
            )
            .attr("objectCategory", &["person"])
            .binary("objectGUID", guid(id))
            .attr("userPrincipalName", &[upn])
            .attr("sAMAccountName", &[upn.split('@').next().unwrap()])
            .attr("givenName", &[given])
            .attr("sn", &[family])
            .attr("displayName", &[&format!("{given} {family}")])
            .attr("mail", &[upn])
            .attr("userAccountControl", &[uac])
    };
    vec![
        person(
            "CN=Ada Lovelace,OU=People,DC=acme,DC=test",
            ADA,
            "ada@acme.test",
            "Ada",
            "Lovelace",
            "512",
        )
        .attr("title", &["Principal engineer"])
        .attr("department", &["Engineering"])
        .attr("memberOf", &["CN=Engineering,OU=Groups,DC=acme,DC=test"]),
        person(
            "CN=Grace Hopper,OU=People,DC=acme,DC=test",
            GRACE,
            "grace@acme.test",
            "Grace",
            "Hopper",
            "514",
        )
        .attr("title", &["Rear admiral"])
        .attr("department", &["Navy"]),
        person(
            "CN=Alan Turing,OU=People,DC=acme,DC=test",
            ALAN,
            "alan@acme.test",
            "Alan",
            "Turing",
            "66048",
        )
        .attr("department", &["Research"]),
        Entry::new("CN=Engineering,OU=Groups,DC=acme,DC=test")
            .attr("objectClass", &["top", "group"])
            .binary("objectGUID", guid(ENG))
            .attr("cn", &["Engineering"])
            .attr("member", &["CN=Grace Hopper,OU=People,DC=acme,DC=test"]),
        Entry::new("CN=Operations,OU=Groups,DC=acme,DC=test")
            .attr("objectClass", &["top", "group"])
            .binary("objectGUID", guid(OPS))
            .attr("cn", &["Operations"])
            .attr(
                "member",
                &[
                    "CN=Alan Turing, OU=People, DC=acme, DC=test",
                    "CN=Engineering,OU=Groups,DC=acme,DC=test",
                ],
            ),
        // Not a user: a computer account.
        Entry::new("CN=DC1,OU=Domain Controllers,DC=acme,DC=test")
            .attr("objectClass", &["top", "computer"])
            .attr("objectCategory", &["computer"]),
    ]
}

const BIND_DN: &str = "CN=navree-sync,OU=Service,DC=acme,DC=test";

/// The fixture servers listen on 127.0.0.1, which the LDAP host policy
/// refuses unless it is allowed.
fn loopback_ldap(builder: TestAppBuilder) -> TestAppBuilder {
    builder.config("directory.ldap.allow_hosts", json!(["127.0.0.1"]))
}

#[tokio::test]
async fn ldap_connections_may_not_reach_the_apps_own_network_by_default() {
    let server = LdapServer::start(active_directory(), None).await;
    let h = real(|b| b).await;
    let id = ldap_connection(
        &h,
        json!({ "url": server.url(), "base_dn": "DC=acme,DC=test" }),
        None,
    )
    .await;
    let run = h
        .directory()
        .await
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(
        run.error.as_deref(),
        Some(
            "LDAP could not be read: 127.0.0.1 resolves to a private address (127.0.0.1); allow it in directory.ldap.allow_hosts"
        )
    );
    assert!(server.searches().is_empty() && server.binds().is_empty());
}

async fn ldap_connection(h: &Harness, settings: Value, secret: Option<&str>) -> Uuid {
    let mut connection = NewConnection::new("Acme AD", "ldap").settings(settings);
    if let Some(secret) = secret {
        connection = connection.secret(secret);
    }
    h.directory()
        .await
        .create_connection(Owner::App, connection)
        .await
        .unwrap()
        .connection
        .id
}

#[tokio::test]
async fn active_directory_gives_the_directory_through_paged_searches() {
    let server = LdapServer::start(active_directory(), Some((BIND_DN, "s3cret!"))).await;
    let h = real(loopback_ldap).await;
    let id = ldap_connection(
        &h,
        json!({ "url": server.url(), "base_dn": "DC=acme,DC=test", "bind_dn": BIND_DN, "page_size": 2 }),
        Some("s3cret!"),
    )
    .await;
    sync_and_compare(&h, id).await;
    assert_eq!(server.binds(), vec![BIND_DN]);
    let searches = server.searches();
    // Users in two pages of two, then groups in one.
    assert_eq!(
        searches
            .iter()
            .map(|s| (s.page_size, s.returned))
            .collect::<Vec<_>>(),
        vec![(Some(2), 2), (Some(2), 1), (Some(2), 2)]
    );
    let ada = h.directory().await.user(id, ADA).await.unwrap().unwrap();
    assert_eq!(
        ada.user.attributes["dn"],
        json!("CN=Ada Lovelace,OU=People,DC=acme,DC=test")
    );
}

#[tokio::test]
async fn a_refused_bind_fails_the_run_without_the_password() {
    let server = LdapServer::start(active_directory(), Some((BIND_DN, "s3cret!"))).await;
    let h = real(loopback_ldap).await;
    let id = ldap_connection(
        &h,
        json!({ "url": server.url(), "base_dn": "DC=acme,DC=test", "bind_dn": BIND_DN }),
        Some("wrong password"),
    )
    .await;
    let run = h
        .directory()
        .await
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    let error = run.error.unwrap();
    assert!(
        error.starts_with(&format!(
            "LDAP could not be read: the bind as {BIND_DN} failed: result code 49"
        )),
        "{error}"
    );
    assert!(!error.contains("wrong password"));
    // An unreachable server fails too.
    let id = ldap_connection(
        &h,
        json!({ "url": "ldap://127.0.0.1:1", "base_dn": "DC=acme,DC=test", "timeout_secs": 2 }),
        None,
    )
    .await;
    let run = h
        .directory()
        .await
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert!(
        run.error
            .unwrap()
            .starts_with("LDAP could not be read: could not connect to ldap://127.0.0.1:1")
    );
}

#[tokio::test]
async fn openldap_over_ldaps_with_the_servers_certificate() {
    let guid = |id: &str| id.to_owned();
    let person = |uid: &str, id: &str, given: &str, family: &str| {
        Entry::new(&format!("uid={uid},ou=people,dc=acme,dc=test"))
            .attr("objectClass", &["inetOrgPerson"])
            .attr("entryUUID", &[&guid(id)])
            .attr("uid", &[&format!("{uid}@acme.test")])
            .attr("cn", &[&format!("{given} {family}")])
            .attr("givenName", &[given])
            .attr("sn", &[family])
            .attr("mail", &[&format!("{uid}@acme.test")])
    };
    let entries = vec![
        person("ada", ADA, "Ada", "Lovelace")
            .attr("title", &["Principal engineer"])
            .attr("departmentNumber", &["Engineering"]),
        person("grace", GRACE, "Grace", "Hopper")
            .attr("title", &["Rear admiral"])
            .attr("pwdAccountLockedTime", &["000001010000Z"]),
        person("alan", ALAN, "Alan", "Turing"),
        Entry::new("cn=engineering,ou=groups,dc=acme,dc=test")
            .attr("objectClass", &["groupOfUniqueNames"])
            .attr("entryUUID", &[ENG])
            .attr("cn", &["Engineering"])
            .attr(
                "uniqueMember",
                &[
                    "uid=ada,ou=people,dc=acme,dc=test",
                    "uid=grace,ou=people,dc=acme,dc=test",
                ],
            ),
        Entry::new("cn=operations,ou=groups,dc=acme,dc=test")
            .attr("objectClass", &["posixGroup"])
            .attr("entryUUID", &[OPS])
            .attr("cn", &["Operations"])
            .attr("memberUid", &["ALAN@acme.test"]),
    ];
    let (server, pem) = LdapServer::start_tls(entries, None).await;
    let h = real(loopback_ldap).await;
    let id = ldap_connection(
        &h,
        json!({
            "url": server.url(),
            "base_dn": "dc=acme,dc=test",
            "flavor": "openldap",
            "ca_certificate": pem,
            "attributes": ["departmentNumber"]
        }),
        None,
    )
    .await;
    let directory = h.directory().await;
    let run = directory
        .sync_now(id, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded, "{run:?}");
    let (users, groups) = synced(&directory, id).await;
    let names: Vec<(&str, bool)> = users.iter().map(|u| (u.1.as_str(), u.2)).collect();
    assert_eq!(
        names,
        vec![
            ("ada@acme.test", true),
            ("alan@acme.test", true),
            ("grace@acme.test", false)
        ]
    );
    assert_eq!(groups, expected().1);
    let ada = directory.user(id, ADA).await.unwrap().unwrap();
    assert_eq!(
        ada.user.attributes["departmentNumber"],
        json!("Engineering")
    );
    assert!(server.binds().is_empty(), "an anonymous search");

    // Without the certificate the TLS handshake fails.
    let untrusted = ldap_connection(
        &h,
        json!({ "url": server.url(), "base_dn": "dc=acme,dc=test", "flavor": "openldap" }),
        None,
    )
    .await;
    let run = directory
        .sync_now(untrusted, SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Failed, "{run:?}");
}

#[tokio::test]
async fn ldap_settings_are_checked_when_saved() {
    let h = real(|b| b).await;
    let directory = Arc::clone(&h.directory().await);
    let error = directory
        .create_connection(
            Owner::App,
            NewConnection::new("AD", "ldap").settings(json!({ "url": "ldap://dc1.acme.test" })),
        )
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "settings.base_dn is required");
    let error = directory
        .create_connection(
            Owner::App,
            NewConnection::new("AD", "ldap").settings(json!({
                "url": "ldap://dc1.acme.test", "base_dn": "DC=acme,DC=test", "bind_dn": BIND_DN
            })),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "settings.bind_dn needs the bind password as the connection's secret"
    );
    let error = directory
        .create_connection(
            Owner::App,
            NewConnection::new("G", "google").settings(json!({ "admin_email": "nope" })),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "settings.admin_email must be an email address"
    );
    assert_eq!(
        directory.sources().unwrap(),
        vec!["scim", "google", "ldap", "microsoft", "okta"]
    );
}
