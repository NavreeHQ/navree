//! Tree: a tree of links or choices, with children loaded from the server.
//!
//! The server writes plain nested lists, links and inputs, so with no script a tree
//! is a list a person can read, follow and post. The `tree` module adds what makes
//! it WAI-ARIA's tree view when it runs: the roles, one tab stop, every key of the
//! pattern, type-ahead, opening and closing, tri-state parents, and children
//! fetched as a branch opens.
//!
//! - **Open state** can live in the address ([`TreeState`]): a branch's caret is a
//!   link that adds it to the open set, so with no script a person opens a branch
//!   by following it, and the app draws that branch's children.
//! - **Children** come with the node ([`TreeNode::branch`]) or from the server
//!   when the branch opens ([`TreeNode::lazy`], answered with [`TreeChildren`]).
//! - **Long lists** come in pages: after the children a "Show more" row
//!   ([`TreeNode::more`]) is a link to the next page, and the module fetches it in
//!   place.
//! - **Choices** are native inputs: radios for one, checkboxes for many, each
//!   posting its node's id as the tree's name. [`TreeSelection`] reads them back,
//!   with limits.

use std::borrow::Cow;
use std::collections::BTreeSet;

use crate::components::icon;
use crate::form::{encode, encode_into};
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{Element, Node, Uses, el, into_node};

/// The most node ids a post or an address may carry.
pub const MAX_IDS: usize = 1000;
/// The longest node id, in bytes.
pub const MAX_ID_BYTES: usize = 256;
/// The most open branches an address keeps.
pub const MAX_OPEN: usize = 200;

/// What choosing in a tree does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TreeSelect {
    /// Nothing is chosen: a tree of links, or of things to open.
    #[default]
    None,
    /// One node, on a radio: it posts as the tree's name.
    Single,
    /// Any nodes, on checkboxes: each posts as the tree's name.
    Multiple,
}

/// The words a tree writes. English by default; `{label}` and `{n}` are filled in.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct TreeLabels {
    pub expand: Cow<'static, str>,
    pub collapse: Cow<'static, str>,
    pub more: Cow<'static, str>,
    pub loading: Cow<'static, str>,
    pub failed: Cow<'static, str>,
    pub retry: Cow<'static, str>,
    pub loaded: Cow<'static, str>,
}

impl Default for TreeLabels {
    fn default() -> Self {
        Self {
            expand: "Expand {label}".into(),
            collapse: "Collapse {label}".into(),
            more: "Show more".into(),
            loading: "Loading".into(),
            failed: "Couldn't load this.".into(),
            retry: "Try again".into(),
            loaded: "{n} items loaded".into(),
        }
    }
}

/// A node's children.
#[derive(Clone, Debug)]
enum Kids {
    None,
    Loaded(Vec<TreeNode>),
    Lazy(Href),
}

/// A row that asks for the next page of a node's children.
#[derive(Clone, Debug)]
struct More {
    href: Href,
    label: Option<String>,
    remaining: Option<usize>,
}

/// One node of a [`Tree`].
#[derive(Clone, Debug)]
#[must_use]
pub struct TreeNode {
    id: String,
    label: Node,
    glyph: Option<Icon>,
    href: Option<Href>,
    kids: Kids,
    open: bool,
    selected: bool,
    current: bool,
    disabled: bool,
    total: Option<usize>,
    more: Option<More>,
}

impl TreeNode {
    fn with(id: impl Into<String>, label: impl Into<Node>, kids: Kids) -> Self {
        let mut id = id.into();
        if id.len() > MAX_ID_BYTES {
            let mut at = MAX_ID_BYTES;
            while !id.is_char_boundary(at) {
                at -= 1;
            }
            id.truncate(at);
        }
        Self {
            id,
            label: label.into(),
            glyph: None,
            href: None,
            kids,
            open: false,
            selected: false,
            current: false,
            disabled: false,
            total: None,
            more: None,
        }
    }

    /// A node with no children. `id` is what it posts and what an address calls it,
    /// and must be unique in the tree; one longer than 256 bytes is cut.
    pub fn leaf(id: impl Into<String>, label: impl Into<Node>) -> Self {
        Self::with(id, label, Kids::None)
    }

    /// A node with its children drawn now.
    pub fn branch(
        id: impl Into<String>,
        label: impl Into<Node>,
        children: impl IntoIterator<Item = TreeNode>,
    ) -> Self {
        Self::with(id, label, Kids::Loaded(children.into_iter().collect()))
    }

    /// A node whose children are fetched from `src` when it opens. The answer is a
    /// [`TreeChildren`].
    pub fn lazy(id: impl Into<String>, label: impl Into<Node>, src: impl Into<Href>) -> Self {
        Self::with(id, label, Kids::Lazy(src.into()))
    }

    /// Draws an icon before the label.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }

    /// Makes the label a link. (In a tree that chooses, the label is the choice.)
    pub fn link(mut self, href: impl Into<Href>) -> Self {
        self.href = Some(href.into());
        self
    }

    /// Opens the branch.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Chooses the node.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Marks the link as the page the person is on.
    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }

    /// Disables the choice.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// How many children the node has in all, when only some are drawn: the
    /// browser's accessibility tree is told, so a long list reads as long.
    pub fn children_total(mut self, total: usize) -> Self {
        self.total = Some(total);
        self
    }

    /// Ends the drawn children with a "Show more" row that links to `href`, the
    /// next page of them. `remaining`, when known, is written in it.
    pub fn more(mut self, href: impl Into<Href>, remaining: Option<usize>) -> Self {
        self.more = Some(More {
            href: href.into(),
            label: None,
            remaining,
        });
        self
    }
}

/// Which branches are open, kept in the address as repeated `open` fields. The
/// caret of each branch is a link that toggles it there, so a tree opens with no
/// script, and the app draws the children of the branches that are open.
///
/// ```
/// use navree_ui::flow::TreeState;
///
/// let state = TreeState::from_query("/files", "q=x&open=docs&open=docs%2Fa");
/// assert!(state.is_open("docs/a"));
/// assert_eq!(state.toggled("docs"), "/files?q=x&open=docs%2Fa");
/// assert_eq!(state.toggled("music"), "/files?q=x&open=docs&open=docs%2Fa&open=music");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeState {
    path: String,
    others: Vec<(String, String)>,
    param: String,
    open: BTreeSet<String>,
}

impl TreeState {
    /// Reads the open branches from a query string. Other fields of the query stay
    /// in the links. At most 200 open branches are kept, and ids over 256 bytes
    /// are dropped.
    pub fn from_query(path: impl Into<String>, query: &str) -> Self {
        let path: String = path.into();
        let path = path.split(['?', '#']).next().unwrap_or_default().to_owned();
        let mut state = Self {
            path,
            others: Vec::new(),
            param: "open".to_owned(),
            open: BTreeSet::new(),
        };
        state.read(query);
        state
    }

    /// Names the field the open branches are kept in (`open` by default). Call it
    /// before reading: it re-reads `query`.
    pub fn param(mut self, name: impl Into<String>, query: &str) -> Self {
        self.param = name.into();
        self.others.clear();
        self.open.clear();
        self.read(query);
        self
    }

    fn read(&mut self, query: &str) {
        for (name, value) in navree_http::extract::urlencoded::parse(query.as_bytes()).take(10_000)
        {
            if name == self.param.as_str() {
                if self.open.len() < MAX_OPEN && !value.is_empty() && value.len() <= MAX_ID_BYTES {
                    self.open.insert(value.into_owned());
                }
            } else if self.others.len() < 100 {
                self.others.push((name.into_owned(), value.into_owned()));
            }
        }
    }

    /// Whether the branch `id` is open.
    pub fn is_open(&self, id: &str) -> bool {
        self.open.contains(id)
    }

    /// The open branches.
    pub fn open_ids(&self) -> impl Iterator<Item = &str> {
        self.open.iter().map(String::as_str)
    }

    /// The address with `id` opened, or closed if it was open.
    pub fn toggled(&self, id: &str) -> String {
        let mut open = self.open.clone();
        if !open.remove(id) && open.len() < MAX_OPEN {
            open.insert(id.to_owned());
        }
        let mut query = String::new();
        let others: Vec<(&str, &str)> = self
            .others
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        query.push_str(&encode(&others));
        for each in &open {
            if !query.is_empty() {
                query.push('&');
            }
            encode_into(&mut query, &self.param);
            query.push('=');
            encode_into(&mut query, each);
        }
        if query.is_empty() {
            self.path.clone()
        } else {
            format!("{}?{query}", self.path)
        }
    }
}

/// Why the choices posted by a tree are not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TreeSelectionError {
    /// More than 1,000 choices.
    TooMany,
    /// An id of more than 256 bytes.
    TooLong,
    /// A single-choice tree got two different ones.
    NotOne,
}

impl std::fmt::Display for TreeSelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooMany => "Too many items are chosen.",
            Self::TooLong => "An item is not one the tree has.",
            Self::NotOne => "Choose one item.",
        })
    }
}

impl std::error::Error for TreeSelectionError {}

/// The nodes a tree posted: ids, without duplicates, in the order sent.
///
/// ```
/// use navree_ui::flow::TreeSelection;
///
/// let pairs = vec![("access".to_owned(), "a".to_owned()), ("access".to_owned(), "a".to_owned()), ("other".to_owned(), "z".to_owned())];
/// let chosen = TreeSelection::parse(&pairs, "access").unwrap();
/// assert_eq!(chosen.ids(), ["a"]);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeSelection {
    ids: Vec<String>,
}

impl TreeSelection {
    /// Reads every value posted as `name`. Empty ones are dropped.
    pub fn parse(pairs: &[(String, String)], name: &str) -> Result<Self, TreeSelectionError> {
        let mut ids: Vec<String> = Vec::new();
        for (field, value) in pairs {
            if field != name || value.is_empty() {
                continue;
            }
            if value.len() > MAX_ID_BYTES {
                return Err(TreeSelectionError::TooLong);
            }
            if ids.iter().any(|seen| seen == value) {
                continue;
            }
            if ids.len() >= MAX_IDS {
                return Err(TreeSelectionError::TooMany);
            }
            ids.push(value.clone());
        }
        Ok(Self { ids })
    }

    /// The one node a single-choice tree posted, or none.
    pub fn one(
        pairs: &[(String, String)],
        name: &str,
    ) -> Result<Option<String>, TreeSelectionError> {
        let chosen = Self::parse(pairs, name)?;
        match chosen.ids.as_slice() {
            [] => Ok(None),
            [only] => Ok(Some(only.clone())),
            _ => Err(TreeSelectionError::NotOne),
        }
    }

    /// The ids.
    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    /// Whether `id` was posted.
    pub fn contains(&self, id: &str) -> bool {
        self.ids.iter().any(|each| each == id)
    }

    /// How many were posted.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether none was.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The first id that `known` doesn't know: a post names nodes by what the
    /// browser sends, and the server holds each to the tree it has.
    pub fn first_unknown(&self, known: impl Fn(&str) -> bool) -> Option<&str> {
        self.ids.iter().map(String::as_str).find(|id| !known(id))
    }

    /// The chosen nodes none of whose ancestors is chosen too: what a tree with
    /// cascading choices means, since a chosen branch stands for everything under
    /// it. `parent` gives a node's parent; a chain of more than 1,000 parents is cut.
    pub fn roots(&self, parent: impl Fn(&str) -> Option<String>) -> Vec<&str> {
        self.ids
            .iter()
            .map(String::as_str)
            .filter(|id| {
                let mut at = (*id).to_owned();
                for _ in 0..1000 {
                    match parent(&at) {
                        Some(up) if self.contains(&up) => return false,
                        Some(up) => at = up,
                        None => return true,
                    }
                }
                true
            })
            .collect()
    }
}

/// A tree.
///
/// ```
/// use navree_ui::{Tree, TreeNode, TreeSelect, render};
///
/// let tree = Tree::new("access", "Folders")
///     .select(TreeSelect::Multiple)
///     .node(TreeNode::branch("docs", "Documents", [TreeNode::leaf("docs/a", "Plans")]).open(true))
///     .node(TreeNode::leaf("music", "Music"));
/// let html = render(&tree.into());
/// assert!(html.contains(r#"name="access""#));
/// assert!(html.contains(r#"value="docs/a""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Tree {
    el: Element,
    id: String,
    name: String,
    label: String,
    select: TreeSelect,
    cascade: bool,
    nodes: Vec<TreeNode>,
    more: Option<More>,
    total: Option<usize>,
    state: Option<TreeState>,
    labels: TreeLabels,
}

/// What a node's row needs from the tree around it.
struct Cx<'a> {
    tree: &'a Tree,
}

/// Whether a node is chosen, in whole, in part or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Check {
    Off,
    Mixed,
    On,
}

impl Tree {
    /// A tree whose chosen nodes post as `name`, named `label` for assistive
    /// technology.
    pub fn new(name: impl Into<String>, label: impl Into<String>) -> Self {
        let name = name.into();
        let id = format!("tree-{}", clean(&name));
        Self {
            el: el("div").slot("tree"),
            id,
            name,
            label: label.into(),
            select: TreeSelect::None,
            cascade: false,
            nodes: Vec::new(),
            more: None,
            total: None,
            state: None,
            labels: TreeLabels::default(),
        }
    }

    /// Sets the root's `id`.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// What choosing does.
    pub fn select(mut self, select: TreeSelect) -> Self {
        self.select = select;
        self
    }

    /// Makes a choice in a branch reach everything under it, and a branch half
    /// chosen show as such.
    pub fn cascade(mut self) -> Self {
        self.cascade = true;
        self
    }

    /// Adds a top-level node.
    pub fn node(mut self, node: TreeNode) -> Self {
        self.nodes.push(node);
        self
    }

    /// Adds top-level nodes.
    pub fn nodes(mut self, nodes: impl IntoIterator<Item = TreeNode>) -> Self {
        self.nodes.extend(nodes);
        self
    }

    /// How many top-level nodes there are in all, when only some are drawn.
    pub fn total(mut self, total: usize) -> Self {
        self.total = Some(total);
        self
    }

    /// Ends the top-level list with a "Show more" row.
    pub fn more(mut self, href: impl Into<Href>, remaining: Option<usize>) -> Self {
        self.more = Some(More {
            href: href.into(),
            label: None,
            remaining,
        });
        self
    }

    /// Keeps the open branches in the address, so each caret is a link.
    pub fn state(mut self, state: TreeState) -> Self {
        self.state = Some(state);
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: TreeLabels) -> Self {
        self.labels = labels;
        self
    }

    /// The children of a node, as the tree draws them: what a lazy branch's address
    /// and a "Show more" link answer. `level` is the depth of the children (the
    /// top level is 1), `offset` how many siblings came before them, and `total`
    /// how many there are in all. Build it from the same tree as the page's.
    pub fn children(
        &self,
        level: u32,
        offset: usize,
        total: Option<usize>,
        nodes: impl IntoIterator<Item = TreeNode>,
        more: Option<(Href, Option<usize>)>,
    ) -> TreeChildren {
        let nodes: Vec<TreeNode> = nodes.into_iter().collect();
        let cx = Cx { tree: self };
        let total = total.unwrap_or(offset + nodes.len());
        let (items, _) = self.items(&cx, &nodes, level.max(1), offset, total, false);
        let more = more.map(|(href, remaining)| {
            self.more_row(
                &More {
                    href,
                    label: None,
                    remaining,
                },
                level.max(1),
            )
        });
        TreeChildren {
            items: items.into_iter().chain(more).collect(),
        }
    }

    /// Whether a choice reaches what is under it: only where many can be chosen.
    fn cascades(&self) -> bool {
        self.cascade && self.select == TreeSelect::Multiple
    }

    fn state_of(&self, node: &TreeNode, inherited: bool) -> Check {
        if node.selected || inherited {
            Check::On
        } else {
            Check::Off
        }
    }

    fn items(
        &self,
        cx: &Cx<'_>,
        nodes: &[TreeNode],
        level: u32,
        offset: usize,
        total: usize,
        inherited: bool,
    ) -> (Vec<Element>, Check) {
        let mut checks = Vec::with_capacity(nodes.len());
        let items: Vec<Element> = nodes
            .iter()
            .enumerate()
            .map(|(at, node)| {
                let (item, check) = self.item(cx, node, level, offset + at + 1, total, inherited);
                // A choice that is disabled is not the person's to make: it doesn't count.
                if !node.disabled {
                    checks.push(check);
                }
                item
            })
            .collect();
        let overall = if checks.is_empty() || checks.iter().all(|c| *c == Check::Off) {
            Check::Off
        } else if checks.iter().all(|c| *c == Check::On) {
            Check::On
        } else {
            Check::Mixed
        };
        (items, overall)
    }

    fn more_row(&self, more: &More, level: u32) -> Element {
        let text = match (&more.label, more.remaining) {
            (Some(label), _) => label.clone(),
            (None, Some(left)) => format!("{} ({left})", self.labels.more),
            (None, None) => self.labels.more.to_string(),
        };
        el("li")
            .slot("tree-more")
            .attr("data-level", i64::from(level))
            .child(
                el("div").slot("tree-row").child(
                    el("a")
                        .slot("tree-more-link")
                        .attr("data-ui-noboost", "")
                        .href(more.href.clone())
                        .child(text),
                ),
            )
    }

    fn item(
        &self,
        cx: &Cx<'_>,
        node: &TreeNode,
        level: u32,
        position: usize,
        size: usize,
        inherited: bool,
    ) -> (Element, Check) {
        let own = self.state_of(node, inherited) == Check::On && self.select != TreeSelect::None;
        let passes = self.cascades() && own;
        let is_branch = !matches!(node.kids, Kids::None);
        let open = is_branch
            && (node.open
                || self
                    .state
                    .as_ref()
                    .is_some_and(|state| state.is_open(&node.id)));
        let loaded = matches!(node.kids, Kids::Loaded(_));
        // The children, and what they say about this node when choices cascade.
        let (group, check) = match &node.kids {
            Kids::None => (None, if own { Check::On } else { Check::Off }),
            Kids::Loaded(children) => {
                let total = node.total.unwrap_or(children.len());
                let (items, overall) = self.items(cx, children, level + 1, 0, total, passes);
                let more = node
                    .more
                    .as_ref()
                    .map(|more| self.more_row(more, level + 1));
                let check = if self.cascades() && !children.is_empty() {
                    overall
                } else if own {
                    Check::On
                } else {
                    Check::Off
                };
                (
                    Some(el("ul").slot("tree-group").children((items, more))),
                    check,
                )
            }
            Kids::Lazy(src) => (
                Some(el("ul").slot("tree-group").attr("data-src", src.clone())),
                if own { Check::On } else { Check::Off },
            ),
        };
        let text = node.label.text_content();
        let toggle = is_branch.then(|| {
            let words = if open {
                &self.labels.collapse
            } else {
                &self.labels.expand
            };
            match &cx.tree.state {
                // The module opens it in place; the address is for a page with no script, so the
                // core runtime's link boosting must leave it alone.
                Some(state) => el("a")
                    .slot("tree-toggle")
                    .attr("data-ui-noboost", "")
                    .href(state.toggled(&node.id))
                    .attr("aria-label", words.replace("{label}", &text))
                    .child(icon(icons::CHEVRON_RIGHT)),
                None => el("span")
                    .slot("tree-toggle")
                    .attr("aria-hidden", "true")
                    .child(icon(icons::CHEVRON_RIGHT)),
            }
        });
        let label_body = (
            node.glyph.map(icon),
            el("span").slot("tree-text").child(node.label.clone()),
        );
        let label = match self.select {
            TreeSelect::None => match &node.href {
                Some(href) => el("a")
                    .slot("tree-label")
                    .href(href.clone())
                    .attr_opt("aria-current", node.current.then_some("page"))
                    .children(label_body),
                None => el("span").slot("tree-label").children(label_body),
            },
            select => {
                let kind = if select == TreeSelect::Single {
                    "radio"
                } else {
                    "checkbox"
                };
                el("label").slot("tree-label").children((
                    el("input")
                        .slot(kind)
                        .attr("type", kind)
                        .attr("name", self.name.clone())
                        .attr("value", node.id.clone())
                        .flag("checked", check == Check::On)
                        .flag("disabled", node.disabled),
                    label_body,
                ))
            }
        };
        let item = el("li")
            .slot("tree-item")
            .attr("data-level", i64::from(level))
            .attr("data-setsize", i64::try_from(size).unwrap_or(i64::MAX))
            .attr("data-posinset", i64::try_from(position).unwrap_or(i64::MAX))
            .flag("data-branch", is_branch)
            .flag("data-open", open)
            .flag("data-lazy", is_branch && !loaded)
            .flag("data-disabled", node.disabled)
            .attr_opt(
                "data-checked",
                (self.select != TreeSelect::None).then_some(match check {
                    Check::On => "true",
                    Check::Off => "false",
                    Check::Mixed => "mixed",
                }),
            )
            .child(el("div").slot("tree-row").children((toggle, label)))
            .child(group);
        (item, check)
    }

    fn into_node(self) -> Node {
        let cx = Cx { tree: &self };
        let total = self.total.unwrap_or(self.nodes.len());
        let (items, _) = self.items(&cx, &self.nodes, 1, 0, total, false);
        let more = self.more.as_ref().map(|more| self.more_row(more, 1));
        let cascades = self.cascades();
        let words = serde_json::json!({
            "loading": self.labels.loading,
            "failed": self.labels.failed,
            "retry": self.labels.retry,
            "loaded": self.labels.loaded,
        })
        .to_string();
        self.el
            .id(self.id.clone())
            .attr("data-ui-tree", "")
            .attr(
                "data-select",
                match self.select {
                    TreeSelect::None => "none",
                    TreeSelect::Single => "single",
                    TreeSelect::Multiple => "multiple",
                },
            )
            .flag("data-cascade", cascades)
            .attr("data-label", self.label.clone())
            .attr("data-l", words)
            .uses(Uses::TREE)
            .children((
                el("ul").slot("tree-root").children((items, more)),
                el("div")
                    .slot("tree-status")
                    .attr("role", "status")
                    .attr("aria-live", "polite")
                    .attr("data-ui-sr", ""),
            ))
            .into()
    }
}

into_node!(Tree);

/// The letters, digits, `-` and `_` of `text`; the rest becomes `_`.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A branch's children, as the server answers when it opens or asks for the next
/// page: the list items, with nothing around them.
#[derive(Clone, Debug)]
#[must_use]
pub struct TreeChildren {
    items: Vec<Element>,
}

impl From<TreeChildren> for Node {
    fn from(children: TreeChildren) -> Self {
        Node::list(children.items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{measure, render};

    fn html(tree: Tree) -> String {
        render(&tree.into())
    }

    fn folders() -> Tree {
        Tree::new("access", "Folders")
            .node(
                TreeNode::branch(
                    "docs",
                    "Documents",
                    [
                        TreeNode::leaf("docs/a", "Plans"),
                        TreeNode::leaf("docs/b", "Notes"),
                    ],
                )
                .open(true),
            )
            .node(TreeNode::lazy("music", "Music", "/tree/music"))
            .node(TreeNode::leaf("misc", "Misc").link("/misc"))
    }

    #[test]
    fn a_tree_is_nested_lists_with_no_roles_until_the_module_runs() {
        let out = html(folders());
        assert!(out.contains("<ul data-slot=\"tree-root\">"), "{out}");
        assert_eq!(out.matches("<li ").count(), 5, "{out}");
        assert!(!out.contains("role=\"tree"), "{out}");
        assert!(out.contains(r#"data-label="Folders""#), "{out}");
        assert!(out.contains(r#"data-open"#), "{out}");
        assert!(out.contains(r#"data-lazy"#), "{out}");
        assert!(out.contains(r#"data-src="/tree/music""#), "{out}");
        assert!(out.contains(r#"href="/misc""#), "{out}");
    }

    #[test]
    fn sizes_and_positions_are_counted_including_pages_not_drawn() {
        let out = html(folders());
        assert!(
            out.contains(r#"data-level="2" data-setsize="2" data-posinset="2""#),
            "{out}"
        );
        let paged = html(
            Tree::new("t", "T")
                .total(500)
                .node(TreeNode::leaf("a", "A"))
                .node(TreeNode::leaf("b", "B")),
        );
        assert!(
            paged.contains(r#"data-setsize="500" data-posinset="2""#),
            "{paged}"
        );
    }

    #[test]
    fn carets_are_links_only_when_the_address_holds_the_open_set() {
        let plain = html(folders());
        assert!(!plain.contains("tree-toggle\" href"), "{plain}");
        assert!(
            plain.contains(r#"<span data-slot="tree-toggle" aria-hidden="true">"#),
            "{plain}"
        );
        let state = TreeState::from_query("/files", "q=1&open=docs");
        let linked = html(folders().state(state));
        assert!(
            linked.contains(r#"<a data-slot="tree-toggle" data-ui-noboost="" href="/files?q=1""#),
            "{linked}"
        );
        assert!(
            linked.contains(r#"aria-label="Collapse Documents""#),
            "{linked}"
        );
        assert!(linked.contains(r#"aria-label="Expand Music""#), "{linked}");
        assert!(
            linked.contains("href=\"/files?q=1&amp;open=docs&amp;open=music\""),
            "{linked}"
        );
    }

    #[test]
    fn the_open_set_in_the_address_is_read_with_limits_and_toggled() {
        let state = TreeState::from_query("/f?x=1#y", "a=1&open=one&open=&open=two%2Fx&open=one");
        assert_eq!(state.open_ids().collect::<Vec<_>>(), ["one", "two/x"]);
        assert_eq!(state.toggled("one"), "/f?a=1&open=two%2Fx");
        let many: String = (0..500).map(|n| format!("open=n{n}&")).collect();
        assert_eq!(
            TreeState::from_query("/f", &many).open_ids().count(),
            MAX_OPEN
        );
        let long = format!("open={}", "x".repeat(300));
        assert_eq!(TreeState::from_query("/f", &long).open_ids().count(), 0);
        // Hostile ids are encoded in the links.
        let hostile = TreeState::from_query("/f", "").toggled("\"><script>&=");
        assert!(!hostile.contains(['<', '>', '"']), "{hostile}");
        // At the limit a branch can still be closed, but no more are opened.
        let full = TreeState::from_query("/f", &many);
        assert!(full.toggled("zzz").matches("open=").count() == MAX_OPEN);
        assert!(full.toggled("n0").matches("open=").count() == MAX_OPEN - 1);
    }

    #[test]
    fn choices_are_native_inputs_that_post_the_node_id() {
        let out = html(folders().select(TreeSelect::Multiple));
        assert_eq!(out.matches(r#"type="checkbox""#).count(), 5, "{out}");
        assert!(out.contains(r#"name="access" value="docs/a""#), "{out}");
        assert!(out.contains(r#"data-select="multiple""#), "{out}");
        let one = html(
            folders()
                .select(TreeSelect::Single)
                .node(TreeNode::leaf("x", "X").selected(true)),
        );
        assert_eq!(one.matches(r#"type="radio""#).count(), 6, "{one}");
        assert!(one.contains(r#"value="x" checked"#), "{one}");
        let off = html(
            Tree::new("t", "T")
                .select(TreeSelect::Multiple)
                .node(TreeNode::leaf("a", "A").disabled(true)),
        );
        assert!(off.contains("disabled"), "{off}");
    }

    #[test]
    fn a_branch_with_some_children_chosen_is_mixed_and_a_chosen_branch_chooses_its_children() {
        let tree = Tree::new("t", "T")
            .select(TreeSelect::Multiple)
            .cascade()
            .node(
                TreeNode::branch(
                    "p",
                    "P",
                    [
                        TreeNode::leaf("p1", "1").selected(true),
                        TreeNode::leaf("p2", "2"),
                    ],
                )
                .open(true),
            )
            .node(
                TreeNode::branch(
                    "q",
                    "Q",
                    [TreeNode::leaf("q1", "1"), TreeNode::leaf("q2", "2")],
                )
                .selected(true),
            )
            .node(TreeNode::branch(
                "r",
                "R",
                [
                    TreeNode::leaf("r1", "1").selected(true),
                    TreeNode::leaf("r2", "2").selected(true),
                ],
            ));
        let out = html(tree);
        assert!(out.contains(r#"data-checked="mixed""#), "{out}");
        // q is chosen, so q1 and q2 are too; r is chosen because both its children are.
        assert_eq!(out.matches(" checked").count(), 1 + 3 + 3, "{out}");
        assert!(out.contains(r#"data-cascade"#), "{out}");
    }

    #[test]
    fn a_lazy_branch_that_is_chosen_stands_for_what_is_under_it() {
        let out = html(
            Tree::new("t", "T")
                .select(TreeSelect::Multiple)
                .cascade()
                .node(TreeNode::lazy("a", "A", "/a").selected(true)),
        );
        assert!(out.contains(r#"value="a" checked"#), "{out}");
        assert!(out.contains(r#"data-checked="true""#), "{out}");
    }

    #[test]
    fn show_more_is_a_link_in_the_list() {
        let out = html(
            Tree::new("t", "T")
                .node(
                    TreeNode::branch("a", "A", [TreeNode::leaf("a1", "1")])
                        .children_total(300)
                        .more("/a?after=1", Some(299)),
                )
                .more("/top?after=1", None),
        );
        assert!(out.contains(r#"data-slot="tree-more-link" data-ui-noboost="" href="/a?after=1">Show more (299)</a>"#), "{out}");
        assert!(
            out.contains(r#"href="/top?after=1">Show more</a>"#),
            "{out}"
        );
        assert!(out.contains(r#"data-setsize="300""#), "{out}");
    }

    #[test]
    fn a_children_answer_is_list_items_with_the_right_positions() {
        let tree = Tree::new("t", "T").select(TreeSelect::Multiple);
        let answer: Node = tree
            .children(
                2,
                50,
                Some(120),
                [TreeNode::leaf("n51", "51"), TreeNode::leaf("n52", "52")],
                Some((Href::from("/p?after=52"), Some(68))),
            )
            .into();
        let out = render(&answer);
        assert!(out.starts_with("<li "), "{out}");
        assert!(
            out.contains(r#"data-level="2" data-setsize="120" data-posinset="51""#),
            "{out}"
        );
        assert!(out.contains(r#"data-posinset="52""#), "{out}");
        assert!(out.contains("Show more (68)"), "{out}");
        assert!(!out.contains("<ul data-slot=\"tree-root\""), "{out}");
    }

    #[test]
    fn deep_wide_and_hostile_trees_render() {
        let mut node = TreeNode::leaf("leaf", "leaf");
        for n in 0..50 {
            node = TreeNode::branch(format!("d{n}"), format!("depth {n}"), [node]).open(true);
        }
        let out = html(Tree::new("deep", "Deep").node(node));
        assert_eq!(out.matches("<li ").count(), 51);
        let wide = Tree::new("w", "W")
            .nodes((0..10_000).map(|n| TreeNode::leaf(format!("n{n}"), format!("Node {n}"))));
        assert_eq!(html(wide).matches("<li ").count(), 10_000);
        let hostile = html(
            Tree::new("a\"><script>", "<b>")
                .select(TreeSelect::Multiple)
                .node(
                    TreeNode::lazy("\"><img src=x>", "<i>x</i>", "javascript:alert(1)")
                        .link("data:text/html,x"),
                ),
        );
        assert!(
            !hostile.contains("<script>") && !hostile.contains("<img") && !hostile.contains("<i>"),
            "{hostile}"
        );
        assert!(!hostile.contains("javascript:"), "{hostile}");
        // An id of a million bytes is cut.
        let big = html(Tree::new("t", "T").node(TreeNode::leaf("é".repeat(500_000), "x")));
        assert!(big.len() < 3_000, "{}", big.len());
    }

    #[test]
    fn it_asks_for_the_tree_module_and_nothing_else() {
        let node: Node = folders().into();
        assert_eq!(measure(&node).uses, Uses::TREE);
    }

    #[test]
    fn the_selection_read_back_is_bounded_and_clean() {
        let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect()
        };
        let chosen = TreeSelection::parse(
            &pairs(&[("t", "a"), ("t", ""), ("u", "x"), ("t", "b"), ("t", "a")]),
            "t",
        )
        .unwrap();
        assert_eq!(chosen.ids(), ["a", "b"]);
        assert!(chosen.contains("b") && !chosen.contains("x"));
        assert_eq!(chosen.first_unknown(|id| id == "a"), Some("b"));
        let long = "x".repeat(300);
        assert_eq!(
            TreeSelection::parse(&pairs(&[("t", &long)]), "t"),
            Err(TreeSelectionError::TooLong)
        );
        let many: Vec<(String, String)> = (0..=MAX_IDS)
            .map(|n| ("t".to_owned(), n.to_string()))
            .collect();
        assert_eq!(
            TreeSelection::parse(&many, "t"),
            Err(TreeSelectionError::TooMany)
        );
        assert_eq!(
            TreeSelection::one(&pairs(&[("t", "a")]), "t"),
            Ok(Some("a".to_owned()))
        );
        assert_eq!(TreeSelection::one(&pairs(&[]), "t"), Ok(None));
        assert_eq!(
            TreeSelection::one(&pairs(&[("t", "a"), ("t", "b")]), "t"),
            Err(TreeSelectionError::NotOne)
        );
    }

    #[test]
    fn roots_drop_what_a_chosen_ancestor_already_covers_and_survive_a_cycle() {
        let pairs: Vec<(String, String)> = ["a", "a/b", "a/b/c", "z"]
            .iter()
            .map(|id| ("t".to_owned(), (*id).to_owned()))
            .collect();
        let chosen = TreeSelection::parse(&pairs, "t").unwrap();
        let parent = |id: &str| id.rsplit_once('/').map(|(up, _)| up.to_owned());
        assert_eq!(chosen.roots(parent), ["a", "z"]);
        // A parent function that loops forever is cut off.
        let looping = TreeSelection::parse(&[("t".to_owned(), "x".to_owned())], "t").unwrap();
        assert_eq!(looping.roots(|_| Some("y".to_owned())), ["x"]);
    }
}
