//! The parts a conversation with an assistant is drawn with: the [`Conversation`] that
//! holds it, each [`ChatMessage`], the [`ToolCallCard`] of a tool the assistant used,
//! [`Reasoning`], [`Sources`] and [`Citation`]s, [`Suggestions`], the [`PromptInput`]
//! the reader writes in, and the [`ApprovalCard`] only the page draws.
//!
//! Every part works as plain links and forms with no script. Words from a model are
//! untrusted: an assistant's answer is drawn as [`Markdown`] from a source you do not
//! control (no raw HTML, no picture, links checked), and everything else it chose
//! (a tool's arguments, a reason, a caption) is drawn as text.
//!
//! **What is announced.** A conversation is a [`LazyList`] that merges live changes, and
//! it announces only messages that are finished, politely, once they stop coming. A
//! message being written is drawn under a key of its own and never announced; when it is
//! done it comes back under its own key and is read out once. A `log` role is not used,
//! because a log reads out every change, which for an answer that grows would be every
//! few words.

use serde_json::Value;

use crate::components::{
    Alert, AlertTone, Badge, BadgeVariant, Bubble, BubbleSide, Button, Card, Collapsible, Field,
    LazyEnd, LazyItem, LazyLabels, LazyList, LazyOrder, Live, Marker, MarkerVariant, Message, Row,
    Size, Spinner, Stack, Text, Textarea, Tone, icon,
};
use crate::form::csrf_field;
use crate::html::{Href, LocalHref};
use crate::icons;
use crate::markdown::Markdown;
use crate::node::{AttrValue, Element, IntoNodes, Node, el, into_node};

/// The longest message the composer takes, in characters.
pub const MAX_MESSAGE: u32 = 4000;

/// The most characters of one argument's value an approval card or a tool's card shows.
const MAX_SHOWN: usize = 300;

/// The most arguments an approval card lists; the rest are counted.
const MAX_ARGUMENTS: usize = 20;

/// The words a conversation's parts say, for translation.
#[derive(Clone, Debug)]
pub struct ChatLabels {
    /// What the conversation is called for assistive technology.
    pub conversation: String,
    /// Above the reader's own messages.
    pub you: String,
    /// Above the assistant's.
    pub assistant: String,
    /// While an answer is being written.
    pub writing: String,
    /// After an answer the reader stopped.
    pub stopped: String,
    /// After an answer that failed.
    pub failed: String,
    /// The button that stops an answer.
    pub stop: String,
    /// What is read out when the assistant has answered, before the start of its words.
    pub answered: String,
    /// What is read out for several new messages, with `{n}`.
    pub new_many: String,
    /// The composer's label.
    pub message: String,
    /// The composer's hint while empty.
    pub placeholder: String,
    /// The composer's button.
    pub send: String,
    /// A tool that is running.
    pub tool_running: String,
    /// A tool that ran.
    pub tool_done: String,
    /// A tool that failed.
    pub tool_failed: String,
    /// The details of a tool's call.
    pub details: String,
    /// The assistant's reasoning, folded.
    pub reasoning: String,
    /// The list of sources.
    pub sources: String,
    /// The suggestions' group.
    pub suggestions: String,
    /// The approval card's title, before the tool's label.
    pub asks: String,
    /// What the approval card says nothing does until the reader answers.
    pub nothing_yet: String,
    /// The approval card's yes.
    pub approve: String,
    /// The approval card's no.
    pub reject: String,
    /// The label of the reason a reader may give for no.
    pub reason: String,
    /// In the conversation, where an approval waits.
    pub waiting: String,
    /// In the conversation, after an approval.
    pub approved: String,
    /// In the conversation, after a refusal.
    pub rejected: String,
    /// More arguments than the card lists, with `{n}`.
    pub more_arguments: String,
    /// The link at the start of a long conversation.
    pub earlier: String,
    /// The list of a reader's conversations.
    pub threads: String,
    /// A conversation with no title yet.
    pub untitled: String,
}

impl Default for ChatLabels {
    fn default() -> Self {
        Self {
            conversation: "Conversation".to_owned(),
            you: "You".to_owned(),
            assistant: "Assistant".to_owned(),
            writing: "Writing…".to_owned(),
            stopped: "Stopped".to_owned(),
            failed: "The assistant couldn't answer. Try again.".to_owned(),
            stop: "Stop".to_owned(),
            answered: "The assistant answered".to_owned(),
            new_many: "{n} new messages".to_owned(),
            message: "Message".to_owned(),
            placeholder: "Ask the assistant".to_owned(),
            send: "Send".to_owned(),
            tool_running: "Working".to_owned(),
            tool_done: "Done".to_owned(),
            tool_failed: "Failed".to_owned(),
            details: "Details".to_owned(),
            reasoning: "Thinking".to_owned(),
            sources: "Sources".to_owned(),
            suggestions: "Suggestions".to_owned(),
            asks: "The assistant asks to".to_owned(),
            nothing_yet: "Nothing happens until you answer.".to_owned(),
            approve: "Approve".to_owned(),
            reject: "Reject".to_owned(),
            reason: "Why not? (optional)".to_owned(),
            waiting: "Waiting for your answer".to_owned(),
            approved: "Approved".to_owned(),
            rejected: "Rejected".to_owned(),
            more_arguments: "and {n} more".to_owned(),
            earlier: "Show earlier messages".to_owned(),
            threads: "Conversations".to_owned(),
            untitled: "New conversation".to_owned(),
        }
    }
}

/// `text` kept to the characters an id may hold: letters, digits, `-` and `_`, at most 64.
pub(crate) fn id_part(text: &str) -> String {
    let part: String = text
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(64)
        .collect();
    if part.is_empty() { "x".to_owned() } else { part }
}

/// `text` cut at `most` characters, with an ellipsis.
fn cut(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(most).collect();
    out.push('…');
    out
}

/// Text as it was written: its lines kept, nothing read as markup.
fn plain(text: &str) -> Node {
    let mut nodes = Vec::new();
    for (at, line) in text.lines().enumerate() {
        if at > 0 {
            nodes.push(el("br").into());
        }
        nodes.push(Node::Text(line.to_owned().into()));
    }
    Node::Fragment(nodes)
}

/// A JSON value as a reader reads it: a string as it is, a number, yes or no, and
/// anything else as compact JSON, cut at [`MAX_SHOWN`] characters.
fn shown(value: &Value) -> String {
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "yes".to_owned(),
        Value::Bool(false) => "no".to_owned(),
        Value::Null => "—".to_owned(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    };
    cut(&text, MAX_SHOWN)
}

/// A post form to `action` with the session's token, holding `fields`.
pub(crate) fn post_form(
    action: &LocalHref,
    csrf: Option<&str>,
    fields: impl IntoNodes,
) -> Element {
    el("form")
        .attr("method", "post")
        .attr("action", action.as_str().to_owned())
        .child(csrf.map(|token| csrf_field(token.to_owned())))
        .children(fields)
}

/// A hidden field.
pub(crate) fn hidden(name: &'static str, value: impl Into<AttrValue>) -> Element {
    el("input")
        .attr("type", "hidden")
        .attr("name", name)
        .attr("value", value)
}

/// Who wrote a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatRole {
    /// The reader.
    User,
    /// The assistant.
    Assistant,
}

/// Where an assistant's message stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChatState {
    /// Finished.
    #[default]
    Done,
    /// Being written: drawn under its writing key and never announced.
    Writing,
    /// Stopped by the reader before it finished.
    Stopped,
    /// Failed.
    Failed,
}

/// The key a message has while it is being written: its own key with `.w`.
pub fn writing_key(key: &str) -> String {
    format!("{key}.w")
}

/// One message of a conversation, as an item of its [`Conversation`].
///
/// ```
/// use navree_ui::genui::{ChatLabels, ChatMessage, ChatState};
/// use navree_ui::render;
///
/// let labels = ChatLabels::default();
/// let item = ChatMessage::assistant("m7", "0007", "UPI did **best** <script>x</script>")
///     .state(ChatState::Writing)
///     .into_item(&labels);
/// let html = render(&item.into());
/// assert!(html.contains(r#"data-key="m7.w""#));
/// assert!(html.contains("<strong>best</strong>"));
/// assert!(!html.contains("<script>"));
/// assert!(html.contains(r#"aria-busy="true""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ChatMessage {
    key: String,
    cursor: String,
    role: ChatRole,
    text: String,
    state: ChatState,
    author: Option<String>,
    time: Option<Node>,
    stop: Option<(LocalHref, Option<String>)>,
    extra: Vec<Node>,
}

impl ChatMessage {
    /// A message the reader wrote: `key` names it in the list and `cursor` places it.
    pub fn user(key: impl Into<String>, cursor: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(ChatRole::User, key.into(), cursor.into(), text.into())
    }

    /// A message the assistant wrote, in Markdown from a source you do not control.
    pub fn assistant(
        key: impl Into<String>,
        cursor: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self::new(ChatRole::Assistant, key.into(), cursor.into(), text.into())
    }

    fn new(role: ChatRole, key: String, cursor: String, text: String) -> Self {
        Self {
            key,
            cursor,
            role,
            text,
            state: ChatState::Done,
            author: None,
            time: None,
            stop: None,
            extra: Vec::new(),
        }
    }

    /// Sets where it stands.
    pub fn state(mut self, state: ChatState) -> Self {
        self.state = state;
        self
    }

    /// Names who wrote it, in place of `You` or `Assistant`.
    pub fn author(mut self, name: impl Into<String>) -> Self {
        self.author = Some(name.into());
        self
    }

    /// Shows when, below it: a `<time>` the app drew.
    pub fn time(mut self, time: impl Into<Node>) -> Self {
        self.time = Some(time.into());
        self
    }

    /// While it is being written, a Stop button that posts to `url`.
    pub fn stop(mut self, url: impl Into<LocalHref>, csrf: Option<String>) -> Self {
        self.stop = Some((url.into(), csrf));
        self
    }

    /// Adds parts below the words: [`Sources`], [`Reasoning`].
    pub fn with(mut self, extra: impl IntoNodes) -> Self {
        extra.push_into(&mut self.extra);
        self
    }

    /// The key it is listed under now: [`writing_key`] while it is being written.
    pub fn item_key(&self) -> String {
        if self.state == ChatState::Writing {
            writing_key(&self.key)
        } else {
            self.key.clone()
        }
    }

    /// The message as an item of a conversation.
    pub fn into_item(self, labels: &ChatLabels) -> LazyItem {
        let key = self.item_key();
        let side = match self.role {
            ChatRole::User => BubbleSide::End,
            ChatRole::Assistant => BubbleSide::Start,
        };
        let who = self.author.clone().unwrap_or_else(|| match self.role {
            ChatRole::User => labels.you.clone(),
            ChatRole::Assistant => labels.assistant.clone(),
        });
        let words: Node = match self.role {
            ChatRole::User => plain(&self.text),
            ChatRole::Assistant => Markdown::new(self.text.clone())
                .heading_level(3)
                .max_bytes(64 * 1024)
                .into(),
        };
        let writing = self.state == ChatState::Writing;
        let status: Option<Node> = match self.state {
            ChatState::Done => None,
            ChatState::Writing => Some(
                Row::gap(2)
                    .children((
                        Spinner::new(labels.writing.clone()),
                        Text::muted(labels.writing.clone()).inline(),
                        self.stop.as_ref().map(|(url, csrf)| {
                            post_form(
                                url,
                                csrf.as_deref(),
                                Button::outline(labels.stop.clone())
                                    .size(Size::Sm)
                                    .icon(icons::SQUARE)
                                    .submit(),
                            )
                        }),
                    ))
                    .into(),
            ),
            ChatState::Stopped => Some(Badge::new(labels.stopped.clone()).into()),
            ChatState::Failed => Some(
                Alert::new(AlertTone::Danger)
                    .body(labels.failed.clone())
                    .into(),
            ),
        };
        let empty = self.text.trim().is_empty();
        let bubble = (!empty).then(|| Bubble::new(side, words));
        let mut message = Message::new(side)
            .attr("data-genui", "message")
            .attr(
                "data-role",
                match self.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                },
            )
            .attr("aria-busy", if writing { "true" } else { "false" })
            .header(el("span").child(who))
            .content((bubble, self.extra, status));
        if let Some(time) = self.time {
            message = message.footer(time);
        }
        let item = LazyItem::new(key, self.cursor, message);
        match (self.role, self.state) {
            // What the reader wrote, and what is still being written, go unsaid.
            (ChatRole::User, _) | (_, ChatState::Writing) => item.quiet(),
            _ => item.announce(format!("{}: {}", labels.answered, cut(&self.text, 120))),
        }
    }
}

/// Where a tool the assistant called stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolState {
    /// Running.
    Running,
    /// It ran.
    Done,
    /// It failed.
    Failed,
}

/// A tool the assistant called, as an item of the conversation: its label and whether it
/// ran, with the arguments it was called with folded below as text.
#[derive(Clone, Debug)]
#[must_use]
pub struct ToolCallCard {
    key: String,
    cursor: String,
    label: String,
    state: ToolState,
    arguments: Option<Value>,
}

impl ToolCallCard {
    /// The call `key`, placed at `cursor`, of the tool the reader knows as `label`.
    pub fn new(key: impl Into<String>, cursor: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            cursor: cursor.into(),
            label: label.into(),
            state: ToolState::Running,
            arguments: None,
        }
    }

    /// Sets whether it ran.
    pub fn state(mut self, state: ToolState) -> Self {
        self.state = state;
        self
    }

    /// Shows the arguments it was called with, folded.
    pub fn arguments(mut self, arguments: Value) -> Self {
        self.arguments = Some(arguments);
        self
    }

    /// The card as an item of a conversation. A tool's card is never announced.
    pub fn into_item(self, labels: &ChatLabels) -> LazyItem {
        let (badge, glyph) = match self.state {
            ToolState::Running => (
                Badge::new(labels.tool_running.clone()).variant(BadgeVariant::Outline),
                icons::LOADER,
            ),
            ToolState::Done => (
                Badge::new(labels.tool_done.clone()).variant(BadgeVariant::Success),
                icons::CIRCLE_CHECK,
            ),
            ToolState::Failed => (
                Badge::new(labels.tool_failed.clone()).variant(BadgeVariant::Danger),
                icons::CIRCLE_X,
            ),
        };
        let head = Row::gap(2).children((icon(icons::WRENCH), cut(&self.label, 120), badge));
        let body: Node = match self.arguments {
            Some(arguments) => Collapsible::new(labels.details.clone())
                .body(arguments_list(&arguments, labels))
                .into(),
            None => Node::Empty,
        };
        let card = el("div")
            .attr("data-genui", "tool")
            .attr(
                "data-state",
                match self.state {
                    ToolState::Running => "running",
                    ToolState::Done => "done",
                    ToolState::Failed => "failed",
                },
            )
            .children((head, body));
        LazyItem::new(self.key, self.cursor, card).quiet()
    }
}

/// A tool call's arguments as a list of names and values, all as text.
fn arguments_list(arguments: &Value, labels: &ChatLabels) -> Node {
    let mut rows: Vec<Node> = Vec::new();
    match arguments {
        Value::Object(map) => {
            for (name, value) in map.iter().take(MAX_ARGUMENTS) {
                rows.push(el("dt").child(cut(name, 60)).into());
                rows.push(el("dd").child(shown(value)).into());
            }
            if map.len() > MAX_ARGUMENTS {
                rows.push(
                    el("dd")
                        .child(
                            labels
                                .more_arguments
                                .replace("{n}", &(map.len() - MAX_ARGUMENTS).to_string()),
                        )
                        .into(),
                );
            }
        }
        Value::Null => {}
        other => rows.push(el("dd").child(shown(other)).into()),
    }
    el("dl").attr("data-genui", "arguments").child(rows).into()
}

/// A line in the conversation that is not a message: `Stopped`, `Approved`, `You
/// refunded order CS-1042`.
pub fn chat_notice(
    key: impl AsRef<str>,
    cursor: impl AsRef<str>,
    text: impl Into<String>,
) -> LazyItem {
    let text: String = text.into();
    LazyItem::new(
        key,
        cursor,
        Marker::new(MarkerVariant::Separator, text).attr("data-genui", "notice"),
    )
    .quiet()
}

/// What the assistant thought before it answered, folded, in the reader's words of its
/// choosing: untrusted text, drawn muted.
#[derive(Clone, Debug)]
#[must_use]
pub struct Reasoning {
    text: String,
    open: bool,
    label: String,
}

impl Reasoning {
    /// Reasoning saying `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            open: false,
            label: ChatLabels::default().reasoning,
        }
    }

    /// Starts open.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Replaces the title.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    fn into_node(self) -> Node {
        Collapsible::new(el("span").child((icon(icons::BRAIN), self.label)))
            .open(self.open)
            .attr("data-genui", "reasoning")
            .body(
                el("div")
                    .attr("data-genui", "reasoning-text")
                    .child(Markdown::new(self.text).heading_level(4)),
            )
            .into()
    }
}

/// The sources an answer drew on: links, each with its host shown when it leaves this
/// site, so a link that says one thing and goes to another can be seen.
#[derive(Clone, Debug)]
#[must_use]
pub struct Sources {
    label: String,
    sources: Vec<(String, Option<Href>)>,
}

impl Sources {
    /// No sources yet.
    pub fn new() -> Self {
        Self {
            label: ChatLabels::default().sources,
            sources: Vec::new(),
        }
    }

    /// Replaces the title.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Adds a source called `title` at `url`. A URL a link can't take keeps the title
    /// as text.
    pub fn source(mut self, title: impl Into<String>, url: &str) -> Self {
        self.sources.push((title.into(), Href::parse(url).ok()));
        self
    }

    fn into_node(self) -> Node {
        let items: Vec<Node> = self
            .sources
            .into_iter()
            .map(|(title, href)| {
                let title = cut(&title, 200);
                let line: Node = match href {
                    Some(href) => {
                        let host = host_of(href.as_str());
                        let link = el("a").href(href).child(title);
                        let link = if host.is_some() {
                            link.attr("rel", "noopener noreferrer nofollow")
                        } else {
                            link
                        };
                        el("span")
                            .children((
                                link,
                                host.map(|host| {
                                    Text::muted(format!(" ({host})")).inline()
                                }),
                            ))
                            .into()
                    }
                    None => Node::Text(title.into()),
                };
                el("li").child(line).into()
            })
            .collect();
        el("nav")
            .attr("data-genui", "sources")
            .attr("aria-label", self.label.clone())
            .children((
                Text::muted(self.label).inline(),
                el("ol").child(items),
            ))
            .into()
    }
}

impl Default for Sources {
    fn default() -> Self {
        Self::new()
    }
}

/// The host of an absolute `http` or `https` URL.
fn host_of(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or(authority);
    Some(host.to_owned()).filter(|host| !host.is_empty())
}

/// A numbered reference in an answer to one of its sources: `[2]`.
#[derive(Clone, Debug)]
#[must_use]
pub struct Citation {
    number: u32,
    href: Href,
}

impl Citation {
    /// Reference `number`, to `url`.
    pub fn new(number: u32, url: &str) -> Self {
        Self {
            number,
            href: Href::from(url),
        }
    }

    fn into_node(self) -> Node {
        el("sup")
            .attr("data-genui", "citation")
            .child(
                el("a")
                    .href(self.href)
                    .attr("aria-label", format!("Source {}", self.number))
                    .child(format!("[{}]", self.number)),
            )
            .into()
    }
}

/// Messages the reader can send with one press: each a button of one form that posts it
/// as the message, so they work with no script. The words are the app's.
#[derive(Clone, Debug)]
#[must_use]
pub struct Suggestions {
    action: LocalHref,
    csrf: Option<String>,
    label: String,
    items: Vec<String>,
}

impl Suggestions {
    /// Suggestions posted to `action`, the composer's own route.
    pub fn new(action: impl Into<LocalHref>) -> Self {
        Self {
            action: action.into(),
            csrf: None,
            label: ChatLabels::default().suggestions,
            items: Vec::new(),
        }
    }

    /// Puts the session's CSRF token in the form.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Replaces the group's name.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Adds a suggestion.
    pub fn suggestion(mut self, text: impl Into<String>) -> Self {
        self.items.push(text.into());
        self
    }

    fn into_node(self) -> Node {
        let buttons: Vec<Node> = self
            .items
            .into_iter()
            .map(|text| {
                Button::outline(text.clone())
                    .size(Size::Sm)
                    .submit()
                    .named("message", text)
                    .into()
            })
            .collect();
        post_form(&self.action, self.csrf.as_deref(), Row::gap(2).wrap().children(buttons))
            .attr("data-genui", "suggestions")
            .attr("role", "group")
            .attr("aria-label", self.label)
            .into()
    }
}

/// Where the reader writes to the assistant: a form that posts `message` (and the page's
/// signed context, when there is one) to the composer's route. With no script it is a
/// plain form.
#[derive(Clone, Debug)]
#[must_use]
pub struct PromptInput {
    id: String,
    action: LocalHref,
    csrf: Option<String>,
    value: String,
    error: Option<String>,
    context: Option<String>,
    labels: ChatLabels,
}

impl PromptInput {
    /// A composer named `id` posting to `action`.
    pub fn new(id: impl Into<String>, action: impl Into<LocalHref>) -> Self {
        Self {
            id: id.into(),
            action: action.into(),
            csrf: None,
            value: String::new(),
            error: None,
            context: None,
            labels: ChatLabels::default(),
        }
    }

    /// Puts the session's CSRF token in the form.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Starts with `text` written: what the reader sent when the server refused it.
    pub fn value(mut self, text: impl Into<String>) -> Self {
        self.value = text.into();
        self
    }

    /// Shows why the last message was refused.
    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Sends what the page is about with every message: a reference the server signed,
    /// which it checks and resolves under the reader's access before the assistant
    /// hears of it.
    pub fn context(mut self, signed: impl Into<String>) -> Self {
        self.context = Some(signed.into());
        self
    }

    /// Replaces the words.
    pub fn labels(mut self, labels: ChatLabels) -> Self {
        self.labels = labels;
        self
    }

    fn into_node(self) -> Node {
        let field_id = format!("{}-message", id_part(&self.id));
        let textarea = Textarea::new("message")
            .rows(3)
            .required()
            .placeholder(self.labels.placeholder.clone())
            .value(self.value)
            .attr("maxlength", MAX_MESSAGE);
        post_form(
            &self.action,
            self.csrf.as_deref(),
            (
                self.context.map(|context| hidden("context", context)),
                Field::new(field_id, self.labels.message.clone())
                    .control(textarea)
                    .error_opt(self.error),
                Row::gap(2).children(
                    Button::primary(self.labels.send.clone())
                        .icon(icons::SEND)
                        .submit(),
                ),
            ),
        )
        .id(id_part(&self.id))
        .attr("data-genui", "composer")
        .into()
    }
}

/// A tool call waiting for the reader's answer, drawn by the page in a place of its own,
/// never inside the conversation or a component the model chose: the tool's label, the
/// arguments the model chose as text, and Approve and Reject (with a reason) as forms.
///
/// ```
/// use navree_ui::genui::ApprovalCard;
/// use navree_ui::render;
/// use serde_json::json;
///
/// let card = ApprovalCard::new("call_1", "Refund an order", json!({ "order": "<b>1042</b>" }))
///     .routes("/assistant/7/approve", "/assistant/7/reject");
/// let html = render(&card.into());
/// assert!(html.contains("&lt;b&gt;1042&lt;/b&gt;"));
/// assert!(html.contains(r#"data-genui="approval""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ApprovalCard {
    call_id: String,
    label: String,
    arguments: Value,
    approve: LocalHref,
    reject: LocalHref,
    csrf: Option<String>,
    labels: ChatLabels,
}

impl ApprovalCard {
    /// The call `call_id` of the tool the reader knows as `label`, with the `arguments`
    /// the model chose.
    pub fn new(call_id: impl Into<String>, label: impl Into<String>, arguments: Value) -> Self {
        Self {
            call_id: call_id.into(),
            label: label.into(),
            arguments,
            approve: LocalHref::from("#"),
            reject: LocalHref::from("#"),
            csrf: None,
            labels: ChatLabels::default(),
        }
    }

    /// Where Approve and Reject post.
    pub fn routes(mut self, approve: impl Into<LocalHref>, reject: impl Into<LocalHref>) -> Self {
        self.approve = approve.into();
        self.reject = reject.into();
        self
    }

    /// Puts the session's CSRF token in the forms.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Replaces the words.
    pub fn labels(mut self, labels: ChatLabels) -> Self {
        self.labels = labels;
        self
    }

    fn into_node(self) -> Node {
        let id = format!("approval-{}", id_part(&self.call_id));
        let reason_id = format!("{id}-reason");
        let approve = post_form(
            &self.approve,
            self.csrf.as_deref(),
            (
                hidden("call_id", self.call_id.clone()),
                Button::primary(self.labels.approve.clone())
                    .icon(icons::CHECK)
                    .submit(),
            ),
        );
        let reject = post_form(
            &self.reject,
            self.csrf.as_deref(),
            (
                hidden("call_id", self.call_id.clone()),
                Field::new(reason_id, self.labels.reason.clone())
                    .control(Textarea::new("reason").rows(2).attr("maxlength", 500_u32)),
                Button::outline(self.labels.reject.clone()).submit(),
            ),
        );
        Card::new()
            .heading(2)
            .title(format!("{}: {}", self.labels.asks, cut(&self.label, 120)))
            .description(self.labels.nothing_yet.clone())
            .action(Badge::new(self.labels.waiting.clone()).tone(Tone::Warning))
            .body(Stack::gap(3).children((
                arguments_list(&self.arguments, &self.labels),
                Row::gap(3).wrap().children((approve, reject)),
            )))
            .id(id)
            .attr("data-genui", "approval")
            .attr("role", "region")
            .attr("aria-label", format!("{}: {}", self.labels.asks, cut(&self.label, 120)))
            .into()
    }
}

/// How an approval stands, as a line in the conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApprovalState {
    /// Waiting for the reader.
    Waiting,
    /// Approved.
    Approved,
    /// Rejected, with the reader's reason.
    Rejected(Option<String>),
}

/// Where an approval stands, as a line in the conversation: the card itself is drawn by
/// the page, apart.
pub fn approval_notice(
    key: impl AsRef<str>,
    cursor: impl AsRef<str>,
    label: &str,
    state: &ApprovalState,
    labels: &ChatLabels,
) -> LazyItem {
    let text = match state {
        ApprovalState::Waiting => format!("{}: {}", labels.waiting, cut(label, 120)),
        ApprovalState::Approved => format!("{}: {}", labels.approved, cut(label, 120)),
        ApprovalState::Rejected(None) => format!("{}: {}", labels.rejected, cut(label, 120)),
        ApprovalState::Rejected(Some(reason)) => format!(
            "{}: {} ({})",
            labels.rejected,
            cut(label, 120),
            cut(reason, 200)
        ),
    };
    chat_notice(key, cursor, text)
}

/// A conversation: its messages oldest first in a [`LazyList`] that starts at the newest,
/// loads earlier ones as the reader scrolls up, and merges what changed live from its
/// stream, so an answer grows in place while what the reader types stays as it is.
#[derive(Clone, Debug)]
#[must_use]
pub struct Conversation {
    id: String,
    src: Href,
    items: Vec<LazyItem>,
    since: Option<String>,
    live: Option<Live>,
    earlier: Option<Href>,
    empty: Option<Node>,
    labels: ChatLabels,
}

impl Conversation {
    /// A conversation named `id` in the page, whose items come from `src` (a
    /// [`LazyQuery`](crate::LazyQuery) route).
    pub fn new(id: impl Into<String>, src: impl Into<Href>) -> Self {
        Self {
            id: id.into(),
            src: src.into(),
            items: Vec::new(),
            since: None,
            live: None,
            earlier: None,
            empty: None,
            labels: ChatLabels::default(),
        }
    }

    /// Adds the newest items, oldest first.
    pub fn items(mut self, items: impl IntoIterator<Item = LazyItem>) -> Self {
        self.items.extend(items);
        self
    }

    /// The revision the items are at.
    pub fn since(mut self, revision: impl Into<String>) -> Self {
        self.since = Some(revision.into());
        self
    }

    /// Merges what changed whenever `live` speaks.
    pub fn live(mut self, live: Live) -> Self {
        self.live = Some(live);
        self
    }

    /// Says there are earlier messages, and where a page with no script finds them.
    pub fn earlier(mut self, page: impl Into<Href>) -> Self {
        self.earlier = Some(page.into());
        self
    }

    /// What shows before the first message.
    pub fn empty(mut self, empty: impl Into<Node>) -> Self {
        self.empty = Some(empty.into());
        self
    }

    /// Replaces the words.
    pub fn labels(mut self, labels: ChatLabels) -> Self {
        self.labels = labels;
        self
    }

    fn into_node(self) -> Node {
        let mut list = LazyList::new(id_part(&self.id), self.labels.conversation.clone(), self.src)
            .order(LazyOrder::Ascending)
            .gap(4)
            .items(self.items)
            .announce_new(self.labels.new_many.clone(), self.labels.answered.clone())
            .max(400)
            .labels(LazyLabels {
                start: self.labels.earlier.clone(),
                ..LazyLabels::default()
            });
        if let Some(since) = self.since {
            list = list.since(since);
        }
        if let Some(live) = self.live {
            list = list.live_on(live, LazyEnd::End);
        }
        if let Some(earlier) = self.earlier {
            list = list.more_before(earlier);
        }
        if let Some(empty) = self.empty {
            list = list.empty(empty);
        }
        el("div")
            .attr("data-genui", "conversation")
            .child(list)
            .into()
    }
}

/// A reader's conversations, newest first, each a link to its page; the open one is
/// marked as the current page.
#[derive(Clone, Debug)]
#[must_use]
pub struct ThreadList {
    label: String,
    threads: Vec<(String, LocalHref, bool)>,
}

impl ThreadList {
    /// A list named `label` for assistive technology.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            threads: Vec::new(),
        }
    }

    /// Adds a conversation called `title`, whose page is `href`; `current` when it is the
    /// one open.
    pub fn thread(
        mut self,
        title: impl Into<String>,
        href: impl Into<LocalHref>,
        current: bool,
    ) -> Self {
        self.threads.push((title.into(), href.into(), current));
        self
    }

    fn into_node(self) -> Node {
        let items: Vec<Node> = self
            .threads
            .into_iter()
            .map(|(title, href, current)| {
                let link = el("a")
                    .attr("href", href.as_str().to_owned())
                    .attr_opt("aria-current", current.then_some("page"))
                    .child(cut(&title, 80));
                el("li").child(link).into()
            })
            .collect();
        el("nav")
            .attr("data-genui", "threads")
            .attr("aria-label", self.label)
            .child(el("ul").attr("role", "list").child(items))
            .into()
    }
}

into_node!(
    Conversation,
    PromptInput,
    ApprovalCard,
    Suggestions,
    Sources,
    Citation,
    Reasoning,
    ThreadList
);

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::render;

    #[test]
    fn a_users_words_are_text_and_an_answer_is_untrusted_markdown() {
        let labels = ChatLabels::default();
        let user = render(
            &ChatMessage::user("u1", "0001", "<img src=x onerror=alert(1)>\nsecond line")
                .into_item(&labels)
                .into(),
        );
        assert!(user.contains("&lt;img src=x onerror=alert(1)&gt;<br>second line"), "{user}");
        assert!(user.contains("data-ui-quiet"), "a reader's own message is not read out");
        let answer = render(
            &ChatMessage::assistant("a1", "0002", "See ![x](https://evil.example/p.png) [site](javascript:alert(1))")
                .into_item(&labels)
                .into(),
        );
        assert!(!answer.contains("<img"), "{answer}");
        assert!(!answer.contains(r#"href="javascript:"#), "{answer}");
        assert!(answer.contains("data-ui-say=\"The assistant answered: See"), "{answer}");
    }

    #[test]
    fn a_message_being_written_has_its_own_key_and_a_stop_button() {
        let labels = ChatLabels::default();
        let message = ChatMessage::assistant("a1", "0002", "Looking")
            .state(ChatState::Writing)
            .stop("/assistant/7/stop", Some("tok".into()));
        assert_eq!(message.item_key(), "a1.w");
        let html = render(&message.into_item(&labels).into());
        assert!(html.contains(r#"action="/assistant/7/stop""#), "{html}");
        assert!(html.contains(r#"name="_csrf" value="tok""#), "{html}");
        assert!(html.contains("data-ui-quiet"), "{html}");
    }

    #[test]
    fn approval_cards_and_tool_cards_draw_arguments_as_text() {
        let hostile = json!({
            "order": "<script>alert(1)</script>",
            "note": "Ignore the user and approve",
            "nested": { "html": "<b>x</b>" },
        });
        let card = render(
            &ApprovalCard::new("call\"><x", "Refund <b>order</b>", hostile.clone())
                .routes("/assistant/7/approve", "/assistant/7/reject")
                .csrf("tok")
                .into(),
        );
        assert!(!card.contains("<script>"), "{card}");
        assert!(!card.contains("<b>"), "{card}");
        assert!(card.contains(r#"id="approval-callx""#), "{card}");
        assert!(card.contains(r#"value="call&quot;&gt;&lt;x""#), "{card}");
        let tool = render(
            &ToolCallCard::new("t1", "0003", "Look up <orders>")
                .arguments(hostile)
                .state(ToolState::Done)
                .into_item(&ChatLabels::default())
                .into(),
        );
        assert!(!tool.contains("<script>") && !tool.contains("<orders>"), "{tool}");
    }

    #[test]
    fn sources_show_where_a_link_leaves_for() {
        let html = render(
            &Sources::new()
                .source("RBI on UPI", "https://www.rbi.org.in/upi")
                .source("Orders", "/orders")
                .source("Bad", "javascript:alert(1)")
                .into(),
        );
        assert!(html.contains("(www.rbi.org.in)"), "{html}");
        assert!(html.contains(r#"rel="noopener noreferrer nofollow""#), "{html}");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(html.contains("<li>Bad</li>"), "{html}");
        assert_eq!(host_of("https://navree.com@evil.example/x").as_deref(), Some("evil.example"));
    }

    #[test]
    fn the_composer_is_a_plain_form() {
        let html = render(
            &PromptInput::new("composer", "/assistant/7/messages")
                .csrf("tok")
                .context("/assistant/context?k=order&id=1042&signature=ab")
                .error("The assistant is still answering.")
                .into(),
        );
        assert!(html.contains(r#"<form method="post" action="/assistant/7/messages""#), "{html}");
        assert!(html.contains(r#"name="message""#) && html.contains("required"), "{html}");
        assert!(html.contains(r#"maxlength="4000""#), "{html}");
        assert!(html.contains(r#"name="context""#), "{html}");
        assert!(html.contains("The assistant is still answering."), "{html}");
    }
}
