//! A health app's day, designed as a native screen and written once: a score
//! with its status and its trend, the pillars it is made of in a grid that
//! shows as many as fit, the day round a clock, the activity rings beside the
//! day's figures, and the control that steps from day to day in the bar.
//!
//! The screen is a function of its data, built with `navree_native::ui`.
//! SwiftUI draws it on Apple systems and Jetpack Compose on Android.
//!
//! `cargo run -p navree-native --example health_today` prints the envelope a
//! client is sent. `corpus/screen-designed-health.json` is the same envelope,
//! held to this file by the crate's tests.

use navree_native::Envelope;
use navree_native::ui::*;

/// One pillar of the score: what it is, where it leads, its score out of 100
/// when the day has a reading, its colour and how much of the score it is.
struct Pillar {
    name: &'static str,
    key: &'static str,
    score: Option<u8>,
    hue: Hue,
    weight: &'static str,
}

const PILLARS: [Pillar; 5] = [
    Pillar {
        name: "Sleep",
        key: "sleep",
        score: Some(87),
        hue: Hue::Violet,
        weight: "25% of score",
    },
    Pillar {
        name: "Heart",
        key: "heart",
        score: Some(90),
        hue: Hue::Rose,
        weight: "20% of score",
    },
    Pillar {
        name: "Recovery",
        key: "recovery",
        score: Some(60),
        hue: Hue::Green,
        weight: "20% of score",
    },
    Pillar {
        name: "Movement",
        key: "fitness",
        score: Some(78),
        hue: Hue::Orange,
        weight: "20% of score",
    },
    Pillar {
        name: "Mind",
        key: "mind",
        score: None,
        hue: Hue::Cyan,
        weight: "No data",
    },
];

/// The steps of each hour so far: it is 8 in the evening.
const STEPS: [u32; 21] = [
    0, 0, 0, 0, 0, 0, 120, 1850, 940, 520, 380, 610, 760, 300, 280, 350, 600, 1200, 940, 420, 142,
];

fn steps_icon() -> Icon {
    Icon::named("footprints")
        .system("figure.walk", "directions_walk")
        .path("M4 16v-2.4C4 11 5 9 7 9s3 2 3 4.6V16zM14 12v-2.4C14 7 15 5 17 5s3 2 3 4.6V12z")
}

/// A heading inside a card.
fn heading(words: &str) -> Text {
    Text::new(words)
        .style(TextStyle::Title2)
        .weight(Weight::Bold)
        .heading(2)
}

/// The control in the bar: the day, its neighbours, and its three scores.
fn pager() -> DayPager {
    DayPager::on("Today", "2026-11-13")
        .id("day")
        .previous(Some("/?date=2026-11-12".to_owned()))
        .pick("2026-01-02", "2026-11-13", "/", "date")
        .words("Previous day", "Next day", "Choose a day")
        .scores(
            Rings::new()
                .id("day-scores")
                .ring("Movement", 78, 100, "", Hue::Orange)
                .ring("Recovery", 60, 100, "", Hue::Green)
                .ring("Sleep", 87, 100, "", Hue::Violet)
                .small()
                .summary("Today's scores: Movement 78, Recovery 60, Sleep 87."),
        )
}

/// The score the screen is about: the ring above its words on a phone, beside
/// them in a wide room.
fn score() -> Card {
    let ring = Ring::of(83, 100)
        .id("health-score")
        .label("Health score")
        .hero()
        .colour(Colour::blend(Hue::Cyan, Hue::Violet))
        .summary("Health score: 83 of 100, 83%.")
        .hint(
            Hint::about(
                "the health score",
                "Your health score blends five pillars, each scored from 0 to 100 from your own readings. A pillar with no data that day is left out and the rest are weighed up. It describes a pattern in your readings; it is not advice.",
            )
            .title("How the score is made"),
        );
    Card::new().id("score").parts(
        Stack::down()
            .gap(4)
            .align(Align::Center)
            .wide(Axis::Horizontal)
            .parts((
                ring,
                Stack::down().gap(2).grow().parts((
                    Stack::across().gap(2).align(Align::Center).wrap().parts((
                        Badge::new("Excellent").tone(Tone::Success),
                        Text::new("2.4% lower than your last 7 days")
                            .style(TextStyle::Callout)
                            .muted(),
                    )),
                    Text::new("Five pillars, each scored from 0 to 100 from your own readings.")
                        .style(TextStyle::Callout)
                        .muted(),
                )),
            )),
    )
}

/// One pillar: a card that is tapped as a whole and grows into its page.
fn pillar(pillar: &Pillar) -> Card {
    let Pillar {
        name, key, weight, ..
    } = pillar;
    let ring = match pillar.score {
        Some(score) => {
            Ring::of(score, 100).summary(format!("{name} score: {score} of 100, {score}%."))
        }
        None => Ring::empty(100).summary(format!("{name} score: no reading.")),
    };
    Card::new()
        .id(format!("pillar-{key}"))
        .opens(
            format!("{name}, {weight}"),
            Action::navigate(format!("/{key}")),
        )
        .zoom()
        .parts(
            Stack::down().gap(1).align(Align::Center).parts((
                ring.id(format!("{key}-score")).colour(pillar.hue),
                Text::new(*name)
                    .style(TextStyle::Headline)
                    .align(Align::Center),
                Text::new(*weight)
                    .style(TextStyle::Caption)
                    .align(Align::Center)
                    .muted(),
            )),
        )
}

/// The day as 24 hours round a dial: the steps of each hour, what each hour
/// was, and the moments that stand out.
fn day() -> Card {
    // 0 active, 1 calm, 2 rest: the hours of the night are rest.
    let kind = |hour: usize, steps: u32| match (hour, steps) {
        (0..=5, _) => 2,
        (_, 900..) => 0,
        _ => 1,
    };
    let hours = (0..24).map(|hour| match STEPS.get(hour) {
        Some(steps) => Hour::of(
            *steps,
            (f64::from(*steps) / 2000.0 * 20.0).round() / 20.0,
            kind(hour, *steps),
        ),
        None => Hour::default(),
    });
    let total: u32 = STEPS.iter().sum();
    let clock = DayClock::of(hours)
        .id("day-clock")
        .title("Steps by hour")
        .key("Active", Hue::Green)
        .key("Calm", Hue::Cyan)
        .key("Rest", Hue::Violet)
        .moment(
            6.5,
            "Woke up, 6:30 am",
            Icon::named("sunrise")
                .system("sunrise", "wb_twilight")
                .path("M12 2v6M4.9 10.9l1.4 1.4M2 18h2M20 18h2M17.7 12.3l1.4-1.4M22 22H2M16 18a4 4 0 0 0-8 0"),
            Hue::Amber,
        )
        .moment(7.25, "Morning run, 3.2 km", steps_icon(), Hue::Green)
        .moment(
            18.4,
            "Highest heart rate, 158 bpm",
            Icon::named("heart-pulse")
                .system("heart.text.square", "monitor_heart")
                .path("M19.5 12.6 12 20l-7.5-7.4a5 5 0 0 1 7.5-6.6 5 5 0 0 1 7.5 6.6zM3 12h4l2-3 3 6 2-3h5"),
            Hue::Rose,
        )
        .now(20)
        .total("9,412", "steps", "Steps")
        .summary(format!(
            "9,412 steps in the day so far ({total} counted by the hour). Busiest at 7 am: 1,850. 3 moments: Woke up, 6:30 am, Morning run, 3.2 km, Highest heart rate, 158 bpm."
        ));
    Card::new()
        .id("day-card")
        .parts((heading("Your day"), clock))
}

/// The activity rings beside the day's figures. The figures take the room
/// the rings leave: one column on a phone, more in a wide room.
fn activity() -> Card {
    let rings = Rings::new()
        .id("activity-rings")
        .title("Activity rings")
        .ring("Move", 540, 600, "kcal", Hue::Red)
        .ring("Exercise", 38, 30, "min", Hue::Green)
        .ring("Stand", 9, 12, "hr", Hue::Cyan)
        .told()
        .summary("Activity rings: Move 540 of 600 kcal, 90%. Exercise 38 of 30 min, 127%. Stand 9 of 12 hr, 75%.");
    let figures = Grid::fit(132).gap(3).id("activity-figures").parts((
        Figure::new("Steps", "9,412")
            .id("figure-steps")
            .icon(steps_icon(), Hue::Green)
            .detail("94% of 10,000")
            .aside(Ring::of(9412, 10_000).small().colour(Hue::Green).summary("Steps: 9,412 of 10,000, 94%."))
            .to("/fitness/steps"),
        Figure::new("Active energy", "540")
            .id("figure-energy")
            .unit("kcal")
            .icon(
                Icon::named("flame")
                    .system("flame", "local_fire_department")
                    .path("M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.4-.5-2-1-3-1.1-2.3.1-4.4 2-5.5.3 2.4 2 4 3.6 5.4 1.4 1.3 2.4 3 2.4 5.1a6 6 0 0 1-12 0c0-.9.3-1.8.5-2.5"),
                Hue::Orange,
            )
            .detail("90% of 600 kcal")
            .to("/fitness/energy"),
        Figure::new("Exercise", "38")
            .id("figure-exercise")
            .unit("min")
            .icon(
                Icon::named("timer")
                    .system("timer", "timer")
                    .path("M10 2h4M12 14l3-3M12 22a8 8 0 1 0 0-16 8 8 0 0 0 0 16z"),
                Hue::Green,
            )
            .trend(Direction::Up, "8 min", true)
            .detail("against your goal of 30")
            .status(Badge::new("Goal reached").tone(Tone::Success))
            .to("/fitness/exercise"),
    ));
    Card::new().id("activity").parts((
        heading("Activity"),
        Stack::across()
            .gap(4)
            .align(Align::Center)
            .parts((rings, figures)),
    ))
}

/// The whole day.
pub fn screen() -> Screen {
    Screen::titled("Today")
        .nav("/")
        .refresh()
        .pager(pager())
        .body(
            Stack::down().id("today").gap(4).parts((
                Text::new("Good evening, Amandeep")
                    .style(TextStyle::LargeTitle)
                    .heading(1),
                Text::new("Your Friday, 13 November, from your own readings.").muted(),
                score(),
                Grid::fit(96)
                    .most(5)
                    .gap(3)
                    .id("pillars")
                    .parts(PILLARS.iter().map(pillar)),
                day(),
                activity(),
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
