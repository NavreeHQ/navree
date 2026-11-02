//! What the pages offer follows what the workspace takes from the user: a
//! role that only reads sees no button that writes, and the server refuses
//! what it asks for anyway.

use super::*;

/// A browser navigation to `path`, working in `workspace`.
async fn html_in(client: &TestClient, workspace: Uuid, path: &str) -> TestResponse {
    client
        .get(path)
        .header("accept", "text/html")
        .header("X-Workspace", workspace.to_string())
        .send()
        .await
}

fn plain(response: TestResponse) -> String {
    response.text().replace("&#x2f;", "/")
}

/// A user with `role` in `workspace` and every ability on notes app-wide.
async fn with_role(world: &World, workspace: Uuid, role: &str) -> TestClient {
    let id = account(&world.app, role).await;
    let members = world.app.get::<Members>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    uow.scope(members.add(&uow, workspace, id, WorkspaceRole::parse(role)))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    give_role(&world.app, id, "editor", &["helm.view", "notes.*"]).await;
    world.app.acting_as(navree_testing::principal(id))
}

#[tokio::test]
async fn a_viewer_is_not_offered_what_their_role_refuses_and_the_server_still_refuses_it() {
    let world = world(app_with_members().await).await;
    let viewer = with_role(&world, world.acme, "viewer").await;
    let note = world.acme_note;

    let list = plain(
        html_in(&viewer, world.acme, "/admin/resources/notes")
            .await
            .assert_status(200),
    );
    assert!(list.contains("Acme secret"), "{list}");
    assert!(!list.contains("value=\"shout\""), "{list}");
    assert!(!list.contains("value=\"whisper\""), "{list}");
    assert!(!list.contains("name=\"ids\""), "no rows to tick: {list}");
    assert!(!list.contains("/notes/new"), "{list}");
    let detail = plain(
        html_in(
            &viewer,
            world.acme,
            &format!("/admin/resources/notes/{note}"),
        )
        .await
        .assert_status(200),
    );
    assert!(detail.contains("Acme secret"), "{detail}");
    assert!(!detail.contains("/edit"), "{detail}");
    assert!(!detail.contains("/delete"), "{detail}");
    assert!(!detail.contains("value=\"shout\""), "{detail}");

    // A member of the same workspace with the same abilities is offered them.
    let list = plain(
        html_in(&world.member, world.acme, "/admin/resources/notes")
            .await
            .assert_status(200),
    );
    assert!(list.contains("value=\"shout\""), "{list}");
    assert!(list.contains("/notes/new"), "{list}");
    let detail = plain(
        html_in(
            &world.member,
            world.acme,
            &format!("/admin/resources/notes/{note}"),
        )
        .await
        .assert_status(200),
    );
    assert!(detail.contains("/edit"), "{detail}");
    assert!(detail.contains("/delete"), "{detail}");

    // The API says what the page shows.
    let body = in_workspace(&viewer, world.acme, &format!("{NOTES}/{note}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body["data"]["can"],
        json!({ "update": false, "delete": false })
    );
    let meta = in_workspace(&viewer, world.acme, "/admin/api/resources")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let notes = meta["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["slug"] == "notes")
        .unwrap();
    assert_eq!(notes["can"], json!({ "create": false }));

    // The same forms posted by hand are refused, as before.
    for (path, form) in [
        (
            "/admin/resources/notes/actions".to_owned(),
            vec![("action", "shout".to_owned()), ("ids", note.to_string())],
        ),
        (
            "/admin/resources/notes".to_owned(),
            vec![("title", "Planted".to_owned())],
        ),
        (
            format!("/admin/resources/notes/{note}"),
            vec![("title", "Changed".to_owned())],
        ),
        (
            format!("/admin/resources/notes/{note}/delete"),
            vec![("_confirmed", "1".to_owned())],
        ),
    ] {
        let form: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
        viewer
            .post(&path)
            .header("accept", "text/html")
            .header("X-Workspace", world.acme.to_string())
            .form(&form)
            .send()
            .await
            .assert_status(403);
    }
    assert_eq!(
        note_in(&world.app, world.acme, note).await.unwrap().title,
        "Acme secret"
    );
}

#[tokio::test]
async fn the_pages_of_a_workspace_stay_shut_to_a_non_member_and_a_removed_member() {
    let world = world(app_with_members().await).await;
    for path in ["/admin/resources/notes", "/admin"] {
        // Someone in no workspace who names one, and the member of one who names the other.
        html_in(&world.outsider, world.acme, path)
            .await
            .assert_status(403);
        html_in(&world.member, world.globex, path)
            .await
            .assert_status(403);
    }
    html_in(&world.member, world.acme, "/admin/resources/notes")
        .await
        .assert_status(200);
    leave(&world.app, world.acme, world.member_id).await;
    html_in(&world.member, world.acme, "/admin/resources/notes")
        .await
        .assert_status(403);
}

fn rows_of(csv: &str) -> Vec<String> {
    csv.split("\r\n")
        .skip(1)
        .filter(|line| !line.is_empty())
        .map(|line| line.split(',').next().unwrap_or_default().to_owned())
        .collect()
}

async fn export_in(client: &TestClient, workspace: Uuid, path: &str) -> TestResponse {
    html_in(
        client,
        workspace,
        &format!("/admin/resources/{path}/export.csv"),
    )
    .await
}

async fn counts_in(client: &TestClient, workspace: Uuid) -> TestResponse {
    client
        .get("/admin/counts")
        .header("X-Workspace", workspace.to_string())
        .send()
        .await
}

#[tokio::test]
async fn an_export_holds_the_rows_of_the_workspace_it_was_asked_in_and_nobody_elses() {
    let world = world(app_with_members().await).await;
    let viewer = with_role(&world, world.acme, "viewer").await;
    for client in [&world.member, &viewer] {
        // A member, and a role that only reads (an export reads).
        let body = export_in(client, world.acme, "notes")
            .await
            .assert_status(200)
            .text();
        assert_eq!(rows_of(&body), ["Acme secret"], "{body}");
        assert!(!body.contains("Globex secret"));
    }
    // Someone in no workspace, and a member naming another one.
    export_in(&world.outsider, world.acme, "notes")
        .await
        .assert_status(403);
    export_in(&world.outsider, world.globex, "notes")
        .await
        .assert_status(403);
    export_in(&world.member, world.globex, "notes")
        .await
        .assert_status(403);
    // A workspace nobody made.
    export_in(&world.member, Uuid::now_v7(), "notes")
        .await
        .assert_status(403);
    // Without one, a resource of workspaces asks for one: it exports nothing.
    let response = world
        .member
        .get("/admin/resources/notes/export.csv")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(400);
    assert!(response.text().contains("Choose a workspace first"));
    // A member who is removed is out at once, the cookie of their last visit included.
    let response = world
        .member
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", world.acme.to_string().as_str())])
        .send()
        .await
        .assert_status(303);
    assert!(response.set_cookie(WORKSPACE_COOKIE).is_some());
    world
        .member
        .get("/admin/resources/notes/export.csv")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200);
    leave(&world.app, world.acme, world.member_id).await;
    world
        .member
        .get("/admin/resources/notes/export.csv")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(400);
    export_in(&world.member, world.acme, "notes")
        .await
        .assert_status(403);
}

#[tokio::test]
async fn an_export_of_a_resource_across_workspaces_is_the_list_of_the_user() {
    let world = world(app_with_members().await).await;
    seed(&world.app, world.globex, "Globex other").await;
    let listed = world
        .member
        .get("/admin/api/resources/all-notes?per_page=100")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let mut expected = titles_of(&listed);
    let body = world
        .member
        .get("/admin/resources/all-notes/export.csv")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    let mut exported = rows_of(&body);
    exported.sort();
    expected.sort();
    assert_eq!(exported, expected, "{body}");
}

#[tokio::test]
async fn counts_are_per_user_and_per_workspace_and_cached_without_leaking() {
    let world = world(app_with_members().await).await;
    // A member of both, and more notes in Globex than in Acme.
    let both = account(&world.app, "both").await;
    join(&world.app, world.acme, both).await;
    join(&world.app, world.globex, both).await;
    give_role(&world.app, both, "editor", &["helm.view", "notes.*"]).await;
    let both = world.app.acting_as(navree_testing::principal(both));
    seed(&world.app, world.globex, "Globex two").await;
    seed(&world.app, world.globex, "Globex three").await;
    // Everyone who is going to ask is made first: a role given to anyone makes
    // every count be read again.
    let viewer = with_role(&world, world.acme, "viewer").await;

    let in_acme = counts_in(&both, world.acme)
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(in_acme["counts"]["notes"]["text"], "1", "{in_acme}");
    let in_globex = counts_in(&both, world.globex)
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(in_globex["counts"]["notes"]["text"], "3", "{in_globex}");
    // Cached, each under its own workspace: the first is not handed to the second.
    seed(&world.app, world.acme, "Acme two").await;
    let again = counts_in(&both, world.acme).await.json_value();
    assert_eq!(again["counts"]["notes"]["text"], "1");
    assert_eq!(
        counts_in(&both, world.globex).await.json_value()["counts"]["notes"]["text"],
        "3"
    );
    // A read-only role is counted for as a member is: counting reads.
    let seen = counts_in(&viewer, world.acme)
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(seen["counts"]["notes"]["text"], "2", "{seen}");
    // The count of a resource across workspaces is what its list shows the user.
    let every = world
        .member
        .get("/admin/api/resources/all-notes")
        .send()
        .await
        .json_value()["meta"]["total"]
        .as_u64()
        .unwrap()
        .to_string();
    let seen = counts_in(&world.member, world.acme).await.json_value();
    assert_eq!(seen["counts"]["all-notes"]["text"], json!(every));

    // A count is not handed to a user who may not have it.
    counts_in(&world.outsider, world.acme)
        .await
        .assert_status(403);
    counts_in(&world.member, world.globex)
        .await
        .assert_status(403);
    counts_in(&world.outsider, world.globex)
        .await
        .assert_status(403);
    // A stale cookie counts for nothing: what belongs to a workspace has no count
    // outside one, and the others are counted as ever.
    world
        .outsider
        .set_cookie(WORKSPACE_COOKIE, &world.globex.to_string());
    let body = world
        .outsider
        .get("/admin/counts")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(body["counts"].get("notes").is_none(), "{body}");
    // A member who is removed is out at once, cached count or not.
    counts_in(&world.member, world.acme)
        .await
        .assert_status(200);
    leave(&world.app, world.acme, world.member_id).await;
    counts_in(&world.member, world.acme)
        .await
        .assert_status(403);
}
