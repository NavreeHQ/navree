//! The detail pane, drawn with navree-ui: the summary, then one card per kind of
//! thing the entry touched, and the replay form. No script of its own and no
//! stylesheet of its own: everything on the page is the toolkit's.

use navree_ui::Justify;
use navree_ui::{
    Badge, BadgeVariant, Button, Card, Checkbox, Empty, Field, Grid, Heading, Input, Node,
    Row as Layout, Size, Stack, Table, TableCell, TableHead, TableRowEl, Text, Textarea, Uses, el,
    icons,
};

use super::{Bar, Fact, Item, Section, breakable, tight};

/// A captured request or response.
pub(super) struct Message {
    pub(super) headers: Vec<(String, String)>,
    pub(super) query: String,
    pub(super) body: String,
    pub(super) body_meta: String,
}

/// One recorded statement.
pub(super) struct Statement {
    pub(super) sql: String,
    pub(super) duration: String,
    pub(super) slow: bool,
    pub(super) duplicate: bool,
}

/// A statement that ran more than once.
pub(super) struct Repeated {
    pub(super) sql: String,
    pub(super) count: u32,
    pub(super) total: String,
}

/// What the replay form starts with.
pub(super) struct Replay {
    pub(super) action: String,
    pub(super) curl: String,
    pub(super) method: String,
    pub(super) path: String,
    pub(super) headers: String,
    pub(super) body: String,
    pub(super) user: String,
}

/// Everything the pane shows.
pub(super) struct Detail {
    pub(super) title: String,
    pub(super) kind_label: &'static str,
    /// The request's status, or the job's outcome.
    pub(super) status: String,
    pub(super) duration: String,
    pub(super) facts: Vec<Fact>,
    pub(super) tags: Vec<(String, String)>,
    pub(super) flags: Vec<&'static str>,
    pub(super) request: Option<Message>,
    pub(super) response: Option<Message>,
    pub(super) statements: Vec<Statement>,
    pub(super) repeated: Vec<Repeated>,
    pub(super) sections: Vec<Section>,
    pub(super) timeline: Vec<Bar>,
    /// How long the whole timeline is: the far end of its scale.
    pub(super) timeline_total: String,
    pub(super) replay: Option<Replay>,
}

/// A block of text that keeps its line breaks and wraps.
fn block(text: String) -> Node {
    if text.is_empty() {
        Node::Empty
    } else {
        el("pre").slot("code").child(text).into()
    }
}

/// Names and values, each value under its name: two columns on a wide screen and
/// one on a phone, where a value (an id, a token) has the whole width and nothing
/// has to scroll sideways.
fn pairs(pairs: Vec<(String, Node)>) -> Node {
    Grid::cols(2)
        .gap(3)
        .children(
            pairs
                .into_iter()
                .map(|(name, value)| {
                    Stack::gap(0).children((Text::muted(breakable(&name)), Text::new(value)))
                })
                .collect::<Vec<_>>(),
        )
        .into()
}

/// Headers as pairs.
fn header_pairs(headers: Vec<(String, String)>) -> Node {
    if headers.is_empty() {
        return Node::Empty;
    }
    Stack::gap(1)
        .children((
            Heading::h3("Headers"),
            pairs(
                headers
                    .into_iter()
                    .map(|(name, value)| (name, Node::from(breakable(&value))))
                    .collect(),
            ),
        ))
        .into()
}

fn message(title: &'static str, id: &'static str, message: Message) -> Node {
    let meta = (!message.body_meta.is_empty()).then(|| Text::muted(message.body_meta));
    el("section")
        .attr("id", id)
        .child(Card::titled(title).body(Stack::gap(3).children(
            (
                (!message.query.is_empty()).then(|| {
                    Stack::gap(1).children((Heading::h3("Query string"), block(message.query)))
                }),
                header_pairs(message.headers),
                (meta.is_some() || !message.body.is_empty()).then(|| {
                    Stack::gap(1).children((Heading::h3("Body"), meta, block(message.body)))
                }),
            ),
        )))
        .into()
}

fn queries(statements: Vec<Statement>, repeated: Vec<Repeated>) -> Node {
    let repeated_table = (!repeated.is_empty()).then(|| {
        Stack::gap(1).children((
            Heading::h3("Repeated statements"),
            Table::new()
                .caption("Statements that ran more than once")
                .head((
                    TableHead::new("Statement"),
                    TableHead::new("Times").numeric(),
                    TableHead::new("Total").numeric(),
                ))
                .rows(
                    repeated
                        .into_iter()
                        .map(|r| {
                            TableRowEl::new((
                                TableCell::new(el("code").child(r.sql)),
                                TableCell::new(format!("× {}", r.count)).numeric(),
                                TableCell::new(r.total).numeric(),
                            ))
                        })
                        .collect::<Vec<_>>(),
                ),
        ))
    });
    let all = Table::new()
        .caption("Every statement")
        .head((
            TableHead::new("Statement"),
            TableHead::new("Time").numeric(),
        ))
        .rows(
            statements
                .into_iter()
                .map(|q| {
                    let mut flags: Vec<Node> = Vec::new();
                    if q.slow {
                        flags.push(Badge::new("slow").variant(BadgeVariant::Warning).into());
                    }
                    if q.duplicate {
                        flags.push(Badge::new("repeated").variant(BadgeVariant::Warning).into());
                    }
                    TableRowEl::new((
                        TableCell::new(Node::list((
                            el("code").child(q.sql),
                            " ",
                            Node::list(flags),
                        ))),
                        TableCell::new(q.duration).numeric(),
                    ))
                })
                .collect::<Vec<_>>(),
        );
    el("section")
        .attr("id", "queries")
        .child(Card::titled("Queries").body(Stack::gap(3).children((repeated_table, all))))
        .into()
}

fn item(item: Item) -> Node {
    let title: Node = if item.href.is_empty() {
        Node::from(breakable(&item.title))
    } else {
        el("a").href(item.href).child(breakable(&item.title)).into()
    };
    el("article")
        .attr("id", format!("entry-{}", item.id))
        .flag("data-selected", item.selected)
        .child(Stack::gap(1).children((
            Heading::h3(Node::list((
                title,
                item.selected.then(|| {
                    Node::list((" ", Badge::new("this entry").variant(BadgeVariant::Outline)))
                }),
            ))),
            (!item.meta.is_empty()).then(|| Text::muted(item.meta)),
            (!item.html_href.is_empty()).then(|| {
                el("a")
                    .href(item.html_href)
                    .attr("target", "_blank")
                    .attr("rel", "noopener")
                    .child("Open HTML")
            }),
            block(item.body),
        )))
        .into()
}

fn section(section: Section) -> Node {
    el("section")
        .attr("id", section.id)
        .child(
            Card::titled(section.label).body(
                Stack::gap(4).children(section.items.into_iter().map(item).collect::<Vec<_>>()),
            ),
        )
        .into()
}

/// One bar of the waterfall, drawn as SVG so that it needs no script and no
/// style attribute: the offset and the width are coordinates.
fn track(bar: &Bar) -> Node {
    let left: f64 = bar.left.parse().unwrap_or(0.0);
    let width: f64 = bar.width.parse().unwrap_or(0.0);
    let tone = match bar.kind {
        "mail" | "exception" => "var(--amber)",
        "request" | "job" => "var(--accent)",
        _ => "var(--bar)",
    };
    el("svg")
        .attr("viewBox", "0 0 100 10")
        .attr("preserveAspectRatio", "none")
        .attr("width", "100%")
        .attr("height", 10_i64)
        .attr("role", "img")
        .attr(
            "aria-label",
            format!("starts {left:.0}% in, lasts {width:.1}% of the whole"),
        )
        .children((
            el("rect")
                .attr("width", 100_i64)
                .attr("height", 10_i64)
                .attr("fill", "var(--ink-3)"),
            el("rect")
                .attr("x", format!("{left:.2}"))
                .attr("width", format!("{:.2}", width.max(0.4)))
                .attr("height", 10_i64)
                .attr("fill", tone),
        ))
        .into()
}

fn timeline(bars: Vec<Bar>, total: String) -> Node {
    let mut rows: Vec<TableRowEl> = bars
        .iter()
        .map(|bar| {
            TableRowEl::new((
                TableCell::new(
                    el("a")
                        .href(format!("#entry-{}", bar.id))
                        .child(tight(&bar.label)),
                ),
                TableCell::new(track(bar)),
                TableCell::new(bar.duration.clone()).numeric(),
            ))
        })
        .collect();
    // The scale: nought, the middle and the end of the request's own time.
    rows.push(TableRowEl::new((
        TableCell::new(Text::muted("Scale")),
        TableCell::new(Layout::gap(0).justify(Justify::Between).children((
            Text::muted("0").inline(),
            Text::muted("½").inline(),
            Text::muted(total).inline(),
        ))),
        TableCell::new(""),
    )));
    el("section")
        .attr("id", "timeline")
        .child(
            Card::titled("Timeline").body(
                Table::new()
                    .caption("Where each entry started and how long it took")
                    .head((
                        TableHead::new("Entry"),
                        TableHead::new("When"),
                        TableHead::new("Took").numeric(),
                    ))
                    .rows(rows),
            ),
        )
        .into()
}

/// The replay form posts to the page's own action: with the runtime it answers
/// with the new entry and a toast, with JavaScript off it redirects there.
fn replay(replay: Replay) -> Node {
    let form = el("form")
        .attr("method", "post")
        .attr("action", replay.action)
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .child(
            Stack::gap(3).children((
                Layout::gap(2).children((
                    Field::new("replay-method", "Method")
                        .control(Input::text("method").value(replay.method)),
                    Field::new("replay-path", "Path and query string")
                        .control(Input::text("path").value(replay.path)),
                )),
                Field::new("replay-headers", "Headers, one per line")
                    .control(Textarea::new("headers").value(replay.headers).rows(6)),
                Field::new("replay-body", "Body")
                    .control(Textarea::new("body").value(replay.body).rows(8)),
                (!replay.user.is_empty()).then(|| {
                    Checkbox::new("as_user")
                        .checked(true)
                        .label(format!("As {}", replay.user))
                }),
                Checkbox::new("confirm")
                    .label("Replaying anything but a GET changes data again: send it anyway"),
                Layout::gap(2).children((
                    Button::primary("Replay").submit(),
                    Button::outline("Show as curl").href(replay.curl),
                )),
            )),
        );
    el("section")
        .attr("id", "replay")
        .child(Card::titled("Replay").body(form))
        .into()
}

/// What opens beside the list when nothing is chosen. On a phone the list has the
/// page to itself, so this goes.
pub(super) fn nothing_chosen() -> Node {
    Stack::gap(3)
        .span(2)
        .attr("data-hide-mobile", "")
        .children(Empty::new("Choose an entry to see what it did.").icon(icons::LIST))
        .into()
}

/// What opens beside the list when the address names an entry that isn't there.
pub(super) fn missing(base: &str) -> Node {
    Stack::gap(3)
        .span(2)
        .children(
            Empty::new("There is no entry with this id. It may have been pruned or cleared.")
                .icon(icons::CIRCLE_ALERT)
                .actions(Button::outline("Back to the list").href(base.to_owned())),
        )
        .into()
}

/// The jump links: what the pane holds, with how many.
fn jumps(detail: &Detail) -> Vec<(&'static str, String)> {
    let mut jump: Vec<(&'static str, String)> = Vec::new();
    if detail.request.is_some() {
        jump.push(("request", "Request".to_owned()));
    }
    if detail.response.is_some() {
        jump.push(("response", "Response".to_owned()));
    }
    if !detail.statements.is_empty() {
        jump.push(("queries", format!("Queries {}", detail.statements.len())));
    }
    jump.extend(
        detail
            .sections
            .iter()
            .map(|s| (s.id, format!("{} {}", s.label, s.items.len()))),
    );
    if !detail.timeline.is_empty() {
        jump.push(("timeline", "Timeline".to_owned()));
    }
    if detail.replay.is_some() {
        jump.push(("replay", "Replay".to_owned()));
    }
    jump
}

/// The badge of a status: green for success, amber for a client's mistake, red for ours.
fn status_badge(status: &str) -> Option<Badge> {
    if status.is_empty() {
        return None;
    }
    let variant = match status.as_bytes().first() {
        Some(b'2') => BadgeVariant::Success,
        Some(b'4') => BadgeVariant::Warning,
        Some(b'5') => BadgeVariant::Danger,
        _ if status == "failed" => BadgeVariant::Danger,
        _ => BadgeVariant::Outline,
    };
    Some(Badge::new(status.to_owned()).variant(variant))
}

/// The detail pane.
pub(super) fn pane(detail: Detail) -> Node {
    let jump = jumps(&detail);
    let tone = if detail.flags.is_empty() {
        BadgeVariant::Outline
    } else {
        BadgeVariant::Warning
    };
    let flags: Vec<Node> = detail
        .flags
        .iter()
        .map(|flag| Badge::new(*flag).variant(tone).into())
        .collect();
    let facts = pairs(
        detail
            .facts
            .into_iter()
            .map(|fact| {
                let value: Node = if fact.href.is_empty() {
                    Node::from(breakable(&fact.value))
                } else {
                    el("a").href(fact.href).child(breakable(&fact.value)).into()
                };
                (fact.label.to_owned(), value)
            })
            .collect(),
    );
    let tags = (!detail.tags.is_empty()).then(|| {
        Layout::gap(2).wrap().children(
            detail
                .tags
                .into_iter()
                .map(|(tag, href)| el("a").href(href).child(breakable(&tag)))
                .collect::<Vec<_>>(),
        )
    });
    Stack::gap(4)
        .span(2)
        .attr("data-pane", "")
        .children((
            Card::new().body(
                Stack::gap(3).children((
                    Layout::gap(2).wrap().children((
                        Badge::new(detail.kind_label).variant(BadgeVariant::Outline),
                        status_badge(&detail.status),
                        (!detail.duration.is_empty())
                            .then(|| Text::muted(detail.duration.clone()).inline()),
                        Node::list(flags),
                    )),
                    facts,
                    tags,
                )),
            ),
            el("nav").attr("aria-label", "On this entry").child(
                Layout::gap(1).wrap().children(
                    jump.into_iter()
                        .map(|(id, label)| {
                            Button::ghost(label).size(Size::Sm).href(format!("#{id}"))
                        })
                        .collect::<Vec<_>>(),
                ),
            ),
            detail.request.map(|m| message("Request", "request", m)),
            detail.response.map(|m| message("Response", "response", m)),
            (!detail.statements.is_empty()).then(|| queries(detail.statements, detail.repeated)),
            Node::list(detail.sections.into_iter().map(section).collect::<Vec<_>>()),
            (!detail.timeline.is_empty()).then(|| timeline(detail.timeline, detail.timeline_total)),
            detail.replay.map(replay),
        ))
        .into()
}
