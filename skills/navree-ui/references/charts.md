# Rings, meters, heat maps and how charts are painted

The charts past the line, area and bar chart, and the options every chart
shares. All of them live in `navree::ui::charts`, are drawn as SVG on the
server, carry a table of the same numbers for a screen reader, and need no
script to show. Each block compiles.

Pick by what the reader has to see:

| The reader sees | Use |
|---|---|
| One score out of 100, or a value toward a goal | `Ring` |
| Two to six values, each against its own goal | `Rings` |
| One value on a scale that may start above zero, with zones | `Gauge` |
| A value for each hour of a day, round a clock face | the day clock (`DayClock`) |
| One whole split into parts (sleep stages, time in heart-rate zones) | `Meter` |
| The same parts for several items, side by side | mini columns (`MiniColumns`) |
| A category or a band for each slot of time | a colour strip (`ColorStrip`) |
| One reading between two ends (a BMI, a resting heart rate) | a range scale (`RangeScale`) |
| How consistent a habit is over months | the calendar heat map (`CalendarHeatmap`) |
| Every measure against every other | the matrix heat map (`MatrixHeatmap`) |
| The pairs that move together, ranked | the relation list (`RelationList`) |
| A cloud of pairs of values | the scatter chart (`Scatter`) |
| A change over time, several series | `Chart::line`, `area` or `bar` (see the index in `SKILL.md`) |

Every chart takes `.id(..)` (two charts of one kind on a page need their
own), `.description(..)`, `.hide_title()`, `.locale("en-IN")` for lakh
grouping, and the states `.loading()`, `.failed(reason)` with
`.retry(href)`, and `.empty(title, text)`. Draw those states; never leave a
blank card.

## Radial: rings, gauges and the day clock

A `Ring` shows one value with the number in its middle. `max` sets the goal
(100 by default); a value past it draws a second lap. A ring's title is what
a screen reader says, and `label` and `sublabel` are the words under the
number. Size it from 56 to 200 pixels; `size_on(Screen::Lg, px)` grows it on
a wide screen.

`Rings` nests two to six `RingValue`s, each with its own goal: activity
rings, a day's scores. `center_values()` writes each value in the middle and
`legend()` names them beside the rings.

A `Gauge` is an open arc, 270 degrees by default (`sweep(180.0)` for half a
circle), on a scale from `min` to `max`, with `Zone`s on its track. Use it
when the scale does not start at zero or has named bands; use a ring for a
share of a whole.

The day clock (`DayClock`) takes 24 values from midnight and draws a wedge
per hour: its length is the value, its colour a category (`category` and
`categories`) or a band (`colors`). `Moment`s mark what happened at an hour,
and `now(hour)` marks the hour it is.

```rust
use navree::ui::charts::{
    ColorScale, DayClock, Gauge, Gradient, Ink, Moment, Motion, NumberFormat, Ring, RingTrack,
    RingValue, Rings, Screen, Zone,
};
use navree::ui::{Tone, icons};

/// Last night's sleep score.
pub fn sleep_score(score: u32) -> Ring {
    Ring::new("Sleep score, Simran Kaur", score)
        .label("Sleep")
        .sublabel("Good")
        .fill(Gradient::sheen(Ink::slot(7)))
        .glow()
        .size(160)
        .size_on(Screen::Lg, 200)
        .motion(Motion::new().delay(200))
}

/// Recovery coloured by the band it falls in: red when low, green when good.
pub fn recovery(percent: u32) -> Ring {
    Ring::new("Recovery, Arjun Mehta", percent)
        .unit("%")
        .label("Recovery")
        .colors(
            ColorScale::new(Ink::tone(Tone::Danger))
                .label("Low")
                .from(34, Ink::tone(Tone::Warning))
                .label("Fair")
                .from(67, Ink::tone(Tone::Success))
                .label("Recovered"),
        )
        .size(140)
}

/// Steps toward a goal of 10,000, written 6,420 in Indian grouping.
pub fn steps(today: u32) -> Ring {
    Ring::new("Steps, goal 10,000", today)
        .max(10_000)
        .locale("en-IN")
        .center_icon(icons::FOOTPRINTS)
        .ink(Ink::slot(3))
        .track(RingTrack::Tinted)
        .size(96)
}

/// A day's activity: each ring against its own goal.
pub fn activity() -> Rings {
    Rings::new("Activity, Harpreet Gill")
        .ring(RingValue::new("Move", 540).max(600).unit("kcal").ink(Ink::slot(8)))
        .ring(RingValue::new("Exercise", 38).max(30).unit("min").ink(Ink::slot(3)))
        .ring(RingValue::new("Stand", 9).max(12).unit("hr").ink(Ink::slot(1)))
        .track(RingTrack::Tinted)
        .center_values()
        .legend()
        .size(180)
}

/// A ratio on a scale from 0 to 2, with named zones.
pub fn training_load(ratio: f64) -> Gauge {
    Gauge::new("Acute to chronic load, Neha Sharma", ratio)
        .max(2)
        .format(NumberFormat::decimals(2).fixed())
        .label("Load ratio")
        .zone(Zone::new(0, 0.8).label("Undertraining").ink(Ink::slot(1)))
        .zone(Zone::new(0.8, 1.3).label("Optimal").ink(Ink::slot(3)))
        .zone(Zone::new(1.3, 2).label("Overreaching").ink(Ink::slot(8)))
        .size(200)
}

/// Body temperature on half a circle, from 34 to 39 degrees.
pub fn temperature(celsius: f64) -> Gauge {
    Gauge::new("Body temperature", celsius)
        .min(34)
        .max(39)
        .sweep(180.0)
        .format(NumberFormat::decimals(1))
        .unit("°C")
        .label("Temperature")
        .ink(Ink::slot(2))
}

/// Steps in each hour from midnight, awake and asleep, with what happened.
pub fn day_of_steps(hours: [u32; 24]) -> DayClock {
    DayClock::new("Steps by hour, Simran Kaur")
        .values(hours)
        .unit("steps")
        .locale("en-IN")
        .category("Awake", Ink::slot(3))
        .category("Asleep", Ink::slot(7))
        .categories([1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1])
        .moment(Moment::new(6.5, "Woke up").icon(icons::SUNRISE).ink(Ink::slot(4)))
        .moment(Moment::new(17.5, "Badminton").icon(icons::FLAME).ink(Ink::slot(2)))
        .now(15)
        .label("Steps")
        .size(280)
}
```

A ring with no reading yet takes `None::<f64>` and says so with `.empty(..)`;
one still loading is `.loading()`. In a figure tile, put a ring of 48 to 72
pixels beside the number with `Stat::aside` (see
[`app-and-site-blocks.md`](app-and-site-blocks.md)).

## Linear: meters, mini columns, colour strips and range scales

A `Meter` is one bar split into `Segment`s that add up to the whole, the
share written inside the wide ones and a legend of values under it.
`minutes()` writes the values as hours and minutes; `format(..)` takes any
`NumberFormat`, rupees included. `compact()` draws it small enough for a
table cell, `pill()` rounds its ends, `gradient()` and `gloss()` paint it.

Mini columns (`MiniColumns`) draw one column per item, each split into the
same `part`s and drawn to 100%: sleep cycles, a week of workouts by kind.

A colour strip (`ColorStrip`) is a row of equal cells. Each cell is either a
category (`category` and `cells`) or a reading coloured by its band
(`colors` and `readings`).

A range scale (`RangeScale`) puts one value on a scale between two ends,
with a marker, words at the ends (`ends`) and a track that can be a
gradient. A reading past either end is drawn at the end and still written
as it is.

A `Trend` is the change beside a figure: an arrow, the change with its sign,
and green or red by what it means (`up_is_bad()` for a resting heart rate).

```rust
use navree::ui::charts::{
    ColorScale, ColorStrip, Gradient, Ink, Meter, MiniColumn, MiniColumns, Motion, NumberFormat,
    RangeScale, Segment,
};
use navree::ui::{Grid, Stat, Tone, Trend};

/// Last night's stages, in minutes.
pub fn sleep_stages() -> Meter {
    Meter::new("Sleep stages")
        .description("Simran Kaur, 11:20 pm to 6:40 am IST")
        .minutes()
        .gradient()
        .motion(Motion::new())
        .segment(Segment::new("Deep", 92).ink(Ink::slot(7)))
        .segment(Segment::new("REM", 98).ink(Ink::tone(Tone::named("cyan"))))
        .segment(Segment::new("Core", 215).ink(Ink::slot(3)))
        .segment(Segment::new("Awake", 35).ink(Ink::slot(4)))
}

/// Revenue by channel, in rupees with Indian grouping (12,40,000).
pub fn revenue_by_channel() -> Meter {
    Meter::new("Programme revenue by channel")
        .id("revenue-meter")
        .format(NumberFormat::currency("₹"))
        .locale("en-IN")
        .pill()
        .segment(Segment::new("Studio", 12_40_000).ink(Ink::slot(1)))
        .segment(Segment::new("Online", 8_75_000).ink(Ink::slot(3)))
        .segment(Segment::new("Corporate", 4_10_500).ink(Ink::slot(7)))
}

/// A week of workouts by kind, one column a day.
pub fn workout_mix() -> MiniColumns {
    MiniColumns::new("Workout mix, Arjun Mehta")
        .height(80)
        .format(NumberFormat::number().suffix(" min"))
        .column_name("Day")
        .part("Strength", Ink::slot(2))
        .part("Running", Ink::slot(1))
        .part("Yoga", Ink::slot(7))
        .column(MiniColumn::new("Mon", [45, 0, 20]))
        .column(MiniColumn::new("Tue", [0, 35, 0]))
        .column(MiniColumn::new("Wed", [50, 20, 0]))
}

/// Heart rate each minute of a run, coloured by zone.
pub fn heart_rate_zones(readings: Vec<(String, u32)>) -> ColorStrip {
    ColorStrip::new("Heart rate zones")
        .description("Harpreet Gill, evening run in Amritsar")
        .cell_name("Time")
        .value_name("Heart rate")
        .format(NumberFormat::number().suffix(" bpm"))
        .legend_shares()
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

/// A body mass index between 15 and 40, with words at the ends.
pub fn bmi(value: f64) -> RangeScale {
    RangeScale::new("Body mass index", value)
        .range(15, 40)
        .ends("Underweight", "Obese")
        .show_value()
        .track(
            Gradient::horizontal()
                .stop_with_opacity(0.0, Ink::tone(Tone::named("cyan")), 0.55)
                .stop_with_opacity(0.3, Ink::tone(Tone::Success), 0.55)
                .stop_with_opacity(0.6, Ink::tone(Tone::Warning), 0.55)
                .stop_with_opacity(1.0, Ink::tone(Tone::Danger), 0.55),
        )
}

/// Figures with their change: green when good, red when not.
pub fn this_week() -> Grid {
    Grid::fit(12).gap(4).children((
        Stat::new("Steps", "9,412").change(Trend::new(12.4)),
        Stat::new("Resting heart rate", "58 bpm").change(Trend::new(3.0).up_is_bad()),
        Stat::new("Time asleep", "7 h 20 min").change(Trend::new(0.0)),
    ))
}
```

A meter that has not loaded yet is `.loading()`; one that failed is
`.failed(None).retry("/sleep")`. A segment of zero still shows in the
legend, hollow, so the reader sees every zone.

## Grids: heat maps, the relation list and the scatter chart

The calendar heat map (`CalendarHeatmap`) draws a day per cell, in weeks,
shaded in steps: `HeatSteps::Even` (the default), `Quartiles`, or
`HeatSteps::at(a, b, c)` at values you choose. A day given as `None` is
drawn hollow, not as zero: the watch was off, not the habit. `range(first,
last)` fixes the window when data starts late.

The matrix heat map (`MatrixHeatmap`) and the relation list
(`RelationList`) take the same `Relation`s: a signed value from -1 to 1,
whether it is significant, and a note. The matrix shows every pair at once
(`lower_triangle()` drops the mirror half); the list ranks the pairs by
strength, with a bar from the centre, and `only_significant()` keeps the
links found. `Relation::untested()` is a pair with too few days.

The scatter chart (`Scatter`) draws a translucent point for each pair, in
one `PointSeries` or several. `equal_domains()` gives both axes one range,
and `Reference::identity()` draws the diagonal.

```rust
use chrono::{Duration, NaiveDate};
use navree::ui::charts::{
    Axis, CalendarHeatmap, Domain, Gridlines, HeatSteps, Ink, MatrixHeatmap, PointSeries,
    Reference, Relation, RelationList, Scatter,
};
use navree::ui::Tone;

/// A day's steps for the last 90 days; `None` where the phone recorded nothing.
pub fn daily_steps(steps: &[Option<f64>]) -> CalendarHeatmap {
    let last = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap_or_default();
    let days = steps
        .iter()
        .rev()
        .enumerate()
        .map(|(back, value)| (last - Duration::days(back as i64), *value));
    CalendarHeatmap::new("Daily steps")
        .description("Neha Sharma, the last 90 days")
        .locale("en-IN")
        .days(days)
        .ink(Ink::tone(Tone::Success))
        .steps(HeatSteps::at(4_000, 8_000, 12_000))
        .unit("steps")
}

/// How the measures moved together over 30 days.
pub fn correlations() -> MatrixHeatmap {
    MatrixHeatmap::new("Metric correlations")
        .description("Last 30 days, Arjun Mehta")
        .metrics(["Sleep", "Stress", "HRV", "Resting HR"])
        .value_name("Correlation")
        .relation("Sleep", "Stress", Relation::new(-0.58).significant(true).note("30 days, p < 0.05"))
        .relation("Sleep", "HRV", Relation::new(0.71).significant(true).note("30 days, p < 0.05"))
        .relation("Stress", "HRV", Relation::new(-0.52).significant(true))
        .relation("HRV", "Resting HR", Relation::new(-0.77).significant(true))
        .relation("Sleep", "Resting HR", Relation::new(-0.21).note("could be chance"))
        .lower_triangle()
}

/// The pairs ranked by strength, the significant ones only.
pub fn links_found() -> RelationList {
    RelationList::new("What moves together")
        .value_name("Correlation (r)")
        .relation("Sleep", "Next-day HRV", Relation::new(0.62).significant(true).note("30 days"))
        .relation("Steps", "Stress", Relation::new(-0.48).significant(true).note("28 days"))
        .relation("Caffeine", "Deep sleep", Relation::new(-0.22).note("could be chance"))
        .relation("Alcohol", "Recovery", Relation::untested().note("too few days with both"))
        .only_significant()
}

/// Each heartbeat's gap against the next, in milliseconds.
pub fn beat_to_beat(pairs: Vec<(f64, f64)>) -> Scatter {
    Scatter::new("Successive heartbeat intervals")
        .description("Simran Kaur, resting")
        .series(PointSeries::new("Resting", pairs).ink(Ink::slot(8)))
        .x_name("This beat")
        .y_name("Next beat")
        .unit("ms")
        .equal_domains()
        .x_axis(Axis::hidden())
        .y_axis(Axis::hidden())
        .grid(Gridlines::none())
        .reference(Reference::identity().label("Equal"))
        .height(320)
}

/// Two series on visible axes, with the average marked.
pub fn steps_and_heart(weekdays: Vec<(f64, f64)>, weekends: Vec<(f64, f64)>) -> Scatter {
    Scatter::new("Steps and resting heart rate")
        .locale("en-IN")
        .series(PointSeries::new("Weekdays", weekdays).ink(Ink::slot(1)))
        .series(PointSeries::new("Weekends", weekends).ink(Ink::slot(2)))
        .x_name("Steps")
        .y_name("Resting heart rate")
        .y_unit("bpm")
        .x_axis(Axis::new().line())
        .y_axis(Axis::new().line())
        .grid(Gridlines::horizontal().dashed())
        .y_domain(Domain::padded(2))
        .reference(Reference::average().label("Average {value}"))
}
```

## Paint, motion, axes and marks

**Colour.** An `Ink` is a colour of the theme, never a hex value:
`Ink::slot(1..=8)` (the chart colours), `Ink::tone(Tone::Success)` or
`Tone::named("cyan")`, `Ink::accent()`, `Ink::muted()`, and `Ink::own()` (the
mark's own colour, inside a gradient). `.vivid()` and `.soft()` mix it
toward the text or the surface. A `Gradient` (`horizontal`, `vertical`,
`diagonal`, `angle`, `radial`, or the presets `sheen`, `fade` and `column`)
takes stops; `across_plot()` spans the whole plot instead of each mark. A
`ColorScale` colours by value: `new(ink)` below the first band, `.from(at,
ink)` from each threshold, `.label(..)` naming each band for the legend.
`glow()` adds a soft light of the mark's own colour; `halo()` a wider one
round a ring. All of it follows light and dark.

**Motion.** `Motion::new()` makes marks grow in once, `.delay(ms)`,
`.stagger(ms)` between marks and `.speed(Speed::Slow)`. It is off by default
and always off for a reader who asks for less motion. `CountUp::new(value,
text)` counts a figure up to its value; the text is what a reader without
motion and a screen reader get.

**Axes and marks** on `Chart::line`, `area` and `bar`:

| Option | What it does |
|---|---|
| `x_axis(Axis::new().line().ticks())`, `Axis::hidden()` | the axis line, its ticks, or no axis |
| `grid(Gridlines::none())`, `horizontal()`, `vertical()`, `both()`, `.dashed()` | the lines behind the data |
| `x_ticks(Ticks::most(6).keep_ends())`, `Ticks::every(3)`, `y_ticks(6)` | how many labels an axis takes |
| `y_domain(Domain::padded(10))`, `Domain::fixed(0, 10)`, `Domain::data()` | the value axis's range |
| `reference(Reference::level(58).label("Resting {value}"))`, `average()`, `zero()`, `identity()`, `.place(Place::StartAbove)` | a line across the plot |
| `zone(Zone::new(50, 60).label("Healthy").ink(..))` | a band of the value axis, shaded behind the data |
| `dots(Dots::None \| End \| All)`, `active_dot(ActiveDot::Halo)` | the points, and the one under the pointer |
| `curve(Curve::Step)`, `smooth()` | steps between levels, or a smooth line |
| `tooltip(TooltipStyle::big())` | the value large in the tooltip |
| `baseline(36)` | bars grow up or down from a level, not from zero |
| `lanes()` | with `y_categories`, each level of a stepped area gets a lane |
| `format(NumberFormat::clock(18))` | values as minutes past 6 PM, written as times through midnight |

```rust
use chrono::NaiveDate;
use navree::ui::charts::{
    ActiveDot, Chart, ColorScale, CountUp, Curve, Domain, Dots, Gradient, Gridlines, Ink, Motion,
    NumberFormat, Reference, Series, Sparkline, Ticks, TooltipStyle, Zone,
};
use navree::ui::{Bars, Grid, Progress, Stat, Tone};

/// Resting heart rate over a fortnight, with the healthy band shaded.
pub fn resting_heart(days: Vec<NaiveDate>, bpm: Vec<u32>) -> Chart {
    Chart::area("Resting heart rate")
        .description("Harpreet Gill, the last 14 days")
        .locale("en-IN")
        .height(180)
        .days(days)
        .series(Series::new("Resting", bpm).ink(Ink::slot(8)).unit("bpm").fade().glow())
        .smooth()
        .dots(Dots::None)
        .active_dot(ActiveDot::Halo)
        .y_domain(Domain::padded(5))
        .grid(Gridlines::horizontal().dashed())
        .x_ticks(Ticks::most(6).keep_ends())
        .zone(Zone::new(50, 60).label("Healthy").ink(Ink::tone(Tone::Success)))
        .reference(Reference::average().label("Average {value}"))
        .tooltip(TooltipStyle::big())
        .motion(Motion::new())
}

/// A night of sleep stages as steps, each stage in a lane of its own.
pub fn stages(minutes: Vec<u32>, stage: Vec<u32>) -> Chart {
    Chart::area("Sleep stages")
        .numbers(minutes)
        .series(Series::new("Stage", stage).ink(Ink::slot(7)))
        .curve(Curve::Step)
        .y_categories(["Deep", "Light", "REM", "Awake"])
        .lanes()
}

/// Each part's age against the person's own: bars grow from 36.
pub fn body_age() -> Chart {
    Chart::bar("Body age by part")
        .description("Arjun Mehta, 36")
        .categories(["Heart", "Lungs", "Muscle", "Metabolism"])
        .series(
            Series::new("Age", [31, 38, 33, 41]).unit("years").colors(
                ColorScale::new(Ink::tone(Tone::Success))
                    .label("Younger")
                    .from(36.5, Ink::tone(Tone::Warning))
                    .label("Older"),
            ),
        )
        .baseline(36)
        .y_domain(Domain::fixed(26, 46))
        .reference(Reference::level(36).label("Your age"))
}

/// Bedtimes and waking times on one axis through midnight.
pub fn bedtimes(days: Vec<NaiveDate>, to_bed: Vec<u32>, woke: Vec<u32>) -> Chart {
    Chart::line("Bedtime and waking")
        .days(days)
        .series(Series::new("To bed", to_bed).ink(Ink::slot(7)))
        .series(Series::new("Woke up", woke).ink(Ink::slot(4)))
        .format(NumberFormat::clock(18))
        .dots(Dots::All)
        .y_ticks(6)
}

/// The small painted charts: a sparkline under a counted figure, a progress bar
/// toward a goal, and bars coloured by band.
pub fn painted() -> Grid {
    Grid::fit(14).gap(4).children((
        Stat::new("Movement", CountUp::new(94, "94")).chart(
            Sparkline::new("Movement score, 7 days", [82, 75, 88, 92, 78, 85, 94])
                .smooth()
                .fade()
                .ink(Ink::slot(2))
                .height(48)
                .motion(Motion::new()),
        ),
        Progress::new(402, 480, "Sleep, 6 h 42 min of 8 h")
            .fill(Gradient::horizontal().stop(0.0, Ink::slot(7)).stop_with_opacity(1.0, Ink::slot(7), 0.6))
            .goal(420)
            .motion(Motion::new().delay(300)),
        Bars::new("Heart rate variability, 7 nights", &[48, 52, 45, 61, 58, 44, 50])
            .colors(ColorScale::new(Ink::tone(Tone::Warning)).from(50, Ink::tone(Tone::Success)))
            .pill(),
    ))
}
```

A colour alone never carries the meaning: name each band in the legend, the
label or the sublabel, as the examples do. The details of each chart, with
every option, are under
[the component pages](https://navree.com/docs/components/overview/).
