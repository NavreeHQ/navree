//! A read-only demo refuses writes whichever way they come: from staff, and
//! through a resource across workspaces.

use super::*;

async fn demo_of(world: &World) -> (Uuid, Uuid) {
    let owner = account(&world.app, "demo-owner").await;
    let demos = world.app.get::<DemoWorkspaces>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let demo = uow
        .scope(demos.create(&uow, owner, NewWorkspace::named("Demo")))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let note = seed(&world.app, demo.id, "Demo note").await;
    (demo.id, note)
}

#[tokio::test]
async fn staff_dont_write_to_a_read_only_demo() {
    let (world, staff) = staff_world().await;
    let (demo, note) = demo_of(&world).await;
    in_workspace(&staff, demo, NOTES)
        .send()
        .await
        .assert_status(200);
    staff
        .post(NOTES)
        .header("X-Workspace", demo.to_string())
        .json(&json!({ "title": "Staff in the demo" }))
        .send()
        .await
        .assert_error_code(403, "demo_read_only");
    staff
        .patch(&format!("{NOTES}/{note}"))
        .header("X-Workspace", demo.to_string())
        .json(&json!({ "title": "Changed by staff" }))
        .send()
        .await
        .assert_error_code(403, "demo_read_only");
    staff
        .post(EVERY)
        .json(&json!({ "title": "Staff by form", "workspace_id": demo }))
        .send()
        .await
        .assert_error_code(403, "demo_read_only");
    assert_eq!(
        note_in(&world.app, demo, note).await.unwrap().title,
        "Demo note"
    );
}

#[tokio::test]
async fn a_resource_across_workspaces_doesnt_write_to_a_read_only_demo() {
    let (world, staff) = staff_world().await;
    let (demo, note) = demo_of(&world).await;
    join(&world.app, demo, world.member_id).await;
    for client in [&world.member, &world.outsider, &staff] {
        client
            .patch(&format!("{EVERY}/{note}"))
            .json(&json!({ "title": "Changed through Every" }))
            .send()
            .await
            .assert_error_code(403, "demo_read_only");
        client
            .delete(&format!("{EVERY}/{note}"))
            .send()
            .await
            .assert_error_code(403, "demo_read_only");
        client
            .post(EVERY)
            .json(&json!({ "title": "Through Every", "workspace_id": demo }))
            .send()
            .await
            .assert_error_code(403, "demo_read_only");
    }
    assert_eq!(
        note_in(&world.app, demo, note).await.unwrap().title,
        "Demo note"
    );
}

#[tokio::test]
async fn a_demo_in_the_sidebar_doesnt_stop_a_form_for_another_workspace() {
    let world = world(app_with_members().await).await;
    let (demo, _) = demo_of(&world).await;
    join(&world.app, demo, world.member_id).await;
    world.member.set_cookie(WORKSPACE_COOKIE, &demo.to_string());
    let created = world
        .member
        .post(EVERY)
        .json(&json!({ "title": "For Agra", "workspace_id": world.agra }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id: Uuid = created["data"]["id"].as_str().unwrap().parse().unwrap();
    assert!(note_in(&world.app, world.agra, id).await.is_some());
    // A form for the demo itself, or a write that leaves it to the sidebar, is refused.
    world
        .member
        .post(EVERY)
        .json(&json!({ "title": "For the demo", "workspace_id": demo }))
        .send()
        .await
        .assert_error_code(403, "demo_read_only");
    world
        .member
        .post(NOTES)
        .json(&json!({ "title": "In the demo" }))
        .send()
        .await
        .assert_error_code(403, "demo_read_only");
}

#[tokio::test]
async fn the_roles_that_edit_a_demo_edit_it_across_workspaces_too() {
    let app = with_members(crate::servers::database(TestApp::builder()))
        .config("workspace.demo.editor_roles", json!(["admin"]))
        .build()
        .await;
    let world = world(app).await;
    let (demo, note) = demo_of(&world).await;
    let id = account(&world.app, "editor").await;
    let members = world.app.get::<Members>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    uow.scope(members.add(&uow, demo, id, WorkspaceRole::Admin))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    give_role(&world.app, id, "editor", &["helm.view", "notes.*"]).await;
    let editor = world.app.acting_as(navree_testing::principal(id));
    editor
        .patch(&format!("{NOTES}/{note}"))
        .header("X-Workspace", demo.to_string())
        .json(&json!({ "title": "Edited by an editor" }))
        .send()
        .await
        .assert_status(200);
    editor
        .patch(&format!("{EVERY}/{note}"))
        .json(&json!({ "title": "Across workspaces" }))
        .send()
        .await
        .assert_status(200);
}
