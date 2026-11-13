//! Rings, concentric rings, gauges and the day clock: health and fitness numbers for a
//! day, the way a tracker shows them.
//!
//! Large counts are written in lakh grouping (`2_84_500`), as the pages print them.
#![allow(clippy::inconsistent_digit_grouping)]

use navree::ui::charts::*;
use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("radial.rs");

pub fn all() -> Vec<Example> {
    vec![
        // Ring
        single!("ring", "thumb", "Rings for three scores", ring_thumb)
            .start()
            .width(520)
            .height(200),
        single!("ring", "default", "A score with its label", ring_default)
            .start()
            .height(300),
        gallery!(
            "ring",
            "paint",
            "Solid, gradient, halo and glow",
            ring_paint
        )
        .height(250),
        single!("ring", "colors", "Colour by band", ring_colors)
            .start()
            .width(820)
            .height(250),
        gallery!("ring", "middle", "What the middle holds", ring_middle).height(230),
        single!("ring", "goal", "Toward a goal, and past it", ring_goal)
            .start()
            .width(820)
            .height(250),
        gallery!(
            "ring",
            "ends",
            "Every chart colour's sheen and second lap",
            ring_ends
        )
        .height(200),
        single!("ring", "sizes", "From 56 to 200 pixels", ring_sizes)
            .start()
            .width(820)
            .height(300),
        single!("ring", "states", "Loading, failed and empty", ring_states)
            .start()
            .width(820)
            .height(230),
        // Rings
        single!("rings", "thumb", "Activity rings", rings_thumb)
            .start()
            .width(420)
            .height(220),
        single!(
            "rings",
            "default",
            "Three rings with a legend",
            rings_default
        )
        .start()
        .height(320),
        single!("rings", "two", "Two rings and an icon", rings_two)
            .start()
            .height(300),
        single!("rings", "five", "Five scores of a day", rings_five)
            .start()
            .height(320),
        single!(
            "rings",
            "beside",
            "Beside their words, in a row that sizes to its content",
            rings_beside
        )
        .start()
        .height(240),
        single!("rings", "pill", "Tiny rings in a date pill", rings_pill)
            .start()
            .width(520)
            .height(120),
        // Gauge
        single!("gauge", "thumb", "A gauge", gauge_thumb)
            .start()
            .width(420)
            .height(220),
        single!("gauge", "default", "A 270 degree gauge", gauge_default)
            .start()
            .height(300),
        gallery!("gauge", "sweeps", "Other sweeps and scales", gauge_sweeps).height(240),
        single!("gauge", "zones", "Zones on the track", gauge_zones)
            .start()
            .height(330),
        // Day clock
        single!(
            "day-clock",
            "thumb",
            "A day round the clock",
            day_clock_thumb
        )
        .start()
        .width(420)
        .height(360),
        single!(
            "day-clock",
            "default",
            "Hours, moods and moments",
            day_clock_default
        )
        .start()
        .height(560),
        single!(
            "day-clock",
            "scale",
            "Colour by a scale of values",
            day_clock_scale
        )
        .start()
        .height(520),
        single!(
            "day-clock",
            "band",
            "A band of categories, asleep hours too",
            day_clock_band
        )
        .start()
        .height(560),
    ]
}

fn ring_thumb() -> Row {
    Row::gap(6).children((
        Ring::new("Sleep score", 82)
            .id("thumb-sleep")
            .label("Sleep")
            .fill(Gradient::sheen(Ink::slot(7)))
            .size(140),
        Ring::new("Recovery", 74)
            .id("thumb-recovery")
            .label("Recovery")
            .fill(Gradient::sheen(Ink::slot(3)))
            .size(140),
        Ring::new("Movement", 61)
            .id("thumb-movement")
            .label("Movement")
            .fill(Gradient::sheen(Ink::slot(2)))
            .size(140),
    ))
}

fn ring_default() -> Card {
    Card::titled("Harleen Kaur · last night").body(
        Ring::new("Sleep score", 82)
            .id("ring-sleep")
            .label("Sleep score")
            .sublabel("Great recovery")
            .fill(Gradient::sheen(Ink::slot(7)))
            .glow()
            .size(200)
            .motion(Motion::new().delay(200)),
    )
}

fn ring_paint() -> Vec<(&'static str, Node)> {
    vec![
        (
            "Solid",
            Ring::new("Heart score", 78)
                .id("paint-solid")
                .label("Heart")
                .ink(Ink::slot(8))
                .size(150)
                .into(),
        ),
        (
            "Two-stop gradient",
            Ring::new("Mind score", 69)
                .id("paint-sheen")
                .label("Mind")
                .fill(Gradient::sheen(Ink::tone(Tone::named("cyan"))))
                .size(150)
                .into(),
        ),
        (
            "Five stops, halo, glow",
            Ring::new("Health score", 92)
                .id("paint-rainbow")
                .label("Health")
                .fill(
                    Gradient::diagonal()
                        .stop(0.0, Ink::tone(Tone::named("cyan")))
                        .stop(0.25, Ink::slot(3))
                        .stop(0.5, Ink::slot(7))
                        .stop(0.75, Ink::slot(5))
                        .stop(1.0, Ink::slot(2)),
                )
                .halo()
                .glow()
                .size(150)
                .into(),
        ),
        (
            "Tinted track",
            Ring::new("Movement score", 64)
                .id("paint-tinted")
                .label("Movement")
                .ink(Ink::slot(2))
                .track(RingTrack::Tinted)
                .size(150)
                .into(),
        ),
    ]
}

fn ring_colors() -> Row {
    let zones = || {
        ColorScale::new(Ink::tone(Tone::Danger))
            .label("Low")
            .from(34, Ink::tone(Tone::Warning))
            .label("Fair")
            .from(67, Ink::tone(Tone::Success))
            .label("Recovered")
    };
    Row::gap(8).wrap().children((
        Ring::new("Recovery, Gurpreet Singh", 87)
            .id("zone-high")
            .label("Recovery")
            .sublabel("Recovered")
            .unit("%")
            .colors(zones())
            .glow()
            .size(160),
        Ring::new("Recovery, Simran Kaur", 52)
            .id("zone-mid")
            .label("Recovery")
            .sublabel("Fair")
            .unit("%")
            .colors(zones())
            .glow()
            .size(160),
        Ring::new("Recovery, Arjun Sharma", 23)
            .id("zone-low")
            .label("Recovery")
            .sublabel("Low")
            .unit("%")
            .colors(zones())
            .glow()
            .size(160),
    ))
}

fn ring_middle() -> Vec<(&'static str, Node)> {
    vec![
        (
            "A value with its unit",
            Ring::new("Blood oxygen", 97)
                .id("middle-unit")
                .unit("%")
                .label("SpO2")
                .ink(Ink::tone(Tone::named("cyan")))
                .size(140)
                .into(),
        ),
        (
            "A value toward a goal",
            Ring::new("Water, goal 2.5 litres", 1.8)
                .id("middle-scale")
                .max(2.5)
                .unit("L")
                .label("Water")
                .ink(Ink::slot(1))
                .size(140)
                .into(),
        ),
        (
            "An icon",
            Ring::new("Active minutes", 45)
                .id("middle-icon")
                .max(60)
                .center_icon(icons::FLAME)
                .ink(Ink::slot(2))
                .size(140)
                .into(),
        ),
        (
            "Your own words",
            Ring::new("VO2 max 48.2, top 8% for age", 92)
                .id("middle-own")
                .center(
                    Stack::gap(0)
                        .align(Align::Center)
                        .children((Text::new("Top 8%"), Text::muted("for your age"))),
                )
                .ink(Ink::slot(7))
                .size(140)
                .into(),
        ),
    ]
}

fn ring_goal() -> Row {
    Row::gap(8).wrap().children((
        Ring::new("Steps, Rohit Malhotra", 6_420)
            .id("goal-under")
            .max(10_000)
            .locale("en-IN")
            .label("Steps")
            .sublabel("Goal 10,000")
            .fill(Gradient::sheen(Ink::slot(3)))
            .size(160),
        Ring::new("Steps, Priya Verma", 12_450)
            .id("goal-over")
            .max(10_000)
            .locale("en-IN")
            .label("Steps")
            .sublabel("125% of goal")
            .fill(Gradient::sheen(Ink::slot(3)))
            .size(160),
        Ring::new("Steps this month, Neha Kapoor", 2_84_500)
            .id("goal-lakh")
            .max(3_00_000)
            .locale("en-IN")
            .label("Steps")
            .sublabel("This month")
            .fill(Gradient::sheen(Ink::slot(3)))
            .size(160),
    ))
}

fn ring_sizes() -> Row {
    Row::gap(6).wrap().align(Align::Center).children((
        Ring::new("Recovery at 56 pixels", 72)
            .id("size-56")
            .label("Recovery")
            .ink(Ink::slot(3))
            .size(56),
        Ring::new("Recovery at 64 pixels", 72)
            .id("size-64")
            .label("Recovery")
            .ink(Ink::slot(3))
            .size(64),
        Ring::new("Recovery at 96 pixels", 72)
            .id("size-96")
            .label("Recovery")
            .ink(Ink::slot(3))
            .size(96),
        Ring::new("Recovery at 128 pixels, 160 on wider screens", 72)
            .id("size-128")
            .label("Recovery")
            .ink(Ink::slot(3))
            .size(128)
            .size_on(Screen::Lg, 160),
        Ring::new("Recovery at 200 pixels", 72)
            .id("size-200")
            .label("Recovery")
            .sublabel("Good")
            .ink(Ink::slot(3))
            .size(200),
    ))
}

fn ring_states() -> Row {
    Row::gap(6).children((
        Ring::new("Sleep score", 0)
            .id("state-loading")
            .size(140)
            .loading(),
        Ring::new("Sleep score", 0)
            .id("state-failed")
            .size(140)
            .failed(Some("The tracker did not sync".into())),
        Ring::new("Sleep score", None::<f64>)
            .id("state-empty")
            .size(140)
            .empty(
                "No sleep yet",
                Some("It shows after the first night.".into()),
            ),
    ))
}

fn rings_thumb() -> Rings {
    Rings::new("Activity")
        .id("rings-thumb")
        .ring(RingValue::new("Move", 540).max(600).ink(Ink::slot(8)))
        .ring(RingValue::new("Exercise", 38).max(30).ink(Ink::slot(3)))
        .ring(
            RingValue::new("Stand", 9)
                .max(12)
                .ink(Ink::tone(Tone::named("cyan"))),
        )
        .track(RingTrack::Tinted)
        .size(180)
}

fn rings_default() -> Card {
    Card::titled("Manpreet Gill · today").body(
        Rings::new("Activity")
            .id("rings-activity")
            .ring(
                RingValue::new("Move", 540)
                    .max(600)
                    .unit("kcal")
                    .fill(Gradient::sheen(Ink::slot(8))),
            )
            .ring(
                RingValue::new("Exercise", 38)
                    .max(30)
                    .unit("min")
                    .fill(Gradient::sheen(Ink::slot(3))),
            )
            .ring(
                RingValue::new("Stand", 9)
                    .max(12)
                    .unit("hr")
                    .fill(Gradient::sheen(Ink::tone(Tone::named("cyan")))),
            )
            .track(RingTrack::Tinted)
            .center_values()
            .legend()
            .glow()
            .size(220)
            .motion(Motion::new().delay(150)),
    )
}

fn rings_two() -> Card {
    Card::titled("Nervous system · Jaspreet Dhillon").body(
        Rings::new("Autonomic balance")
            .id("rings-balance")
            .ring(
                RingValue::new("Sympathetic", 42)
                    .unit("%")
                    .fill(Gradient::sheen(Ink::slot(8)))
                    .track(RingTrack::Tinted),
            )
            .ring(
                RingValue::new("Parasympathetic", 68)
                    .unit("%")
                    .fill(Gradient::sheen(Ink::tone(Tone::named("cyan"))))
                    .track(RingTrack::Tinted),
            )
            .center_icon(icons::ACTIVITY)
            .legend()
            .size(160),
    )
}

fn rings_pill() -> Row {
    let scores = |id: &str, size: u32| {
        Rings::new("Movement 35, recovery 72, sleep 78")
            .id(id.to_owned())
            .ring(RingValue::new("Movement", 35).ink(Ink::slot(2)))
            .ring(RingValue::new("Recovery", 72).ink(Ink::slot(3)))
            .ring(RingValue::new("Sleep", 78).ink(Ink::slot(7)))
            .size(size)
            .inline()
    };
    Row::gap(4).align(Align::Center).children((
        Button::outline(Node::list((scores("pill-28", 28), "Wed, 11 Nov")))
            .icon_end(icons::CALENDAR),
        Button::outline(Node::list((scores("pill-36", 36), "Yesterday")))
            .icon_end(icons::CALENDAR)
            .size(Size::Lg),
    ))
}

fn gauge_thumb() -> Gauge {
    Gauge::new("Relaxation", 73)
        .id("gauge-thumb")
        .label("Recovery index")
        .fill(
            Gradient::horizontal()
                .stop(0.0, Ink::slot(3))
                .stop(1.0, Ink::tone(Tone::named("cyan"))),
        )
        .size(180)
}

fn gauge_default() -> Card {
    Card::titled("Stress rhythm · Kavya Bhatia").body(
        Gauge::new("Relaxation", 73)
            .id("gauge-relax")
            .label("Recovery index")
            .sublabel("Calm")
            .fill(
                Gradient::horizontal()
                    .stop(0.0, Ink::slot(3))
                    .stop(1.0, Ink::tone(Tone::named("cyan"))),
            )
            .glow()
            .size(200)
            .motion(Motion::new()),
    )
}

fn gauge_sweeps() -> Vec<(&'static str, Node)> {
    vec![
        (
            "Half a circle, a scale from 34",
            Gauge::new("Body temperature", 36.7)
                .id("sweep-half")
                .min(34)
                .max(39)
                .format(NumberFormat::decimals(1))
                .unit("°C")
                .label("Temperature")
                .sweep(180.0)
                .ink(Ink::slot(2))
                .size(170)
                .into(),
        ),
        (
            "240 degrees, colour by band",
            Gauge::new("Stress", 64)
                .id("sweep-stress")
                .label("Stress")
                .sublabel("Moderate")
                .sweep(240.0)
                .colors(
                    ColorScale::new(Ink::tone(Tone::Success))
                        .label("Relaxed")
                        .from(26, Ink::tone(Tone::named("cyan")))
                        .label("Balanced")
                        .from(51, Ink::tone(Tone::Warning))
                        .label("Moderate")
                        .from(76, Ink::tone(Tone::Danger))
                        .label("Elevated"),
                )
                .size(170)
                .into(),
        ),
        (
            "Small, with a tinted track",
            Gauge::new("Training load", 1.3)
                .id("sweep-small")
                .max(2)
                .format(NumberFormat::decimals(1))
                .label("Load")
                .ink(Ink::slot(4))
                .track(RingTrack::Tinted)
                .size(110)
                .into(),
        ),
    ]
}

/// Steps in each hour of Ravneet Kaur's day, from midnight.
const STEPS: [u32; 24] = [
    0, 0, 0, 0, 0, 30, 640, 1_850, 920, 410, 380, 520, 760, 310, 280, 350, 620, 1_240, 940, 430,
    260, 120, 30, 0,
];

fn day_clock_thumb() -> DayClock {
    DayClock::new("Steps by hour")
        .id("clock-thumb")
        .values(STEPS)
        .unit("steps")
        .locale("en-IN")
        .size(280)
}

fn day_clock_default() -> Card {
    Card::titled("Ravneet Kaur · Wednesday").body(
        DayClock::new("Steps and mood by hour")
            .id("clock-day")
            .hide_title()
            .values(STEPS)
            .unit("steps")
            .category("Great", Ink::slot(3))
            .category("Good", Ink::tone(Tone::named("cyan")))
            .category("Neutral", Ink::muted())
            .category("Stressed", Ink::slot(8))
            .category("Rest", Ink::slot(7))
            .categories([
                4, 4, 4, 4, 4, 4, 1, 0, 0, 1, 2, 1, 1, 3, 3, 2, 1, 0, 0, 1, 2, 4, 4, 4,
            ])
            .moment(
                Moment::new(6.5, "Woke up")
                    .icon(icons::SUNRISE)
                    .ink(Ink::slot(4)),
            )
            .moment(
                Moment::new(7.5, "Morning walk")
                    .icon(icons::FOOTPRINTS)
                    .ink(Ink::slot(3)),
            )
            .moment(
                Moment::new(13.5, "Lunch")
                    .icon(icons::UTENSILS)
                    .ink(Ink::slot(2)),
            )
            .moment(
                Moment::new(14.5, "Long meeting")
                    .icon(icons::BRAIN)
                    .ink(Ink::slot(8)),
            )
            .moment(
                Moment::new(17.5, "Badminton")
                    .icon(icons::FLAME)
                    .ink(Ink::slot(2)),
            )
            .moment(
                Moment::new(22.5, "Asleep")
                    .icon(icons::MOON)
                    .ink(Ink::slot(7)),
            )
            .now(15)
            .label("Steps")
            .center_icon(icons::FOOTPRINTS)
            .locale("en-IN")
            .motion(Motion::new()),
    )
}

fn day_clock_scale() -> Card {
    Card::titled("Aditya Arora · active minutes").body(
        DayClock::new("Active minutes by hour")
            .id("clock-scale")
            .hide_title()
            .values([
                0, 0, 0, 0, 0, 0, 12, 38, 22, 6, 4, 8, 10, 3, 2, 5, 14, 46, 30, 9, 4, 1, 0, 0,
            ])
            .max(60)
            .unit("min")
            .colors(
                ColorScale::new(Ink::slot(1))
                    .label("Light")
                    .from(15, Ink::slot(4))
                    .label("Moderate")
                    .from(30, Ink::slot(8))
                    .label("Vigorous"),
            )
            .moment(
                Moment::new(7.25, "Cycling")
                    .icon(icons::BIKE)
                    .ink(Ink::slot(4)),
            )
            .moment(
                Moment::new(17.25, "Gym")
                    .icon(icons::DUMBBELL)
                    .ink(Ink::slot(8)),
            )
            .now(20)
            .label("Active min")
            .hub(Ink::slot(4)),
    )
}

fn ring_ends() -> Vec<(&'static str, Node)> {
    // Past the goal, so each ring shows its sheen's far end and its second lap: the
    // colour deeper on a light page and brighter on a dark one, never brown.
    (1..=8)
        .map(|slot: u8| {
            let caption: &'static str = [
                "Chart 1", "Chart 2", "Chart 3", "Chart 4", "Chart 5", "Chart 6", "Chart 7",
                "Chart 8",
            ][usize::from(slot - 1)];
            (
                caption,
                Ring::new(caption, 128)
                    .id(format!("ring-end-{slot}"))
                    .unit("%")
                    .fill(Gradient::sheen(Ink::slot(slot)))
                    .size(112)
                    .into(),
            )
        })
        .collect()
}

fn rings_five() -> Card {
    Card::titled("Harleen Sandhu · today").body(
        Rings::new("Today's scores")
            .id("rings-five")
            .ring(RingValue::new("Sleep", 84).fill(Gradient::sheen(Ink::slot(7))))
            .ring(RingValue::new("Recovery", 71).fill(Gradient::sheen(Ink::slot(3))))
            .ring(RingValue::new("Movement", 58).fill(Gradient::sheen(Ink::slot(2))))
            .ring(RingValue::new("Stress", 36).fill(Gradient::sheen(Ink::slot(4))))
            .ring(RingValue::new("Mind", 66).fill(Gradient::sheen(Ink::slot(1))))
            .track(RingTrack::Tinted)
            .center_values()
            .legend()
            .size(220)
            .motion(Motion::new().delay(150)),
    )
}

fn rings_beside() -> Card {
    // The row sizes each item to its content: the rings take their drawing's width.
    Card::titled("Karan Bedi · this week").body(
        Row::gap(6).children((
            Rings::new("This week")
                .id("rings-beside")
                .ring(
                    RingValue::new("Move", 4_820)
                        .max(4_200)
                        .unit("kcal")
                        .ink(Ink::slot(8)),
                )
                .ring(
                    RingValue::new("Exercise", 190)
                        .max(210)
                        .unit("min")
                        .ink(Ink::slot(3)),
                )
                .ring(
                    RingValue::new("Stand", 72)
                        .max(84)
                        .unit("hr")
                        .ink(Ink::slot(1)),
                )
                .size(150),
            Stack::gap(1).children((
                Text::new("Move goal met on five days"),
                Text::muted("Exercise 20 minutes short of the week's goal."),
            )),
        )),
    )
}

fn gauge_zones() -> Card {
    Card::titled("Training load · Gurleen Grewal").body(
        Gauge::new("Load ratio", 1.12)
            .id("gauge-zones")
            .max(2)
            .format(NumberFormat::decimals(2).fixed())
            .label("Acute : chronic")
            .sublabel("Optimal")
            .zone(Zone::new(0, 0.8).label("Undertraining").ink(Ink::slot(1)))
            .zone(Zone::new(0.8, 1.3).label("Optimal").ink(Ink::slot(3)))
            .zone(Zone::new(1.3, 1.5).label("Overreaching").ink(Ink::slot(4)))
            .zone(Zone::new(1.5, 2).label("Overuse").ink(Ink::slot(8)))
            .size(220)
            .motion(Motion::new()),
    )
}

fn day_clock_band() -> Card {
    Card::titled("Ravneet Kaur · awake and asleep").body(
        DayClock::new("Steps by hour, awake and asleep")
            .id("clock-band")
            .hide_title()
            .values(STEPS)
            .unit("steps")
            .category("Awake", Ink::slot(3))
            .category("Asleep", Ink::slot(7))
            .categories([
                1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1,
            ])
            .band()
            .label("Steps")
            .locale("en-IN"),
    )
}
