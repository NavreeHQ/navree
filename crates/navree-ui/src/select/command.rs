//! [`Command`]: a command palette in a modal dialog.

use super::labels::SelectLabels;
use crate::cmd::Commands;
use crate::components::{Button, ButtonVariant, Kbd, icon};
use crate::form::csrf_field;
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{Element, Node, Uses, el, into_node};

/// What choosing an item does.
#[derive(Clone, Debug)]
enum Action {
    /// Goes to a page.
    Link(Href),
    /// Posts a form to an action, with no script needed.
    Post(Href),
    /// Runs browser commands.
    Run(Commands),
    /// Does nothing, and says why.
    Disabled(String),
}

/// One item of a [`Command`]: a page to go to, an action to post, or a browser command.
///
/// ```
/// use navree_ui::{CommandItem, Cmd, icons};
///
/// let go = CommandItem::link("Deals", "/deals").icon(icons::SEARCH).shortcut("mod+d");
/// let post = CommandItem::post("Sign out", "/sign-out");
/// let run = CommandItem::run("Open filters", Cmd::open("filters"));
/// let no = CommandItem::disabled("Billing", "Only owners can open billing.");
/// # let _ = (go, post, run, no);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct CommandItem {
    action: Action,
    label: String,
    id: Option<String>,
    icon: Option<Icon>,
    description: Option<String>,
    keywords: Vec<String>,
    shortcut: Option<String>,
    recent: bool,
}

impl CommandItem {
    fn new(action: Action, label: impl Into<String>) -> Self {
        Self {
            action,
            label: label.into(),
            id: None,
            icon: None,
            description: None,
            keywords: Vec::new(),
            shortcut: None,
            recent: true,
        }
    }

    /// Goes to `href`.
    pub fn link(label: impl Into<String>, href: impl Into<Href>) -> Self {
        Self::new(Action::Link(href.into()), label)
    }

    /// Posts to `action`. Without script it is an ordinary form post.
    pub fn post(label: impl Into<String>, action: impl Into<Href>) -> Self {
        Self::new(Action::Post(action.into()), label)
    }

    /// Runs `command` in the browser: opens a dialog, copies text.
    pub fn run(label: impl Into<String>, command: impl Into<Commands>) -> Self {
        Self::new(Action::Run(command.into()), label)
    }

    /// Shows the item but refuses it, and says why in its line.
    pub fn disabled(label: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(Action::Disabled(reason.into()), label)
    }

    /// Names the item for the recent list; its link, action or label by default.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// An icon before the label.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A line under the label.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Words that find this item though the label doesn't hold them.
    pub fn keywords<S: Into<String>>(mut self, words: impl IntoIterator<Item = S>) -> Self {
        self.keywords = words.into_iter().map(Into::into).collect();
        self
    }

    /// A key chord that runs this item from anywhere on the page, such as `mod+shift+n`
    /// (`mod` is `Cmd` on Apple devices and `Ctrl` elsewhere). It is shown on the item.
    /// A chord needs `ctrl`, `alt`, `meta` or `mod`, and one key: a bare letter would
    /// fire while someone types, so it is refused and nothing is shown.
    pub fn shortcut(mut self, chord: &str) -> Self {
        self.shortcut = Chord::parse(chord).map(|chord| chord.0);
        self
    }

    /// Keeps this item out of the recent list (a sign-out, a delete).
    pub fn not_recent(mut self) -> Self {
        self.recent = false;
        self
    }

    fn value(&self) -> String {
        if let Some(id) = &self.id {
            return id.clone();
        }
        match &self.action {
            Action::Link(href) | Action::Post(href) => href.as_str().to_owned(),
            Action::Run(_) | Action::Disabled(_) => self.label.clone(),
        }
    }

    fn into_element(self, form: &str) -> Element {
        let value = self.value();
        let disabled = matches!(self.action, Action::Disabled(_));
        let mut row = match &self.action {
            Action::Link(href) => el("a").attr("role", "option").attr("href", href.clone()),
            Action::Post(action) => el("button")
                .attr("role", "option")
                .attr("type", "submit")
                .attr("form", form.to_owned())
                .attr("formaction", action.clone())
                .attr("formmethod", "post"),
            Action::Run(commands) => el("button")
                .attr("role", "option")
                .attr("type", "button")
                .on_click(commands.clone()),
            Action::Disabled(_) => el("div").attr("role", "option"),
        };
        row = row
            .slot("command-item")
            .attr("data-value", value)
            .attr_opt(
                "data-keywords",
                (!self.keywords.is_empty()).then(|| self.keywords.join(" ")),
            )
            .attr_opt("data-shortcut", self.shortcut.clone())
            .attr_opt("aria-disabled", disabled.then_some("true"))
            .flag("data-no-recent", !self.recent);
        let reason = match self.action {
            Action::Disabled(reason) => Some(reason),
            _ => None,
        };
        row.children((
            self.icon.map(icon),
            el("span").slot("option-body").children((
                el("span")
                    .slot("option-text")
                    .attr("dir", "auto")
                    .child(self.label),
                self.description.map(|text| {
                    el("span")
                        .slot("option-description")
                        .attr("dir", "auto")
                        .child(text)
                }),
                reason.map(|text| {
                    el("span")
                        .slot("command-reason")
                        .attr("dir", "auto")
                        .child(text)
                }),
            )),
            self.shortcut.as_deref().map(|chord| {
                el("kbd")
                    .slot("command-shortcut")
                    .attr("aria-hidden", "true")
                    .child(Chord(chord.to_owned()).display())
            }),
        ))
    }
}

/// A validated key chord, lower case, `+` between parts, the key last.
struct Chord(String);

impl Chord {
    fn parse(text: &str) -> Option<Self> {
        let parts: Vec<String> = text
            .split('+')
            .map(|part| part.trim().to_ascii_lowercase())
            .collect();
        let (key, modifiers) = parts.split_last()?;
        if modifiers.is_empty() || modifiers.len() > 3 {
            return None;
        }
        let strong = modifiers
            .iter()
            .any(|m| matches!(m.as_str(), "ctrl" | "alt" | "meta" | "mod"));
        let known = modifiers
            .iter()
            .all(|m| matches!(m.as_str(), "ctrl" | "alt" | "meta" | "mod" | "shift"));
        let key_ok = (key.chars().count() == 1 && key.chars().all(|c| c.is_ascii_graphic()))
            || (key.len() >= 2
                && key.starts_with('f')
                && key[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)));
        (strong && known && key_ok && key != "+").then(|| Self(parts.join("+")))
    }

    /// The chord as a person reads it on a keyboard that isn't Apple's.
    fn display(&self) -> Node {
        let words: Vec<String> = self
            .0
            .split('+')
            .map(|part| match part {
                "mod" | "ctrl" => "Ctrl".to_owned(),
                "alt" => "Alt".to_owned(),
                "meta" => "Meta".to_owned(),
                "shift" => "Shift".to_owned(),
                other => other.to_ascii_uppercase(),
            })
            .collect();
        Node::text(words.join("+"))
    }
}

/// A titled group of items.
#[derive(Clone, Debug)]
#[must_use]
pub struct CommandGroup {
    label: Option<String>,
    items: Vec<CommandItem>,
}

impl CommandGroup {
    /// A group headed `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            items: Vec::new(),
        }
    }

    /// Adds an item.
    pub fn item(mut self, item: CommandItem) -> Self {
        self.items.push(item);
        self
    }

    /// Adds items.
    pub fn items(mut self, items: impl IntoIterator<Item = CommandItem>) -> Self {
        self.items.extend(items);
        self
    }
}

/// A command palette: a modal dialog with a search box and a list of groups of
/// items, opened by `Ctrl` or `Cmd` and `K`.
///
/// Typing ranks the items (label start, word start, then anywhere; keywords last).
/// `Enter` runs the active item, `Esc` closes the palette. The page builds only
/// the items this person may use; every action's route still checks.
///
/// ```
/// use navree_ui::{Command, CommandGroup, CommandItem, render};
///
/// let palette = Command::new("palette")
///     .group(CommandGroup::new("Go to").item(CommandItem::link("Deals", "/deals")))
///     .recent(5);
/// let html = render(&palette.into());
/// assert!(html.contains("<dialog"));
/// assert!(html.contains(r#"data-ui-recent="5""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Command {
    id: String,
    title: String,
    placeholder: String,
    groups: Vec<CommandGroup>,
    recent: Option<u32>,
    source: Option<Href>,
    key: bool,
    labels: SelectLabels,
    csrf: Option<String>,
}

impl Command {
    /// A palette in a dialog with the id `id`.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: SelectLabels::default().palette.to_string(),
            placeholder: "Type a command or search…".to_owned(),
            groups: Vec::new(),
            recent: None,
            source: None,
            key: true,
            labels: SelectLabels::default(),
            csrf: None,
        }
    }

    /// A button that opens the palette with no script, showing the shortcut.
    pub fn trigger(id: impl Into<String>, label: impl Into<Node>) -> Button {
        Button::new(Node::list((
            icon(icons::SEARCH),
            el("span").child(label.into()),
            Kbd::new("Ctrl K"),
        )))
        .variant(ButtonVariant::Outline)
        .opens_dialog(id)
    }

    /// Names the dialog for assistive technology.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the hint in the search box.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Adds a group.
    pub fn group(mut self, group: CommandGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// Adds an item that belongs to no group.
    pub fn item(mut self, item: CommandItem) -> Self {
        self.groups.push(CommandGroup {
            label: None,
            items: vec![item],
        });
        self
    }

    /// Shows the last `n` items chosen (kept in this browser) while the box is empty.
    pub fn recent(mut self, n: u32) -> Self {
        self.recent = Some(n.min(20));
        self
    }

    /// Adds results from the server: the runtime asks `url` with `?q=text` and shows
    /// the answer, an [`ChoiceList`](super::ChoiceList) whose groups hold links, below
    /// the items in the page.
    pub fn source(mut self, url: impl Into<Href>) -> Self {
        self.source = Some(url.into());
        self
    }

    /// Whether `Ctrl` or `Cmd` and `K` open it. On by default.
    pub fn shortcut(mut self, on: bool) -> Self {
        self.key = on;
        self
    }

    /// Sets the words, for another language.
    pub fn labels(mut self, labels: &SelectLabels) -> Self {
        self.labels = labels.clone();
        self
    }

    /// Sets the token the posting items send when the page runs with no script.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    fn into_node(self) -> Node {
        let id = self.id;
        let form = format!("{id}-post");
        let list_id = format!("{id}-list");
        let groups: Vec<Node> = self
            .groups
            .into_iter()
            .map(|group| {
                let rows: Vec<Element> = group
                    .items
                    .into_iter()
                    .map(|item| item.into_element(&form))
                    .collect();
                match group.label {
                    Some(label) => el("div")
                        .attr("role", "group")
                        .attr("aria-label", label.clone())
                        .children((
                            el("div")
                                .slot("option-group-label")
                                .attr("aria-hidden", "true")
                                .child(label),
                            rows,
                        ))
                        .into(),
                    None => Node::list(rows),
                }
            })
            .collect();
        el("dialog")
            .slot("command-dialog")
            .attr("id", id.clone())
            .attr("aria-label", self.title)
            .attr("closedby", "any")
            .flag("data-ui-palette", true)
            .attr_opt("data-ui-recent", self.recent)
            .attr_opt("data-ui-search", self.source)
            .flag("data-ui-key", self.key)
            .attr_opt("data-l", self.labels.changed())
            .uses(Uses::OVERLAY.with(Uses::SELECT))
            .children((
                el("div").slot("command").children((
                    el("div").slot("command-input-wrap").children((
                        icon(icons::SEARCH),
                        el("input")
                            .slot("command-input")
                            .attr("type", "text")
                            .attr("role", "combobox")
                            .attr("aria-autocomplete", "list")
                            .attr("aria-haspopup", "listbox")
                            .attr("aria-expanded", "true")
                            .attr("aria-controls", list_id.clone())
                            .attr("aria-label", self.placeholder.clone())
                            .attr("autocomplete", "off")
                            .attr("autocapitalize", "none")
                            .attr("spellcheck", "false")
                            .attr("dir", "auto")
                            .attr("placeholder", self.placeholder),
                    )),
                    el("div")
                        .slot("command-list")
                        .attr("id", list_id)
                        .attr("role", "listbox")
                        .attr("aria-label", self.labels.list.to_string())
                        .children((
                            el("div")
                                .slot("command-recent")
                                .attr("role", "group")
                                .flag("hidden", true),
                            groups,
                            el("div").slot("command-remote"),
                        )),
                    el("p")
                        .slot("select-note")
                        .attr("aria-hidden", "true")
                        .flag("hidden", true),
                )),
                el("form")
                    .attr("id", form)
                    .attr("method", "post")
                    .flag("hidden", true)
                    .child(self.csrf.map(csrf_field)),
                el("div")
                    .slot("select-status")
                    .attr("role", "status")
                    .attr("aria-live", "polite")
                    .attr("aria-atomic", "true"),
            ))
            .into()
    }
}

into_node!(Command);
