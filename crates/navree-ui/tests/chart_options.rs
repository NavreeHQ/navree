//! Chart options measured against their output: sizes that hold in any container, colour
//! bands in a scale's own units, the colours a sheen ends in, zones on a gauge's track, a
//! day clock's band of categories, more rings, bars from a level other than zero, a ranked
//! list of relations, a value axis in clock time, a stepped area over named levels and a
//! meter's full legend.

use navree_ui::Tone;
use navree_ui::charts::*;
use navree_ui::node::Node;
use navree_ui::render;

fn html(node: impl Into<Node>) -> String {
    render(&node.into())
}

fn sheet(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("styles/extra")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

/// The first rule of `sheet` that contains every one of `parts`.
fn rule<'a>(sheet: &'a str, parts: &[&str]) -> Option<&'a str> {
    sheet
        .lines()
        .flat_map(|line| line.split_inclusive('}'))
        .find(|r| parts.iter().all(|p| r.contains(p)))
}

// ---- Rings in a box that shrinks to fit -------------------------------------------------

#[test]
fn rings_size_themselves_like_a_ring_in_a_box_that_shrinks_to_fit() {
    // A chart figure is an inline-size container, which has no width of its own: in a grid
    // column sized `auto` or a flex item it would be drawn zero wide. Every radial kind
    // drawn at a size of its own leaves that containment and takes its drawing's width.
    let css = sheet("chart-radial.css");
    let r = rule(&css, &["[data-slot=chart]", "container-type:normal"]).unwrap_or("");
    for kind in ["[data-kind=ring]", "[data-kind=gauge]", "[data-kind=rings]"] {
        assert!(r.contains(kind), "{kind} is not sized by its drawing: {r}");
    }
    assert!(r.contains("width:max-content"), "{r}");
}

// ---- Colour bands in the scale's own units ----------------------------------------------

fn ratio_bands() -> ColorScale {
    ColorScale::new(Ink::slot(7))
        .label("Undertraining")
        .from(0.8, Ink::slot(3))
        .label("Optimal")
        .from(1.3, Ink::slot(4))
        .label("Overreaching")
        .from(1.5, Ink::slot(8))
        .label("Overuse")
}

#[test]
fn a_gauges_bands_read_values_on_its_own_scale() {
    // 1.1 on a scale of 0 to 2 is optimal: the thresholds are values, as a series reads them,
    // not shares of the scale.
    let out = html(Gauge::new("Load ratio", 1.1).max(2).colors(ratio_bands()));
    assert!(out.contains("Optimal."), "{out}");
    assert!(
        out.contains(r#"data-slot="radial" data-kind="gauge" data-color="3""#),
        "{out}"
    );
    // On a scale that starts above zero too: 30 on 20 to 60 is fair from 28.
    let vo2 = ColorScale::new(Ink::slot(8))
        .label("Low")
        .from(28, Ink::slot(4))
        .label("Fair")
        .from(42, Ink::slot(3))
        .label("Good");
    let out = html(
        Gauge::new("VO2 max", 30)
            .min(20)
            .max(60)
            .colors(vo2.clone()),
    );
    assert!(out.contains("Fair."), "{out}");
    let out = html(Gauge::new("VO2 max", 45).min(20).max(60).colors(vo2));
    assert!(out.contains("Good."), "{out}");
}

#[test]
fn a_rings_bands_read_values_against_its_goal() {
    let steps = ColorScale::new(Ink::tone(Tone::Danger))
        .label("Short")
        .from(8_000, Ink::tone(Tone::Success))
        .label("Reached");
    let out = html(Ring::new("Steps", 9_000).max(12_000).colors(steps));
    assert!(out.contains("Reached."), "{out}");
}

// ---- More rings -------------------------------------------------------------------------

fn five_scores() -> Rings {
    ["Sleep", "Recovery", "Movement", "Stress", "Mind"]
        .iter()
        .zip([82, 74, 61, 45, 68])
        .fold(Rings::new("Today's scores"), |rings, (name, value)| {
            rings.ring(RingValue::new(*name, value))
        })
}

#[test]
fn five_and_six_rings_are_drawn_thinner_so_the_middle_keeps_its_room() {
    let out = html(five_scores().size(200).center_values());
    assert_eq!(out.matches(r#"data-slot="radial-arc""#).count(), 5, "{out}");
    assert!(out.contains(r#"data-rings="5""#), "{out}");
    assert_eq!(out.matches(r#"data-slot="radial-row""#).count(), 5, "{out}");
    assert!(!out.contains("chart-note"), "nothing is cut: {out}");
    // 0.3 / (1.3 * 5) of 200 is 9.23 thick, three tenths of it apart: the fifth ring's
    // radius is still more than a third of the outer one's.
    assert!(out.contains(r#"stroke-width="9.23""#), "{out}");
    let six = html(five_scores().ring(RingValue::new("Heart", 90)).size(200));
    assert_eq!(six.matches(r#"data-slot="radial-arc""#).count(), 6, "{six}");
}

#[test]
fn rings_that_do_not_fit_are_said_under_the_drawing_and_listed_in_the_table() {
    // Rings 15 thick at 120 pixels: three fit, the fourth and fifth would have no hole.
    let out = html(five_scores().size(120).thickness(15.0).gap(4.0).legend());
    assert_eq!(out.matches(r#"data-slot="radial-arc""#).count(), 3, "{out}");
    assert!(out.contains(r#"data-rings="3""#), "{out}");
    assert!(
        out.contains("The first 3 of 5 rings are drawn; the table lists every value."),
        "{out}"
    );
    assert!(
        out.contains(">Mind<"),
        "the legend and the table list the fifth: {out}"
    );
}

// ---- Zones on a gauge's track -----------------------------------------------------------

fn load_gauge(value: f64) -> Gauge {
    Gauge::new("Load ratio", value)
        .id("load")
        .max(2)
        .size(200)
        .thickness(20.0)
        .format(NumberFormat::decimals(2).fixed())
        .zone(Zone::new(0, 0.8).label("Undertraining").ink(Ink::slot(7)))
        .zone(Zone::new(0.8, 1.3).label("Optimal").ink(Ink::slot(3)))
        .zone(Zone::new(1.3, 1.5).label("Overreaching").ink(Ink::slot(4)))
        .zone(Zone::new(1.5, 2).label("Overuse").ink(Ink::slot(8)))
}

#[test]
fn a_gauges_zones_lie_on_its_track_with_the_arc_down_their_middle() {
    let out = html(load_gauge(1.1));
    assert_eq!(
        out.matches(r#"data-slot="radial-zone""#).count(),
        4,
        "{out}"
    );
    // Each zone is its stretch of the 270 degrees: 0.8 of 2 is 108 degrees from -135.
    assert!(
        out.contains(r#"<path data-slot="radial-zone" data-color="7" d="M36.36 163.64A90 90 0 0 1 59.14 19.81" stroke-width="20">"#),
        "{out}"
    );
    // The zones come after the track and before the arc, which is half as thick.
    let zones = out.find("radial-zones").unwrap_or(usize::MAX);
    assert!(
        out.find("radial-track").unwrap_or(usize::MAX) < zones,
        "{out}"
    );
    assert!(zones < out.find("radial-arc").unwrap_or(0), "{out}");
    assert!(
        out.contains(r#"data-slot="radial-arc" data-color="3" d="M36.36 163.64"#)
            && out.contains(r#"pathLength="1" stroke-width="10""#),
        "{out}"
    );
    // The arc wears the zone it is in; the summary and the table name it.
    assert!(out.contains(r#"data-kind="gauge" data-color="3""#), "{out}");
    assert!(out.contains("Optimal."), "{out}");
    assert!(
        out.contains(">Status</th>") && out.contains(">Optimal</td>"),
        "{out}"
    );
    // A legend names the zones, and the table's note gives their ends.
    assert!(
        out.contains(r#"data-slot="chart-legend""#) && out.contains(">Overuse<"),
        "{out}"
    );
    assert!(
        out.contains("Zones: Undertraining from 0.00 to 0.80; Optimal from 0.80 to 1.30;"),
        "{out}"
    );
    let over = html(load_gauge(1.62));
    assert!(
        over.contains(r#"data-kind="gauge" data-color="8""#) && over.contains("Overuse."),
        "{over}"
    );
}

#[test]
fn a_gauge_with_zones_links_the_sheet_its_legend_needs() {
    let page = navree_ui::View::new("x").body(load_gauge(1.1)).to_html();
    assert!(page.contains(navree_ui::assets::CHARTS_URL), "{page}");
    let plain = navree_ui::View::new("x")
        .body(Gauge::new("Stress", 40))
        .to_html();
    assert!(!plain.contains(navree_ui::assets::CHARTS_URL), "{plain}");
}

#[test]
fn an_ink_or_a_colour_scale_of_its_own_wins_over_the_zone() {
    let out = html(load_gauge(1.1).ink(Ink::slot(1)));
    assert!(out.contains(r#"data-kind="gauge" data-color="1""#), "{out}");
    assert!(
        out.contains("Optimal."),
        "the zone still names the value: {out}"
    );
}

// ---- A day clock's band of categories ---------------------------------------------------

fn day() -> DayClock {
    let mut steps = [0_u32; 24];
    steps[7] = 1_800;
    steps[18] = 1_200;
    let asleep: Vec<Option<usize>> = (0..24)
        .map(|h| match h {
            0..=5 | 22.. => Some(1),
            6..=21 => Some(0),
            _ => None,
        })
        .collect();
    DayClock::new("Your day")
        .id("day")
        .values(steps)
        .unit("steps")
        .category("Awake", Ink::slot(3))
        .category("Asleep", Ink::slot(7))
        .categories(asleep)
}

#[test]
fn a_band_shows_every_hours_category_an_hour_of_nothing_too() {
    let plain = html(day());
    assert!(!plain.contains("clock-band"), "{plain}");
    let out = html(day().band());
    assert_eq!(
        out.matches(r#"data-slot="clock-band""#).count(),
        24,
        "{out}"
    );
    // Midnight, asleep with no steps: its band segment is in the asleep colour.
    assert!(
        out.contains(r#"<path data-slot="clock-band" data-color="7" data-i="0""#),
        "{out}"
    );
    assert!(
        out.contains(r#"<path data-slot="clock-band" data-color="3" data-i="7""#),
        "{out}"
    );
    // The band lies after the anchors (the tooltip finds an hour's anchor first) and
    // under the wedges, which start outside it.
    let anchors = out.find("clock-anchors").unwrap_or(usize::MAX);
    let band = out.find("clock-bands").unwrap_or(0);
    let wedges = out.find("clock-wedges").unwrap_or(0);
    assert!(anchors < band && band < wedges, "{out}");
    assert!(
        out.contains("A57 57 0 0 0 "),
        "the wedges start at 57: {out}"
    );
    assert!(!plain.contains("A57 57 0 0 0 "), "{plain}");
}

#[test]
fn an_hour_with_no_category_leaves_a_gap_in_the_band() {
    let out = html(day().categories([Some(1), None, Some(1)]).band());
    assert_eq!(out.matches(r#"data-slot="clock-band""#).count(), 2, "{out}");
}

// ---- Bars from a level other than zero --------------------------------------------------

fn ages() -> Chart {
    Chart::bar("Body age by part")
        .id("ages")
        .height(200)
        .categories(["Heart", "Lungs", "Muscle", "Metabolism"])
        .series(Series::new("Age", [31, 38, 34, 41]).unit("years"))
}

/// The `y` and `height` of each bar, in order.
fn bars(out: &str) -> Vec<(f64, f64)> {
    out.split(r#"data-slot="chart-bar""#)
        .skip(1)
        .map(|rest| {
            let attr = |name: &str| -> f64 {
                let at = rest.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
                let end = rest[at..].find('"').unwrap();
                rest[at..at + end].parse().unwrap()
            };
            (attr("y"), attr("height"))
        })
        .collect()
}

#[test]
fn bars_grow_up_and_down_from_a_baseline() {
    let out = html(ages().baseline(36).y_domain(Domain::fixed(18, 60)));
    let drawn = bars(&out);
    assert_eq!(drawn.len(), 4, "{out}");
    // Every bar has one end at the baseline: the lower edge of those above it, the upper
    // edge of those below.
    let tops: Vec<f64> = drawn.iter().map(|(y, _)| *y).collect();
    let feet: Vec<f64> = drawn.iter().map(|(y, h)| y + h).collect();
    let level = feet[1];
    assert!((tops[0] - level).abs() < 0.2, "31 hangs from 36: {drawn:?}");
    assert!((feet[3] - level).abs() < 0.2, "41 stands on 36: {drawn:?}");
    assert!((tops[2] - level).abs() < 0.2, "34 hangs from 36: {drawn:?}");
    // The axis keeps the domain: it does not reach down to zero.
    assert!(!out.contains(">0</text>"), "{out}");
    assert!(
        out.contains(">18</text>") || out.contains(">20</text>"),
        "{out}"
    );
    // A line marks the baseline, and the bars grow from it.
    assert!(out.contains(r#"data-slot="chart-zero""#), "{out}");
    let moving = html(ages().baseline(36).motion(Motion::new()));
    assert!(
        moving.contains(r#"data-motion="grow-down""#)
            && moving.contains(r#"data-motion="grow-up""#),
        "{moving}"
    );
    // The table says the values as they are.
    assert!(out.contains(">41</span>") || out.contains(">41<"), "{out}");
}

#[test]
fn without_a_baseline_bars_still_start_at_zero() {
    let out = html(ages().y_domain(Domain::fixed(18, 60)));
    let drawn = bars(&out);
    let feet: Vec<f64> = drawn.iter().map(|(y, h)| y + h).collect();
    assert!(
        feet.windows(2).all(|w| (w[0] - w[1]).abs() < 0.2),
        "{drawn:?}"
    );
}

#[test]
fn horizontal_bars_grow_either_side_of_a_baseline() {
    let out = html(ages().horizontal().baseline(35));
    assert!(out.contains(r#"data-slot="chart-zero""#), "{out}");
    assert!(
        out.contains(r#"data-motion"#) || out.matches(r#"data-slot="chart-bar""#).count() == 4,
        "{out}"
    );
}

// ---- A value axis in clock time ---------------------------------------------------------

fn bedtimes() -> Chart {
    // Minutes past 6 PM: 11:10 PM, 11:40 PM, 12:25 AM, 10:50 PM.
    Chart::line("To bed")
        .id("bed")
        .categories(["Mon", "Tue", "Wed", "Thu"])
        .series(Series::new("To bed", [310, 340, 385, 290]))
        .format(NumberFormat::clock(18))
}

#[test]
fn a_clock_axis_writes_times_of_day_through_midnight() {
    let out = html(bedtimes());
    // Ticks on whole hours, written as times, across midnight; no tick at 6 PM, as a line
    // of times does not reach down to its origin.
    assert!(out.contains(">11:00 PM</text>"), "{out}");
    assert!(out.contains(">12:00 AM</text>"), "{out}");
    assert!(!out.contains(">6:00 PM</text>"), "{out}");
    // The table and the tooltip write the times.
    assert!(out.contains("12:25 AM"), "{out}");
    assert!(out.contains("11:40 PM"), "{out}");
    // In a language with a 24 hour clock, the axis is on one.
    let de = html(bedtimes().locale("de"));
    assert!(de.contains(">00:00</text>") && de.contains("00:25"), "{de}");
}

#[test]
fn clock_times_wrap_past_midnight_and_before_their_origin() {
    let en = NumberLocale::english();
    let night = NumberFormat::clock(21);
    assert_eq!(night.format(0.0, &en), "9:00 PM");
    assert_eq!(night.format(180.0, &en), "12:00 AM");
    assert_eq!(night.format(605.0, &en), "7:05 AM");
    assert_eq!(night.format(-60.0, &en), "8:00 PM");
    assert_eq!(night.format(f64::NAN, &en), "–");
    assert_eq!(NumberFormat::clock(30).format(0.0, &en), "6:00 AM");
}

// ---- A stepped area over named levels ---------------------------------------------------

fn night() -> Chart {
    Chart::area("Sleep stages")
        .id("night")
        .numbers([0, 30, 60, 90, 120, 150])
        .series(Series::new("Stage", [3, 1, 0, 2, 0, 1]))
        .curve(Curve::Step)
        .y_categories(["Deep", "Core", "REM", "Awake"])
}

fn area_d(out: &str) -> String {
    let at = out.find(r#"data-slot="chart-area""#).unwrap();
    let rest = &out[at..];
    let d = rest.find(" d=\"").unwrap() + 4;
    let end = rest[d..].find('"').unwrap();
    rest[d..d + end].to_owned()
}

#[test]
fn lanes_fill_the_lowest_level_down_to_the_foot_of_its_band() {
    // Without lanes the lowest level is the foot of the plot: deep sleep is a line.
    let plain = html(night());
    let lanes = html(night().lanes());
    let plain_d = area_d(&plain);
    let lanes_d = area_d(&lanes);
    // Over lanes the area closes below the deepest level: its lowest point is lower
    // (a larger y) than any point the line reaches.
    // The y of every point of a path of M, L, H, V and Z.
    let ys = |d: &str| -> Vec<f64> {
        let mut out = Vec::new();
        let mut rest = d;
        while let Some(c) = rest.chars().next() {
            let end = rest[1..]
                .find(|c: char| c.is_ascii_alphabetic())
                .map_or(rest.len(), |e| e + 1);
            let numbers: Vec<f64> = rest[1..end]
                .split([' ', ','])
                .filter_map(|p| p.parse().ok())
                .collect();
            match c {
                'M' | 'L' => out.extend(numbers.get(1)),
                'V' => out.extend(numbers.first()),
                _ => {}
            }
            rest = &rest[end..];
        }
        out
    };
    let lane_ys = ys(&lanes_d);
    let line_low = {
        let at = lanes.find(r#"data-slot="chart-line""#).unwrap();
        let rest = &lanes[at..];
        let d = rest.find(" d=\"").unwrap() + 4;
        let end = rest[d..].find('"').unwrap();
        ys(&rest[d..d + end]).into_iter().fold(f64::MIN, f64::max)
    };
    let floor = lane_ys.iter().copied().fold(f64::MIN, f64::max);
    assert!(floor > line_low + 5.0, "{lanes_d} / line to {line_low}");
    let plain_floor = ys(&plain_d).into_iter().fold(f64::MIN, f64::max);
    let plain_line = {
        let at = plain.find(r#"data-slot="chart-line""#).unwrap();
        let rest = &plain[at..];
        let d = rest.find(" d=\"").unwrap() + 4;
        let end = rest[d..].find('"').unwrap();
        ys(&rest[d..d + end]).into_iter().fold(f64::MIN, f64::max)
    };
    assert!(
        (plain_floor - plain_line).abs() < 0.2,
        "the default is unchanged: {plain_floor} {plain_line} {plain_d}"
    );
    // Each name is a tick in the middle of its lane; the table writes the names.
    for name in ["Deep", "Core", "REM", "Awake"] {
        assert!(
            lanes.contains(&format!(">{name}</text>")),
            "{name}: {lanes}"
        );
    }
}

// ---- A meter's legend: every part, with a second line -----------------------------------

fn zones() -> Meter {
    Meter::new("Time in heart rate zones")
        .id("zones")
        .minutes()
        .detail_name("Heart rate")
        .segment(Segment::new("Zone 1", 22).detail("under 109 bpm"))
        .segment(Segment::new("Zone 2", 31).detail("109–127 bpm"))
        .segment(Segment::new("Zone 3", 12).detail("127–146 bpm"))
        .segment(Segment::new("Zone 4", 0).detail("146–164 bpm"))
        .segment(Segment::new("Zone 5", 0).detail("164 bpm and over"))
}

#[test]
fn a_meter_legend_lists_every_part_with_its_detail_and_empty_ones_hollow() {
    let out = html(zones());
    assert_eq!(
        out.matches(r#"data-slot="chart-legend-item""#).count(),
        5,
        "{out}"
    );
    assert_eq!(out.matches(r#"data-slot="seg-part""#).count(), 3, "{out}");
    assert!(
        out.contains(r#"<span data-slot="chart-legend-detail">109–127 bpm</span>"#),
        "{out}"
    );
    assert_eq!(
        out.matches("data-empty").count(),
        2,
        "the empty zones: {out}"
    );
    // The table has the details in a column of their own, named for them.
    assert!(out.contains(">Heart rate</th>"), "{out}");
    assert!(out.contains(">164 bpm and over</td>"), "{out}");
    // A meter with no detail has no such column.
    let plain = html(Meter::new("x").segment(Segment::new("a", 3)));
    assert!(
        !plain.contains("Detail") && !plain.contains("chart-legend-detail"),
        "{plain}"
    );
}

// ---- A ranked list of relations ---------------------------------------------------------

fn found() -> RelationList {
    RelationList::new("What moves together")
        .id("found")
        .relation(
            "Caffeine",
            "Deep sleep",
            Relation::new(-0.21).significant(false),
        )
        .relation(
            "Sleep",
            "Next-day HRV",
            Relation::new(0.62).note("30 days, p < 0.001"),
        )
        .relation("Steps", "Stress", Relation::new(-0.48).note("28 days"))
        .relation("Mood", "Alcohol", Relation::untested())
        .value_name("Correlation (r)")
}

#[test]
fn relations_are_ranked_by_size_with_a_bar_from_the_centre_by_sign() {
    let out = html(found());
    let order: Vec<usize> = ["Sleep", "Steps", "Caffeine", "Mood"]
        .iter()
        .map(|n| {
            out.find(&format!(r#"data-slot="relation-pair">{n} "#))
                .unwrap()
        })
        .collect();
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "strongest first: {out}"
    );
    // +0.62 runs right of the centre to 62 of 100; -0.48 runs left to 48.
    assert!(
        out.contains(
            r#"<rect data-slot="relation-fill" x="100" y="0" width="62" height="8" data-color="1">"#
        ),
        "{out}"
    );
    assert!(
        out.contains(
            r#"<rect data-slot="relation-fill" x="52" y="0" width="48" height="8" data-color="2">"#
        ),
        "{out}"
    );
    assert!(
        out.contains(">+0.62</span>") && out.contains(">-0.48</span>"),
        "{out}"
    );
    // Not significant: neutral and said; never tested: last, with no bar.
    assert!(
        out.contains(r#"data-result="ns""#) && out.contains("Not significant"),
        "{out}"
    );
    assert_eq!(
        out.matches(r#"data-slot="relation-bar""#).count(),
        3,
        "{out}"
    );
    // The note, the summary and the table.
    assert!(
        out.contains(r#"data-slot="relation-note">30 days, p &lt; 0.001<"#),
        "{out}"
    );
    assert!(
        out.contains("4 pairs, 2 significant. Strongest: Sleep ↔ Next-day HRV, +0.62."),
        "{out}"
    );
    assert!(out.contains(">Correlation (r)</th>"), "{out}");
    assert_eq!(
        out.matches("<tr>").count(),
        5,
        "a head and every pair: {out}"
    );
}

#[test]
fn only_significant_relations_are_listed_and_the_table_keeps_them_all() {
    let out = html(found().only_significant());
    assert_eq!(out.matches(r#"data-slot="relation""#).count(), 2, "{out}");
    assert_eq!(out.matches("<tr>").count(), 5, "{out}");
    let empty = html(RelationList::new("Nothing yet"));
    assert!(empty.contains(r#"data-state="empty""#), "{empty}");
}

#[test]
fn a_relation_list_links_the_grids_sheet_and_writes_no_style() {
    let page = navree_ui::View::new("x").body(found()).to_html();
    assert!(page.contains(navree_ui::assets::CHART_GRIDS_URL), "{page}");
    assert!(page.contains(navree_ui::assets::CHARTS_URL), "{page}");
    assert!(!html(found()).contains("style="));
}
