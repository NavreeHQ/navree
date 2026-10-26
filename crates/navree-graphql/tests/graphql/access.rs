//! Field access: deny by default at compile time, checked per field at run
//! time like routes, with `can(.., from = "id")` loading the record.

use async_graphql::{Request, Response, Variables};
use navree_contracts::AuditKind;
use navree_graphql::GraphqlSchema;
use navree_testing::{TestApp, principal};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{app, give_role, insert_post};

async fn run(app: &TestApp, query: &str, viewer: Option<Uuid>, variables: Value) -> Response {
    let mut request = Request::new(query).variables(Variables::from_json(variables));
    if let Some(user) = viewer {
        request = request.data(principal(user));
    }
    app.get::<GraphqlSchema>().await.execute(request).await
}

fn code(response: &Response, index: usize) -> String {
    response.errors[index]
        .extensions
        .as_ref()
        .and_then(|e| e.get("code"))
        .and_then(|code| code.clone().into_json().ok())
        .and_then(|code| code.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[tokio::test]
async fn guests_get_unauthenticated_for_fields_that_need_a_user() {
    let (app, _audit) = app().await;
    let response = run(&app, "{ hello whoami }", None, json!({})).await;
    let data = response.data.clone().into_json().unwrap();
    assert_eq!(
        data["hello"], "hello",
        "a public field in the same query still resolves"
    );
    assert!(data["whoami"].is_null(), "{data}");
    assert_eq!(response.errors.len(), 1);
    assert_eq!(code(&response, 0), "UNAUTHENTICATED");
    assert_eq!(response.errors[0].path.len(), 1);

    let me = Uuid::now_v7();
    let response = run(&app, "{ me whoami }", Some(me), json!({})).await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        response.data.into_json().unwrap(),
        json!({ "me": me, "whoami": me })
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn roles_and_permissions_are_checked_by_the_route_authorizer() {
    let (app, _audit) = app().await;
    let user = Uuid::now_v7();
    let response = run(&app, "{ adminArea }", Some(user), json!({})).await;
    assert_eq!(code(&response, 0), "FORBIDDEN");
    let response = run(&app, "{ canPublish }", Some(user), json!({})).await;
    assert_eq!(code(&response, 0), "FORBIDDEN");

    give_role(&app, user, "admin", &["posts.publish"]).await;
    let response = run(&app, "{ adminArea canPublish }", Some(user), json!({})).await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        response.data.into_json().unwrap(),
        json!({ "adminArea": true, "canPublish": true })
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn can_from_loads_the_record_named_by_the_argument() {
    let (app, audit) = app().await;
    let author = Uuid::now_v7();
    let draft = insert_post(&app, "Draft", Some(author), false).await;
    let query = "query($id: UUID!) { post(id: $id) { id title } }";
    let variables = json!({ "id": draft.id });

    let response = run(&app, query, Some(author), variables.clone()).await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        response.data.into_json().unwrap(),
        json!({ "post": { "id": draft.id, "title": "Draft" } })
    );

    let stranger = Uuid::now_v7();
    let response = run(&app, query, Some(stranger), variables.clone()).await;
    assert_eq!(code(&response, 0), "FORBIDDEN");
    let denied = audit
        .entries()
        .into_iter()
        .find(|entry| entry.action == "access.denied")
        .expect("the refusal is audited");
    assert_eq!(denied.kind, AuditKind::Security);
    assert_eq!(denied.metadata["field"], "post");
    assert_eq!(denied.metadata["route"], "/graphql");
    assert_eq!(denied.actor_id, Some(stranger));

    let response = run(&app, query, None, variables).await;
    assert_eq!(code(&response, 0), "UNAUTHENTICATED");

    let missing = json!({ "id": Uuid::now_v7() });
    let response = run(&app, query, Some(author), missing).await;
    assert_eq!(code(&response, 0), "NOT_FOUND");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_refused_field_leaves_the_other_fields_of_the_query_working() {
    let (app, _audit) = app().await;
    let author = Uuid::now_v7();
    let draft = insert_post(&app, "Draft", Some(author), false).await;
    insert_post(&app, "Public", None, true).await;
    let stranger = Uuid::now_v7();
    let response = run(
        &app,
        "query($id: UUID!) { posts { title } other: post(id: $id) { title } }",
        Some(stranger),
        json!({ "id": draft.id }),
    )
    .await;
    // The refusal is the only error: the shared unit of work stays usable
    // for the other field.
    assert_eq!(response.errors.len(), 1, "{:?}", response.errors);
    assert_eq!(code(&response, 0), "FORBIDDEN");
    assert_eq!(response.errors[0].path.len(), 1);
    let data = response.data.into_json().unwrap();
    assert_eq!(data["posts"], json!([{ "title": "Public" }]), "{data}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn accessible_filters_lists_with_the_viewers_rules() {
    let (app, _audit) = app().await;
    let author = Uuid::now_v7();
    insert_post(&app, "B draft", Some(author), false).await;
    insert_post(&app, "A public", None, true).await;
    insert_post(&app, "C other draft", Some(Uuid::now_v7()), false).await;

    let titles = |response: Response| {
        assert!(response.errors.is_empty(), "{:?}", response.errors);
        response.data.into_json().unwrap()["posts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|post| post["title"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let guest = run(&app, "{ posts { title } }", None, json!({})).await;
    assert_eq!(titles(guest), ["A public"]);
    let mine = run(&app, "{ posts { title } }", Some(author), json!({})).await;
    assert_eq!(titles(mine), ["A public", "B draft"]);

    let response = run(&app, "{ readsPosts }", Some(author), json!({})).await;
    assert_eq!(
        response.data.into_json().unwrap(),
        json!({ "readsPosts": true })
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn authorized_without_a_from_check_is_a_server_error() {
    let (app, _audit) = app().await;
    let response = run(&app, "{ misdeclared }", Some(Uuid::now_v7()), json!({})).await;
    assert_eq!(code(&response, 0), "INTERNAL_SERVER_ERROR");
    assert_eq!(response.errors[0].message, "Internal server error");
    app.shutdown().await.unwrap();
}
