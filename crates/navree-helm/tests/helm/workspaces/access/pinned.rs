//! A credential limited to one workspace reaches that one, whoever holds it and
//! whichever way a request or a form names another.

use navree_http::ModuleBuilderHttpExt;

use super::*;

/// What an authentication strategy does for a credential that belongs to a
/// workspace, for a test header that names it.
struct PinLayer;

impl Module for PinLayer {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(navree_http::GlobalLayer::new(
            navree_http::LayerSlot::Context,
            "test-pin",
            |_ctx, router| {
                Ok(router.layer(navree_http::middleware::from_fn(
                    |mut request: navree_http::extract::Request,
                     next: navree_http::middleware::Next| async move {
                        let pin = request
                            .headers()
                            .get("x-test-pin")
                            .and_then(|value| value.to_str().ok())
                            .and_then(|value| Uuid::parse_str(value).ok());
                        if let Some(pin) = pin {
                            request
                                .extensions_mut()
                                .insert(navree_workspace::PinnedWorkspace(pin));
                        }
                        next.run(request).await
                    },
                )))
            },
        ));
    }
}

/// A world with staff, where the member is in both workspaces.
async fn pinned_world() -> (World, TestClient) {
    let builder = crate::servers::database(TestApp::builder())
        .config("audit.routes", false)
        .module(navree_audit::AuditModule::from_config())
        .module(StaffNotesModule)
        .module(PinLayer);
    let world = world(with_members(builder).build().await).await;
    join(&world.app, world.globex, world.member_id).await;
    let staff_id = account(&world.app, "staff").await;
    give_role(
        &world.app,
        staff_id,
        "support",
        &["helm.view", "notes.*", "support.enter"],
    )
    .await;
    let staff = world.app.acting_as(navree_testing::principal(staff_id));
    (world, staff)
}

fn pinned(request: navree_testing::TestRequest, to: Uuid) -> navree_testing::TestRequest {
    request.header("x-test-pin", to.to_string())
}

#[tokio::test]
async fn staff_stay_inside_the_workspace_of_their_token() {
    let (world, staff) = pinned_world().await;
    pinned(staff.get(NOTES), world.acme)
        .header("X-Workspace", world.globex.to_string())
        .send()
        .await
        .assert_error_code(403, "token_workspace_mismatch");
    let body = pinned(staff.get(NOTES), world.acme)
        .header("X-Workspace", world.acme.to_string())
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), ["Acme secret"]);
    assert_eq!(staff_entries(&world.app).await, [world.acme.to_string()]);
}

#[tokio::test]
async fn a_request_that_names_nothing_works_in_the_workspace_of_its_token() {
    let (world, _) = pinned_world().await;
    let body = pinned(world.member.get(NOTES), world.acme)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), ["Acme secret"]);
    // A token for a workspace the user isn't in gets nothing.
    pinned(world.outsider.get(NOTES), world.acme)
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn a_resource_across_workspaces_works_inside_the_token() {
    let (world, _) = pinned_world().await;
    let member = &world.member;
    let body = pinned(member.get(EVERY), world.acme)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), ["Acme secret"]);
    pinned(
        member.get(&format!("{EVERY}/{}", world.globex_note)),
        world.acme,
    )
    .send()
    .await
    .assert_error(404, None);
    pinned(
        member.patch(&format!("{EVERY}/{}", world.globex_note)),
        world.acme,
    )
    .json(&json!({ "title": "Across the pin" }))
    .send()
    .await
    .assert_error(404, None);
    pinned(
        member.delete(&format!("{EVERY}/{}", world.globex_note)),
        world.acme,
    )
    .send()
    .await
    .assert_error(404, None);
    pinned(member.post(EVERY), world.acme)
        .json(&json!({ "title": "Across the pin", "workspace_id": world.globex }))
        .send()
        .await
        .assert_status(400);
    pinned(member.post(&format!("{EVERY}/actions/locate")), world.acme)
        .json(&json!({ "ids": [world.globex_note] }))
        .send()
        .await
        .assert_error(403, None);
    assert_eq!(
        note_in(&world.app, world.globex, world.globex_note)
            .await
            .unwrap()
            .title,
        "Globex secret"
    );
    // Its own workspace works as ever.
    pinned(
        member.patch(&format!("{EVERY}/{}", world.acme_note)),
        world.acme,
    )
    .json(&json!({ "title": "Inside the pin" }))
    .send()
    .await
    .assert_status(200);
    pinned(member.post(EVERY), world.acme)
        .json(&json!({ "title": "Mine" }))
        .send()
        .await
        .assert_status(201);
}

#[tokio::test]
async fn a_cookie_doesnt_take_a_token_elsewhere_and_the_form_refuses() {
    let (world, staff) = pinned_world().await;
    staff.set_cookie(WORKSPACE_COOKIE, &world.globex.to_string());
    let body = pinned(staff.get(NOTES), world.acme)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(titles_of(&body), ["Acme secret"]);
    staff.remove_cookie(WORKSPACE_COOKIE);
    let response = pinned(staff.post("/admin/workspace"), world.acme)
        .header("accept", "text/html")
        .form(&[("workspace", world.globex.to_string().as_str())])
        .send()
        .await
        .assert_status(303);
    assert!(response.set_cookie(WORKSPACE_COOKIE).is_none());
}
