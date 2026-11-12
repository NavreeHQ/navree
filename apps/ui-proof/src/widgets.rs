//! A home-screen widget builder: a sortable grid of tiles saved as they are moved and
//! taken away, and the widget's picture drawn on the server.
//!
//! | Page | Shows |
//! |---|---|
//! | `/widgets` | the builder: the size, the tiles in a sortable grid, the tiles to add, the accent, and the picture in dark and light |
//! | `/widgets/image.png?size=&mode=` | the widget's picture, tagged with an `ETag` from what draws it |
//! | `/widgets/state` | what the server holds, as JSON, for the browser suite |
//!
//! `POST /widgets/tiles` takes the grid's order, `/widgets/add`, `/widgets/size` and
//! `/widgets/accent` the other controls. `POST /widgets/reset` puts everything back and
//! `/widgets/fail` makes the next save of the order fail. The people and numbers are
//! invented.

use std::sync::{Arc, LazyLock, Mutex};

use http::Method;
use http::request::Parts;
use navree::http::extract::urlencoded;
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::charts::{Ink, Ring, Sparkline};
use navree::ui::image::{Block, ChartImage, Tile, TileSize};
use navree::ui::theme::{Mode, Rgb};
use navree::ui::*;

/// A kind of tile: its key, its name, and what it draws.
struct Kind {
    key: &'static str,
    name: &'static str,
}

const KINDS: [Kind; 8] = [
    Kind {
        key: "health",
        name: "Health score",
    },
    Kind {
        key: "recovery",
        name: "Recovery",
    },
    Kind {
        key: "resting-hr",
        name: "Resting HR",
    },
    Kind {
        key: "hrv",
        name: "HRV",
    },
    Kind {
        key: "sleep",
        name: "Sleep",
    },
    Kind {
        key: "movement",
        name: "Movement",
    },
    Kind {
        key: "stress",
        name: "24h stress",
    },
    Kind {
        key: "versus",
        name: "vs 7-day avg",
    },
];

const ACCENTS: [(&str, &str, u32); 6] = [
    ("cyan", "Cyan", 0x22d3ee),
    ("green", "Green", 0x34d399),
    ("purple", "Purple", 0xa78bfa),
    ("amber", "Amber", 0xfbbf24),
    ("red", "Red", 0xfb7185),
    ("blue", "Blue", 0x60a5fa),
];

fn name_of(key: &str) -> &'static str {
    KINDS
        .iter()
        .find(|k| k.key == key)
        .map_or("Tile", |k| k.name)
}

/// What the server holds.
struct Widget {
    size: TileSize,
    accent: &'static str,
    tiles: Vec<String>,
    saves: u32,
    fail_next: bool,
    last: String,
}

impl Widget {
    fn new() -> Self {
        Self {
            size: TileSize::Medium,
            accent: "cyan",
            tiles: vec!["health".into(), "recovery".into(), "sleep".into()],
            saves: 0,
            fail_next: false,
            last: String::new(),
        }
    }
}

static WIDGET: LazyLock<Mutex<Widget>> = LazyLock::new(|| Mutex::new(Widget::new()));

fn with<T>(f: impl FnOnce(&mut Widget) -> T) -> T {
    let mut widget = WIDGET
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut widget)
}

fn week(base: f64, swing: f64) -> Vec<f64> {
    (0..7)
        .map(|i| base + swing * ((f64::from(i) * 1.3).sin() + f64::from(i % 3) * 0.2))
        .collect()
}

/// The block a tile draws in the picture.
fn block(key: &str) -> Block {
    match key {
        "health" => Block::ring("Health score", 78, 100).note("Good"),
        "recovery" => Block::ring("Recovery", 82, 100)
            .unit("%")
            .note("Well rested"),
        "resting-hr" => Block::sparkline("Resting HR", week(60.0, 2.0))
            .value("58")
            .unit("bpm"),
        "hrv" => Block::sparkline("HRV", week(48.0, 6.0))
            .value("52")
            .unit("ms"),
        "sleep" => Block::stat("Sleep", "7h 42m")
            .note("+18 min vs 7-day avg")
            .tone(Tone::Success),
        "movement" => Block::ring("Movement", 540, 600).unit("kcal"),
        "stress" => Block::columns("24h stress", [22, 35, 48, 30, 26, 18, 40, 52, 33, 21])
            .value("31")
            .ink(Ink::slot(4)),
        _ => Block::stat("vs 7-day avg", "+4%")
            .note("Steadier than last week")
            .tone(Tone::Success),
    }
}

fn tile_of(widget: &Widget) -> Tile {
    Tile::new(widget.size)
        .title("Simran's day")
        .note("Updated 07:30")
        .blocks(widget.tiles.iter().map(|key| block(key)))
}

fn image_of(widget: &Widget, tile: &Tile, mode: Mode) -> ChartImage {
    let accent = ACCENTS
        .iter()
        .find(|(key, ..)| *key == widget.accent)
        .map_or(0x22d3ee, |(.., hex)| *hex);
    ChartImage::for_tile(tile)
        .mode(mode)
        .accent(Rgb::hex(accent))
}

/// A tile in the grid: its name, its figure, and a small chart.
fn grid_tile(key: &str) -> GridTile {
    let name = name_of(key);
    let (figure, chart): (&str, Node) = match key {
        "health" => (
            "78",
            Ring::new(name, 78)
                .size(44)
                .inline()
                .ink(Ink::accent())
                .into(),
        ),
        "recovery" => (
            "82%",
            Ring::new(name, 82)
                .size(44)
                .inline()
                .ink(Ink::accent())
                .into(),
        ),
        "movement" => (
            "540 kcal",
            Ring::new(name, 540)
                .max(600)
                .size(44)
                .inline()
                .ink(Ink::accent())
                .into(),
        ),
        "resting-hr" => (
            "58 bpm",
            Sparkline::new(name, week(60.0, 2.0))
                .height(28)
                .ink(Ink::accent())
                .into(),
        ),
        "hrv" => (
            "52 ms",
            Sparkline::new(name, week(48.0, 6.0))
                .height(28)
                .ink(Ink::accent())
                .into(),
        ),
        "stress" => (
            "31",
            Sparkline::new(name, [22, 35, 48, 30, 26, 18, 40])
                .columns()
                .height(28)
                .ink(Ink::slot(4))
                .into(),
        ),
        "sleep" => ("7h 42m", Node::Empty),
        _ => ("+4%", Node::Empty),
    };
    GridTile::new(key, name).body(Stack::gap(1).children((
        Text::muted(name),
        el("strong").attr("data-figure", "").child(figure),
        chart,
    )))
}

fn grid(widget: &Widget) -> SortableGrid {
    SortableGrid::new("tiles", "Widget tiles")
        .post("/widgets/tiles")
        .columns(2)
        .tiles(widget.tiles.iter().map(|key| {
            let tile = grid_tile(key);
            if key == "health" { tile.fixed() } else { tile }
        }))
}

/// The forms that add a tile, each kind not on the widget yet; none when it is full.
fn adds(widget: &Widget) -> Element {
    let full = widget.tiles.len() >= widget.size.most_blocks();
    let hint = format!(
        "{} fits {}",
        match widget.size {
            TileSize::Small => "Small",
            TileSize::Large => "Large",
            _ => "Medium",
        },
        widget.size.most_blocks()
    );
    let buttons: Vec<Node> = KINDS
        .iter()
        .filter(|kind| !widget.tiles.iter().any(|t| t == kind.key))
        .map(|kind| {
            Button::outline(format!("+ {}", kind.name))
                .size(Size::Sm)
                .submit()
                .named("kind", kind.key)
                .disabled(full)
                .into()
        })
        .collect();
    el("form")
        .attr("id", "widget-adds")
        .attr("method", "post")
        .attr("action", "/widgets/add")
        .attr("data-ui-form", "")
        .children((
            Heading::h2("Add a tile"),
            Text::muted(if full {
                format!("Full · {hint}")
            } else {
                hint
            }),
            Row::gap(2).wrap().children(buttons),
        ))
}

fn picker(widget: &Widget) -> Element {
    let sizes: Vec<Node> = [TileSize::Small, TileSize::Medium, TileSize::Large]
        .into_iter()
        .map(|size| {
            let label = match size {
                TileSize::Small => "Small",
                TileSize::Large => "Large",
                _ => "Medium",
            };
            Button::outline(label)
                .size(Size::Sm)
                .submit()
                .named("size", size.name())
                .attr(
                    "aria-pressed",
                    if size == widget.size { "true" } else { "false" },
                )
                .into()
        })
        .collect();
    let accents: Vec<Node> = ACCENTS
        .iter()
        .map(|(key, label, hex)| {
            let dot = el("svg")
                .attr("viewBox", "0 0 16 16")
                .attr("width", "14")
                .attr("height", "14")
                .attr("aria-hidden", "true")
                .child(
                    el("circle")
                        .attr("cx", "8")
                        .attr("cy", "8")
                        .attr("r", "7")
                        .attr("fill", Rgb::hex(*hex).to_hex()),
                );
            Button::outline(Node::list((dot, *label)))
                .size(Size::Sm)
                .submit()
                .named("accent", *key)
                .attr(
                    "aria-pressed",
                    if *key == widget.accent {
                        "true"
                    } else {
                        "false"
                    },
                )
                .into()
        })
        .collect();
    el("div").attr("id", "widget-picker").children((
        el("form")
            .attr("method", "post")
            .attr("action", "/widgets/size")
            .attr("data-ui-form", "")
            .attr("aria-label", "Size")
            .child(Row::gap(2).wrap().children(sizes)),
        el("form")
            .attr("method", "post")
            .attr("action", "/widgets/accent")
            .attr("data-ui-form", "")
            .attr("aria-label", "Accent")
            .child(Row::gap(2).wrap().children(accents)),
    ))
}

fn editor(widget: &Widget) -> Element {
    el("div")
        .attr("id", "widget-editor")
        .child(Stack::gap(5).children((
            picker(widget),
            el("div").children((
                Heading::h2("Live preview · drag tiles to rearrange"),
                grid(widget),
            )),
            adds(widget),
        )))
}

/// The picture in dark and light, each address carrying the picture's tag so a change
/// is a new address.
fn preview(widget: &Widget) -> Element {
    let size = widget.size.name();
    let (w, h) = match widget.size {
        TileSize::Small => (176, 176),
        TileSize::Large => (224, 224),
        _ => (352, 165),
    };
    let pictures: Vec<Node> = [(Mode::Dark, "dark"), (Mode::Light, "light")]
        .into_iter()
        .map(|(mode, name)| {
            let tag = image_of(widget, &tile_of(widget), mode)
                .etag(&tile_of(widget).into())
                .unwrap_or_default();
            let version: String = tag
                .chars()
                .filter(char::is_ascii_hexdigit)
                .take(12)
                .collect();
            el("img")
                .attr("data-mode", name)
                .attr(
                    "src",
                    format!("/widgets/image.png?size={size}&mode={name}&v={version}"),
                )
                .attr("width", i64::from(w))
                .attr("height", i64::from(h))
                .attr("alt", format!("The widget, {name}"))
                .into()
        })
        .collect();
    el("div")
        .attr("id", "widget-preview")
        .child(Stack::gap(2).children((
            Heading::h2("Exactly what your phone shows"),
            Row::gap(3).wrap().children(pictures),
        )))
}

fn page(shell: &AppShell) -> AppPage {
    with(|widget| {
        shell.page("Widgets").body(
            Stack::gap(6).children((
                Text::muted("Design your widget: Simran Kaur's home screen."),
                Grid::ratio([7, 5])
                    .gap(6)
                    .attr("data-widget-builder", "")
                    .children((editor(widget), preview(widget))),
            )),
        )
    })
}

/// The answer to a change of the editor: its parts drawn again, or the page.
fn redrawn(widget: &Widget) -> Reply {
    Reply::ops_or_redirect("/widgets")
        .op(Op::replace("widget-editor", &editor(widget).into()))
        .op(Op::replace("widget-preview", &preview(widget).into()))
}

fn field(body: &str, name: &str) -> Option<String> {
    urlencoded::parse(body.as_bytes())
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.into_owned())
}

fn save(order: GridOrder) -> Response {
    with(|widget| {
        if widget.fail_next {
            widget.fail_next = false;
            return Reply::ops(vec![Op::toast(
                ToastTone::Danger,
                "That order wasn't saved. Try again.",
            )])
            .status(http::StatusCode::SERVICE_UNAVAILABLE)
            .into_response();
        }
        let arranged = order.keep(["health"]).arrange(&widget.tiles);
        widget.saves += 1;
        widget.last = arranged.order.join(",");
        widget.tiles = arranged.order;
        Reply::ops_or_redirect("/widgets#tiles")
            .op(Op::replace("widget-preview", &preview(widget).into()))
            .op(Op::replace("widget-adds", &adds(widget).into()))
            .into_response()
    })
}

fn add(body: &str) -> Reply {
    with(|widget| {
        if let Some(kind) = field(body, "kind")
            && KINDS.iter().any(|k| k.key == kind)
            && !widget.tiles.contains(&kind)
            && widget.tiles.len() < widget.size.most_blocks()
        {
            widget.tiles.push(kind);
        }
        redrawn(widget)
    })
}

fn resize(body: &str) -> Reply {
    with(|widget| {
        if let Some(size) = field(body, "size").as_deref().and_then(TileSize::from_name) {
            widget.size = size;
            widget.tiles.truncate(size.most_blocks());
        }
        redrawn(widget)
    })
}

fn recolour(body: &str) -> Reply {
    with(|widget| {
        if let Some(accent) = field(body, "accent")
            && let Some((key, ..)) = ACCENTS.iter().find(|(k, ..)| *k == accent)
        {
            widget.accent = key;
        }
        redrawn(widget)
    })
}

fn picture(parts: &Parts) -> Response {
    let query = parts.uri.query().unwrap_or_default();
    let size = field(query, "size")
        .as_deref()
        .and_then(TileSize::from_name);
    let mode = match field(query, "mode").as_deref() {
        Some("light") => Mode::Light,
        _ => Mode::Dark,
    };
    with(|widget| {
        let mut tile = tile_of(widget);
        if let Some(size) = size
            && size != widget.size
        {
            tile = Tile::new(size)
                .title("Simran's day")
                .note("Updated 07:30")
                .blocks(widget.tiles.iter().map(|key| block(key)));
        }
        image_of(widget, &tile, mode)
            .cache_control("private, no-cache")
            .respond(&tile.into(), &parts.headers)
    })
}

fn state() -> Response {
    with(|widget| {
        let body = format!(
            r#"{{"size":"{}","accent":"{}","tiles":[{}],"saves":{},"last":"{}"}}"#,
            widget.size.name(),
            widget.accent,
            widget
                .tiles
                .iter()
                .map(|t| format!("\"{t}\""))
                .collect::<Vec<_>>()
                .join(","),
            widget.saves,
            widget.last
        );
        ([(http::header::CONTENT_TYPE, "application/json")], body).into_response()
    })
}

pub struct WidgetPages;

impl Injectable for WidgetPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for WidgetPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/widgets", |shell: AppShell| async move {
                page(&shell)
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/widgets/tiles",
                |order: GridOrder| async move { save(order) },
            )
            .public(),
            RouteDef::on(Method::POST, "!/widgets/add", |body: String| async move {
                add(&body)
            })
            .public(),
            RouteDef::on(Method::POST, "!/widgets/size", |body: String| async move {
                resize(&body)
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/widgets/accent",
                |body: String| async move { recolour(&body) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/widgets/image.png",
                |parts: Parts| async move { picture(&parts) },
            )
            .public(),
            RouteDef::on(Method::GET, "!/widgets/state", || async move { state() }).public(),
            RouteDef::on(Method::POST, "!/widgets/reset", || async move {
                with(|widget| *widget = Widget::new());
                http::StatusCode::NO_CONTENT
            })
            .public(),
            RouteDef::on(Method::POST, "!/widgets/fail", || async move {
                with(|widget| widget.fail_next = true);
                http::StatusCode::NO_CONTENT
            })
            .public(),
        ]
    }
}
