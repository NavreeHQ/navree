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
