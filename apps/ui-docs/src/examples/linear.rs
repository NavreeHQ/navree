//! Meter, mini columns, colour strip, range scale, trend and status badges.
//!
//! The rupee amounts are written in lakh grouping (`12_40_000`), as the pages print them.
#![allow(clippy::inconsistent_digit_grouping)]

use navree::ui::charts::*;
use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("linear.rs");

pub fn all() -> Vec<Example> {
    vec![
        // Meter
        single!("meter", "thumb", "A meter", meter_thumb)
            .start()
            .width(420)
            .height(80),
        single!("meter", "default", "Sleep stages", meter_default)
            .start()
            .height(150),
        single!("meter", "zones", "Heart rate zones", meter_zones)
            .start()
            .height(150),
        single!("meter", "compact", "The compact form", meter_compact)
            .start()
            .height(270),
        single!("meter", "rupees", "Rupees in lakh", meter_rupees)
            .start()
            .height(150),
        single!("meter", "states", "Loading, failed and empty", meter_states)
            .start()
            .width(820)
            .height(150),
        // Mini columns
        single!("mini-columns", "thumb", "Mini columns", mini_columns_thumb)
            .start()
            .width(420)
            .height(130),
        single!(
            "mini-columns",
            "default",
            "Sleep cycles",
            mini_columns_default
        )
        .start()
        .height(250),
        single!(
            "mini-columns",
            "week",
            "A week of workouts",
            mini_columns_week
        )
        .start()
        .height(250),
        // Colour strip
        single!("color-strip", "thumb", "A colour strip", color_strip_thumb)
            .start()
            .width(420)
            .height(60),
        single!(
            "color-strip",
            "default",
            "Heart rate zones of a run",
            color_strip_default
        )
        .start()
        .height(170),
        single!(
            "color-strip",
            "stages",
            "Sleep stages, half hour by half hour",
            color_strip_stages
        )
        .start()
        .height(140),
        // Range scale
        single!("range-scale", "thumb", "A range scale", range_scale_thumb)
            .start()
            .width(420)
            .height(90),
        single!(
            "range-scale",
            "default",
            "Resting heart rate",
            range_scale_default
        )
        .start()
        .height(120),
        gallery!("range-scale", "scales", "Other scales", range_scale_scales)
            .wide()
            .height(260),
        // Trend
        single!("trend", "thumb", "A trend", trend_thumb).start(),
        single!("trend", "default", "Trends beside figures", trend_default)
            .start()
            .width(820)
            .height(120),
        single!("trend", "words", "Another language", trend_words)
            .start()
            .height(100),
        // Status badges
        single!("badge", "status", "Status badges", badge_status)
            .start()
            .height(80),
    ]
}

fn meter_thumb() -> Meter {
    Meter::new("Sleep stages")
        .id("meter-thumb")
        .hide_title()
        .legend(false)
        .minutes()
        .gradient()
        .table(TableMode::ScreenReader)
        .segment(Segment::new("Deep", 92).ink(Ink::slot(7)))
        .segment(Segment::new("REM", 98).ink(Ink::tone(Tone::named("cyan"))))
        .segment(Segment::new("Core", 215).ink(Ink::slot(3)))
        .segment(Segment::new("Awake", 35).ink(Ink::slot(4)))
}

fn meter_default() -> Meter {
    Meter::new("Sleep stages")
        .id("meter-default")
        .description("Gurpreet Kaur, 11:20 pm to 6:40 am")
        .minutes()
        .gradient()
        .gloss()
        .motion(Motion::new())
        .segment(Segment::new("Deep", 92).ink(Ink::slot(7)))
        .segment(Segment::new("REM", 98).ink(Ink::tone(Tone::named("cyan"))))
        .segment(Segment::new("Core", 215).ink(Ink::slot(3)))
        .segment(Segment::new("Awake", 35).ink(Ink::slot(4)))
}

fn meter_zones() -> Meter {
    Meter::new("Heart rate zones")
        .id("meter-zones")
        .description("Rohan Malhotra, morning run at Sukhna Lake, 52 minutes")
        .minutes()
        .label_from(10.0)
        .segment(Segment::new("Warm-up", 6).ink(Ink::tone(Tone::named("slate"))))
        .segment(Segment::new("Fat burn", 14).ink(Ink::tone(Tone::named("cyan"))))
        .segment(Segment::new("Cardio", 22).ink(Ink::tone(Tone::Success)))
        .segment(Segment::new("Peak", 8).ink(Ink::tone(Tone::Warning)))
        .segment(Segment::new("Max", 2).ink(Ink::tone(Tone::Danger)))
}

fn meter_compact() -> Table {
    let night = |day: &'static str, total: &'static str, stages: [u32; 4]| {
        TableRowEl::new((
            TableCell::new(day),
            TableCell::new(total).numeric(),
            TableCell::new(
                Meter::new(format!("Sleep stages, {day}"))
                    .id(format!("meter-compact-{day}"))
                    .compact()
                    .minutes()
                    .gradient()
                    .segment(Segment::new("Deep", stages[0]).ink(Ink::slot(7)))
                    .segment(Segment::new("REM", stages[1]).ink(Ink::tone(Tone::named("cyan"))))
                    .segment(Segment::new("Core", stages[2]).ink(Ink::slot(3)))
                    .segment(Segment::new("Awake", stages[3]).ink(Ink::slot(4))),
            ),
        ))
    };
    Table::new()
        .head((
            TableHead::new("Night"),
            TableHead::new("Asleep").numeric(),
            TableHead::new("Stages"),
        ))
        .rows((
            night("Mon", "7 h 20 min", [88, 92, 210, 30]),
            night("Tue", "7 h 5 min", [74, 101, 198, 52]),
            night("Wed", "7 h 31 min", [96, 87, 224, 24]),
            night("Thu", "6 h 27 min", [61, 79, 187, 66]),
            night("Fri", "7 h 24 min", [90, 104, 219, 31]),
            night("Sat", "7 h 37 min", [112, 96, 231, 18]),
            night("Sun", "7 h 14 min", [99, 90, 205, 40]),
        ))
}

fn meter_rupees() -> Meter {
    Meter::new("Programme revenue by channel")
        .id("meter-rupees")
        .description("Fitness programmes, April to June")
        .format(NumberFormat::currency("₹"))
        .locale("en-IN")
        .pill()
        .segment(Segment::new("Studio", 12_40_000).ink(Ink::slot(1)))
        .segment(Segment::new("Online", 8_75_000).ink(Ink::slot(3)))
        .segment(Segment::new("Corporate", 4_10_500).ink(Ink::slot(7)))
        .segment(Segment::new("Retail", 95_000).ink(Ink::slot(4)))
}

fn meter_states() -> Grid {
    let state =
        |caption: &'static str, chart: Meter| Stack::gap(2).children((Text::muted(caption), chart));
    Grid::cols(3).gap(4).children((
        state(
            "Loading",
            Meter::new("Sleep stages").id("meter-loading").loading(),
        ),
        state(
            "Failed",
            Meter::new("Sleep stages")
                .id("meter-failed")
                .failed(None)
                .retry("/sleep"),
        ),
        state("Empty", Meter::new("Sleep stages").id("meter-empty")),
    ))
}

fn mini_columns_thumb() -> MiniColumns {
    MiniColumns::new("Sleep cycles")
        .id("mini-columns-thumb")
        .hide_title()
        .table(TableMode::ScreenReader)
        .height(56)
        .minutes()
        .part("Deep", Ink::slot(7))
        .part("REM", Ink::tone(Tone::named("cyan")))
        .part("Core", Ink::slot(3))
        .column(MiniColumn::new("Cycle 1", [34, 8, 38]))
        .column(MiniColumn::new("Cycle 2", [28, 16, 42]))
        .column(MiniColumn::new("Cycle 3", [18, 24, 45]))
        .column(MiniColumn::new("Cycle 4", [8, 31, 40]))
}

fn mini_columns_default() -> MiniColumns {
    MiniColumns::new("Sleep cycles")
        .id("mini-columns-default")
        .description("Gurpreet Kaur, five cycles")
        .minutes()
        .column_name("Cycle")
        .motion(Motion::new())
        .part("Deep", Ink::slot(7))
        .part("REM", Ink::tone(Tone::named("cyan")))
        .part("Core", Ink::slot(3))
        .column(MiniColumn::new("Cycle 1", [34, 8, 38]))
        .column(MiniColumn::new("Cycle 2", [28, 16, 42]))
        .column(MiniColumn::new("Cycle 3", [18, 24, 45]))
        .column(MiniColumn::new("Cycle 4", [8, 31, 40]))
        .column(MiniColumn::new("Cycle 5", [2, 35, 38]))
}

fn mini_columns_week() -> MiniColumns {
    MiniColumns::new("Workout mix")
        .id("mini-columns-week")
        .description("Minutes by kind of workout, Navdeep Bedi")
        .height(80)
        .format(NumberFormat::number().suffix(" min"))
        .column_name("Day")
        .part("Strength", Ink::slot(2))
        .part("Running", Ink::slot(1))
        .part("Yoga", Ink::slot(7))
        .column(MiniColumn::new("Mon", [45, 0, 20]))
        .column(MiniColumn::new("Tue", [0, 35, 0]))
        .column(MiniColumn::new("Wed", [50, 20, 0]))
        .column(MiniColumn::new("Thu", [0, 0, 40]))
        .column(MiniColumn::new("Fri", [40, 30, 10]))
        .column(MiniColumn::new("Sat", [30, 60, 0]))
        .column(MiniColumn::new("Sun", [0, 25, 45]))
}

fn color_strip_thumb() -> ColorStrip {
    ColorStrip::new("Sleep stages")
        .id("color-strip-thumb")
        .hide_title()
        .table(TableMode::ScreenReader)
        .category("Awake", Ink::slot(4))
        .category("REM", Ink::tone(Tone::named("cyan")))
        .category("Core", Ink::slot(3))
        .category("Deep", Ink::slot(7))
        .cells([
            ("11:00 pm", "Awake"),
            ("11:30 pm", "Core"),
            ("12:00 am", "Deep"),
            ("12:30 am", "Deep"),
            ("1:00 am", "Core"),
            ("1:30 am", "REM"),
            ("2:00 am", "Core"),
            ("2:30 am", "Deep"),
            ("3:00 am", "Core"),
            ("3:30 am", "REM"),
            ("4:00 am", "REM"),
            ("4:30 am", "Core"),
        ])
}

fn color_strip_default() -> ColorStrip {
    // One reading a minute for an hour and a half, from a run.
    let readings = (0..90_u32).map(|minute| {
        let ramp = 62 + minute.min(12) * 6;
        let surge = if (30..44).contains(&minute) { 48 } else { 0 };
        let wobble = (minute * 37 % 11) as i32 - 5;
        let bpm = (ramp + surge) as i32 + wobble;
        (
            format!("{}:{:02} pm", 6 + minute / 60, minute % 60),
            bpm.clamp(55, 185),
        )
    });
    ColorStrip::new("Heart rate zones")
        .id("color-strip-default")
        .description("Navdeep Bedi, evening run, one reading a minute")
        .cell_name("Time")
        .value_name("Heart rate")
        .format(NumberFormat::number().suffix(" bpm"))
        .legend_shares()
        .motion(Motion::new())
        .colors(
            ColorScale::new(Ink::tone(Tone::named("slate")))
                .label("Rest")
                .from(100, Ink::tone(Tone::named("cyan")))
                .label("Fat burn")
                .from(130, Ink::tone(Tone::Warning))
                .label("Cardio")
                .from(160, Ink::tone(Tone::Danger))
                .label("Peak"),
        )
        .readings(readings)
}

fn color_strip_stages() -> ColorStrip {
    ColorStrip::new("Sleep stages")
        .id("color-strip-stages")
        .description("Simran Dhillon, every half hour")
        .category("Awake", Ink::slot(4))
        .category("REM", Ink::tone(Tone::named("cyan")))
        .category("Core", Ink::slot(3))
        .category("Deep", Ink::slot(7))
        .cell_name("Half hour")
        .cells([
            ("11:00 pm", "Awake"),
            ("11:30 pm", "Core"),
            ("12:00 am", "Deep"),
            ("12:30 am", "Deep"),
            ("1:00 am", "Core"),
            ("1:30 am", "REM"),
            ("2:00 am", "Core"),
            ("2:30 am", "Deep"),
            ("3:00 am", "Core"),
            ("3:30 am", "REM"),
            ("4:00 am", "REM"),
            ("4:30 am", "Core"),
            ("5:00 am", "REM"),
            ("5:30 am", "Core"),
            ("6:00 am", "Awake"),
        ])
}

fn range_scale_thumb() -> RangeScale {
    RangeScale::new("Resting heart rate", 58)
        .id("range-scale-thumb")
        .hide_title()
        .range(40, 120)
        .track(
            Gradient::horizontal()
                .stop_with_opacity(0.0, Ink::tone(Tone::Success), 0.55)
                .stop_with_opacity(0.3, Ink::tone(Tone::named("cyan")), 0.55)
                .stop_with_opacity(0.6, Ink::tone(Tone::named("orange")), 0.55)
                .stop_with_opacity(1.0, Ink::tone(Tone::Danger), 0.55),
        )
}

fn range_scale_default() -> RangeScale {
    RangeScale::new("Resting heart rate", 58)
        .id("range-scale-default")
        .range(40, 120)
        .format(NumberFormat::number().suffix(" bpm"))
        .show_value()
        .motion(Motion::new().delay(300))
        .track(
            Gradient::horizontal()
                .stop_with_opacity(0.0, Ink::tone(Tone::Success), 0.55)
                .stop_with_opacity(0.3, Ink::tone(Tone::named("cyan")), 0.55)
                .stop_with_opacity(0.6, Ink::tone(Tone::named("orange")), 0.55)
                .stop_with_opacity(1.0, Ink::tone(Tone::Danger), 0.55),
        )
}

fn range_scale_scales() -> impl IntoIterator<Item = (&'static str, RangeScale)> {
    [
        (
            "Body mass index, with words at the ends",
            RangeScale::new("Body mass index", 23.4)
                .id("range-scale-bmi")
                .range(15, 40)
                .ends("Underweight", "Obese")
                .show_value()
                .track(
                    Gradient::horizontal()
                        .stop_with_opacity(0.0, Ink::tone(Tone::named("cyan")), 0.55)
                        .stop_with_opacity(0.14, Ink::tone(Tone::Success), 0.55)
                        .stop_with_opacity(0.34, Ink::tone(Tone::Success), 0.55)
                        .stop_with_opacity(0.4, Ink::tone(Tone::Warning), 0.55)
                        .stop_with_opacity(0.6, Ink::tone(Tone::named("orange")), 0.55)
                        .stop_with_opacity(1.0, Ink::tone(Tone::Danger), 0.55),
                ),
        ),
        (
            "Systolic pressure above the scale, in the quiet track",
            RangeScale::new("Systolic blood pressure", 152)
                .id("range-scale-pressure")
                .range(90, 140)
                .format(NumberFormat::number().suffix(" mmHg"))
                .show_value(),
        ),
        (
            "Balance in rupees",
            RangeScale::new("Wallet balance", 4_50_000)
                .id("range-scale-rupees")
                .range(0, 10_00_000)
                .format(NumberFormat::currency("₹"))
                .locale("en-IN")
                .show_value()
                .track(
                    Gradient::horizontal()
                        .stop_with_opacity(0.0, Ink::accent(), 0.0)
                        .stop_with_opacity(1.0, Ink::accent(), 0.7),
                ),
        ),
    ]
}

fn trend_thumb() -> Row {
    Row::gap(4).wrap().children((
        Trend::new(12.4),
        Trend::new(3.0).up_is_bad(),
        Trend::new(-4.5),
    ))
}

fn trend_default() -> Grid {
    Grid::cols(4).gap(4).children((
        Stat::new("Steps", "9,412").change(Trend::new(12.4)),
        Stat::new("Resting heart rate", "58 bpm").change(Trend::new(3.0).up_is_bad()),
        Stat::new("Heart rate variability", "52 ms").change(Trend::new(-4.5)),
        Stat::new("Time asleep", "7 h 20 min").change(Trend::new(0.0)),
    ))
}

fn trend_words() -> Row {
    Row::gap(4).wrap().children((
        Trend::new(123_456.0)
            .decimals(0)
            .suffix(" कदम")
            .locale("en-IN")
            .words("बढ़ा", "घटा", "कोई बदलाव नहीं"),
        Trend::new(-2.5)
            .up_is_bad()
            .words("बढ़ा", "घटा", "कोई बदलाव नहीं"),
    ))
}

fn badge_status() -> Row {
    Row::gap(3).wrap().children((
        Badge::new("Excellent")
            .tone(Tone::Success)
            .icon(icons::SPARKLES),
        Badge::new("Good")
            .tone(Tone::named("cyan"))
            .icon(icons::HEART_PULSE),
        Badge::new("Fair").tone(Tone::Warning).icon(icons::ACTIVITY),
        Badge::new("Moderate")
            .tone(Tone::named("orange"))
            .icon(icons::BATTERY_LOW),
        Badge::new("Needs attention")
            .tone(Tone::Danger)
            .icon(icons::TRIANGLE_ALERT),
        Badge::new("No data")
            .tone(Tone::named("slate"))
            .icon(icons::CIRCLE_MINUS),
    ))
}
