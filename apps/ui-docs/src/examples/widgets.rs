//! A sortable grid of tiles, and charts drawn as pictures on the server: a home-screen
//! widget's tiles and the picture of the widget, with a day's health numbers.

use navree::ui::charts::*;
use navree::ui::image::{Block, ChartImage, Tile, TileSize};
use navree::ui::theme::{Mode, Rgb};
use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("widgets.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("sortable-grid", "thumb", "Widget tiles", grid_thumb)
            .start()
            .width(520)
            .height(260),
        single!(
            "sortable-grid",
            "default",
            "Tiles in two columns",
            grid_default
        )
        .start()
        .live()
        .height(360),
        single!("sortable-grid", "fit", "As many columns as fit", grid_fit)
            .start()
            .live()
            .height(300),
        single!("sortable-grid", "still", "With nowhere to post", grid_still)
            .start()
            .height(220),
        single!("chart-image", "thumb", "A widget's picture", image_thumb)
            .width(520)
            .height(260),
        gallery!(
            "chart-image",
            "default",
            "Small, medium and large",
            image_sizes
        )
        .height(320),
        gallery!(
            "chart-image",
            "modes",
            "Dark and light, two accents",
            image_modes
        )
        .height(280),
        single!("chart-image", "ring", "One ring, on its own", image_ring)
            .width(320)
            .height(280),
    ]
}

/// A tile in the grid: its name, its figure and a small ring.
fn tile(key: &str, name: &str, figure: &str, score: u32) -> GridTile {
    GridTile::new(key, name).body(
        Stack::gap(1).children((
            Text::muted(name.to_owned()),
            el("strong").child(figure.to_owned()),
            Ring::new(name.to_owned(), score)
                .size(40)
                .inline()
                .ink(Ink::accent()),
        )),
    )
}

fn grid_thumb() -> SortableGrid {
    SortableGrid::new("thumb-tiles", "Widget tiles")
        .post("/widgets/7/tiles")
        .columns(2)
        .tile(tile("recovery", "Recovery", "82%", 82))
        .tile(tile("sleep", "Sleep score", "88", 88))
}

fn grid_default() -> SortableGrid {
    SortableGrid::new("tiles", "Simran's widget")
        .post("/widgets/7/tiles")
        .columns(2)
        .tile(tile("health", "Health score", "78", 78).fixed())
        .tile(tile("recovery", "Recovery", "82%", 82))
        .tile(
            GridTile::new("sleep", "Sleep")
                .body(Stack::gap(1).children((Text::muted("Sleep"), el("strong").child("7h 42m")))),
        )
        .tile(
            GridTile::new("resting-hr", "Resting HR").body(
                Stack::gap(1).children((
                    Text::muted("Resting HR"),
                    el("strong").child("58 bpm"),
                    Sparkline::new("Resting HR", [62, 61, 63, 60, 59, 61, 58])
                        .height(28)
                        .ink(Ink::accent()),
                )),
            ),
        )
}

fn grid_fit() -> SortableGrid {
    let names = [
        "Steps",
        "Water",
        "Mindful minutes",
        "Weight",
        "Calories",
        "Stand hours",
    ];
    SortableGrid::new("fit-tiles", "Today's figures")
        .post("/today/tiles")
        .fit(GridFit::Narrow)
        .autosave_ms(800)
        .tiles(names.iter().enumerate().map(|(at, name)| {
            GridTile::new(format!("t{at}"), *name).body(Stack::gap(1).children((
                Text::muted(*name),
                el("strong").child(["8,432", "2.1 L", "18 min", "61.4 kg", "1,840", "9 of 12"][at]),
            )))
        }))
}

fn grid_still() -> SortableGrid {
    SortableGrid::new("still-tiles", "Gurpreet's tiles")
        .columns(3)
        .tile(GridTile::new("a", "Recovery"))
        .tile(GridTile::new("b", "Sleep"))
        .tile(GridTile::new("c", "Movement"))
}

/// The widget every picture here draws: Simran's day, in blocks.
fn day(size: TileSize) -> Tile {
    Tile::new(size)
        .title("Simran's day")
        .note("Updated 07:30")
        .block(
            Block::ring("Recovery", 82, 100)
                .unit("%")
                .note("Well rested"),
        )
        .block(
            Block::sparkline("Resting HR", [62, 61, 63, 60, 59, 61, 58])
                .unit("bpm")
                .note("-3 vs 7-day avg")
                .tone(Tone::Success),
        )
        .block(
            Block::stat("Sleep", "7h 42m")
                .note("+18 min vs 7-day avg")
                .tone(Tone::Success),
        )
        .block(
            Block::columns("24h stress", [22, 35, 48, 30, 26, 18, 40, 52, 33, 21])
                .value("31")
                .ink(Ink::slot(4)),
        )
}

/// The picture of `tile` as an inline drawing: what the PNG is drawn from.
fn picture(tile: Tile, mode: Mode, accent: u32) -> Node {
    ChartImage::for_tile(&tile)
        .mode(mode)
        .accent(Rgb::hex(accent))
        .svg_node(&tile.into())
        .unwrap_or_default()
}

fn image_thumb() -> Node {
    picture(day(TileSize::Medium), Mode::Dark, 0x22d3ee)
}

fn image_sizes() -> Vec<(&'static str, Node)> {
    vec![
        (
            "Small, 474 × 474",
            picture(day(TileSize::Small), Mode::Dark, 0x22d3ee),
        ),
        (
            "Medium, 1014 × 474",
            picture(day(TileSize::Medium), Mode::Dark, 0x22d3ee),
        ),
        (
            "Large, 1014 × 1014",
            picture(day(TileSize::Large), Mode::Dark, 0x22d3ee),
        ),
    ]
}

fn image_modes() -> Vec<(&'static str, Node)> {
    vec![
        (
            "Dark, green",
            picture(day(TileSize::Medium), Mode::Dark, 0x34d399),
        ),
        (
            "Light, purple",
            picture(day(TileSize::Medium), Mode::Light, 0xa78bfa),
        ),
    ]
}

fn image_ring() -> Node {
    let ring = Ring::new("Sleep score", 88).fill(Gradient::sheen(Ink::slot(7)));
    ChartImage::new(240, 240)
        .mode(Mode::Light)
        .svg_node(&ring.into())
        .unwrap_or_default()
}
