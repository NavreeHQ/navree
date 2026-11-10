//! A run started inside a request writes in units of work of its own, never in
//! the request's: its conversation, its messages and state, and each tool call
//! commit apart, whether the run answers once, waits for an approval, is
//! approved or declined, or streams. On SQLite an open write transaction holds
//! the database's only write lock until it ends, so a run that wrote through
//! the request's would wait on itself.

use std::time::Duration;

use futures_util::StreamExt;
use navree_ai::{Ai, Conversations, FakeReply, ModuleBuilderAiExt, StreamEvent};
use navree_db::{Db, Uow};
use navree_kernel::{Module, ModuleBuilder};
use navree_orm::Model as _;
use navree_testing::TestApp;
use serde_json::json;
use uuid::Uuid;

use crate::agents::NotesAssistant;
use crate::support::{Note, agent_app, user};

struct AssistantModule;

impl Module for AssistantModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.agent::<NotesAssistant>();
    }
}

async fn app() -> TestApp {
    agent_app().module(AssistantModule).build().await
}

/// A unit of work like the one the request layer opens for `user`.
async fn request(app: &TestApp, user: Uuid) -> Uow {
    app.get::<Db>()
        .await
        .uow(Some(user), Some("request-1".to_owned()))
        .await
}

async fn notes(app: &TestApp) -> Vec<Note> {
    Note::query()
        .get(&*app.get::<Db>().await)
        .await
        .unwrap()
        .into_models()
}

/// The request ends without committing: what the run wrote stays, and nothing
/// was written through the request.
async fn fail(request: Uow) {
    assert!(
        !request.has_open_transaction().await,
        "nothing was written through the request's unit of work"
    );
    request.rollback().await.unwrap();
}

#[tokio::test]
async fn a_run_in_a_request_commits_its_conversation_and_tools_apart() {
    let app = app().await;
    let daljit = user(&app, "Daljit", &[]).await;
    let ai = app.get::<Ai>().await;
    let fake = ai.fake().unwrap();
    fake.respond_with([
        FakeReply::tool_call(
            "add_note",
            json!({ "title": "Call Harpreet about the quote" }),
        ),
        FakeReply::text("Logged the call."),
    ]);

    let request = request(&app, daljit).await;
    let run = ai
        .agent::<NotesAssistant>()
        .acting_as_user(daljit)
        .conversation(None)
        .prompt("Log a call with Harpreet");
    let reply = tokio::time::timeout(Duration::from_secs(10), request.scope(run))
        .await
        .expect("the run does not wait on the request")
        .unwrap();

    assert!(reply.steps[0].ok, "the tool wrote beside the request");
    assert_eq!(reply.text, "Logged the call.");
    fail(request).await;
    let conversation = reply.conversation_id.expect("the run has memory");
    let conversations = app.get::<Conversations>().await;
    let messages = conversations.messages(daljit, conversation).await.unwrap();
    // user, assistant (tool call), tool result, assistant.
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[3].text, "Logged the call.");
    assert_eq!(notes(&app).await.len(), 1);
}

#[tokio::test]
async fn approving_and_declining_in_a_request_commit_apart() {
    let app = app().await;
    let daljit = user(&app, "Daljit", &[]).await;
    let ai = app.get::<Ai>().await;
    let fake = ai.fake().unwrap();
    fake.respond_with([
        FakeReply::tool_call("add_note", json!({ "title": "Quote for Ludhiana" })),
        FakeReply::text("Added."),
    ]);
    ai.agent::<NotesAssistant>()
        .acting_as_user(daljit)
        .prompt("Add")
        .await
        .unwrap();
    let id = notes(&app).await[0].id;
    let conversations = app.get::<Conversations>().await;

    // A run that stops for an approval keeps the call it waits on.
    fake.respond_with([
        FakeReply::tool_call(
            "rename_note",
            json!({ "note_id": id, "title": "Quote for Amritsar" }),
        ),
        FakeReply::text("Renamed it."),
    ]);
    let asked = request(&app, daljit).await;
    let reply = asked
        .scope(
            ai.agent::<NotesAssistant>()
                .acting_as_user(daljit)
                .conversation(None)
                .prompt("Rename my note"),
        )
        .await
        .unwrap();
    fail(asked).await;
    let conversation = reply.conversation_id.unwrap();
    let call = reply.pending_approval.expect("rename_note waits").call_id;

    // Approving runs the tool and stores the answer, both apart.
    let approved = request(&app, daljit).await;
    let run = ai
        .agent::<NotesAssistant>()
        .acting_as_user(daljit)
        .conversation(Some(conversation))
        .approve(call.clone());
    let reply = tokio::time::timeout(Duration::from_secs(10), approved.scope(run))
        .await
        .expect("the approved tool does not wait on the request")
        .unwrap();
    assert!(reply.steps[0].ok);
    fail(approved).await;
    assert_eq!(notes(&app).await[0].title, "Quote for Amritsar");
    let messages = conversations.messages(daljit, conversation).await.unwrap();
    assert_eq!(messages.last().unwrap().text, "Renamed it.");
    // The call was decided, and that was stored too.
    let error = ai
        .agent::<NotesAssistant>()
        .acting_as_user(daljit)
        .conversation(Some(conversation))
        .approve(call)
        .await
        .unwrap_err();
    assert_eq!(error.status().as_u16(), 404);

    // Declining stores the model's answer apart.
    fake.respond_with([
        FakeReply::tool_call(
            "rename_note",
            json!({ "note_id": id, "title": "Quote for Jalandhar" }),
        ),
        FakeReply::text("Okay, I left it."),
    ]);
    let reply = ai
        .agent::<NotesAssistant>()
        .acting_as_user(daljit)
        .conversation(Some(conversation))
        .prompt("Rename it again")
        .await
        .unwrap();
    let declined = request(&app, daljit).await;
    let reply = declined
        .scope(
            ai.agent::<NotesAssistant>()
                .acting_as_user(daljit)
                .conversation(Some(conversation))
                .reject(reply.pending_approval.unwrap().call_id, None),
        )
        .await
        .unwrap();
    assert_eq!(reply.text, "Okay, I left it.");
    fail(declined).await;
    let messages = conversations.messages(daljit, conversation).await.unwrap();
    assert_eq!(messages.last().unwrap().text, "Okay, I left it.");
    assert_eq!(notes(&app).await[0].title, "Quote for Amritsar");
}

#[tokio::test]
async fn a_streamed_run_started_in_a_request_does_not_wait_for_it() {
    let app = app().await;
    let daljit = user(&app, "Daljit", &[]).await;
    let ai = app.get::<Ai>().await;
    let fake = ai.fake().unwrap();
    fake.respond_with([
        FakeReply::tool_call("add_note", json!({ "title": "Visit Patiala" })),
        FakeReply::text("Noted."),
    ]);

    // The request is still open while the run goes on beside it.
    let request = request(&app, daljit).await;
    let stream = request
        .scope(
            ai.agent::<NotesAssistant>()
                .acting_as_user(daljit)
                .conversation(None)
                .stream("Remind me to visit Patiala"),
        )
        .await
        .unwrap();
    let events: Vec<StreamEvent> = tokio::time::timeout(
        Duration::from_secs(10),
        stream.map(Result::unwrap).collect::<Vec<_>>(),
    )
    .await
    .expect("the run does not wait on the request");

    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::ToolResult { tool, ok: true, .. } if tool == "add_note")),
        "{events:?}"
    );
    let Some(StreamEvent::Done(done)) = events.last() else {
        panic!("{events:?}");
    };
    let conversation = done.conversation_id.expect("the run has memory");
    fail(request).await;
    let conversations = app.get::<Conversations>().await;
    let messages = conversations.messages(daljit, conversation).await.unwrap();
    assert_eq!(messages.last().unwrap().text, "Noted.");
    assert_eq!(notes(&app).await[0].title, "Visit Patiala");
}
