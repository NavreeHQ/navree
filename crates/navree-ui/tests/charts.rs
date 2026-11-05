//! Charts: what each draws, what it ships, and that no data and no label can break
//! one.

use chrono::{Duration, NaiveDate, NaiveDateTime};
use navree_ui::charts::*;
use navree_ui::{Node, Stat, Tone, Uses, View, assets, render};

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn html(node: impl Into<Node>) -> String {
    render(&node.into())
}

/// The rows of the table twin.
fn table_rows(html: &str) -> usize {
    let body = html.split("<tbody>").nth(1).unwrap_or_default();
    body.split("</tbody>")
        .next()
        .unwrap_or_default()
        .matches("<tr>")
        .count()
}

#[test]
fn every_chart_type_draws_a_figure_with_its_table() {
    let week: Vec<NaiveDate> = (1..=7).map(day).collect();
    let charts: Vec<(&str, Node, usize)> = vec![
        (
            "line",
            Chart::line("Opens")
                .days(week.clone())
                .series(Series::new("Opens", [1, 2, 3, 4, 5, 6, 7]))
                .into(),
            7,
        ),
        (
            "area",
            Chart::area("Users")
                .days(week.clone())
                .stacked()
                .series(Series::new("New", [1, 2, 3, 4, 5, 6, 7]))
                .series(Series::new("Back", [7, 6, 5, 4, 3, 2, 1]))
                .into(),
            7,
        ),
        (
            "bar",
            Chart::bar("Sent")
                .categories(["a", "b", "c"])
                .series(Series::new("Sent", [3, 1, 2]))
                .into(),
            3,
        ),
        (
            "bar-horizontal",
            Chart::bar("Rate")
                .horizontal()
                .categories(["a", "b"])
                .series(Series::new("Rate", [0.2, 0.4]))
                .into(),
            2,
        ),
        (
            "donut",
            Donut::new("Delivery")
                .part("Delivered", 90)
                .part("Bounced", 10)
                .into(),
            2,
        ),
        (
            "funnel",
            Funnel::new("Funnel")
                .stage(Stage::new("Sent", 100))
                .stage(Stage::new("Opened", 40))
                .stage(Stage::new("Clicked", 9))
                .into(),
            3,
        ),
        ("sparkline", Sparkline::new("Trend", [1, 3, 2, 5]).into(), 4),
    ];
    for (kind, node, rows) in charts {
        let out = html(node);
        assert!(
            out.starts_with("<figure data-slot=\"chart\""),
            "{kind}: {out}"
        );
        assert!(out.contains(&format!("data-kind=\"{kind}\"")), "{kind}");
        assert!(out.contains("<svg"), "{kind}");
        assert!(
            out.contains("<caption>"),
            "{kind}: the table is named by the chart's title"
        );
        assert_eq!(
            table_rows(&out),
            rows,
            "{kind}: one table row per value drawn"
        );
        assert!(!out.contains("style="), "{kind}: no style attribute");
        // The tooltip the module fills in has a line for each value column of the table, and
        // every row of the table has a cell for each: the module reads the one from the other.
        let columns = out.matches("<th scope=\"col\"").count() - 1;
        let tip = out.contains("data-slot=\"chart-tip\"");
        assert_eq!(
            tip,
            kind != "sparkline",
            "{kind}: only a chart with keys has a tooltip"
        );
        if tip {
            assert_eq!(
                out.matches("data-tip=\"row\"").count(),
                columns,
                "{kind}: a line for each column"
            );
            assert_eq!(out.matches("data-tip=\"head\"").count(), 1, "{kind}");
        }
        let body = out.split("<tbody>").nth(1).unwrap_or_default();
        for row in body.split("<tr>").skip(1) {
            assert_eq!(
                row.matches("<td").count(),
                columns,
                "{kind}: a cell for each column in {row}"
            );
        }
    }
}

#[test]
fn a_chart_loads_the_charts_module_and_nothing_else() {
    let page = View::new("x").body(
        Chart::bar("Sent")
            .categories(["a"])
            .series(Series::new("Sent", [1])),
    );
    assert_eq!(page.uses(), Uses::CHARTS);
    let out = page.to_html();
    assert!(out.contains(assets::CHARTS_URL), "{out}");
    assert!(
        !out.contains(assets::CORE_URL),
        "a chart needs no core runtime"
    );
    // A sparkline is a picture with no keys: it ships no script, and the toolkit's own sheet
    // draws it.
    let page =
        View::new("x").body(Stat::new("Opened", "4").chart(Sparkline::new("Trend", [1, 2, 3])));
    assert!(page.uses().is_none(), "{:?}", page.uses());
    let out = page.to_html();
    assert!(!out.contains("<script"), "{out}");
    assert!(!out.contains(assets::CHARTS_STYLES_URL), "{out}");
    // The states have nothing to point at, but they are drawn with the charts sheet and the
    // chart that replaces one needs the module.
    for chart in [
        Chart::line("x").loading(),
        Chart::line("x").failed(None),
        Chart::line("x").series(Series::new("a", Vec::<f64>::new())),
    ] {
        assert_eq!(View::new("x").body(chart).uses(), Uses::CHARTS);
    }
    let state = View::new("x")
        .body(Stat::new("Opened", "4").chart(Sparkline::new("Trend", [1, 2, 3]).loading()));
    assert_eq!(state.uses(), Uses::CHARTS);
}

/// The charts' styles are a sheet of their own, linked after the toolkit's own on a page that
/// draws a chart, and on no other.
#[test]
fn the_charts_sheet_is_linked_where_a_chart_is_drawn_and_nowhere_else() {
    let chart = View::new("x").body(
        Chart::bar("Sent")
            .categories(["a"])
            .series(Series::new("Sent", [1])),
    );
    let out = chart.to_html();
    let toolkit = out
        .find(assets::NAVREE_UI_URL)
        .expect("the toolkit's own sheet");
    let charts = out
        .find(assets::CHARTS_STYLES_URL)
        .expect("the charts sheet");
    assert!(
        toolkit < charts,
        "the charts sheet comes after the toolkit's: {out}"
    );
    for page in [
        View::new("x").body(Stat::new("Opened", "4").chart(Sparkline::new("Trend", [1, 2, 3]))),
        View::new("x").body(navree_ui::el("p").child("No chart here")),
    ] {
        assert!(!page.to_html().contains(assets::CHARTS_STYLES_URL));
    }
}

/// The charts sheet keeps the rule of every module's own sheet, 3 KB gzip, and the toolkit's
/// sheet holds only what a sparkline is drawn with.
#[test]
fn the_charts_sheet_is_at_most_3_kb_gzip_and_the_toolkits_holds_only_a_sparklines() {
    let asset = assets::get("charts-styles").expect("the charts sheet");
    let size = asset.gzip.map_or(asset.bytes.len(), <[u8]>::len);
    assert!(size <= 3 * 1024, "{size}");
    let toolkit = assets::get("navree-ui").expect("the toolkit's sheet");
    let css = std::str::from_utf8(toolkit.bytes).expect("text");
    assert!(
        css.contains("chart-line"),
        "a sparkline is drawn by the toolkit's sheet"
    );
    assert!(
        !css.contains("chart-legend"),
        "the rest of the charts' styles are in their own sheet"
    );
}

#[test]
fn a_legend_comes_with_two_series_and_mirrors_the_mark() {
    let one = html(
        Chart::line("x")
            .categories(["a", "b"])
            .series(Series::new("A", [1, 2])),
    );
    assert!(
        !one.contains("chart-legend"),
        "one series: the title names it"
    );
    let two = html(
        Chart::line("x")
            .categories(["a", "b"])
            .series(Series::new("A", [1, 2]))
            .series(Series::new("B", [2, 1]).tone(Tone::Danger)),
    );
    assert!(two.contains("data-slot=\"chart-legend\""));
    assert!(two.contains("data-key=\"line\""), "a line's key is a line");
    assert!(two.contains("data-tone=\"danger\""));
    assert!(
        two.contains("lucide-circle-alert") || two.contains("data-slot=\"icon\""),
        "a tone has its icon"
    );
    let bars = html(
        Chart::bar("x")
            .categories(["a"])
            .series(Series::new("A", [1]))
            .series(Series::new("B", [2])),
    );
    assert!(
        bars.contains("data-key=\"block\""),
        "a bar's key is a block"
    );
}

#[test]
fn toggles_are_checkboxes_tied_to_their_series() {
    let out = html(
        Chart::line("Opens")
            .id("opens")
            .categories(["a", "b"])
            .toggles()
            .series(Series::new("A", [1, 2]))
            .series(Series::new("B", [2, 1])),
    );
    assert!(
        out.contains(
            r#"<input type="checkbox" data-slot="chart-toggle" id="opens-s0" data-s="0" checked>"#
        ),
        "{out}"
    );
    assert!(out.contains(r#"<label for="opens-s1">"#));
    // A stack loses a layer if one is hidden: no switches there.
    let stacked = html(
        Chart::bar("x")
            .categories(["a"])
            .stacked()
            .toggles()
            .series(Series::new("A", [1]))
            .series(Series::new("B", [2])),
    );
    assert!(!stacked.contains("chart-toggle"));
}

#[test]
fn the_table_switch_and_names_point_at_each_other() {
    let out = html(
        Chart::bar("Sent")
            .id("sent")
            .categories(["a"])
            .series(Series::new("Sent", [1])),
    );
    for needle in [
        r#"id="sent""#,
        r#"aria-labelledby="sent-title""#,
        r#"id="sent-title""#,
        r#"aria-describedby="sent-summary""#,
        r#"id="sent-summary""#,
        r#"data-keys="sent-keys""#,
        r#"id="sent-keys" hidden"#,
        r#"id="sent-table-switch""#,
        r#"for="sent-table-switch""#,
        r#"role="group" aria-label="Sent, chart""#,
        r#"aria-live="polite""#,
    ] {
        assert!(out.contains(needle), "{needle} in {out}");
    }
    let shown = html(
        Chart::bar("x")
            .categories(["a"])
            .series(Series::new("a", [1]))
            .table(TableMode::Shown),
    );
    assert!(shown.contains("data-shown"));
    assert!(!shown.contains("chart-table-switch"));
}

#[test]
fn summaries_say_what_the_chart_shows() {
    let out = html(
        Chart::line("Opens")
            .days((1..=4).map(day))
            .series(Series::new("Opens", [10, 40, 25, 30]))
            .annotation(Annotation::target(35, "Goal")),
    );
    assert!(
        out.contains("Opens: 10 at Nov 1, 2026, 30 at Nov 4, 2026. Highest 40 at Nov 2, 2026, lowest 10 at Nov 1, 2026. Marked: Goal (35)."),
        "{out}"
    );
    let bars = html(
        Chart::bar("Sent")
            .categories(["a", "b"])
            .series(Series::new("Sent", [3, 9])),
    );
    assert!(
        bars.contains("Sent: highest 9 for b, lowest 3 for a, total 12."),
        "{bars}"
    );
    let donut = html(Donut::new("D").part("Big", 3).part("Small", 1));
    assert!(
        donut.contains("2 parts, total 4. Largest: Big, 75%."),
        "{donut}"
    );
    let funnel = html(
        Funnel::new("F")
            .stage(Stage::new("Sent", 200))
            .stage(Stage::new("Won", 5)),
    );
    assert!(
        funnel.contains("From Sent, 200, to Won, 5: 2.5% of the first stage."),
        "{funnel}"
    );
    let own = html(
        Chart::bar("x")
            .categories(["a"])
            .series(Series::new("a", [1]))
            .summary("Mine."),
    );
    assert!(own.contains(">Mine.</p>"));
}

#[test]
fn one_value_on_a_line_is_a_dot_with_a_note() {
    let out = html(
        Chart::line("Sent")
            .days([day(3)])
            .series(Series::new("Sent", [1240])),
    );
    assert!(out.contains("data-slot=\"chart-point\""), "{out}");
    assert!(out.contains("A line needs two values; one is shown."));
    assert!(out.contains("Sent: 1,240 at Nov 3, 2026."));
}

#[test]
fn gaps_break_a_line_and_drop_a_bar() {
    let out = html(
        Chart::line("x")
            .categories(["a", "b", "c", "d", "e"])
            .series(Series::new(
                "s",
                [Some(1.0), Some(2.0), None, Some(f64::NAN), Some(3.0)],
            )),
    );
    let d = out.split("data-slot=\"chart-line\"").nth(1).unwrap();
    let d = d.split("d=\"").nth(1).unwrap().split('"').next().unwrap();
    assert_eq!(
        d.matches('M').count(),
        1,
        "the run after the gap is one value: a dot, {d}"
    );
    assert!(out.contains("chart-point"));
    assert_eq!(
        out.matches("No value").count(),
        2,
        "the table says so for each gap"
    );
    let bars = html(
        Chart::bar("x")
            .categories(["a", "b"])
            .series(Series::new("s", [None, Some(2.0)])),
    );
    assert_eq!(bars.matches("data-slot=\"chart-bar\"").count(), 1);
}

#[test]
fn values_are_written_for_the_language() {
    let out = html(
        Chart::bar("Umsatz")
            .locale("de")
            .categories(["Juli"])
            .format(NumberFormat::currency("€ ").max_decimals(0))
            .series(Series::new("Umsatz", [1_250_000.0])),
    );
    assert!(out.contains("€ 1.250.000"), "the table in full");
    assert!(out.contains("Mio."), "the axis short");
    let india = html(
        Chart::bar("x")
            .locale("en-IN")
            .categories(["a"])
            .series(Series::new("s", [1_234_567.0])),
    );
    assert!(india.contains("12,34,567"), "{india}");
}

#[test]
fn time_axes_read_on_the_wall_clock() {
    let hours: Vec<NaiveDateTime> = (0..24)
        .map(|h| day(3).and_hms_opt(h, 0, 0).unwrap())
        .collect();
    let out = html(
        Chart::line("x")
            .times(hours)
            .series(Series::new("s", (0..24).collect::<Vec<i32>>())),
    );
    assert!(out.contains(">6:00 AM</text>"), "{out}");
    assert!(
        out.contains("Nov 3, 2026, 2:00 PM"),
        "the table names the hour"
    );
    let months: Vec<NaiveDate> = (1..=12)
        .map(|m| NaiveDate::from_ymd_opt(2026, m, 1).unwrap())
        .collect();
    let out = html(
        Chart::bar("x")
            .days(months)
            .grain(Grain::Month)
            .series(Series::new("s", [1; 12])),
    );
    assert!(out.contains(">November 2026</th>"), "{out}");
    let weeks = html(
        Chart::bar("x")
            .days([day(2), day(9)])
            .grain(Grain::Week)
            .series(Series::new("s", [1, 2])),
    );
    assert!(weeks.contains("Week of Nov 2, 2026"), "{weeks}");
}

#[test]
fn out_of_order_times_are_put_in_order_with_their_values() {
    let out = html(
        Chart::line("x")
            .days([day(3), day(1), day(2)])
            .series(Series::new("s", [30, 10, 20])),
    );
    let rows: Vec<&str> = out.split("<tr><th scope=\"row\">").skip(1).collect();
    assert!(
        rows[0].starts_with("Nov 1, 2026</th><td data-num=\"\">10<"),
        "{rows:?}"
    );
    assert!(
        rows[2].starts_with("Nov 3, 2026</th><td data-num=\"\">30<"),
        "{rows:?}"
    );
}

#[test]
fn a_ninth_series_folds_into_other() {
    let out = html(
        Chart::line("x")
            .categories(["a"])
            .series_list((0..12).map(|i| Series::new(format!("S{i}"), [i]))),
    );
    assert_eq!(out.matches("data-slot=\"chart-legend-item\"").count(), 8);
    assert!(out.contains(">Other<"));
    assert!(out.contains("data-color=\"other\""));
    assert!(!out.contains("data-color=\"9\""));
    // 7 + 8 + 9 + 10 + 11 in Other.
    assert!(out.contains(">45</td>"), "{out}");
}

#[test]
fn bars_past_the_most_merge_or_fold() {
    let many: Vec<NaiveDate> = (0..1000).map(|i| day(1) - Duration::days(i)).collect();
    let out = html(
        Chart::bar("x")
            .days(many)
            .series(Series::new("s", vec![1; 1000]))
            .max_bars(100),
    );
    assert_eq!(out.matches("data-slot=\"chart-bar\"").count(), 100);
    assert!(out.contains("The drawing shows 100 of 1,000 points"));
    let cats: Vec<String> = (0..300).map(|i| format!("c{i}")).collect();
    let out = html(
        Chart::bar("x")
            .categories(cats)
            .series(Series::new("s", vec![1; 300]))
            .max_bars(10),
    );
    assert_eq!(out.matches("data-slot=\"chart-bar\"").count(), 10);
    assert!(out.contains(">Other</th><td data-num=\"\">291<"), "{out}");
}

#[test]
fn a_million_points_draw_at_most_the_most() {
    let n = 1_000_000;
    let values: Vec<f64> = (0..n)
        .map(|i| ((i as f64) / 5000.0).sin() * 100.0 + if i == 777_777 { 900.0 } else { 0.0 })
        .collect();
    let out = html(
        Chart::line("Big")
            .numbers((0..n).map(|i| i as f64))
            .series(Series::new("Walk", values)),
    );
    assert!(table_rows(&out) <= 640 * 2, "{}", table_rows(&out));
    assert!(out.contains("The drawing shows"));
    assert!(
        out.contains(" at 777,777, lowest"),
        "the summary reads every value: {}",
        &out[..2000.min(out.len())]
    );
    assert!(out.len() < 400_000, "{}", out.len());
}

const NASTY: &str = "<script>alert(1)</script><img src=x onerror=alert(2)>\"'&";

#[test]
fn hostile_text_is_text_everywhere() {
    let nodes: Vec<Node> = vec![
        Chart::bar(NASTY)
            .id(NASTY)
            .description(NASTY)
            .categories([NASTY])
            .x_label(NASTY)
            .toggles()
            .series(Series::new(NASTY, [1]))
            .series(Series::new("b", [2]))
            .annotation(Annotation::target(1, NASTY))
            .annotation(Annotation::event(NASTY, NASTY))
            .annotation(Annotation::band(NASTY, NASTY, NASTY))
            .empty(NASTY, Some(NASTY.to_owned()))
            .summary(NASTY)
            .into(),
        Chart::bar(NASTY)
            .horizontal()
            .categories([NASTY])
            .series(Series::new(NASTY, [1]))
            .into(),
        Chart::line(NASTY)
            .failed(Some(NASTY.to_owned()))
            .retry("javascript:alert(1)")
            .into(),
        Chart::line(NASTY).loading().into(),
        Donut::new(NASTY).part(NASTY, 1).center(NASTY, NASTY).into(),
        Funnel::new(NASTY)
            .stage(Stage::new(NASTY, 1).rate(NASTY))
            .into(),
        Sparkline::new(NASTY, [1, 2]).names([NASTY, NASTY]).into(),
    ];
    for node in nodes {
        let out = html(node);
        assert!(!out.contains("javascript:"), "{out}");
        // Every tag is one a chart draws: the text in it came through the escaping path.
        for tag in out.split('<').skip(1) {
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .collect();
            assert!(
                [
                    "figure", "div", "span", "ul", "ol", "li", "svg", "text", "line", "rect",
                    "path", "circle", "g", "title", "p", "input", "label", "table", "caption",
                    "thead", "tbody", "tr", "th", "td", "strong", "a", "nav",
                ]
                .contains(&name.as_str()),
                "<{name} in {out}"
            );
        }
    }
    // An id is kept to safe characters, so a selector or a `for` built on it holds.
    let out = html(
        Chart::bar("x")
            .id("a\"b c<d")
            .categories(["a"])
            .series(Series::new("s", [1])),
    );
    assert!(out.contains(r#"id="a-b-c-d""#), "{out}");
}

/// A sequence of numbers in `[0, 1)`.
struct Seq(u64);

impl Seq {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() * n as f64) as usize
    }

    fn value(&mut self) -> Option<f64> {
        Some(match self.below(14) {
            0 => return None,
            1 => f64::NAN,
            2 => f64::INFINITY,
            3 => f64::NEG_INFINITY,
            4 => f64::MAX,
            5 => f64::MIN,
            6 => f64::MIN_POSITIVE,
            7 => 0.0,
            8 => -0.0,
            9 => 1e-300,
            10 => -1e15 * self.next(),
            _ => (self.next() - 0.3) * 1000.0,
        })
    }
}

#[test]
fn no_data_breaks_a_chart() {
    let mut seq = Seq(0x9e37_79b9_7f4a_7c15);
    for round in 0..3_000 {
        let rows = match seq.below(5) {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => seq.below(60),
        };
        let mut chart = match seq.below(4) {
            0 => Chart::line("x"),
            1 => Chart::area("x"),
            _ => Chart::bar("x"),
        };
        if seq.below(3) == 0 {
            chart = chart.horizontal();
        }
        chart = match seq.below(4) {
            0 => chart.stacked(),
            1 => chart.stacked_percent(),
            _ => chart,
        };
        if seq.below(2) == 0 {
            chart = chart.smooth();
        }
        chart = match seq.below(4) {
            0 => chart.categories((0..rows).map(|i| format!("c{i}"))),
            1 => chart.numbers((0..rows).map(|_| seq.value())),
            2 => {
                let base = match seq.below(3) {
                    0 => NaiveDate::MIN,
                    1 => NaiveDate::MAX - Duration::days(70),
                    _ => day(1),
                };
                chart.times((0..rows).map(|i| {
                    base.and_hms_opt(0, 0, 0).unwrap()
                        + Duration::seconds((i as i64) * (1 + seq.below(100_000) as i64))
                }))
            }
            _ => chart,
        };
        for s in 0..seq.below(11) {
            let len = if seq.below(4) == 0 {
                seq.below(70)
            } else {
                rows
            };
            let values: Vec<Option<f64>> = (0..len).map(|_| seq.value()).collect();
            let mut series = Series::new(format!("s{s}"), values);
            if seq.below(5) == 0 {
                series = series.tone(Tone::Warning);
            }
            chart = chart.series(series);
        }
        if seq.below(3) == 0 {
            chart = chart.annotation(Annotation::target(seq.value(), "t"));
            chart = chart.annotation(Annotation::event(At::Index(seq.below(80)), "e"));
            chart = chart.annotation(Annotation::band(
                At::Index(seq.below(80)),
                At::Number(1.0),
                "b",
            ));
        }
        if seq.below(4) == 0 {
            chart = chart.y_min(seq.value()).y_max(seq.value());
        }
        chart = chart.format(match seq.below(9) {
            0 => NumberFormat::percent(),
            1 => NumberFormat::percent_of_100().compact(Compact::Always),
            2 => NumberFormat::number().compact(Compact::Always),
            3 => NumberFormat::number().compact(Compact::Never),
            4 => NumberFormat::currency("$").fixed(),
            5 => NumberFormat::integer().compact(Compact::Always),
            6 => NumberFormat::decimals(12).suffix(" ms"),
            _ => NumberFormat::number(),
        });
        if seq.below(3) == 0 {
            chart = chart.locale(["en", "en-IN", "de", "fr", "ar", "hi", "ja", "xx"][seq.below(8)]);
        }
        let out = html(
            chart
                .max_points(16 + seq.below(100))
                .max_bars(4 + seq.below(30)),
        );
        assert!(out.starts_with("<figure"), "round {round}");
        for bad in ["NaN", "inf"] {
            if let Some(at) = out.find(bad) {
                panic!(
                    "round {round}: {bad} at {}",
                    &out[at.saturating_sub(300)..(at + 100).min(out.len())]
                );
            }
        }

        let short = NumberFormat::number().compact(Compact::Always);
        let mut donut = Donut::new("d").format(short.clone());
        for p in 0..seq.below(12) {
            donut = donut.part(format!("p{p}"), seq.value());
        }
        assert!(html(donut.max_parts(seq.below(10))).starts_with("<figure"));
        let mut funnel = Funnel::new("f").format(short.clone());
        for p in 0..seq.below(20) {
            funnel = funnel.stage(Stage::new(format!("s{p}"), seq.value()));
        }
        assert!(html(funnel).starts_with("<figure"));
        let values: Vec<Option<f64>> = (0..seq.below(300)).map(|_| seq.value()).collect();
        assert!(html(Sparkline::new("s", values).format(short)).starts_with("<figure"));
    }
}

#[test]
fn compact_numbers_never_overflow_the_stack() {
    let out = html(
        Chart::bar("Small and large")
            .categories(["a", "b", "c"])
            .format(NumberFormat::number().compact(Compact::Always))
            .series(Series::new("s", [5.0, 1_200.0, 3_400_000.0])),
    );
    assert!(
        out.contains(">5</td>") && out.contains(">1.2K</td>") && out.contains(">3.4M</td>"),
        "{out}"
    );
}

#[test]
fn two_charts_titled_in_other_scripts_never_share_an_id() {
    let ids = |title: &str| {
        let out = html(
            Chart::line(title.to_owned())
                .categories(["a", "b"])
                .toggles()
                .series(Series::new("A", [1, 2]))
                .series(Series::new("B", [2, 1])),
        );
        let mut found: Vec<String> = out
            .match_indices(" id=\"")
            .map(|(at, m)| {
                out[at + m.len()..]
                    .split('"')
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect();
        found.sort();
        found
    };
    for (a, b) in [
        ("Продажи", "Выручка"),
        ("बिक्री", "राजस्व"),
        ("ਵਿਕਰੀ", "ਆਮਦਨ"),
        ("売上", "利益"),
    ] {
        let (a, b) = (ids(a), ids(b));
        assert!(a.iter().all(|id| !b.contains(id)), "{a:?} and {b:?}");
        // The table switch's `for`, the legend's and the names point inside their own chart.
        assert!(a.iter().any(|id| id.ends_with("-table-switch")));
    }
}

#[test]
fn the_hour_the_clocks_repeat_has_its_own_name() {
    use chrono::Utc;
    let ny: chrono_tz::Tz = "America/New_York".parse().unwrap();
    let report = Report::new(
        navree_ui::dates::DateRange::single(day(1)),
        Grain::Hour,
        &ny,
    );
    let out = html(
        report
            .chart(Chart::line("Hours"))
            .series(Series::new("a", vec![1; report.buckets().len()])),
    );
    let mut heads: Vec<&str> = out
        .match_indices("<th scope=\"row\">")
        .map(|(at, m)| {
            let rest = &out[at + m.len()..];
            &rest[..rest.find('<').unwrap()]
        })
        .collect();
    assert_eq!(heads.len(), 25);
    assert!(
        heads.contains(&"Nov 1, 2026, 1:00 AM, second time"),
        "{heads:?}"
    );
    heads.sort();
    heads.dedup();
    assert_eq!(heads.len(), 25, "every row its own name");
    let _ = Utc;
}

#[test]
fn a_funnel_draws_at_most_its_limit_and_says_so() {
    let mut funnel = Funnel::new("Many");
    for at in 0..200 {
        funnel = funnel.stage(Stage::new(format!("Stage {at}"), 1000 - at));
    }
    let out = html(funnel);
    assert_eq!(out.matches("data-slot=\"chart-stage\"").count(), MAX_STAGES);
    assert!(out.contains("The first 12 of 200 stages are drawn."));
}

#[test]
fn a_donut_of_the_largest_numbers_still_draws() {
    let out = html(
        Donut::new("Huge")
            .part("a", f64::MAX)
            .part("b", f64::MAX)
            .part("c", 1.0),
    );
    assert!(out.contains("data-slot=\"chart-slice\""), "{out}");
    assert!(out.contains("50%"), "each half: {out}");
    // A long list of parts that draw nothing doesn't fill the table.
    let mut many = Donut::new("Many").part("Big", 10);
    for at in 0..10_000 {
        many = many.part(format!("Zero {at}"), 0);
    }
    for at in 0..20 {
        many = many.part(format!("Small {at}"), 1);
    }
    let out = html(many);
    assert!(table_rows(&out) <= 8, "{}", table_rows(&out));
}

#[test]
fn a_state_keeps_the_height_of_its_chart() {
    for (node, height) in [
        (Node::from(Chart::line("x").height(300).loading()), 332),
        (Node::from(Chart::line("x").height(120).failed(None)), 152),
        (
            Node::from(
                Chart::bar("x")
                    .categories(Vec::<String>::new())
                    .series(Series::new("s", Vec::<f64>::new())),
            ),
            232,
        ),
    ] {
        let out = html(node);
        assert!(
            out.contains(&format!(
                "data-slot=\"chart-spacer\" width=\"0\" height=\"{height}\""
            )),
            "{out}"
        );
    }
}

#[test]
fn annotations_find_their_rows_after_the_rows_are_reduced() {
    // 1,000 days merged into 100 bars: an event on day 555 sits on the bar that holds it.
    let days: Vec<NaiveDate> = (0..1000)
        .map(|i| day(1) - Duration::days(999 - i))
        .collect();
    let out = html(
        Chart::bar("x")
            .days(days)
            .max_bars(100)
            .series(Series::new("s", vec![1; 1000]))
            .annotation(Annotation::event(At::Index(555), "Here")),
    );
    assert!(
        out.contains("x1=\"55.5%\""),
        "bar 55 of 100 is centred at 55.5%: {out}"
    );
    // A category line reduced to fewer points keeps each point where its category stood.
    let names: Vec<String> = (0..2000).map(|i| format!("c{i}")).collect();
    let out = html(
        Chart::line("x")
            .categories(names)
            .max_points(64)
            .series(Series::new(
                "s",
                (0..2000).map(|i| (i % 50) as f64).collect::<Vec<_>>(),
            ))
            .annotation(Annotation::event("c1000", "Middle")),
    );
    assert!(
        out.contains("x1=\"50.025%\""),
        "category 1000 of 2000: {out}"
    );
}

#[test]
fn reports_draw_over_their_periods() {
    use chrono::Utc;
    let report = Report::new(
        navree_ui::dates::DateRange::new(day(1), day(7)),
        Grain::Day,
        &Utc,
    );
    let counts = report.count(report.buckets().iter().map(|b| b.start));
    assert_eq!(counts, vec![1; 7]);
    let out = html(
        report
            .chart(Chart::bar("Per day"))
            .series(report.series("Events", &counts)),
    );
    assert_eq!(table_rows(&out), 7);
    assert!(out.contains(">Nov 7, 2026</th>"));
    let links = render(&report.grain_links("/r", "Grain"));
    assert!(
        links.contains(r#"href="/r?from=2026-11-01&amp;to=2026-11-07&amp;grain=hour""#),
        "{links}"
    );
    assert!(links.contains(r#"aria-current="true">Day<"#), "{links}");
}
