//! An assistant on Navree UI pages: a conversation's items and revisions as an answer
//! streams, stopping it, components the model chooses (checked, retried, drawn under
//! the reader's access), their buttons' signed actions, approvals in the page's own
//! place, the page's context and the conversation's working state; scripted with
//! `AiFake`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::Uri;
use http::header::HeaderName;
use navree_access::{AbilityFactory, PrincipalAccess, Read, Update};
use navree_ai::ui::genui::{
    Action, ActionButton, ActionDone, AppError, BoxFuture, Catalog, Key, RunAction, Surface,
    SurfaceView, UserText,
};
use navree_ai::ui::{
    Acted, Assistant, AssistantConfig, ModuleBuilderAssistantExt, PageContext, PageOptions,
};
use navree_ai::{AgentRun, Ai, FakeReply, ModuleBuilderAiExt};
use navree_contracts::{AuthVia, Broadcaster, Principal};
use navree_db::{Db, ExecutorExt as _, Statement, Value as DbValue};
use navree_http::HttpModule;
use navree_http::response::IntoResponse;
use navree_kernel::{BoxError, Module, ModuleBuilder};
use navree_orm::Model as _;
use navree_orm::access::AccessibleQuery;
use navree_realtime::Live;
use navree_testing::TestApp;
use navree_ui::{LazyAsk, el};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::agents::NotesAssistant;
use crate::support::{Note, agent_app, user};

// ---------------------------------------------------------------------------
// The catalogue the tests' assistant may choose from
// ---------------------------------------------------------------------------

navree_ui::props! {
    /// How many notes the user can read, with a heading.
    pub struct NotesSummary {
        /// A heading above the count.
        pub heading: Option<UserText<60>>,
    }
}

navree_ui::props! {
    /// One note, with a button to archive it.
    pub struct NoteProps {
        /// The note's key.
        pub note: Key,
    }
}

navree_ui::props! {
    /// A note to archive or delete.
    pub struct NoteInput {
        /// The note's key.
        pub note: Key,
    }
}

/// The count of notes the reader can read, loaded when the page is drawn.
pub struct Summary;

impl Surface<AgentRun> for Summary {
    type Props = NotesSummary;
    const NAME: &'static str = "notes_summary";
    const CAN: Option<(&'static str, &'static str)> = Some(("read", "Note"));

    fn title(&self, props: &NotesSummary) -> String {
        props
            .heading
            .as_ref()
            .map_or_else(|| "Your notes".to_owned(), |h| h.as_str().to_owned())
    }

    fn render<'a>(
        &'a self,
        _props: &'a NotesSummary,
        run: &'a AgentRun,
    ) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
        Box::pin(async move {
            let notes = run
                .accessible::<Note, Read>()
                .apply(Note::query())
                .get(run.uow())
                .await?
                .into_models();
            Ok(SurfaceView::new(
                el("p").id("count").child(format!("{} notes", notes.len())),
            ))
        })
    }
}

/// One note, which only someone who may update it sees, with Archive and Delete.
pub struct NoteCard;

impl Surface<AgentRun> for NoteCard {
    type Props = NoteProps;
    const NAME: &'static str = "note_card";
    const ACTIONS: &'static [&'static str] = &["archive_note", "delete_note"];

    fn title(&self, _props: &NoteProps) -> String {
        "A note".to_owned()
    }

    fn render<'a>(
        &'a self,
        props: &'a NoteProps,
        run: &'a AgentRun,
    ) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
        Box::pin(async move {
            let note = run
                .authorize::<Note, Update>(props.note.as_str())
                .await?
                .into_inner();
            let input = NoteInput {
                note: props.note.clone(),
            };
            Ok(SurfaceView::new(el("p").child(note.title.clone()))
                .action(ActionButton::new::<ArchiveNote>("Archive", &input))
                .action(ActionButton::new::<DeleteNote>("Delete", &input).confirm(
                    "Delete this note?",
                    format!("“{}” goes for good.", note.title),
                    "Delete",
                )))
        })
    }
}

/// Puts `Archived: ` before a note's title.
pub struct ArchiveNote;

impl Action for ArchiveNote {
    type Input = NoteInput;
    const NAME: &'static str = "archive_note";
    const CAN: Option<(&'static str, &'static str)> = Some(("read", "Note"));
}

impl RunAction<AgentRun> for ArchiveNote {
    fn run<'a>(
        &'a self,
        input: NoteInput,
        run: &'a AgentRun,
    ) -> BoxFuture<'a, Result<ActionDone, AppError>> {
        Box::pin(async move {
            let mut note = run
                .authorize::<Note, Update>(input.note.as_str())
                .await?
                .into_inner();
            note.update(run.uow(), |note| {
                note.title = format!("Archived: {}", note.title)
            })
            .await?;
            Ok(ActionDone::new("Archived the note").note("The user archived a note."))
        })
    }
}

/// Deletes a note: the page asks first.
pub struct DeleteNote;

impl Action for DeleteNote {
    type Input = NoteInput;
    const NAME: &'static str = "delete_note";
    const DESTRUCTIVE: bool = true;
}

impl RunAction<AgentRun> for DeleteNote {
    fn run<'a>(
        &'a self,
        input: NoteInput,
        run: &'a AgentRun,
    ) -> BoxFuture<'a, Result<ActionDone, AppError>> {
        Box::pin(async move {
            let mut note = run
                .authorize::<Note, Update>(input.note.as_str())
                .await?
                .into_inner();
            note.delete(run.uow()).await?;
            Ok(ActionDone::new("Deleted the note"))
        })
    }
}

navree_ui::choices! {
    /// Where a step stands.
    pub enum StepState {
        /// Not started.
        Todo = "todo",
        /// Finished.
        Done = "done",
    }
}

navree_ui::props! {
    /// One step of the plan.
    pub struct Step {
        /// What to do.
        pub title: UserText<80>,
        /// Where it stands.
        pub state: StepState,
    }
}

navree_ui::props! {
    /// The plan the user and the assistant work on together.
    pub struct Plan {
        /// What the plan is for.
        pub goal: UserText<120>,
        /// Its steps, in order.
        pub steps: navree_ui::genui::List<Step, 10>,
    }
}

/// What a note page is about: the note's title, if the reader may update it.
pub struct NoteContext;

impl PageContext for NoteContext {
    fn describe<'a>(
        &'a self,
        key: &'a str,
        run: &'a AgentRun,
    ) -> BoxFuture<'a, Result<String, AppError>> {
        Box::pin(async move {
            let note = run.authorize::<Note, Update>(key).await?.into_inner();
            Ok(format!("note “{}”", note.title))
        })
    }
}

/// The tests' catalogue.
pub fn catalog() -> Catalog<AgentRun> {
    Catalog::new()
        .surface(Summary)
        .surface(NoteCard)
        .action(ArchiveNote)
        .action(DeleteNote)
        .link("/notes/{key}")
}

// ---------------------------------------------------------------------------
// The app
// ---------------------------------------------------------------------------

/// Records every nudge's channels.
#[derive(Default)]
pub struct Nudges(pub Mutex<Vec<String>>);

impl Broadcaster for Nudges {
    fn broadcast(
        &self,
        channels: Vec<String>,
        _event: String,
        _data: Value,
    ) -> navree_kernel::BoxFuture<'_, Result<(), BoxError>> {
        self.0.lock().unwrap().extend(channels);
        Box::pin(async { Ok(()) })
    }
}

struct PageModule;

impl Module for PageModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.agent::<NotesAssistant>().assistant::<NotesAssistant>(
            AssistantConfig::at("/assistant")
                .catalog(catalog())
                .tool_label("rename_note", "Rename a note")
                .context("note", NoteContext)
                .state::<Plan>()
                .flush_every(Duration::from_millis(50)),
        );
    }
}

pub struct Page {
    pub app: TestApp,
    pub nudges: Arc<Nudges>,
}

pub async fn page() -> Page {
    let nudges = Arc::new(Nudges::default());
    let app = agent_app()
        .module(HttpModule::from_config())
        .override_value::<Live>(Arc::new(Live::new(nudges.clone())))
        .module(PageModule)
        .build()
        .await;
    Page { app, nudges }
}

/// The access of `user`, as a request of theirs would have it.
pub async fn access_of(app: &TestApp, user: Uuid) -> Arc<PrincipalAccess> {
    let principal = Principal {
        user_id: user,
        via: AuthVia::Token,
        token_abilities: None,
        session_id: None,
    };
    app.get::<AbilityFactory>()
        .await
        .access_for(Some(&principal))
        .await
        .unwrap()
}

pub async fn assistant(app: &TestApp) -> Arc<Assistant<NotesAssistant>> {
    app.get::<Assistant<NotesAssistant>>().await
}

/// The conversation's items as a page draws them.
pub async fn items(app: &TestApp, access: &Arc<PrincipalAccess>, id: Uuid) -> String {
    assistant(app)
        .await
        .items(access.clone(), id, &LazyAsk::First, Some("tok"))
        .await
        .unwrap()
        .to_html()
}

/// Whether the conversation's run has ended. Between two steps (a tool's call and its
/// component, or its approval) nothing on the page is being written, but the run goes on;
/// its end is written after its last item.
pub async fn run_ended(app: &TestApp, id: Uuid) -> bool {
    app.get::<Db>()
        .await
        .fetch_scalar::<String>(&Statement::portable(
            "SELECT status FROM ai_runs WHERE conversation_id = ?",
            [DbValue::from(id)],
        ))
        .await
        .is_ok_and(|status| status != "running")
}

/// Waits until the conversation's run has ended.
pub async fn settled(app: &TestApp, access: &Arc<PrincipalAccess>, id: Uuid) -> String {
    for _ in 0..400 {
        if run_ended(app, id).await {
            return items(app, access, id).await;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the answer is still being written");
}

/// Sends `message` once the run before it has ended (its end is written last).
pub async fn send_when_idle(
    app: &TestApp,
    access: &Arc<PrincipalAccess>,
    id: Uuid,
    message: &str,
    context: Option<&str>,
) {
    let assistant = assistant(app).await;
    for _ in 0..200 {
        match assistant.send(access.clone(), id, message, context).await {
            Ok(()) => return,
            Err(error) if error.status.as_u16() == 409 => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(error) => panic!("{error:?}"),
        }
    }
    panic!("the run before never ended");
}

pub async fn notes(app: &TestApp) -> Vec<Note> {
    Note::query()
        .get(&*app.get::<Db>().await)
        .await
        .unwrap()
        .into_models()
}

async fn add_note(app: &TestApp, owner: Uuid, title: &str) -> Uuid {
    let note = Note {
        id: Uuid::now_v7(),
        owner_id: owner,
        title: title.to_owned(),
    };
    Note::create(&*app.get::<Db>().await, note.clone())
        .await
        .unwrap();
    note.id
}

/// The signed URLs of the buttons drawn in `html`, in order.
pub fn button_urls(html: &str) -> Vec<String> {
    html.match_indices(r#"action="/assistant/act?"#)
        .map(|(at, _)| {
            let rest = &html[at + r#"action=""#.len()..];
            rest[..rest.find('"').unwrap()].replace("&amp;", "&")
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The answer streams into the page
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_message_streams_into_the_conversation_as_items_at_new_revisions() {
    let Page { app, nudges } = page().await;
    let dana = user(&app, "Daljit", &[]).await;
    let access = access_of(&app, dana).await;
    let ai = app.get::<Ai>().await;
    ai.fake().unwrap().respond(FakeReply::text(
        "You have no notes yet. Add one by telling me what to remember.",
    ));
    let assistant = assistant(&app).await;

    let id = assistant
        .start(access.clone(), "How many notes do I have?", None)
        .await
        .unwrap();
    let html = settled(&app, &access, id).await;

    assert!(html.contains("How many notes do I have?"), "{html}");
    assert!(html.contains("You have no notes yet."), "{html}");
    assert!(html.contains(r#"data-role="user""#) && html.contains(r#"data-role="assistant""#));
    // Each write was told to Daljit's pages only.
    let channel = format!("private-navree-live.user.{dana}.assistant.{id}");
    let told = nudges.0.lock().unwrap().clone();
    assert!(told.contains(&channel), "{told:?}");
    assert!(
        told.iter().all(|c| c.contains(&dana.to_string())),
        "{told:?}"
    );
    // What changed since the first revision is the answer, finished, under its own key,
    // and its writing key is gone.
    let changes = assistant
        .items(access.clone(), id, &LazyAsk::Since("1".into()), None)
        .await
        .unwrap()
        .into_response();
    let gone = changes
        .headers()
        .get(HeaderName::from_static("x-ui-gone"))
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    assert!(gone.ends_with(".w"), "{gone}");
    let since: i64 = changes.headers()["x-ui-since"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(since > 1, "{since}");
    // Nothing changed since the newest revision.
    let none = assistant
        .items(access.clone(), id, &LazyAsk::Since(since.to_string()), None)
        .await
        .unwrap()
        .to_html();
    assert!(none.is_empty(), "{none}");
}

#[tokio::test]
async fn another_reader_never_sees_the_conversation() {
    let Page { app, .. } = page().await;
    let (dana, ravi) = (
        user(&app, "Daljit", &[]).await,
        user(&app, "Ravi", &[]).await,
    );
    let (dana_access, ravi_access) = (access_of(&app, dana).await, access_of(&app, ravi).await);
    app.get::<Ai>()
        .await
        .fake()
        .unwrap()
        .respond(FakeReply::text("Hello Daljit."));
    let assistant = assistant(&app).await;
    let id = assistant
        .start(dana_access.clone(), "Hello", None)
        .await
        .unwrap();
    settled(&app, &dana_access, id).await;

    let error = assistant
        .items(ravi_access.clone(), id, &LazyAsk::First, None)
        .await
        .unwrap_err();
    assert_eq!(error.status.as_u16(), 404);
    let error = assistant
        .send(ravi_access.clone(), id, "Show me", None)
        .await
        .unwrap_err();
    assert_eq!(error.status.as_u16(), 404);
    assert_eq!(
        assistant.stop(ravi, id).await.unwrap_err().status.as_u16(),
        404
    );
    let error = assistant
        .page(ravi_access, id, PageOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.status.as_u16(), 404);
}

#[tokio::test]
async fn stop_ends_the_answer_and_a_second_message_waits_for_it() {
    let Page { app, .. } = page().await;
    let dana = user(&app, "Daljit", &[]).await;
    let access = access_of(&app, dana).await;
    // `slow` runs past its one-second limit: the run is busy in the tool.
    app.get::<Ai>()
        .await
        .fake()
        .unwrap()
        .respond(FakeReply::tool_call("slow", json!({})));
    let assistant = assistant(&app).await;
    let id = assistant
        .start(access.clone(), "Do the slow thing", None)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let busy = assistant
        .send(access.clone(), id, "And another", None)
        .await
        .unwrap_err();
    assert_eq!(busy.status.as_u16(), 409);

    assistant.stop(dana, id).await.unwrap();
    let html = settled(&app, &access, id).await;
    assert!(html.contains("Stopped"), "{html}");
    assert!(
        html.contains(r#"data-state="failed""#),
        "the tool it stopped in: {html}"
    );
    // The run is over: a new message is taken, and the model is asked once more only.
    let fake = app.get::<Ai>().await.fake().unwrap();
    let asked = fake.requests().len();
    fake.respond(FakeReply::text("Here now."));
    send_when_idle(&app, &access, id, "Still there?", None).await;
    let html = settled(&app, &access, id).await;
    assert_eq!(fake.requests().len(), asked + 1);
    assert!(html.contains("Here now."), "{html}");
}

#[tokio::test]
async fn a_message_is_checked_before_anything_runs() {
    let Page { app, .. } = page().await;
    let dana = user(&app, "Daljit", &[]).await;
    let access = access_of(&app, dana).await;
    let assistant = assistant(&app).await;
    let long = "x".repeat(5000);
    for bad in ["", "   ", long.as_str()] {
        let error = assistant
            .start(access.clone(), bad, None)
            .await
            .unwrap_err();
        assert_eq!(error.status.as_u16(), 400, "{bad:?}");
    }
    app.get::<Ai>()
        .await
        .fake()
        .unwrap()
        .assert_nothing_prompted();
}

// ---------------------------------------------------------------------------
// Components the model chooses
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_component_is_checked_stored_and_drawn_from_the_readers_data() {
    let Page { app, .. } = page().await;
    let dana = user(&app, "Daljit", &[]).await;
    let access = access_of(&app, dana).await;
    add_note(&app, dana, "Call Harpreet about the quote").await;
    add_note(&app, dana, "Send Arjun the invoice").await;
    let fake = app.get::<Ai>().await.fake().unwrap();
    fake.respond_with([
        FakeReply::tool_call("show_notes_summary", json!({ "heading": "Daljit's notes" })),
        FakeReply::text("There they are."),
    ]);
    let assistant = assistant(&app).await;
    let id = assistant
        .start(access.clone(), "How many notes?", None)
        .await
        .unwrap();
    let html = settled(&app, &access, id).await;

    assert!(html.contains(r#"data-genui="surface""#), "{html}");
    assert!(html.contains("Daljit&#39;s notes"), "{html}");
    assert!(html.contains("2 notes"), "{html}");
    // The page's id is put under the component's own prefix.
    assert!(!html.contains(r#"id="count""#), "{html}");
    // The model was offered the tools the reader may see, and read back that the
    // component was shown, not its numbers.
    assert!(
        fake.requests()[0]
            .tools()
            .iter()
            .any(|t| t == "show_notes_summary")
    );
    let result = &fake.tool_results("show_notes_summary")[0];
    assert_eq!(result["shown"], "notes_summary");
    assert!(!result.to_string().contains("2 notes"));
    assert!(
        fake.requests()[0]
            .system()
            .unwrap()
            .contains("You can show components on the page")
    );
}

#[tokio::test]
async fn refused_arguments_are_retried_then_the_model_answers_in_words() {
    let Page { app, .. } = page().await;
    let dana = user(&app, "Daljit", &[]).await;
    let access = access_of(&app, dana).await;
    let fake = app.get::<Ai>().await.fake().unwrap();
    fake.respond_with([
        FakeReply::tool_call(
            "show_notes_summary",
            json!({ "heading": "x".repeat(200), "style": "color:red" }),
        ),
        FakeReply::tool_call(
            "show_notes_summary",
            json!({ "heading": "<script>" , "html": "<b>" }),
        ),
        FakeReply::text("You have some notes."),
    ]);
    let assistant = assistant(&app).await;
    let id = assistant
        .start(access.clone(), "How many notes?", None)
        .await
        .unwrap();
    let html = settled(&app, &access, id).await;

    let results = fake.tool_results("show_notes_summary");
    assert_eq!(results.len(), 2);
    let first = results[0]["error"]["message"].as_str().unwrap();
    assert!(first.contains("style: is not a property here"), "{first}");
    assert!(
        first.contains("heading: must be at most 60 characters"),
        "{first}"
    );
    let second = results[1]["error"]["message"].as_str().unwrap();
    assert!(second.contains("answer in words"), "{second}");
    assert!(
        !html.contains(r#"data-genui="surface""#),
        "nothing refused is drawn: {html}"
    );
    assert!(html.contains("You have some notes."), "{html}");
}

#[tokio::test]
async fn a_component_the_reader_may_not_see_shows_nothing_of_its_data() {
    let Page { app, .. } = page().await;
    let (dana, ravi) = (
        user(&app, "Daljit", &[]).await,
        user(&app, "Ravi", &[]).await,
    );
    let access = access_of(&app, ravi).await;
    // Daljit's note: Ravi may read it, but the card shows only notes the reader may update.
    let note = add_note(&app, dana, "Daljit's private plan").await;
    app.get::<Ai>().await.fake().unwrap().respond_with([
        FakeReply::tool_call("show_note_card", json!({ "note": note })),
        FakeReply::text("Here is the note."),
    ]);
    let assistant = assistant(&app).await;
    let id = assistant
        .start(access.clone(), "Show me Daljit's note", None)
        .await
        .unwrap();
    let html = settled(&app, &access, id).await;

    assert!(html.contains("You can&#39;t see this any more."), "{html}");
    assert!(!html.contains("private plan"), "{html}");
    assert!(
        button_urls(&html).is_empty(),
        "no button for what can't be seen: {html}"
    );
}

// ---------------------------------------------------------------------------
// A component's buttons act safely
// ---------------------------------------------------------------------------

/// A conversation in which Daljit's note card is drawn: the app, Daljit, her access, the
/// conversation, the note, and the drawn items.
async fn card() -> (TestApp, Uuid, Arc<PrincipalAccess>, Uuid, Uuid, String) {
    let Page { app, .. } = page().await;
    let dana = user(&app, "Daljit", &[]).await;
    let access = access_of(&app, dana).await;
    let note = add_note(&app, dana, "Call Harpreet").await;
    app.get::<Ai>().await.fake().unwrap().respond_with([
        FakeReply::tool_call("show_note_card", json!({ "note": note })),
        FakeReply::text("Here it is."),
    ]);
    let id = assistant(&app)
        .await
        .start(access.clone(), "Show my note", None)
        .await
        .unwrap();
    let html = settled(&app, &access, id).await;
    (app, dana, access, id, note, html)
}

#[tokio::test]
async fn a_button_runs_its_action_once_for_its_own_reader() {
    let (app, _dana, access, id, _note, html) = card().await;
    let urls = button_urls(&html);
    assert_eq!(urls.len(), 2, "{html}");
    let archive: Uri = urls[0].parse().unwrap();
    let assistant = assistant(&app).await;

    let (conversation, acted) = assistant
        .act(access.clone(), &archive, false)
        .await
        .unwrap();
    assert_eq!(conversation, id);
    assert_eq!(acted, Acted::Done("Archived the note".into()));
    assert_eq!(notes(&app).await[0].title, "Archived: Call Harpreet");
    // The same post again is refused, and changes nothing.
    let again = assistant
        .act(access.clone(), &archive, false)
        .await
        .unwrap_err();
    assert_eq!(again.status.as_u16(), 409);
    assert_eq!(notes(&app).await[0].title, "Archived: Call Harpreet");
    // The page shows what was done, and the assistant hears of it in its next run.
    let html = settled(&app, &access, id).await;
    assert!(html.contains("Archived the note"), "{html}");
    let fake = app.get::<Ai>().await.fake().unwrap();
    fake.respond(FakeReply::text("Noted."));
    send_when_idle(&app, &access, id, "Thanks", None).await;
    settled(&app, &access, id).await;
    assert!(
        fake.requests()
            .last()
            .unwrap()
            .system()
            .unwrap()
            .contains("The user archived a note.")
    );
}

#[tokio::test]
async fn forged_cross_user_and_cross_conversation_posts_are_refused() {
    let (app, _dana, access, id, _note, html) = card().await;
    let archive = button_urls(&html)[0].clone();
    let assistant = assistant(&app).await;
    let ravi = user(&app, "Ravi", &[]).await;
    let ravi_access = access_of(&app, ravi).await;

    // Another reader's post of Daljit's button.
    let error = assistant
        .act(ravi_access.clone(), &archive.parse().unwrap(), false)
        .await
        .unwrap_err();
    assert_eq!(error.status.as_u16(), 403);
    // Any part changed: the conversation, the item, the action, the input, the reader.
    let other = Uuid::now_v7().to_string();
    for forged in [
        archive.replace(&id.to_string(), &other),
        archive.replace("archive_note", "delete_note"),
        archive.replace(&access.user.unwrap().to_string(), &ravi.to_string()),
        archive.replacen("&n=", "&n=0", 1),
        format!("{archive}&confirmed=1"),
        archive.replace("/assistant/act", "/assistant/act/"),
    ] {
        let uri: Uri = forged.parse().unwrap();
        let error = assistant.act(access.clone(), &uri, true).await.unwrap_err();
        assert_eq!(error.status.as_u16(), 403, "{forged}");
    }
    assert_eq!(notes(&app).await[0].title, "Call Harpreet", "nothing ran");
}

#[tokio::test]
async fn a_destructive_action_asks_first_and_rechecks_access() {
    let (app, dana, access, _id, note, html) = card().await;
    let delete: Uri = button_urls(&html)[1].parse().unwrap();
    assert!(
        html.contains(r#"role="alertdialog""#),
        "the page asks in its own dialog: {html}"
    );
    assert!(html.contains("Delete this note?"), "{html}");
    let assistant = assistant(&app).await;

    // Unanswered, it asks.
    let (_, acted) = assistant.act(access.clone(), &delete, false).await.unwrap();
    assert_eq!(
        acted,
        Acted::Ask {
            label: "Delete".into()
        }
    );
    assert_eq!(notes(&app).await.len(), 1);
    // The note changes hands before the answer: the action checks access again, and
    // its post can be used once that is put right.
    let db = app.get::<Db>().await;
    let ravi = user(&app, "Ravi", &[]).await;
    let mut row = Note::find(&*db, note).await.unwrap().unwrap();
    row.update(&*db, |n| n.owner_id = ravi).await.unwrap();
    let error = assistant
        .act(access.clone(), &delete, true)
        .await
        .unwrap_err();
    assert_eq!(error.status.as_u16(), 403);
    assert_eq!(notes(&app).await.len(), 1);
    let mut row = Note::find(&*db, note).await.unwrap().unwrap();
    row.update(&*db, |n| n.owner_id = dana).await.unwrap();
    let (_, acted) = assistant.act(access.clone(), &delete, true).await.unwrap();
    assert_eq!(acted, Acted::Done("Deleted the note".into()));
    assert!(notes(&app).await.is_empty());
}

// ---------------------------------------------------------------------------
// Approvals in the page's own place
// ---------------------------------------------------------------------------

/// A conversation stopped at `rename_note` for Daljit's note.
async fn waiting() -> (TestApp, Arc<PrincipalAccess>, Uuid, String) {
    let Page { app, .. } = page().await;
    let dana = user(&app, "Daljit", &[]).await;
    let access = access_of(&app, dana).await;
    let note = add_note(&app, dana, "Old").await;
    app.get::<Ai>()
        .await
        .fake()
        .unwrap()
        .respond(FakeReply::tool_call(
            "rename_note",
            json!({ "note_id": note, "title": "New <b>title</b>" }),
        ));
    let assistant = assistant(&app).await;
    let id = assistant
        .start(access.clone(), "Rename my note", None)
        .await
        .unwrap();
    settled(&app, &access, id).await;
    let region = navree_ui::render(
        &assistant
            .approval(access.clone(), id, Some("tok"))
            .await
            .unwrap(),
    );
    (app, access, id, region)
}

#[tokio::test]
async fn an_approval_waits_in_its_own_place_and_approving_streams_the_rest() {
    let (app, access, id, region) = waiting().await;
    assert!(region.contains(r#"data-genui="approval""#), "{region}");
    assert!(
        region.contains("The assistant asks to: Rename a note"),
        "{region}"
    );
    assert!(region.contains("New &lt;b&gt;title&lt;/b&gt;"), "{region}");
    assert!(region.contains(r#"action="/assistant/"#), "{region}");
    let html = items(&app, &access, id).await;
    assert!(
        !html.contains(r#"data-genui="approval""#),
        "the card is never in the conversation"
    );
    assert!(
        html.contains("Waiting for your answer: Rename a note"),
        "{html}"
    );
    assert_eq!(notes(&app).await[0].title, "Old", "nothing ran yet");

    let fake = app.get::<Ai>().await.fake().unwrap();
    fake.respond(FakeReply::text("Renamed it."));
    let call = region
        .split(r#"name="call_id" value=""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_owned();
    let assistant = assistant(&app).await;
    assistant.approve(access.clone(), id, &call).await.unwrap();
    let html = settled(&app, &access, id).await;

    assert!(html.contains("Renamed it."), "{html}");
    assert!(html.contains("Approved: Rename a note"), "{html}");
    assert_eq!(notes(&app).await[0].title, "New <b>title</b>");
    let region = navree_ui::render(&assistant.approval(access.clone(), id, None).await.unwrap());
    assert!(!region.contains(r#"data-genui="approval""#), "{region}");
    // Decided once.
    let again = assistant.approve(access, id, &call).await.unwrap_err();
    assert_eq!(again.status.as_u16(), 404);
}

#[tokio::test]
async fn rejecting_tells_the_model_why() {
    let (app, access, id, region) = waiting().await;
    let call = region
        .split(r#"name="call_id" value=""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_owned();
    let fake = app.get::<Ai>().await.fake().unwrap();
    fake.respond(FakeReply::text("I left it as it was."));
    let assistant = assistant(&app).await;
    assistant
        .reject(access.clone(), id, &call, Some("Keep the old name"))
        .await
        .unwrap();
    let html = settled(&app, &access, id).await;

    assert!(
        html.contains("Rejected: Rename a note (Keep the old name)"),
        "{html}"
    );
    assert!(html.contains("I left it as it was."), "{html}");
    assert_eq!(notes(&app).await[0].title, "Old");
    assert_eq!(
        fake.tool_results("rename_note")[0]["message"],
        "The user declined this call: Keep the old name"
    );
    let unknown = assistant.approve(access, id, "call_999").await.unwrap_err();
    assert_eq!(unknown.status.as_u16(), 404);
}

// ---------------------------------------------------------------------------
// Working state and page context
// ---------------------------------------------------------------------------

#[tokio::test]
async fn state_the_model_writes_is_checked_and_the_page_edits_it() {
    let Page { app, .. } = page().await;
    let dana = user(&app, "Daljit", &[]).await;
    let access = access_of(&app, dana).await;
    let fake = app.get::<Ai>().await.fake().unwrap();
    fake.respond_with([
        FakeReply::tool_call("update_state", json!({ "goal": "Diwali sale", "steps": [{ "title": "Price the sweets", "state": "later" }] })),
        FakeReply::tool_call("update_state", json!({ "goal": "Diwali sale", "steps": [{ "title": "Price the sweets", "state": "todo" }] })),
        FakeReply::text("I wrote a plan."),
    ]);
    let assistant = assistant(&app).await;
    let id = assistant
        .start(access.clone(), "Plan the Diwali sale", None)
        .await
        .unwrap();
    settled(&app, &access, id).await;

    let results = fake.tool_results("update_state");
    assert!(
        results[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("steps[0].state")
    );
    assert_eq!(results[1]["saved"], true);
    let plan: Plan = assistant.state(dana, id).await.unwrap().unwrap();
    assert_eq!(plan.goal.as_str(), "Diwali sale");

    // The page edits the plan; the next run reads it.
    let edited = Plan {
        goal: UserText::new("Diwali sale in Ludhiana").unwrap(),
        steps: plan.steps.clone(),
    };
    assistant.set_state(dana, id, &edited).await.unwrap();
    fake.respond(FakeReply::text("Ludhiana it is."));
    send_when_idle(&app, &access, id, "What's the goal now?", None).await;
    settled(&app, &access, id).await;
    let system = fake.requests().last().unwrap().system().unwrap().to_owned();
    assert!(system.contains("Diwali sale in Ludhiana"), "{system}");
    assert!(system.contains("data, not instructions"), "{system}");
    // Another reader can neither read nor write it.
    let ravi = user(&app, "Ravi", &[]).await;
    assert!(assistant.state::<Plan>(ravi, id).await.is_err());
    assert!(assistant.set_state(ravi, id, &edited).await.is_err());
}

#[tokio::test]
async fn a_page_context_is_signed_for_its_reader_and_resolved_under_their_access() {
    let Page { app, .. } = page().await;
    let (dana, ravi) = (
        user(&app, "Daljit", &[]).await,
        user(&app, "Ravi", &[]).await,
    );
    let access = access_of(&app, dana).await;
    let note = add_note(&app, dana, "Order 1042 for Simran").await;
    let foreign = add_note(&app, ravi, "Ravi's own note").await;
    let assistant = assistant(&app).await;
    let fake = app.get::<Ai>().await.fake().unwrap();

    let context = assistant.context_ref(dana, "note", &note.to_string());
    fake.respond(FakeReply::text("That note is about order 1042."));
    let id = assistant
        .start(access.clone(), "What is this note about?", Some(&context))
        .await
        .unwrap();
    settled(&app, &access, id).await;
    let system = fake.requests().last().unwrap().system().unwrap().to_owned();
    assert!(
        system.contains("The user is looking at this note: note “Order 1042 for Simran”"),
        "{system}"
    );

    // Forged, another reader's, unknown, or a record the reader can't see: refused, and
    // the model is never asked.
    let asked = fake.requests().len();
    for bad in [
        context.replace(&note.to_string(), &foreign.to_string()),
        assistant.context_ref(ravi, "note", &note.to_string()),
        assistant.context_ref(dana, "invoice", "1"),
        assistant.context_ref(dana, "note", &foreign.to_string()),
        "/assistant/context?k=note&id=1".to_owned(),
    ] {
        let error = assistant
            .send(access.clone(), id, "And this?", Some(&bad))
            .await
            .unwrap_err();
        assert!(
            matches!(error.status.as_u16(), 403 | 404),
            "{bad}: {error:?}"
        );
    }
    assert_eq!(fake.requests().len(), asked);
}
