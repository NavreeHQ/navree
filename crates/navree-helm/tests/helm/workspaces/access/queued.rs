//! A queued action in a workspace the app entered itself: the job can't ask the
//! app again, so what it will find is what the request is told.

use navree_contracts::{Job, JobContext};
use navree_http::ModuleBuilderHttpExt;

use super::*;

/// What an app does that enters workspaces itself, for a test header naming one.
struct EnterLayer;

impl Module for EnterLayer {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(navree_http::GlobalLayer::new(
            navree_http::LayerSlot::Context,
            "test-enter",
            |_ctx, router| {
                Ok(router.layer(navree_http::middleware::from_fn(
                    |request: navree_http::extract::Request,
                     next: navree_http::middleware::Next| async move {
                        let named = request
                            .headers()
                            .get("x-test-entered")
                            .and_then(|value| value.to_str().ok())
                            .and_then(|value| Uuid::parse_str(value).ok());
                        let uow = request.extensions().get::<navree_db::Uow>().cloned();
                        if let (Some(workspace), Some(uow)) = (named, uow) {
                            enter(&uow, workspace).await;
                        }
                        next.run(request).await
                    },
                )))
            },
        ));
    }
}

#[tokio::test]
async fn with_nothing_to_check_a_queued_action_is_refused_when_it_is_asked_for() {
    let fake = QueueFake::new();
    let app = crate::servers::database(TestApp::builder())
        .config("http.per_ip_rate_limit", Value::Null)
        .override_value::<dyn navree_contracts::Dispatcher>(fake.dispatcher())
        .module(navree_db::DatabaseModule)
        .module(navree_access::AccessModule::for_root(options()))
        .module(MemberNotesModule)
        .module(EnterLayer)
        .module(navree_helm::HelmModule::from_config())
        .build()
        .await;
    let acme = Uuid::now_v7();
    let note = seed(&app, acme, "Acme Note").await;
    let editor = editor(&app).await;
    // What runs now runs as the app entered it.
    editor
        .post(&format!("{NOTES}/actions/shout"))
        .header("x-test-entered", acme.to_string())
        .json(&json!({ "ids": [note] }))
        .send()
        .await
        .assert_status(200);
    // What would run later is refused now, with the reason, and isn't queued.
    let response = editor
        .post(&format!("{NOTES}/actions/whisper"))
        .header("x-test-entered", acme.to_string())
        .json(&json!({ "ids": [note] }))
        .send()
        .await
        .assert_error_code(403, "helm_workspace_unchecked");
    assert!(
        response.text().contains("WorkspaceModule"),
        "{}",
        response.text()
    );
    assert!(fake.dispatched::<HelmActionJob>().is_empty());
}

#[tokio::test]
async fn a_user_the_gate_refuses_is_told_so_when_they_ask() {
    let fake = QueueFake::new();
    let builder = crate::servers::database(TestApp::builder())
        .override_value::<dyn navree_contracts::Dispatcher>(fake.dispatcher())
        .module(EnterLayer);
    let world = world(with_members(builder).build().await).await;
    world
        .outsider
        .post(&format!("{NOTES}/actions/whisper"))
        .header("x-test-entered", world.acme.to_string())
        .json(&json!({ "ids": [world.acme_note] }))
        .send()
        .await
        .assert_error_code(403, "helm_workspace_refused");
    assert!(fake.dispatched::<HelmActionJob>().is_empty());
    // A member is queued, and the job finds them.
    world
        .member
        .post(&format!("{NOTES}/actions/whisper"))
        .header("x-test-entered", world.acme.to_string())
        .json(&json!({ "ids": [world.acme_note] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("queued", json!(true));
    fake.dispatched::<HelmActionJob>()
        .pop()
        .unwrap()
        .handle(JobContext {
            app: world.app.ctx().clone(),
            job_id: "1".into(),
            attempt: 1,
        })
        .await
        .unwrap();
    assert_eq!(
        note_in(&world.app, world.acme, world.acme_note)
            .await
            .unwrap()
            .title,
        "acme secret"
    );
}
