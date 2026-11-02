//! A member's role in a workspace limits what they do there: a role that can't
//! write gets read-only, in the panel and in queued actions, whatever they hold
//! app-wide, and `workspace.roles` decides for the models it names.

use super::*;

/// A user with `role` in `workspace` and every ability on notes app-wide.
async fn with_role(world: &World, workspace: Uuid, role: &str) -> TestClient {
    let id = account(&world.app, role).await;
    add_with_role(world, workspace, id, role).await;
    give_role(&world.app, id, "editor", &["helm.view", "notes.*"]).await;
    world.app.acting_as(navree_testing::principal(id))
}

async fn add_with_role(world: &World, workspace: Uuid, user: Uuid, role: &str) {
    let members = world.app.get::<Members>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    uow.scope(members.add(&uow, workspace, user, WorkspaceRole::parse(role)))
        .await
        .unwrap();
    uow.commit().await.unwrap();
}

/// Every write of the resource at `base`, on Acme, is refused as read-only.
async fn assert_read_only(world: &World, client: &TestClient, base: &str) {
    let acme = world.acme;
    let note = world.acme_note;
    let mut writes = vec![
        (
            "create",
            client
                .post(base)
                .json(&json!({ "title": "By a viewer", "workspace_id": acme })),
        ),
        (
            "update",
            client
                .patch(&format!("{base}/{note}"))
                .json(&json!({ "title": "By a viewer" })),
        ),
        ("delete", client.delete(&format!("{base}/{note}"))),
    ];
    if base == NOTES {
        writes.push((
            "action",
            client
                .post(&format!("{base}/actions/shout"))
                .json(&json!({ "ids": [note] })),
        ));
    } else {
        writes.push((
            "action",
            client
                .post(&format!("{base}/actions/locate"))
                .json(&json!({ "ids": [note] })),
        ));
    }
    for (what, request) in writes {
        let request = if base == NOTES {
            request.header("X-Workspace", acme.to_string())
        } else {
            request
        };
        let response = request.send().await;
        assert_eq!(
            response.status().as_u16(),
            403,
            "{what}: {}",
            response.text()
        );
        assert!(
            response.text().contains("workspace_read_only"),
            "{what}: {}",
            response.text()
        );
    }
    assert_eq!(
        note_in(&world.app, acme, note).await.unwrap().title,
        "Acme secret"
    );
}

#[tokio::test]
async fn a_viewer_reads_and_doesnt_write_whatever_they_hold_app_wide() {
    let world = world(app_with_members().await).await;
    let viewer = with_role(&world, world.acme, "viewer").await;
    in_workspace(&viewer, world.acme, NOTES)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(1));
    assert_read_only(&world, &viewer, NOTES).await;
    // A member with the same abilities writes.
    world
        .member
        .patch(&format!("{NOTES}/{}", world.acme_note))
        .header("X-Workspace", world.acme.to_string())
        .json(&json!({ "title": "By a member" }))
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn a_viewer_doesnt_write_through_a_resource_across_workspaces() {
    let world = world(app_with_members().await).await;
    let viewer = with_role(&world, world.acme, "viewer").await;
    assert_read_only(&world, &viewer, EVERY).await;
}

#[tokio::test]
async fn a_viewer_chooses_the_workspace_and_then_only_reads_it() {
    let world = world(app_with_members().await).await;
    let viewer = with_role(&world, world.acme, "viewer").await;
    let response = viewer
        .post("/admin/workspace")
        .header("accept", "text/html")
        .form(&[("workspace", world.acme.to_string().as_str())])
        .send()
        .await
        .assert_status(303);
    assert!(response.set_cookie(WORKSPACE_COOKIE).is_some());
    viewer.get(NOTES).send().await.assert_status(200);
    let response = viewer
        .post(NOTES)
        .json(&json!({ "title": "By a viewer" }))
        .send()
        .await
        .assert_status(403);
    assert!(
        response.text().contains("workspace_read_only"),
        "{}",
        response.text()
    );
}

#[tokio::test]
async fn a_queued_action_refuses_a_member_who_became_a_viewer() {
    use navree_contracts::{Job, JobContext};

    let fake = QueueFake::new();
    let app = with_members(crate::servers::database(TestApp::builder()))
        .override_value::<dyn navree_contracts::Dispatcher>(fake.dispatcher())
        .build()
        .await;
    let world = world(app).await;
    world
        .member
        .post(&format!("{NOTES}/actions/whisper"))
        .header("X-Workspace", world.acme.to_string())
        .json(&json!({ "ids": [world.acme_note] }))
        .send()
        .await
        .assert_status(200);
    let members = world.app.get::<Members>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    uow.scope(members.change_role(&uow, world.acme, world.member_id, WorkspaceRole::Viewer))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let error = fake
        .dispatched::<HelmActionJob>()
        .pop()
        .unwrap()
        .handle(JobContext {
            app: world.app.ctx().clone(),
            job_id: "1".into(),
            attempt: 1,
        })
        .await
        .expect_err("a viewer's action");
    assert!(error.to_string().contains("may not"), "{error}");
    assert_eq!(
        note_in(&world.app, world.acme, world.acme_note)
            .await
            .unwrap()
            .title,
        "Acme secret"
    );
}

#[tokio::test]
async fn a_role_with_no_permissions_of_its_own_is_read_only() {
    let app = with_members(crate::servers::database(TestApp::builder()))
        .config(
            "workspace.roles",
            json!({ "auditor": { "permissions": ["workspace.read"] } }),
        )
        .build()
        .await;
    let world = world(app).await;
    let auditor = with_role(&world, world.acme, "auditor").await;
    assert_read_only(&world, &auditor, NOTES).await;
}

#[tokio::test]
async fn workspace_roles_decide_for_the_models_they_name() {
    let app = with_members(crate::servers::database(TestApp::builder()))
        .config(
            "workspace.roles",
            json!({
                "viewer": { "permissions": ["notes.update"] },
                "member": { "permissions": ["notes.read"] },
                "clerk": { "permissions": ["notes.*"] },
            }),
        )
        .build()
        .await;
    let world = world(app).await;
    let name = world.acme.to_string();
    let note = world.acme_note;
    // A viewer the app lets update notes updates them, and nothing else.
    let viewer = with_role(&world, world.acme, "viewer").await;
    viewer
        .patch(&format!("{NOTES}/{note}"))
        .header("X-Workspace", name.clone())
        .json(&json!({ "title": "Updated by a viewer" }))
        .send()
        .await
        .assert_status(200);
    let response = viewer
        .post(NOTES)
        .header("X-Workspace", name.clone())
        .json(&json!({ "title": "Made by a viewer" }))
        .send()
        .await
        .assert_status(403);
    assert!(
        response.text().contains("workspace_read_only"),
        "{}",
        response.text()
    );
    viewer
        .delete(&format!("{NOTES}/{note}"))
        .header("X-Workspace", name.clone())
        .send()
        .await
        .assert_status(403);
    // A member the app holds to reading notes reads them.
    world
        .member
        .patch(&format!("{NOTES}/{note}"))
        .header("X-Workspace", name.clone())
        .json(&json!({ "title": "Updated by a member" }))
        .send()
        .await
        .assert_status(403);
    in_workspace(&world.member, world.acme, NOTES)
        .send()
        .await
        .assert_status(200);
    // A role of the app's own writes what it names.
    let clerk = with_role(&world, world.acme, "clerk").await;
    clerk
        .post(NOTES)
        .header("X-Workspace", name.clone())
        .json(&json!({ "title": "Made by a clerk" }))
        .send()
        .await
        .assert_status(201);
    // The mapping doesn't take the place of the app-wide ability.
    let reader = account(&world.app, "reader").await;
    add_with_role(&world, world.acme, reader, "clerk").await;
    give_role(&world.app, reader, "reader", &["helm.view", "notes.read"]).await;
    world
        .app
        .acting_as(navree_testing::principal(reader))
        .post(NOTES)
        .header("X-Workspace", name)
        .json(&json!({ "title": "Made by a reader" }))
        .send()
        .await
        .assert_status(403);
}

#[tokio::test]
async fn a_guest_of_a_demo_with_a_read_only_role_doesnt_write_to_it() {
    let app = with_members(crate::servers::database(TestApp::builder()))
        .config("workspace.demo.mode", "reset")
        .config("workspace.demo.guest_role", "viewer")
        .build()
        .await;
    let world = world(app).await;
    let owner = account(&world.app, "demo-owner").await;
    let demos = world.app.get::<DemoWorkspaces>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let demo = uow
        .scope(demos.create(&uow, owner, NewWorkspace::named("Demo")))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    in_workspace(&world.outsider, demo.id, NOTES)
        .send()
        .await
        .assert_status(200);
    let response = world
        .outsider
        .post(NOTES)
        .header("X-Workspace", demo.id.to_string())
        .json(&json!({ "title": "By a guest" }))
        .send()
        .await
        .assert_status(403);
    assert!(
        response.text().contains("workspace_read_only"),
        "{}",
        response.text()
    );
}
