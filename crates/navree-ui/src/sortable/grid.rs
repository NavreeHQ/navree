//! A sortable grid: tiles in rows and columns that people put in their own order by
//! pointer, touch or keys, and take away, with the whole order saved to the server.

use std::borrow::Cow;

use crate::components::icon;
use crate::form::csrf_field;
use crate::html::Href;
use crate::icons;
use crate::node::{IntoNodes, Node, Uses, el, into_node};

/// How many columns a [`SortableGrid`] has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GridColumns {
    /// Exactly this many, 1 to 6, at every width.
    Fixed(u8),
    /// As many as fit, each at least as wide as the [`GridFit`] says: fewer on a
    /// phone, more on a wide screen.
    Fit(GridFit),
}

impl Default for GridColumns {
    fn default() -> Self {
        Self::Fit(GridFit::Medium)
    }
}

/// The narrowest a tile gets when the grid fits as many columns as it can.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum GridFit {
    /// 8 rem: small tiles, a figure each.
    Narrow,
    /// 12 rem: a figure with a small chart. The default.
    #[default]
    Medium,
    /// 16 rem: a card with a title, a chart and a line of text.
    Wide,
}

impl GridFit {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Narrow => "narrow",
            Self::Medium => "medium",
            Self::Wide => "wide",
        }
    }
}

/// The words a [`SortableGrid`] says and shows. `{item}`, `{position}` and `{total}`
/// are filled in.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SortableGridLabels {
    /// How to move with the keys, read with every handle.
    pub help: Cow<'static, str>,
    /// The handle's name.
    pub handle: Cow<'static, str>,
    /// The remove button's name.
    pub remove: Cow<'static, str>,
    /// The name of the button that moves a tile one place earlier, with JavaScript off.
    pub earlier: Cow<'static, str>,
    /// The name of the button that moves a tile one place later, with JavaScript off.
    pub later: Cow<'static, str>,
    /// Said when a tile is picked up.
    pub picked: Cow<'static, str>,
    /// Said after each step of a move.
    pub moved: Cow<'static, str>,
    /// Said when a tile is dropped.
    pub dropped: Cow<'static, str>,
    /// Said when a move is cancelled.
    pub cancelled: Cow<'static, str>,
    /// Said when a tile is taken away; `{total}` is how many are left.
    pub removed: Cow<'static, str>,
    /// What a grid with no tiles says.
    pub empty: Cow<'static, str>,
}

impl Default for SortableGridLabels {
    fn default() -> Self {
        Self {
            help: Cow::Borrowed(
                "Press Space to pick it up, the arrow keys to move it, Space again to drop it, Escape to cancel.",
            ),
            handle: Cow::Borrowed("Move {item}"),
            remove: Cow::Borrowed("Remove {item}"),
            earlier: Cow::Borrowed("Move {item} earlier"),
            later: Cow::Borrowed("Move {item} later"),
            picked: Cow::Borrowed("Picked up {item}. Position {position} of {total}."),
            moved: Cow::Borrowed("Position {position} of {total}."),
            dropped: Cow::Borrowed("Dropped {item} at position {position} of {total}."),
            cancelled: Cow::Borrowed(
                "Move cancelled. {item} is back at position {position} of {total}.",
            ),
            removed: Cow::Borrowed("Removed {item}. {total} left."),
            empty: Cow::Borrowed("Nothing here yet"),
        }
    }
}

/// One tile of a [`SortableGrid`].
#[derive(Clone, Debug)]
#[must_use]
pub struct GridTile {
    key: String,
    name: String,
    body: Vec<Node>,
    removable: bool,
}

impl GridTile {
    /// A tile: `key` is what the order posts and must be unique in the grid, `name`
    /// is what it is called when it is announced and on its buttons. With no
    /// [`body`](Self::body), the name is what shows.
    pub fn new(key: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            body: Vec::new(),
            removable: true,
        }
    }

    /// Sets what the tile shows: a figure, a small chart, a card. Links in it still
    /// work: a press that doesn't move is a click.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        self.body.clear();
        body.push_into(&mut self.body);
        self
    }

    /// Draws it with no remove button: a tile the grid always holds. The server must
    /// keep it too, with [`GridOrder::keep`](crate::sortable::GridOrder::keep): this is
    /// only what the page shows.
    pub fn fixed(mut self) -> Self {
        self.removable = false;
        self
    }
}

/// Tiles in a grid that people put in their own order and take away: the tiles of a
/// dashboard, of a home-screen widget, of a launcher.
///
/// A tile moves by dragging it with a mouse or a finger (anywhere on a mouse, by its
/// handle or after a held press on touch), or with the keys on its handle: Space or
/// Enter picks it up, the arrows move it (up and down by a row), Space or Enter drops
/// it and Escape puts it back. Every step is said in the grid's own live region.
///
/// The whole order is posted to [`post`](Self::post) a moment after the last change
/// (half a second unless [`autosave_ms`](Self::autosave_ms) says otherwise), as
/// repeated `order` fields, with a `remove` field for each tile taken away: read it
/// with [`GridOrder`](crate::sortable::GridOrder). The post goes through the page's
/// forms, so it carries the session's token, the latest order is the one that lands
/// when changes come quickly, and the answer's ops apply: a preview drawn again, a
/// count. With JavaScript off, each tile has buttons that post a move one place
/// earlier or later, and its remove button posts too.
///
/// ```
/// use navree_ui::{GridTile, SortableGrid, render};
///
/// let grid = SortableGrid::new("tiles", "Widget tiles")
///     .post("/widgets/7/tiles")
///     .columns(2)
///     .tile(GridTile::new("recovery", "Recovery"))
///     .tile(GridTile::new("sleep", "Sleep"));
/// let html = render(&grid.into());
/// assert!(html.contains("data-ui-grid"));
/// assert!(html.contains(r#"name="order" value="recovery""#));
/// ```
///
/// Without [`post`](Self::post) the tiles are drawn in their grid with no handles,
/// no buttons and no script.
#[derive(Clone, Debug)]
#[must_use]
pub struct SortableGrid {
    id: String,
    label: String,
    action: Option<Href>,
    tiles: Vec<GridTile>,
    columns: GridColumns,
    labels: SortableGridLabels,
    csrf: Option<String>,
    autosave: u32,
    deadline: u32,
}

impl SortableGrid {
    /// A grid with the id `id`, named `label` for assistive technology. The tiles'
    /// ids and the order form's are made from it.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            action: None,
            tiles: Vec::new(),
            columns: GridColumns::default(),
            labels: SortableGridLabels::default(),
            csrf: None,
            autosave: 500,
            deadline: 15_000,
        }
    }

    /// Adds a tile.
    pub fn tile(mut self, tile: GridTile) -> Self {
        self.tiles.push(tile);
        self
    }

    /// Adds tiles, in order.
    pub fn tiles(mut self, tiles: impl IntoIterator<Item = GridTile>) -> Self {
        self.tiles.extend(tiles);
        self
    }

    /// Makes the tiles move and go, the order posted to `action` after each change.
    /// Without it nothing moves.
    pub fn post(mut self, action: impl Into<Href>) -> Self {
        self.action = Some(action.into());
        self
    }

    /// Exactly `n` columns, 1 to 6, at every width.
    pub fn columns(mut self, n: u8) -> Self {
        self.columns = GridColumns::Fixed(n.clamp(1, 6));
        self
    }

    /// As many columns as fit, each tile at least as wide as `fit` says. The default,
    /// with [`GridFit::Medium`].
    pub fn fit(mut self, fit: GridFit) -> Self {
        self.columns = GridColumns::Fit(fit);
        self
    }

    /// How long the grid waits after the last change before it posts the order, in
    /// milliseconds: 500 by default, at most 10 s. 0 posts at once.
    pub fn autosave_ms(mut self, ms: u32) -> Self {
        self.autosave = ms.min(10_000);
        self
    }

    /// How long a post may wait for the server's answer before it counts as failed,
    /// in milliseconds: 15 s by default, at least 1 s.
    pub fn deadline_ms(mut self, ms: u32) -> Self {
        self.deadline = ms.max(1_000);
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: SortableGridLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Puts the session's token in the order form, for a server whose session checks
    /// a field when JavaScript is off.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    fn into_node(self) -> Node {
        let labels = &self.labels;
        let movable = self.action.is_some();
        let form_id = format!("{}-form", self.id);
        let help_id = format!("{}-help", self.id);
        let slugs = super::slugs(self.tiles.iter().map(|tile| tile.key.as_str()));
        let last = self.tiles.len().saturating_sub(1);
        let tiles: Vec<Node> = self
            .tiles
            .iter()
            .zip(slugs)
            .enumerate()
            .map(|(at, (tile, slug))| {
                let tool = |slot: &'static str, name: &'static str, words: &str, glyph| {
                    el("button")
                        .slot(slot)
                        .attr("type", "submit")
                        .attr("form", form_id.clone())
                        .attr("name", name)
                        .attr("value", tile.key.clone())
                        .attr("aria-label", words.replace("{item}", &tile.name))
                        .child(icon(glyph))
                };
                let tools = movable.then(|| {
                    el("div").slot("sortable-grid-tools").children((
                        // WebKit gives a button it clicks the focus only when it has a tabindex of its own.
                        el("button")
                            .slot("sortable-grid-handle")
                            .attr("type", "button")
                            .attr("tabindex", "0")
                            .attr("aria-label", labels.handle.replace("{item}", &tile.name))
                            .attr("aria-describedby", help_id.clone())
                            .child(icon(icons::GRIP_VERTICAL)),
                        el("span").slot("sortable-grid-steps").children((
                            tool(
                                "sortable-grid-step",
                                "up",
                                &labels.earlier,
                                icons::ARROW_LEFT,
                            )
                            .flag("disabled", at == 0),
                            tool(
                                "sortable-grid-step",
                                "down",
                                &labels.later,
                                icons::ARROW_RIGHT,
                            )
                            .flag("disabled", at == last),
                        )),
                        tile.removable.then(|| {
                            tool("sortable-grid-remove", "remove", &labels.remove, icons::X)
                        }),
                    ))
                });
                let body = if tile.body.is_empty() {
                    vec![Node::from(el("span").child(tile.name.clone()))]
                } else {
                    tile.body.clone()
                };
                el("li")
                    .slot("sortable-grid-item")
                    .attr("id", format!("{}-tile-{slug}", self.id))
                    .attr("data-key", tile.key.clone())
                    .attr("data-name", tile.name.clone())
                    .flag("data-fixed", !tile.removable)
                    .children((tools, el("div").slot("sortable-grid-body").child(body)))
                    .into()
            })
            .collect();
        let form = self.action.as_ref().map(|action| {
            el("form")
                .slot("sortable-grid-form")
                .attr("id", form_id.clone())
                .attr("method", "post")
                .attr("action", action.clone())
                .flag("data-ui-form", true)
                .attr("data-ui-deadline", i64::from(self.deadline))
                .children((
                    self.csrf.clone().map(csrf_field),
                    self.tiles
                        .iter()
                        .map(|tile| {
                            Node::from(
                                el("input")
                                    .attr("type", "hidden")
                                    .attr("name", "order")
                                    .attr("value", tile.key.clone()),
                            )
                        })
                        .collect::<Vec<_>>(),
                ))
        });
        let words = movable.then(|| {
            serde_json::json!({
                "picked": labels.picked,
                "moved": labels.moved,
                "dropped": labels.dropped,
                "cancelled": labels.cancelled,
                "removed": labels.removed,
            })
            .to_string()
        });
        let mut root = el("div")
            .slot("sortable-grid")
            .attr("id", self.id.clone())
            .attr("role", "group")
            .attr("aria-label", self.label.clone())
            .uses(Uses::SORTABLE_GRID_STYLES);
        root = match self.columns {
            GridColumns::Fixed(n) => root.attr("data-columns", i64::from(n.clamp(1, 6))),
            GridColumns::Fit(fit) => root.attr("data-fit", fit.as_str()),
        };
        if let Some(words) = words {
            root = root
                .flag("data-ui-grid", true)
                .attr("tabindex", "-1")
                .attr("data-ui-autosave", i64::from(self.autosave))
                .attr("data-l", words)
                .uses(Uses::CORE.with(Uses::SORTABLE_GRID));
        }
        root.children((
            movable.then(|| {
                el("p")
                    .slot("sortable-grid-help")
                    .attr("id", help_id.clone())
                    .flag("hidden", true)
                    .child(labels.help.clone())
            }),
            movable.then(|| {
                el("div")
                    .slot("sortable-grid-status")
                    .attr("aria-live", "assertive")
                    .attr("aria-atomic", "true")
                    .flag("data-ui-sr", true)
            }),
            form,
            el("ol").slot("sortable-grid-items").child(tiles),
            el("p")
                .slot("sortable-grid-empty")
                .child(labels.empty.clone()),
        ))
        .into()
    }
}

into_node!(SortableGrid);
