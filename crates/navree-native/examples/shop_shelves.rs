//! A shop's shelves that need a hand, designed as a native screen: how many
//! shelves are empty, low and stocked, the emptiest shelves as a wall of
//! tiles, and what sold today.
//!
//! A table of products can't say this at a glance, so the screen is composed:
//! tinted cards with a number each, a grid that shows as many tiles as fit,
//! a mark on a soft ground with a dot at its corner, a run of cards that
//! scrolls across on a phone and is a list beside the wall in a wide room.
//! Every tile is still one control with a name, its menu is also in the
//! product it opens, and no colour says anything alone.
//!
//! `cargo run -p navree-native --example shop_shelves` prints the envelope a
//! client is sent. `corpus/screen-designed-store.json` is the same envelope,
//! held to this file by the crate's tests.

use navree_native::Envelope;
use navree_native::ui::*;

/// A shelf: a product, how many are left and the stock it reorders at.
struct Shelf {
    id: u32,
    name: &'static str,
    icon: (&'static str, &'static str),
    stock: u32,
    reorder_at: u32,
}

const SHELVES: [Shelf; 6] = [
    Shelf {
        id: 7,
        name: "Brass lota",
        icon: ("cup-soda", "M6 8h12l-1.5 12h-9zM5 4h14l-1 4H6z"),
        stock: 0,
        reorder_at: 12,
    },
    Shelf {
        id: 31,
        name: "Masala dabba, steel",
        icon: ("soup", "M4 12h16a8 8 0 0 1-16 0zM7 21h10M12 3v5"),
        stock: 0,
        reorder_at: 8,
    },
    Shelf {
        id: 12,
        name: "Khadi kurta, indigo",
        icon: (
            "shirt",
            "M8 3 3 6l2 4 2-1v11h10V9l2 1 2-4-5-3a4 4 0 0 1-8 0z",
        ),
        stock: 3,
        reorder_at: 10,
    },
    Shelf {
        id: 44,
        name: "Punjabi jutti, size 8",
        icon: (
            "footprints",
            "M4 16v-2.4C4 11 5 9 7 9s3 2 3 4.6V16zM14 12v-2.4C14 7 15 5 17 5s3 2 3 4.6V12z",
        ),
        stock: 4,
        reorder_at: 8,
    },
    Shelf {
        id: 18,
        name: "Phulkari dupatta",
        icon: (
            "flower",
            "M12 7.5a4.5 4.5 0 1 1 4.5 4.5M12 7.5A4.5 4.5 0 1 0 7.5 12M12 7.5V9M7.5 12a4.5 4.5 0 1 0 4.5 4.5M7.5 12H9M16.5 12a4.5 4.5 0 1 1-4.5 4.5M16.5 12H15M12 16.5V15",
        ),
        stock: 6,
        reorder_at: 12,
    },
    Shelf {
        id: 52,
        name: "Block-print bedsheet",
        icon: ("bed", "M2 4v16M2 8h18a2 2 0 0 1 2 2v10M2 17h20M6 8v9"),
        stock: 9,
        reorder_at: 15,
    },
];

/// What sold today: a product, how many, and what they brought in.
const SELLERS: [(&str, &str); 4] = [
    ("Amritsari papad", "31 sold · ₹4,340"),
    ("Phulkari dupatta", "14 sold · ₹27,860"),
    ("Punjabi jutti, size 8", "9 sold · ₹11,691"),
    ("Brass lota", "6 sold · ₹5,094"),
];

/// A shelf's level in its hue, its tone and a word: the word is always there,
/// so the colour never speaks alone.
fn level(shelf: &Shelf) -> (Hue, Tone, &'static str) {
    if shelf.stock == 0 {
        (Hue::Danger, Tone::Danger, "Out of stock")
    } else {
        (Hue::Warning, Tone::Warning, "Running low")
    }
}

/// How many shelves hold a level: a tinted card with the number set large. A
/// number an answer changes counts to its new value.
fn count(id: &str, shown: &str, label: &str, hue: Hue, weight: Weight) -> Card {
    Card::new().id(id).tint(hue).parts(
        Stack::across().gap(3).align(Align::Center).parts((
            Shape::capsule().fill(hue).size(Size::new(6, 36).scaled()),
            Stack::down().grow().parts((
                Text::new(shown)
                    .style(TextStyle::Title)
                    .weight(weight)
                    .change(Change::Count),
                Text::new(label)
                    .style(TextStyle::Caption)
                    .weight(Weight::Semibold)
                    .muted(),
            )),
        )),
    )
}

/// One shelf: a tile that opens the product where the reader is, grows into
/// it, and offers a restock on a long press.
fn tile(shelf: &Shelf) -> Card {
    let (hue, tone, word) = level(shelf);
    let left = match shelf.stock {
        0 => format!("None left · reorders at {}", shelf.reorder_at),
        stock => format!("{stock} left · reorders at {}", shelf.reorder_at),
    };
    let (name, path) = shelf.icon;
    Card::new()
        .id(format!("shelf-{}", shelf.id))
        .opens(
            format!("{}, {}", shelf.name, word.to_lowercase()),
            Action::request(format!("/inventory/{}/peek", shelf.id)),
        )
        .zoom()
        .appear(Appear::Scale)
        .menu((
            Button::new("Restock 24").post(
                format!("/inventory/{}/restock", shelf.id),
                [("quantity", "24")],
            ),
            Link::to(
                "Open in the inventory",
                format!("/inventory?search={}", shelf.id),
            ),
        ))
        .parts((
            Stack::across()
                .justify(Justify::Between)
                .align(Align::Start)
                .parts((
                    // The product's mark, with a dot of its level at its corner.
                    Layers::new().place(Place::BottomEnd).parts((
                        chip(Icon::named(name).path(path), hue),
                        Shape::circle()
                            .fill(hue)
                            .stroke(Hue::Neutral)
                            .size(Size::square(12))
                            .label(word),
                    )),
                    Badge::new(word).tone(tone),
                )),
            Text::new(shelf.name).style(TextStyle::Headline).lines(2),
            Text::new(left).style(TextStyle::Caption).muted(),
            Progress::of(f64::from(shelf.stock), f64::from(shelf.reorder_at))
                .label("Stock against the point it reorders at"),
        ))
}

/// What sold today, as a card of a run that scrolls across.
fn seller((name, sold): &(&str, &str)) -> Card {
    Card::new().pad(3).parts((
        Layers::new().size(Size::square(44).scaled()).parts((
            Shape::rounded().fill(Hue::Accent).soft(),
            Icon::named("trending-up")
                .system("chart.line.uptrend.xyaxis", "trending_up")
                .path("M22 7 13.5 15.5l-5-5L2 17M16 7h6v6")
                .scale(TextStyle::Title2)
                .colour(Hue::Accent),
        )),
        Text::new(*name)
            .style(TextStyle::Callout)
            .weight(Weight::Medium)
            .lines(2),
        Text::new(*sold).style(TextStyle::Caption).muted(),
    ))
}

/// The whole screen.
pub fn screen() -> Screen {
    let out = SHELVES.iter().filter(|shelf| shelf.stock == 0).count();
    let low = SHELVES.len() - out;
    let sellers_heading = || {
        Text::new("Top sellers today")
            .style(TextStyle::Title2)
            .weight(Weight::Bold)
            .heading(2)
    };
    // The key to the dots: a mark of each level's hue beside its words.
    let key = |hue: Hue, word: &str| {
        Stack::across().gap(1).align(Align::Center).parts((
            Shape::rect().fill(hue).size(Size::square(10).scaled()),
            Text::new(word).style(TextStyle::Caption).muted(),
        ))
    };
    let wall = Stack::down().gap(3).grow().parts((
        Text::new("The emptiest shelves")
            .style(TextStyle::Title2)
            .weight(Weight::Bold)
            .heading(2),
        Stack::across().gap(3).wrap().parts((
            key(Hue::Danger, "Out of stock"),
            key(Hue::Warning, "Running low"),
        )),
        Grid::fit(160)
            .gap(3)
            .id("wall")
            .parts(SHELVES.iter().map(tile)),
    ));
    // A run that scrolls across on a phone; a list beside the wall in a wide room.
    let sellers = Adaptive::new()
        .compact(
            Stack::down().gap(3).appear(Appear::Fade).parts((
                sellers_heading(),
                Carousel::new()
                    .min(148)
                    .gap(3)
                    .label("Top sellers today")
                    .parts(SELLERS.iter().map(seller)),
            )),
        )
        .wide(
            Section::titled("Top sellers today").inset().parts(
                List::new().parts(
                    SELLERS
                        .iter()
                        .map(|(name, sold)| Row::new(*name).subtitle(*sold)),
                ),
            ),
        );
    Screen::titled("Shelves")
        .back("Today", "/")
        .nav("/inventory")
        .refresh()
        .action(
            Button::new("Inventory")
                .icon(
                    Icon::named("boxes")
                        .system("shippingbox", "inventory_2")
                        .path("M3 7l9-4 9 4v10l-9 4-9-4zM3 7l9 4 9-4M12 11v10"),
                )
                .role(Role::Secondary)
                .to("/inventory"),
        )
        .body(
            Stack::down().id("shelves").gap(4).parts((
                // The one figure the screen is about.
                Stack::down().parts((
                    Text::new(SHELVES.len().to_string())
                        .id("needing")
                        .style(TextStyle::Display)
                        .change(Change::Count),
                    Text::new("shelves need a hand today")
                        .style(TextStyle::Callout)
                        .muted(),
                )),
                Grid::fit(104).most(3).gap(3).id("levels").parts((
                    count(
                        "level-out",
                        &out.to_string(),
                        "Out of stock",
                        Hue::Danger,
                        Weight::Bold,
                    ),
                    count(
                        "level-low",
                        &low.to_string(),
                        "Running low",
                        Hue::Warning,
                        Weight::Bold,
                    ),
                    count(
                        "level-in",
                        "1,994",
                        "Stocked",
                        Hue::Success,
                        Weight::Regular,
                    ),
                )),
                Card::new().id("next-delivery").appear(Appear::Rise).parts((
                    Text::new("Next delivery")
                        .style(TextStyle::Caption)
                        .weight(Weight::Semibold)
                        .muted(),
                    Text::new("Monday, 16 November · from Moradabad").style(TextStyle::Headline),
                    Text::new("Checked a minute ago")
                        .style(TextStyle::Caption)
                        .change(Change::Fade)
                        .align(Align::End)
                        .muted(),
                )),
                Stack::down()
                    .gap(4)
                    .wide(Axis::Horizontal)
                    .align(Align::Start)
                    .parts((wall, sellers)),
            )),
        )
}

/// The screen as a client is sent it: each icon's shapes said once, in the
/// envelope's table.
pub fn envelope(build: &str) -> Envelope {
    let mut envelope = Envelope::screen(build, screen());
    envelope.share_icons();
    envelope
}

#[allow(dead_code)]
fn main() {
    let envelope = envelope("3f9a1c07b2de");
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).unwrap_or_default()
    );
}
