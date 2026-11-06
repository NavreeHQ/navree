//! [`Assistant`]: an agent's conversations on Navree UI pages.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::Utc;
use futures_util::StreamExt;
use http::Uri;
use navree_access::{AbilityFactory, PrincipalAccess};
use navree_auth::AuthService;
use navree_contracts::{Clock, SystemClock};
use navree_db::Uow;
use navree_http::{AppError, UrlSigner};
use navree_kernel::AppContext;
use navree_realtime::{Live, Topic};
use navree_ui::genui::{
    ApprovalCard, ApprovalState, ChatMessage, ChatState, Checker, Conversation, LinkPattern,
    PromptInput, Props, SurfaceFrame, ThreadList, ToolCallCard, ToolState, approval_notice,
    chat_notice, writing_key,
};
use navree_ui::node::AttrValue;
use navree_ui::{LazyAsk, LazyItem, LazyPage, LiveStream, LocalHref, Node, Region};
use serde_json::{Value, json};
use tokio::sync::Notify;
use uuid::Uuid;

use super::signing::{ActionClaim, action_url, context_ref, nonce, read_action, read_context};
use super::store::{Begun, ItemRow, RUNNING, Store, kind, no_conversation, status};
use super::tools::{AssistantTools, STATE_TOOL};
use super::{AssistantConfig, topic};
use crate::agent::{Agent, AgentRun, AgentStream, PageTools, RunShared};
use crate::memory::Conversations;
use crate::stream::StreamEvent;
use crate::{Ai, AiError};

/// The longest message a reader may send, in characters.
const MAX_MESSAGE: usize = navree_ui::genui::MAX_MESSAGE as usize;

/// How often a run checks whether someone, in any process, asked it to stop.
const STOP_CHECK: Duration = Duration::from_secs(1);

/// The ids a page's parts have, so a reply can redraw them.
pub(crate) const COMPOSER_ID: &str = "assistant-composer";
pub(crate) const APPROVAL_ID: &str = "assistant-approval";
pub(crate) const CONVERSATION_ID: &str = "assistant-conversation";

/// What the bridge, the routes and the pages share.
pub(crate) struct Core {
    pub(crate) ai: Ai,
    pub(crate) store: Store,
    pub(crate) conversations: Conversations,
    pub(crate) live: Option<Arc<Live>>,
    pub(crate) signer: Arc<UrlSigner>,
    pub(crate) config: AssistantConfig,
    stops: Mutex<HashMap<Uuid, Arc<Notify>>>,
}

impl Core {
    pub(crate) fn new(
        ai: Ai,
        store: Store,
        conversations: Conversations,
        live: Option<Arc<Live>>,
        signer: Arc<UrlSigner>,
        config: AssistantConfig,
    ) -> Self {
        Self {
            ai,
            store,
            conversations,
            live,
            signer,
            config,
            stops: Mutex::new(HashMap::new()),
        }
    }

    /// The app, while it runs (the assistant holds it weakly, through `Ai`).
    fn ctx(&self) -> Result<AppContext, AppError> {
        self.ai
            .app()
            .ok_or_else(|| AppError::service_unavailable("The app is shutting down."))
    }

    /// The page's tools for a run.
    fn tools(&self) -> Arc<dyn PageTools> {
        Arc::new(AssistantTools {
            catalog: self.config.catalog.clone(),
            state: self.config.state.clone(),
        })
    }

    /// Whether `tool` is one of the page's own, which get no card of their own.
    fn is_page_tool(&self, tool: &str) -> bool {
        (tool == STATE_TOOL && self.config.state.is_some())
            || self
                .config
                .catalog
                .as_ref()
                .is_some_and(|catalog| catalog.is_tool(tool))
    }

    /// Tells the reader's pages that the conversation changed.
    pub(crate) async fn nudge(&self, user: Uuid, conversation: Uuid) {
        if let Some(live) = &self.live
            && let Err(error) = live.nudge([Topic::user(user, topic(conversation))]).await
        {
            tracing::warn!(target: "navree::ai", %error, "an assistant's nudge could not be sent");
        }
    }

    /// The signal that stops the conversation's run in this process.
    fn stop_signal(&self, conversation: Uuid) -> Arc<Notify> {
        self.stops
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(conversation)
            .or_insert_with(|| Arc::new(Notify::new()))
            .clone()
    }

    /// Stops the conversation's run if it goes in this process.
    fn signal_stop(&self, conversation: Uuid) {
        if let Some(signal) = self
            .stops
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&conversation)
        {
            signal.notify_one();
        }
    }

    /// The run a component or an action sees for the reader: their user, their access
    /// as it is now, and `uow`.
    pub(crate) async fn reader(
        &self,
        access: Arc<PrincipalAccess>,
        conversation: Uuid,
        uow: Uow,
    ) -> Result<AgentRun, AppError> {
        let user_id = access.user.ok_or_else(AppError::unauthenticated)?;
        let ctx = self.ctx()?;
        let auth = ctx
            .get::<AuthService>()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        let user = auth
            .find_user(user_id)
            .await
            .map_err(|error| AppError::internal(error.to_string()))?
            .ok_or_else(AppError::unauthenticated)?;
        let prefixes = match ctx.try_get::<AbilityFactory>().await {
            Some(factory) => factory.permission_subjects().clone(),
            None => Arc::new(BTreeMap::new()),
        };
        let clock = match ctx.try_get::<dyn Clock>().await {
            Some(clock) => clock,
            None => Arc::new(SystemClock) as Arc<dyn Clock>,
        };
        let shared = Arc::new(RunShared {
            ctx,
            user,
            access,
            prefixes,
            clock,
            conversation_id: Some(conversation),
        });
        Ok(AgentRun::new(shared, uow))
    }

    /// What the reader's page context says, resolved under their access: a note for
    /// the model. A context the server did not sign for this reader, or of a kind the
    /// assistant has no resolver for, is refused.
    async fn resolve_context(
        &self,
        access: &Arc<PrincipalAccess>,
        conversation: Uuid,
        signed: &str,
    ) -> Result<String, AppError> {
        let user = user_of(access)?;
        let claim = read_context(&self.signer, &self.config.path, signed, user, Utc::now())?;
        let Some(resolver) = self.config.contexts.get(&claim.kind) else {
            return Err(AppError::forbidden()
                .with_detail("The page's context is not one the assistant reads."));
        };
        let uow = self.store.uow(user).await;
        let run = self
            .reader(access.clone(), conversation, uow.clone())
            .await?;
        let described = resolver.describe(&claim.key, &run).await;
        let _ = uow.rollback().await;
        Ok(format!(
            "The user is looking at this {}: {}",
            claim.kind, described?
        ))
    }

    /// Follows run `begun`'s stream in a task of its own, writing what it says into the
    /// conversation's items.
    fn follow(self: &Arc<Self>, conversation: Uuid, user: Uuid, begun: Begun, stream: AgentStream) {
        let core = self.clone();
        let stop = self.stop_signal(conversation);
        let task = async move {
            core.bridge(conversation, user, begun, stream, stop).await;
        };
        // The app's tracker waits for the run at shutdown, which stops it.
        match self.ai.app() {
            Some(ctx) => {
                ctx.tracker().spawn(task);
            }
            None => {
                tokio::spawn(task);
            }
        }
    }

    /// Writes a run into the conversation: text flushed every [`AssistantConfig::flush`]
    /// with a nudge each time, a card for each tool, each component, each approval, and
    /// the end. Stops when the reader stops it (in any process), the app shuts down, or
    /// the stream ends.
    async fn bridge(
        &self,
        conversation: Uuid,
        user: Uuid,
        begun: Begun,
        mut stream: AgentStream,
        stop: Arc<Notify>,
    ) {
        let mut writing = Writing {
            answer: Some(begun.answer),
            text: String::new(),
            dirty: false,
        };
        let mut tools: HashMap<String, Uuid> = HashMap::new();
        let mut tick = tokio::time::interval(self.config.flush);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut since_check = Duration::ZERO;
        let Some(ctx) = self.ai.app() else {
            self.log(self.store.end(conversation, user, begun.run_id).await);
            return;
        };
        let shutdown = ctx.shutdown_token();
        drop(ctx);
        let outcome = loop {
            tokio::select! {
                biased;
                () = stop.notified() => break status::STOPPED,
                () = shutdown.cancelled() => break status::STOPPED,
                event = stream.next() => match event {
                    Some(Ok(StreamEvent::Text(piece))) => {
                        if writing.answer.is_none() {
                            writing.answer = self
                                .store
                                .add(conversation, user, kind::ASSISTANT, status::WRITING, json!({ "text": "" }))
                                .await
                                .ok();
                            writing.text.clear();
                        }
                        writing.text.push_str(&piece);
                        writing.dirty = true;
                    }
                    Some(Ok(StreamEvent::ToolCall { id, tool, arguments })) => {
                        self.close(conversation, user, &mut writing).await;
                        if !self.is_page_tool(&tool) {
                            let label = self.config.label_of(&tool);
                            let content = json!({ "call_id": id, "tool": tool, "label": label, "arguments": arguments });
                            if let Ok(item) = self.store.add(conversation, user, kind::TOOL, status::RUNNING, content).await {
                                tools.insert(id, item);
                            }
                        }
                        self.nudge(user, conversation).await;
                    }
                    Some(Ok(StreamEvent::ToolResult { id, ok, .. })) => {
                        if let Some(item) = tools.remove(&id) {
                            let state = if ok { status::DONE } else { status::FAILED };
                            self.log(self.store.change(conversation, user, item, state, None).await);
                            self.nudge(user, conversation).await;
                        }
                    }
                    Some(Ok(StreamEvent::Surface { id, surface, props })) => {
                        self.close(conversation, user, &mut writing).await;
                        let content = json!({ "call_id": id, "surface": surface, "props": props });
                        self.log(self.store.add(conversation, user, kind::SURFACE, status::DONE, content).await);
                        self.nudge(user, conversation).await;
                    }
                    Some(Ok(StreamEvent::State(state))) => {
                        self.log(self.store.set_state(conversation, user, &state).await);
                        self.nudge(user, conversation).await;
                    }
                    Some(Ok(StreamEvent::Approval(pending))) => {
                        self.close(conversation, user, &mut writing).await;
                        // The approval stands in for the call's own card.
                        if let Some(item) = tools.remove(&pending.call_id) {
                            self.log(self.store.change(conversation, user, item, status::EMPTY, None).await);
                        }
                        let label = self.config.label_of(&pending.tool);
                        let content = json!({
                            "call_id": pending.call_id,
                            "tool": pending.tool,
                            "label": label,
                            "arguments": pending.arguments,
                        });
                        self.log(self.store.add(conversation, user, kind::APPROVAL, status::PENDING, content).await);
                        self.nudge(user, conversation).await;
                    }
                    Some(Ok(StreamEvent::Done(_))) => break status::DONE,
                    Some(Err(error)) => {
                        tracing::warn!(target: "navree::ai", %error, "an assistant's run failed");
                        break status::FAILED;
                    }
                    None => break status::STOPPED,
                },
                _ = tick.tick() => {
                    if writing.dirty {
                        self.flush(conversation, user, &mut writing).await;
                    }
                    since_check += self.config.flush;
                    if since_check >= STOP_CHECK {
                        since_check = Duration::ZERO;
                        if self.store.should_stop(conversation, begun.run_id).await {
                            break status::STOPPED;
                        }
                    }
                }
            }
        };
        // Dropping the stream stops the agent at its next step.
        drop(stream);
        // A tool that never answered, because the run stopped or failed in it.
        for item in tools.into_values() {
            self.log(
                self.store
                    .change(conversation, user, item, status::FAILED, None)
                    .await,
            );
        }
        match writing.answer.take() {
            Some(answer) => {
                let state = if outcome == status::DONE && writing.text.trim().is_empty() {
                    status::EMPTY
                } else {
                    outcome
                };
                let content = json!({ "text": writing.text });
                self.log(
                    self.store
                        .change(conversation, user, answer, state, Some(content))
                        .await,
                );
            }
            None if outcome == status::FAILED => {
                self.log(
                    self.store
                        .add(
                            conversation,
                            user,
                            kind::ASSISTANT,
                            status::FAILED,
                            json!({ "text": "" }),
                        )
                        .await,
                );
            }
            None if outcome == status::STOPPED => {
                let text = self.config.labels.stopped.clone();
                self.log(
                    self.store
                        .add(
                            conversation,
                            user,
                            kind::NOTICE,
                            status::DONE,
                            json!({ "text": text }),
                        )
                        .await,
                );
            }
            None => {}
        }
        self.log(self.store.end(conversation, user, begun.run_id).await);
        self.stops
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&conversation);
        self.nudge(user, conversation).await;
    }

    /// Writes the answer's text so far, and tells the pages.
    async fn flush(&self, conversation: Uuid, user: Uuid, writing: &mut Writing) {
        writing.dirty = false;
        if let Some(answer) = writing.answer {
            let content = json!({ "text": writing.text });
            self.log(
                self.store
                    .change(conversation, user, answer, status::WRITING, Some(content))
                    .await,
            );
            self.nudge(user, conversation).await;
        }
    }

    /// Finishes the answer being written, before a tool, a component or an approval: an
    /// answer with no words yet is left out of the page.
    async fn close(&self, conversation: Uuid, user: Uuid, writing: &mut Writing) {
        writing.dirty = false;
        if let Some(answer) = writing.answer.take() {
            let state = if writing.text.trim().is_empty() {
                status::EMPTY
            } else {
                status::DONE
            };
            let content = json!({ "text": writing.text });
            self.log(
                self.store
                    .change(conversation, user, answer, state, Some(content))
                    .await,
            );
        }
        writing.text.clear();
    }

    /// Logs a write that failed: the page catches up with the next one.
    fn log<T>(&self, result: Result<T, AppError>) {
        if let Err(error) = result {
            tracing::warn!(target: "navree::ai", message = %error.body()["message"], "writing an assistant's item failed");
        }
    }

    /// Ends a run that could not start.
    async fn fail(&self, conversation: Uuid, user: Uuid, begun: &Begun, error: &AiError) {
        tracing::warn!(target: "navree::ai", %error, "an assistant's run could not start");
        self.log(
            self.store
                .change(conversation, user, begun.answer, status::FAILED, None)
                .await,
        );
        self.log(self.store.end(conversation, user, begun.run_id).await);
        self.nudge(user, conversation).await;
    }

    /// The page's items for `rows`, drawn for the reader.
    async fn draw(
        &self,
        access: &Arc<PrincipalAccess>,
        conversation: Uuid,
        rows: &[ItemRow],
        csrf: Option<&str>,
    ) -> Result<Vec<LazyItem>, AppError> {
        let user = user_of(access)?;
        let labels = &self.config.labels;
        let stop = format!("{}/{conversation}/stop", self.config.path);
        let mut reader: Option<(AgentRun, Uow)> = None;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let key = row.id.to_string();
            let cursor = key.clone();
            let content = row.json();
            let item = match row.kind.as_str() {
                kind::USER => ChatMessage::user(key, cursor, row.text("text")).into_item(labels),
                _ if row.status == status::EMPTY => LazyItem::new(key, cursor, Node::Empty)
                    .attr("hidden", AttrValue::Flag)
                    .quiet(),
                kind::ASSISTANT => {
                    let state = match row.status.as_str() {
                        status::WRITING => ChatState::Writing,
                        status::STOPPED => ChatState::Stopped,
                        status::FAILED => ChatState::Failed,
                        _ => ChatState::Done,
                    };
                    let mut message =
                        ChatMessage::assistant(key, cursor, row.text("text")).state(state);
                    if state == ChatState::Writing {
                        message = message.stop(stop.as_str(), csrf.map(str::to_owned));
                    }
                    message.into_item(labels)
                }
                kind::TOOL => ToolCallCard::new(key, cursor, row.text("label"))
                    .state(match row.status.as_str() {
                        status::DONE => ToolState::Done,
                        status::FAILED => ToolState::Failed,
                        _ => ToolState::Running,
                    })
                    .arguments(content.get("arguments").cloned().unwrap_or(Value::Null))
                    .into_item(labels),
                kind::SURFACE => {
                    if reader.is_none() {
                        let uow = self.store.uow(user).await;
                        let run = self
                            .reader(access.clone(), conversation, uow.clone())
                            .await?;
                        reader = Some((run, uow));
                    }
                    let (run, _) = reader.as_ref().expect("made above");
                    self.surface(user, conversation, row, &content, run, csrf)
                        .await
                }
                kind::APPROVAL => {
                    let state = match row.status.as_str() {
                        status::APPROVED => ApprovalState::Approved,
                        status::REJECTED => ApprovalState::Rejected(
                            Some(row.text("reason")).filter(|reason| !reason.is_empty()),
                        ),
                        _ => ApprovalState::Waiting,
                    };
                    approval_notice(key, cursor, &row.text("label"), &state, labels)
                }
                _ => chat_notice(key, cursor, row.text("text")),
            };
            items.push(item);
        }
        if let Some((_, uow)) = reader {
            let _ = uow.rollback().await;
        }
        Ok(items)
    }

    /// A component's item: drawn under the reader's access now, with its buttons signed
    /// for this reader, conversation and item. A reader who lost access sees its place
    /// and nothing it would show.
    async fn surface(
        &self,
        user: Uuid,
        conversation: Uuid,
        row: &ItemRow,
        content: &Value,
        run: &AgentRun,
        csrf: Option<&str>,
    ) -> LazyItem {
        let key = row.id.to_string();
        let cursor = key.clone();
        let Some(catalog) = &self.config.catalog else {
            return SurfaceFrame::failed(key).into_item(cursor);
        };
        let surface = content
            .get("surface")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let props = content.get("props").cloned().unwrap_or(Value::Null);
        match catalog.render(surface, &props, run).await {
            Ok((title, view)) => {
                let (body, buttons) = view.into_parts();
                let mut frame =
                    SurfaceFrame::new(key, title, body).labels(self.config.frame_labels.clone());
                let now = Utc::now();
                for button in &buttons {
                    let claim = ActionClaim {
                        conversation,
                        item: row.id,
                        action: button.action_name().to_owned(),
                        label: button.label().to_owned(),
                        user,
                        nonce: nonce(),
                        input: button.input().clone(),
                    };
                    if let Some(url) = action_url(&self.signer, &self.config.path, &claim, now) {
                        frame = frame.button(button, LocalHref::from(url), csrf);
                    }
                }
                frame.into_item(cursor)
            }
            Err(error) if matches!(error.status.as_u16(), 401 | 403 | 404) => {
                SurfaceFrame::unavailable(key)
                    .labels(self.config.frame_labels.clone())
                    .into_item(cursor)
            }
            Err(error) => {
                tracing::warn!(target: "navree::ai", surface, message = %error.body()["message"], "a component could not be drawn");
                SurfaceFrame::failed(key)
                    .labels(self.config.frame_labels.clone())
                    .into_item(cursor)
            }
        }
    }

    /// The pending approval of the conversation, when one waits.
    async fn pending(&self, conversation: Uuid) -> Result<Option<ItemRow>, AppError> {
        let (rows, _) = self.store.page(conversation, None).await?;
        Ok(rows
            .into_iter()
            .rev()
            .find(|row| row.kind == kind::APPROVAL && row.status == status::PENDING))
    }

    /// The approval's place in the page: a live region, holding the card when an
    /// approval waits.
    pub(crate) async fn approval_region(
        &self,
        conversation: Uuid,
        csrf: Option<&str>,
        catch_up: bool,
    ) -> Result<Node, AppError> {
        let path = &self.config.path;
        let card = self.pending(conversation).await?.map(|row| {
            let mut card = ApprovalCard::new(
                row.text("call_id"),
                row.text("label"),
                row.json().get("arguments").cloned().unwrap_or(Value::Null),
            )
            .routes(
                format!("{path}/{conversation}/approve"),
                format!("{path}/{conversation}/reject"),
            )
            .labels(self.config.labels.clone());
            if let Some(token) = csrf {
                card = card.csrf(token.to_owned());
            }
            card
        });
        let stream = LiveStream::topics([topic(conversation)]);
        let mut live = stream.on(topic(conversation));
        if catch_up {
            live = live.catch_up();
        }
        Ok(Region::new(APPROVAL_ID)
            .src(format!("{path}/{conversation}/approval"))
            .live_on(live)
            .children(card)
            .into())
    }

    /// The composer, as a page draws it.
    pub(crate) fn composer(
        &self,
        conversation: Option<Uuid>,
        csrf: Option<&str>,
        context: Option<&str>,
        value: Option<&str>,
        error: Option<&str>,
    ) -> Node {
        let path = &self.config.path;
        let action = match conversation {
            Some(id) => format!("{path}/{id}/messages"),
            None => path.clone(),
        };
        let mut composer = PromptInput::new(COMPOSER_ID, action).labels(self.config.labels.clone());
        if let Some(token) = csrf {
            composer = composer.csrf(token.to_owned());
        }
        if let Some(context) = context {
            composer = composer.context(context.to_owned());
        }
        if let Some(value) = value {
            composer = composer.value(value.to_owned());
        }
        if let Some(error) = error {
            composer = composer.error(error.to_owned());
        }
        composer.into()
    }
}

/// The answer being written.
struct Writing {
    answer: Option<Uuid>,
    text: String,
    dirty: bool,
}

/// The reader's user id.
fn user_of(access: &PrincipalAccess) -> Result<Uuid, AppError> {
    access.user.ok_or_else(AppError::unauthenticated)
}

/// A message as it may be sent: trimmed, not empty, not too long.
fn checked_message(message: &str) -> Result<&str, AppError> {
    let message = message.trim();
    if message.is_empty() {
        return Err(AppError::bad_request("Write a message first."));
    }
    if message.chars().count() > MAX_MESSAGE {
        return Err(AppError::bad_request(format!(
            "A message is at most {MAX_MESSAGE} characters."
        )));
    }
    Ok(message)
}

/// What an action's post did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Acted {
    /// It ran: the words to show.
    Done(String),
    /// It is destructive and was not answered yet: what to ask, with no script.
    Ask {
        /// The button's words.
        label: String,
    },
}

/// What a page draws for one conversation, in the places the app chooses: the
/// conversation itself, the approval's place (outside it, where nothing a model chose
/// is drawn), and the composer.
#[derive(Clone, Debug)]
pub struct AssistantPage {
    /// The conversation, live.
    pub conversation: Node,
    /// The approval's place: a live region that holds the card when one waits.
    pub approval: Node,
    /// The composer.
    pub composer: Node,
}

/// What a page passes when it draws a conversation.
#[derive(Clone, Debug, Default)]
pub struct PageOptions {
    /// The session's CSRF token, for every form.
    pub csrf: Option<String>,
    /// What the page is about: a record's kind and key, signed for the reader and sent
    /// with every message.
    pub context: Option<(String, String)>,
    /// Text to start the composer with.
    pub draft: Option<String>,
    /// Why the last message was refused.
    pub error: Option<String>,
}

/// An agent's conversations on Navree UI pages: inject `Arc<Assistant<A>>` after
/// registering it with
/// [`m.assistant::<A>(..)`](super::ModuleBuilderAssistantExt::assistant). See the
/// [module's docs](super).
pub struct Assistant<A> {
    core: Arc<Core>,
    _agent: PhantomData<fn() -> A>,
}

impl<A> fmt::Debug for Assistant<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Assistant")
            .field("path", &self.core.config.path)
            .finish_non_exhaustive()
    }
}

impl<A: Agent> Assistant<A> {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self {
            core,
            _agent: PhantomData,
        }
    }

    /// Where its routes are: `/assistant`.
    pub fn path(&self) -> &str {
        &self.core.config.path
    }

    /// The page of conversation `id`: `/assistant/<id>`, which the app draws.
    pub fn page_path(&self, id: Uuid) -> String {
        format!("{}/{id}", self.core.config.path)
    }

    /// The conversation's live topic, heard by its owner only.
    pub fn topic(&self, conversation: Uuid) -> String {
        topic(conversation)
    }

    /// Checks that conversation `id` is `user`'s, with this agent.
    async fn owned(&self, user: Uuid, id: Uuid) -> Result<(), AppError> {
        self.core
            .conversations
            .check_owner(user, A::NAME, id)
            .await
            .map_err(|_| no_conversation())
    }

    /// Starts a conversation with the reader's first `message` (and their page's signed
    /// `context`): its id. The answer is written in the background, into the page.
    pub async fn start(
        &self,
        access: Arc<PrincipalAccess>,
        message: &str,
        context: Option<&str>,
    ) -> Result<Uuid, AppError> {
        let user = user_of(&access)?;
        let message = checked_message(message)?;
        if let Some(signed) = context.filter(|text| !text.is_empty()) {
            read_context(
                &self.core.signer,
                &self.core.config.path,
                signed,
                user,
                Utc::now(),
            )?;
        }
        // The conversation is committed before its run starts, which reads and writes
        // it beside the request (see the store's docs).
        let conversations = &self.core.conversations;
        let row = self
            .core
            .store
            .own(user, async {
                conversations
                    .create(user, A::NAME, message)
                    .await
                    .map_err(|error| error.to_app_error())
            })
            .await?;
        if let Err(error) = self.send(access, row.id, message, context).await {
            // A first message that was refused leaves no conversation behind.
            let removed = self
                .core
                .store
                .own(user, async {
                    conversations
                        .delete(user, row.id)
                        .await
                        .map_err(|error| error.to_app_error())
                })
                .await;
            if let Err(removal) = removed {
                tracing::warn!(target: "navree::ai", message = %removal.body()["message"], "a refused conversation could not be removed");
            }
            return Err(error);
        }
        Ok(row.id)
    }

    /// Sends the reader's `message` in conversation `id` and starts the answer, which is
    /// written in the background, into the page. While an answer is being written, a
    /// new message is a 409.
    pub async fn send(
        &self,
        access: Arc<PrincipalAccess>,
        id: Uuid,
        message: &str,
        context: Option<&str>,
    ) -> Result<(), AppError> {
        let user = user_of(&access)?;
        let message = checked_message(message)?;
        self.owned(user, id).await?;
        let mut notes = Vec::new();
        if let Some(signed) = context.filter(|text| !text.is_empty()) {
            notes.push(self.core.resolve_context(&access, id, signed).await?);
        }
        let run = self.core.store.run(id, user).await?;
        if let Some(state) = run.state.as_deref() {
            notes.push(format!(
                "The conversation's working state, as JSON: {state}"
            ));
        }
        let mut begun = self.core.store.begin(id, user, Some(message)).await?;
        for note in std::mem::take(&mut begun.notes) {
            notes.push(format!("Since your last answer, on the page: {note}"));
        }
        self.core.nudge(user, id).await;
        let mut run = self
            .core
            .ai
            .agent::<A>()
            .acting_as(access)
            .conversation(Some(id))
            .page(self.core.tools());
        for note in notes {
            run = run.note(note);
        }
        match run.stream(message).await {
            Ok(stream) => {
                self.core.follow(id, user, begun, stream);
                Ok(())
            }
            Err(error) => {
                self.core.fail(id, user, &begun, &error).await;
                Err(error.to_app_error())
            }
        }
    }

    /// Stops the answer being written in conversation `id`, in whichever process writes
    /// it. Nothing happens when none is.
    pub async fn stop(&self, user: Uuid, id: Uuid) -> Result<(), AppError> {
        self.core.store.owned(id, user).await?;
        self.core.store.ask_to_stop(id, user).await?;
        self.core.signal_stop(id);
        Ok(())
    }

    /// Approves the call `call_id` the conversation waits on: the tool runs (its access
    /// checked again), and the rest of the run is written into the page.
    pub async fn approve(
        &self,
        access: Arc<PrincipalAccess>,
        id: Uuid,
        call_id: &str,
    ) -> Result<(), AppError> {
        self.decide(access, id, call_id, None, true).await
    }

    /// Declines the call `call_id`: the model is told the reader declined, and why, and
    /// its answer is written into the page.
    pub async fn reject(
        &self,
        access: Arc<PrincipalAccess>,
        id: Uuid,
        call_id: &str,
        reason: Option<&str>,
    ) -> Result<(), AppError> {
        let reason = reason
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(|reason| reason.chars().take(500).collect::<String>());
        self.decide(access, id, call_id, reason, false).await
    }

    async fn decide(
        &self,
        access: Arc<PrincipalAccess>,
        id: Uuid,
        call_id: &str,
        reason: Option<String>,
        approve: bool,
    ) -> Result<(), AppError> {
        let user = user_of(&access)?;
        self.owned(user, id).await?;
        let Some(item) = self
            .core
            .pending(id)
            .await?
            .filter(|item| item.text("call_id") == call_id)
        else {
            return Err(AppError::not_found().with_detail("No call is waiting for that answer."));
        };
        let begun = self.core.store.begin(id, user, None).await?;
        let mut content = item.json();
        if let (Some(reason), Some(map)) = (&reason, content.as_object_mut()) {
            map.insert("reason".to_owned(), Value::String(reason.clone()));
        }
        let state = if approve {
            status::APPROVED
        } else {
            status::REJECTED
        };
        self.core
            .store
            .change(id, user, item.id, state, Some(content))
            .await?;
        self.core.nudge(user, id).await;
        let run = self
            .core
            .ai
            .agent::<A>()
            .acting_as(access)
            .conversation(Some(id))
            .page(self.core.tools());
        let stream = if approve {
            run.approve_stream(call_id).await
        } else {
            run.reject_stream(call_id, reason).await
        };
        match stream {
            Ok(stream) => {
                self.core.follow(id, user, begun, stream);
                Ok(())
            }
            Err(error) => {
                self.core.log(
                    self.core
                        .store
                        .change(id, user, item.id, status::FAILED, None)
                        .await,
                );
                self.core.fail(id, user, &begun, &error).await;
                Err(error.to_app_error())
            }
        }
    }

    /// What a page draws for conversation `id`: the conversation, live, at its newest
    /// items; the approval's place; and the composer.
    pub async fn page(
        &self,
        access: Arc<PrincipalAccess>,
        id: Uuid,
        options: PageOptions,
    ) -> Result<AssistantPage, AppError> {
        let user = user_of(&access)?;
        self.owned(user, id).await?;
        let run = self.core.store.run(id, user).await?;
        let (rows, earlier) = self.core.store.page(id, None).await?;
        let csrf = options.csrf.as_deref();
        let items = self.core.draw(&access, id, &rows, csrf).await?;
        let path = &self.core.config.path;
        // An answer being written goes on between this draw and the moment the page's
        // stream opens: the page asks once then for what it missed.
        let writing = run.status == RUNNING;
        let stream = LiveStream::topics([topic(id)]);
        let mut live = stream.on(topic(id));
        if writing {
            live = live.catch_up();
        }
        let mut conversation = Conversation::new(CONVERSATION_ID, format!("{path}/{id}/items"))
            .items(items)
            .since(run.revision.to_string())
            .live(live)
            .labels(self.core.config.labels.clone());
        if earlier && let Some(first) = rows.first() {
            conversation =
                conversation.earlier(format!("{path}/{id}/items?dir=start&cursor={}", first.id));
        }
        let context = options
            .context
            .as_ref()
            .map(|(kind, key)| self.context_ref(user, kind, key));
        Ok(AssistantPage {
            conversation: conversation.into(),
            approval: self.core.approval_region(id, csrf, writing).await?,
            composer: self.core.composer(
                Some(id),
                csrf,
                context.as_deref(),
                options.draft.as_deref(),
                options.error.as_deref(),
            ),
        })
    }

    /// The composer, for a reply that draws it again.
    pub(crate) fn composer_node(
        &self,
        conversation: Option<Uuid>,
        csrf: Option<&str>,
        context: Option<&str>,
        value: Option<&str>,
        error: Option<&str>,
    ) -> Node {
        self.core
            .composer(conversation, csrf, context, value, error)
    }

    /// The composer of a page with no conversation yet: its first message starts one.
    pub fn new_composer(&self, user: Uuid, options: PageOptions) -> Node {
        let context = options
            .context
            .as_ref()
            .map(|(kind, key)| self.context_ref(user, kind, key));
        self.core.composer(
            None,
            options.csrf.as_deref(),
            context.as_deref(),
            options.draft.as_deref(),
            options.error.as_deref(),
        )
    }

    /// The reader's conversations with this agent, newest first, as links to their
    /// pages; `current` is marked.
    pub async fn threads(&self, user: Uuid, current: Option<Uuid>) -> Result<Node, AppError> {
        let page = self
            .core
            .conversations
            .list_for(user, navree_db::PageQuery::default())
            .await
            .map_err(|error| error.to_app_error())?;
        let mut list = ThreadList::new(self.core.config.labels.threads.clone());
        for thread in page
            .data
            .into_iter()
            .filter(|thread| thread.agent == A::NAME)
        {
            list = list.thread(
                thread
                    .title
                    .unwrap_or_else(|| self.core.config.labels.untitled.clone()),
                self.page_path(thread.id),
                Some(thread.id) == current,
            );
        }
        Ok(list.into())
    }

    /// What a live list of conversation `id` asked for: a page of items before or after
    /// a cursor, or what changed since a revision. An answer that was being written and
    /// is now finished leaves under its writing key and comes back under its own.
    pub async fn items(
        &self,
        access: Arc<PrincipalAccess>,
        id: Uuid,
        ask: &LazyAsk,
        csrf: Option<&str>,
    ) -> Result<LazyPage, AppError> {
        let user = user_of(&access)?;
        let run = self.core.store.owned(id, user).await?;
        let cursor = |text: &str| text.parse::<Uuid>().ok();
        match ask {
            LazyAsk::Since(since) => {
                let since = since.parse::<i64>().unwrap_or(run.revision);
                let (rows, more) = self.core.store.changes(id, since).await?;
                let next = rows.iter().map(|row| row.revision).max().unwrap_or(since);
                let gone: Vec<String> = rows
                    .iter()
                    .filter(|row| row.kind == kind::ASSISTANT && row.status != status::WRITING)
                    .map(|row| writing_key(&row.id.to_string()))
                    .collect();
                let items = self.core.draw(&access, id, &rows, csrf).await?;
                Ok(LazyPage::changes(items, next.to_string(), gone).more(more))
            }
            LazyAsk::Before(before) => {
                let (rows, more) = self.core.store.page(id, cursor(before)).await?;
                let items = self.core.draw(&access, id, &rows, csrf).await?;
                Ok(LazyPage::new(items, more))
            }
            LazyAsk::After(after) => {
                let Some(after) = cursor(after) else {
                    return Ok(LazyPage::new(Vec::new(), false));
                };
                let (rows, more) = self.core.store.after(id, after).await?;
                let items = self.core.draw(&access, id, &rows, csrf).await?;
                Ok(LazyPage::new(items, more))
            }
            LazyAsk::First => {
                let (rows, more) = self.core.store.page(id, None).await?;
                let items = self.core.draw(&access, id, &rows, csrf).await?;
                Ok(LazyPage::new(items, more))
            }
        }
    }

    /// The approval's place of conversation `id`, for its live region.
    pub async fn approval(
        &self,
        access: Arc<PrincipalAccess>,
        id: Uuid,
        csrf: Option<&str>,
    ) -> Result<Node, AppError> {
        let user = user_of(&access)?;
        self.core.store.owned(id, user).await?;
        self.core.approval_region(id, csrf, false).await
    }

    /// Runs the post of a component's button, sent to `uri`: refused unless the server
    /// signed that URL for this reader, the item is a component of their conversation
    /// that declares the action, the reader may take it now, its input passes the check
    /// again, and the post was not used before. A destructive action not yet answered
    /// (`confirmed` false) asks first.
    pub async fn act(
        &self,
        access: Arc<PrincipalAccess>,
        uri: &Uri,
        confirmed: bool,
    ) -> Result<(Uuid, Acted), AppError> {
        let user = user_of(&access)?;
        let claim = read_action(&self.core.signer, &self.core.config.path, uri, Utc::now())?;
        if claim.user != user {
            return Err(AppError::forbidden().with_detail("This button is someone else's."));
        }
        self.owned(user, claim.conversation).await?;
        let Some(item) = self
            .core
            .store
            .item(claim.conversation, claim.item)
            .await?
            .filter(|item| item.kind == kind::SURFACE)
        else {
            return Err(AppError::not_found().with_detail("No such component."));
        };
        let Some(catalog) = &self.core.config.catalog else {
            return Err(AppError::not_found().with_detail("No such component."));
        };
        let surface = item.text("surface");
        if !catalog.takes(&surface, &claim.action) {
            return Err(AppError::not_found().with_detail("This component takes no such action."));
        }
        if catalog.is_destructive(&claim.action) && !confirmed {
            return Ok((claim.conversation, Acted::Ask { label: claim.label }));
        }
        let uow = self.core.store.uow(user).await;
        let run = self
            .core
            .reader(access.clone(), claim.conversation, uow.clone())
            .await?;
        if let Err(error) = self
            .core
            .store
            .use_nonce(&uow, &claim.nonce, claim.item, user)
            .await
        {
            let _ = uow.rollback().await;
            return Err(error);
        }
        let done = match uow
            .scope(catalog.run_action(&surface, &claim.action, &claim.input, &run))
            .await
        {
            Ok(done) => done,
            Err(refusal) => {
                let _ = uow.rollback().await;
                return Err(refused(refusal));
            }
        };
        if let Some(note) = &done.note {
            self.core
                .store
                .add_note(&uow, claim.conversation, note)
                .await?;
        }
        let revision = self.core.store.bump(&uow, claim.conversation).await?;
        self.core
            .store
            .insert(
                &uow,
                claim.conversation,
                kind::NOTICE,
                status::DONE,
                json!({ "text": done.message }),
                revision,
            )
            .await?;
        uow.commit().await.map_err(|error| {
            navree_db::map_db_error(&error).unwrap_or_else(|| AppError::internal(error))
        })?;
        self.core.nudge(user, claim.conversation).await;
        Ok((claim.conversation, Acted::Done(done.message)))
    }

    /// A signed reference to record `key` of `kind`, for `user`'s composer to send: the
    /// assistant hears what that page is about, resolved under their access.
    pub fn context_ref(&self, user: Uuid, kind: &str, key: &str) -> String {
        context_ref(
            &self.core.signer,
            &self.core.config.path,
            user,
            kind,
            key,
            Utc::now(),
        )
    }

    /// The conversation's working state, as `T`; `None` when it has none, or one that
    /// no longer passes `T`'s check.
    pub async fn state<T: Props>(&self, user: Uuid, id: Uuid) -> Result<Option<T>, AppError> {
        self.owned(user, id).await?;
        let Some(value) = self.core.store.run(id, user).await?.state else {
            return Ok(None);
        };
        let value: Value = serde_json::from_str(&value).unwrap_or(Value::Null);
        let links: [LinkPattern; 0] = [];
        let mut checker = Checker::new(&links);
        Ok(T::check(&value, "", &mut checker))
    }

    /// Replaces the conversation's working state, as the page edited it: the page's
    /// other tabs see it, and the assistant reads it in its next run.
    pub async fn set_state<T: Props>(
        &self,
        user: Uuid,
        id: Uuid,
        state: &T,
    ) -> Result<(), AppError> {
        self.owned(user, id).await?;
        self.core
            .store
            .set_state(id, user, &state.to_json())
            .await?;
        self.core.nudge(user, id).await;
        Ok(())
    }
}

/// An action's refusal, as the post's answer.
fn refused(refusal: navree_ui::genui::ActionRefusal) -> AppError {
    use navree_ui::genui::ActionRefusal;
    match refusal {
        ActionRefusal::Unknown => {
            AppError::not_found().with_detail("This component takes no such action.")
        }
        ActionRefusal::Forbidden => AppError::forbidden(),
        ActionRefusal::Invalid(refusal) => AppError::bad_request(format!(
            "The button's input was refused: {}",
            refusal.message()
        )),
        ActionRefusal::Failed(error) => error,
    }
}
