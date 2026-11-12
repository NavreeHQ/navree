//! The chart primitives in use: paint, motion, axes, marks, and the upgrades they bring
//! to Chart, Sparkline, Progress and Bars. What each writes, what each links, and that
//! the chart reads the same without any of it.

use navree_ui::charts::*;
use navree_ui::{Bars, Node, Progress, Tone, Uses, View, assets, render};

fn html(node: impl Into<Node>) -> String {
    render(&node.into())
}

fn uses(node: impl Into<Node>) -> Uses {
    View::new("x").body(node.into()).uses()
}

fn hours() -> Vec<String> {
    (0..24).map(|h| format!("{h:02}:00")).collect()
}

/// The text between `start` and the next `end` after it.
fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    text.split(start)
        .nth(1)
        .and_then(|rest| rest.split(end).next())
        .unwrap_or_default()
}

/// Pages import the toolkit and its charts by glob side by side, so no chart name may be
/// one the toolkit's root already has: this module would not compile.
#[allow(unused_imports, dead_code)]
mod both_globs {
    use navree_ui::charts::*;
    use navree_ui::*;

    fn names() {
        let _ = Grid::cols(2);
        let _ = Tooltip::new("t", "Trigger", "Words");
        let _ = Gridlines::none();
        let _ = TooltipStyle::big();
        let _ = (Axis::new(), Ticks::auto(), Domain::auto(), Dots::All, Curve::Step);
        let _ = (Ink::slot(1), Gradient::vertical(), Motion::new(), Place::EndAbove);
        let _ = (Reference::zero(), Zone::new(0, 1), ActiveDot::Halo, Speed::Fast);
        let _ = (ColorScale::new(Ink::muted()), CountUp::new(1, "1"));
    }
}

#[test]
fn a_gradient_area_names_its_gradient_after_the_chart_and_paints_with_it() {
    let chart = Chart::area("Heart rate")
        .id("hr")
        .categories(["06:00", "06:05", "06:10", "06:15"])
        .series(
            Series::new("Heart rate", [64, 71, 69, 75])
                .ink(Ink::slot(8))
                .fade()
                .stroke(Gradient::horizontal().stop(0.0, Ink::own()).stop(0.5, Ink::own().vivid()).stop(1.0, Ink::own())),
        );
    let out = html(chart.clone());
    assert!(out.contains(r#"<linearGradient id="hr-p0""#), "{out}");
    assert!(out.contains(r#"<linearGradient id="hr-p1" gradientUnits="userSpaceOnUse""#), "{out}");
    assert!(out.contains(r#"fill="url(#hr-p0)""#), "{out}");
    assert!(out.contains(r#"stroke="url(#hr-p1)""#), "{out}");
    // The series' own colour in the stops, vivid in the middle; no colour anywhere.
    assert!(out.contains(r#"<stop offset="0.5" data-color="8" data-mix="vivid">"#), "{out}");
    assert!(!out.contains('#') || !out.contains("stop-color"), "{out}");
    assert!(!out.contains("style="), "{out}");
    assert!(uses(chart).has(Uses::CHARTS.with(Uses::CHART_PAINT)));
    // A plain chart links no paint sheet.
    let plain = Chart::area("x").categories(["a", "b"]).series(Series::new("a", [1, 2]));
    assert!(!uses(plain).meets(Uses::CHART_PAINT));
}

#[test]
fn two_charts_never_share_a_gradient() {
    let make = |title: &str| {
        html(
            Chart::area(title)
                .categories(["a", "b"])
                .series(Series::new("a", [1, 2]).fade()),
        )
    };
    let (a, b) = (make("Sleep"), make("Steps"));
    let id = |out: &str| between(out, "<linearGradient id=\"", "\"").to_owned();
    assert_ne!(id(&a), id(&b));
    assert!(id(&a).starts_with("chart-sleep-"), "{}", id(&a));
}

#[test]
fn glow_puts_a_line_on_a_layer_of_its_own() {
    let out = html(
        Chart::line("Recovery")
            .categories(["D1", "D2", "D3"])
            .series(Series::new("Recovery", [4, -2, 6]).glow())
            .series(Series::new("Plain", [1, 2, 3])),
    );
    assert_eq!(out.matches("data-slot=\"chart-marks\"").count(), 2, "{out}");
    let glowing = between(&out, "data-glow", "</svg>");
    assert!(glowing.contains("data-slot=\"chart-line\""), "{out}");
}

#[test]
fn axes_and_grid_can_each_be_switched() {
    let base = || {
        Chart::line("Trend")
            .categories(["a", "b", "c", "d"])
            .series(Series::new("v", [1, 5, 3, 8]))
    };
    let plain = html(base());
    assert!(plain.contains("chart-y-axis") && plain.contains("chart-grid"));
    let bare = html(base().x_axis(Axis::hidden()).y_axis(Axis::hidden()).grid(Gridlines::none()));
    assert!(!bare.contains("chart-y-axis"), "{bare}");
    assert!(!bare.contains("chart-grid"), "{bare}");
    assert!(bare.contains("data-bare"), "{bare}");
    assert!(!between(&bare, "chart-x-axis", "</g>").contains("<text"), "{bare}");
    // Without the labels' band the drawing is shorter.
    let height = |out: &str| between(out, "data-slot=\"chart-plot\" width=\"100%\" height=\"", "\"").to_owned();
    assert!(height(&bare).parse::<u32>().unwrap() < height(&plain).parse::<u32>().unwrap());
    let lined = html(base().x_axis(Axis::new().line().ticks()).y_axis(Axis::new().line()).grid(Gridlines::both().dashed()));
    assert_eq!(lined.matches("chart-axis-line").count(), 2, "{lined}");
    assert!(lined.contains("chart-tick"), "{lined}");
    assert!(lined.contains("data-dashed"), "{lined}");
    // Down lines stand at the x labels.
    let down = html(base().grid(Gridlines::vertical()));
    assert!(down.contains(r#"data-slot="chart-grid" x1="0%" x2="0%""#) || down.contains(r#"x1="0%""#), "{down}");
}

#[test]
fn sparse_ticks_keep_what_they_are_asked_to() {
    let out = html(
        Chart::bar("Stress by hour")
            .categories(hours())
            .series(Series::new("Stress", (0..24).map(|h| (h % 7) as f64).collect::<Vec<_>>()))
            .x_ticks(Ticks::every(3)),
    );
    let axis = between(&out, "data-slot=\"chart-x-axis\"", "</g>");
    assert_eq!(axis.matches("<text").count(), 8, "{axis}");
    assert!(axis.contains(">00:00<") && axis.contains(">21:00<"), "{axis}");
    assert!(!axis.contains("data-minor"), "labels picked by the page stay: {axis}");
    let ends = html(
        Chart::bar("Stress by hour")
            .categories(hours())
            .series(Series::new("Stress", vec![1; 24]))
            .x_ticks(Ticks::every(5).keep_ends()),
    );
    let axis = between(&ends, "data-slot=\"chart-x-axis\"", "</g>");
    assert!(axis.contains(">23:00<"), "{axis}");
}

#[test]
fn domains_set_the_value_axis() {
    let hr = html(
        Chart::line("Heart rate")
            .categories(["a", "b", "c"])
            .series(Series::new("bpm", [62, 118, 75]))
            .y_domain(Domain::padded(10)),
    );
    let axis = between(&hr, "chart-y-axis", "</svg>");
    assert!(!axis.contains(">0<"), "a padded domain doesn't start at zero: {axis}");
    let fixed = html(
        Chart::bar("Stress")
            .categories(["a", "b"])
            .series(Series::new("s", [3, 7]))
            .y_domain(Domain::fixed(0, 10)),
    );
    let axis = between(&fixed, "chart-y-axis", "</svg>");
    assert!(axis.contains(">10<") && !axis.contains(">12<"), "{axis}");
}

#[test]
fn references_and_zones_are_drawn_and_named_in_the_summary() {
    let out = html(
        Chart::bar("Stress by hour")
            .categories(["06:00", "07:00", "08:00", "09:00"])
            .series(Series::new("Stress", [2, 4, 6, 8]))
            .reference(Reference::average().label("Avg: {value}"))
            .reference(Reference::level(7).label("High").ink(Ink::tone(Tone::Warning)).place(Place::StartBelow).solid())
            .zone(Zone::new(0, 3).label("Calm").ink(Ink::tone(Tone::Success))),
    );
    assert_eq!(out.matches("data-slot=\"chart-ref\"").count(), 2, "{out}");
    assert!(out.contains(">Avg: 5</text>"), "{out}");
    assert!(out.contains(r#"data-slot="chart-ref" data-solid="" data-tone="warning""#), "{out}");
    assert!(out.contains(r#"data-slot="chart-zone" data-tone="success""#), "{out}");
    assert!(out.contains("Marked: Avg: 5, High (7), Calm (0 – 3)."), "{out}");
    // The zero baseline and the identity diagonal.
    let zero = html(
        Chart::area("Recovery")
            .categories(["D1", "D2", "D3"])
            .series(Series::new("Recovery", [4, -3, 6]))
            .reference(Reference::zero()),
    );
    assert!(zero.contains("chart-ref"), "{zero}");
    let identity = html(
        Chart::line("Next against this")
            .numbers([600, 700, 800, 900])
            .series(Series::new("Next", [650, 720, 790, 880]))
            .y_zero(false)
            .reference(Reference::identity()),
    );
    let line = between(&identity, "data-slot=\"chart-ref\"><line", "/>");
    assert!(line.contains("x1=") && line.contains("x2=\"100%\""), "{identity}");
    // No numbers along x: no diagonal, and nothing breaks.
    let none = html(
        Chart::line("x")
            .categories(["a", "b"])
            .series(Series::new("a", [1, 2]))
            .reference(Reference::identity()),
    );
    assert!(!none.contains("chart-ref"), "{none}");
}

#[test]
fn a_step_curve_over_named_levels_reads_as_names() {
    let out = html(
        Chart::area("Sleep stages")
            .categories(["23:10", "23:40", "00:30", "01:15", "02:05"])
            .series(
                Series::new("Stage", [1, 0, 1, 2, 3]).colors(
                    ColorScale::new(Ink::slot(7).vivid())
                        .label("Deep")
                        .from(1, Ink::slot(1))
                        .label("Light")
                        .from(2, Ink::slot(7))
                        .label("REM")
                        .from(3, Ink::tone(Tone::Danger))
                        .label("Awake"),
                ),
            )
            .curve(Curve::Step)
            .y_categories(["Deep", "Light", "REM", "Awake"])
            .detail("Duration", ["30 min", "50 min", "45 min", "50 min", "5 min"])
            .tooltip(TooltipStyle::big()),
    );
    let axis = between(&out, "chart-y-axis", "</svg>");
    for name in ["Deep", "Light", "REM", "Awake"] {
        assert!(axis.contains(&format!(">{name}<")), "{axis}");
    }
    let line = between(&out, "data-slot=\"chart-line\"", "/>");
    assert!(line.contains('H') && line.contains('V') && !line.contains('C'), "{line}");
    // The table writes the names, each in its colour, and the details after them.
    let table = between(&out, "<tbody>", "</tbody>");
    assert!(table.contains(r#"<span data-tone="danger">Awake</span>"#), "{table}");
    assert!(table.contains("50 min"), "{table}");
    let head = between(&out, "<thead>", "</thead>");
    assert!(head.contains(">Duration<"), "{head}");
    // The tooltip is big, and its detail line has no key.
    let tip = between(&out, "data-slot=\"chart-tip\"", "</div></div>");
    assert!(out.contains(r#"data-slot="chart-tip" aria-hidden="true" data-big="""#), "{out}");
    assert!(tip.contains(r#"<strong></strong><span>Duration</span>"#), "{tip}");
    // The legend shows the bands, not the series.
    let legend = between(&out, "data-slot=\"chart-legend\"", "</ul>");
    assert!(legend.contains(">Deep<") && legend.contains(">Awake<") && !legend.contains(">Stage<"), "{legend}");
    // The summary speaks in names.
    assert!(out.contains("Stage: Light at 23:10, Awake at 02:05."), "{out}");
}

#[test]
fn bars_take_the_colour_of_their_band() {
    let out = html(
        Chart::bar("Recovery")
            .categories(["Mon", "Tue", "Wed"])
            .series(Series::new("Recovery", [30, 60, 90]).colors(
                ColorScale::new(Ink::tone(Tone::Danger))
                    .from(50, Ink::tone(Tone::Warning))
                    .from(75, Ink::tone(Tone::Success)),
            )),
    );
    let bars = between(&out, "data-slot=\"chart-bars\"", "</g>");
    assert!(bars.contains("data-tone=\"danger\""), "{bars}");
    assert!(bars.contains("data-tone=\"warning\""), "{bars}");
    assert!(bars.contains("data-tone=\"success\""), "{bars}");
    // Unlabelled bands: no legend for one series.
    assert!(!out.contains("chart-legend"), "{out}");
}

#[test]
fn a_unit_follows_the_value_in_the_tooltip_and_the_name_in_the_table() {
    let out = html(
        Chart::area("Heart rate")
            .categories(["a", "b"])
            .series(Series::new("Heart rate", [60, 70]).unit("BPM")),
    );
    assert!(out.contains("<strong></strong><em>BPM</em><span>Heart rate</span>"), "{out}");
    assert!(between(&out, "<thead>", "</thead>").contains("Heart rate (BPM)"), "{out}");
}

#[test]
fn dots_and_the_active_dot() {
    let base = || Chart::line("x").categories(["a", "b", "c"]).series(Series::new("a", [1, 3, 2]));
    let all = html(base().dots(Dots::All).active_dot(ActiveDot::Halo));
    assert_eq!(all.matches("data-slot=\"chart-point\"").count(), 3, "{all}");
    assert!(all.contains(r#"data-dot="halo""#), "{all}");
    let none = html(base().dots(Dots::None));
    assert!(!none.contains("chart-end") && !none.contains("chart-point"), "{none}");
    let end = html(base());
    assert_eq!(end.matches("data-slot=\"chart-end\"").count(), 1);
}

#[test]
fn motion_is_attributes_only_and_off_by_default() {
    let base = || {
        Chart::bar("Stress")
            .categories(["a", "b", "c"])
            .series(Series::new("s", [3, -2, 5]))
    };
    let still = html(base());
    assert!(!still.contains("data-motion"), "{still}");
    let moving = html(base().motion(Motion::new().delay(100).stagger(50)));
    assert!(moving.contains(r#"data-motion="grow-up" data-delay="2""#), "{moving}");
    assert!(moving.contains(r#"data-motion="grow-down" data-delay="3""#), "{moving}");
    assert!(!moving.contains("style="));
    let line = html(
        Chart::line("x")
            .categories(["a", "b"])
            .series(Series::new("a", [1, 2]))
            .motion(Motion::new()),
    );
    assert!(line.contains(r#"data-slot="chart-marks""#) && line.contains(r#"data-motion="wipe""#), "{line}");
    let rtl = html(
        Chart::line("x")
            .categories(["a", "b"])
            .series(Series::new("a", [1, 2]))
            .rtl()
            .motion(Motion::new()),
    );
    assert!(rtl.contains(r#"data-motion="wipe-back""#), "{rtl}");
}

#[test]
fn a_painted_sparkline_links_the_paint_sheet_and_a_plain_one_nothing() {
    let plain = Sparkline::new("Trend", [1, 2, 3]);
    assert!(uses(plain).is_none());
    let tone = Sparkline::new("Trend", [1, 2, 3]).tone(Tone::Danger).area();
    assert!(uses(tone).is_none());
    let smooth = Sparkline::new("Movement", [82, 75, 88, 92, 78]).smooth().fade().ink(Ink::slot(2)).points().motion(Motion::new());
    assert_eq!(uses(smooth.clone()), Uses::CHART_PAINT);
    let out = html(smooth);
    assert!(out.contains("C"), "a curve: {out}");
    assert!(out.contains("<linearGradient"), "{out}");
    assert_eq!(out.matches("data-slot=\"chart-point\"").count(), 4, "every point but the end: {out}");
    assert!(out.contains(r#"data-motion="wipe""#), "{out}");
}

#[test]
fn sparkline_columns_stand_on_zero_in_their_bands_colour() {
    let out = html(
        Sparkline::new("Recovery, 7 days", [40, 60, 75, 0, 85, 94, 80])
            .columns()
            .fade()
            .colors(ColorScale::new(Ink::tone(Tone::Warning)).from(70, Ink::tone(Tone::Success)))
            .motion(Motion::new()),
    );
    assert_eq!(out.matches("data-slot=\"chart-column\"").count(), 7, "{out}");
    assert!(out.contains("data-tone=\"warning\"") && out.contains("data-tone=\"success\""), "{out}");
    // One gradient for each colour the columns take.
    assert_eq!(out.matches("<linearGradient").count(), 2, "{out}");
    assert!(out.contains("data-motion=\"grow-up\""), "{out}");
    // Still a picture with its summary and table.
    assert!(out.contains("role=\"img\"") && out.contains("<table"), "{out}");
}

#[test]
fn a_painted_progress_bar_is_a_meter_with_the_same_values() {
    let plain = html(Progress::new(3, 8, "Profile complete"));
    assert!(plain.starts_with("<progress"), "{plain}");
    let out = html(
        Progress::new(402, 480, "Sleep, 6 h 42 min of 8 h")
            .fill(Gradient::horizontal().stop(0.0, Ink::slot(7)).stop_with_opacity(1.0, Ink::slot(7), 0.6))
            .goal(420)
            .glow()
            .motion(Motion::new())
            .height(8),
    );
    assert!(out.starts_with(r#"<div aria-label="Sleep, 6 h 42 min of 8 h" data-slot="meter" role="progressbar" aria-valuemin="0" aria-valuemax="480" aria-valuenow="402""#), "{out}");
    assert!(out.contains(r#"data-slot="meter-fill" width="83.75%""#), "{out}");
    assert!(out.contains(r#"data-slot="meter-goal" x1="87.5%""#), "{out}");
    assert!(out.contains("fill=\"url(#meter-"), "{out}");
    assert!(out.contains("data-glow") && out.contains("data-motion=\"grow-right\""), "{out}");
    // The colour of its band.
    let banded = html(
        Progress::new(30, 100, "Recovery 30%")
            .tone(Tone::Info)
            .colors(ColorScale::new(Ink::tone(Tone::Danger)).from(50, Ink::tone(Tone::Success))),
    );
    assert!(banded.contains("data-tone=\"danger\"") && !banded.contains("data-tone=\"info\""), "{banded}");
    // A value with it shows its percentage beside the meter.
    let row = html(Progress::new(1, 4, "x").ink(Ink::slot(3)).show_value());
    assert!(row.contains("progress-row") && row.contains("data-slot=\"meter\"") && row.contains(">25%<"), "{row}");
    // No value, no fill; an id names the gradient.
    let empty = html(Progress::new(0, 4, "x").id("sleep").fill(Gradient::fade(Ink::own())));
    assert!(!empty.contains("meter-fill"), "{empty}");
    assert!(uses(Progress::new(1, 2, "x").goal(1)).has(Uses::CHART_PAINT));
}

#[test]
fn painted_bars_are_columns_in_percent() {
    let plain = html(Bars::new("Events", &[1, 2, 3]));
    assert!(plain.contains("viewBox"), "{plain}");
    let out = html(
        Bars::new("HRV, 14 days", &[48, 52, 0, 61])
            .colors(ColorScale::new(Ink::tone(Tone::Warning)).from(50, Ink::tone(Tone::Success)))
            .pill()
            .motion(Motion::new()),
    );
    assert!(!out.contains("viewBox"), "{out}");
    assert!(out.contains(r#"x="25%" width="25%""#), "{out}");
    assert!(out.contains("round 99px"), "{out}");
    assert!(out.contains("data-tone=\"warning\"") && out.contains("data-tone=\"success\""), "{out}");
    assert!(out.contains("data-motion=\"grow-up\""), "{out}");
}

#[test]
fn a_count_up_loads_its_module_and_the_paint_sheet() {
    let node = CountUp::new(87, "87");
    assert_eq!(uses(node), Uses::COUNT_UP.with(Uses::CHART_PAINT));
    let page = View::new("x").body(CountUp::new(87, "87")).to_html();
    assert!(page.contains(assets::COUNT_UP_URL), "{page}");
    assert!(page.contains(assets::CHART_PAINT_URL), "{page}");
    assert!(!page.contains(assets::CHARTS_URL), "{page}");
}

#[test]
fn the_paint_sheet_holds_no_colour_and_motion_only_without_a_request_for_less() {
    let sheet = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/extra/chart-paint.css")).unwrap();
    let code: String = sheet
        .lines()
        .filter(|l| !l.trim_start().starts_with("/*"))
        .collect();
    assert!(!code.contains('#'), "no colour literal");
    assert!(!code.contains("rgb(") && !code.contains("hsl(") && !code.contains("oklch("), "no colour literal");
    // Every animation is declared inside the media query that asks for motion.
    let (outside, inside) = code.split_once("@media (prefers-reduced-motion:no-preference)").unwrap();
    assert!(!outside.contains("animation"), "{outside}");
    assert!(inside.contains("animation"));
}

#[test]
fn hostile_words_in_new_marks_stay_text() {
    let bad = "\"><script>alert(1)</script>";
    let out = html(
        Chart::bar(bad)
            .categories([bad, "b"])
            .series(Series::new(bad, [1, 2]).unit(bad).colors(ColorScale::new(Ink::slot(2)).label(bad)))
            .reference(Reference::average().label(bad))
            .zone(Zone::new(0, 1).label(bad))
            .detail(bad, [bad, bad])
            .y_categories([bad, bad]),
    );
    assert!(!out.contains("<script"), "{out}");
    let out = html(CountUp::new(1, bad));
    assert!(!out.contains("<script"), "{out}");
}

#[test]
fn odd_data_never_breaks_the_new_options() {
    let values: Vec<Option<f64>> = vec![None, Some(f64::NAN), Some(1e300), Some(-1e300), Some(0.0), None];
    for curve in [Curve::Linear, Curve::Monotone, Curve::Step] {
        for domain in [Domain::auto(), Domain::data(), Domain::padded(10), Domain::relative(0.2), Domain::fixed(0, 10)] {
            let out = html(
                Chart::area("x")
                    .series(Series::new("a", values.clone()).fade().glow().colors(ColorScale::new(Ink::slot(1)).from(0, Ink::slot(2))))
                    .curve(curve)
                    .y_domain(domain)
                    .dots(Dots::All)
                    .x_ticks(Ticks::most(3).keep_ends())
                    .reference(Reference::average())
                    .reference(Reference::identity())
                    .zone(Zone::new(f64::NAN, 5))
                    .motion(Motion::new()),
            );
            assert!(!out.contains("NaN") && !out.contains("inf"), "{out}");
        }
    }
    let out = html(Sparkline::new("x", values.clone()).columns().smooth().points());
    assert!(!out.contains("NaN"), "{out}");
    let out = html(Chart::line("x").series(Series::new("a", [1])).y_categories(Vec::<String>::new()));
    assert!(out.contains("<svg"), "{out}");
}
