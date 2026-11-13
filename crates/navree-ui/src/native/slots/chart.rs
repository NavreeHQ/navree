//! Charts: `Chart`, `Donut`, `Funnel` and `Sparkline`.
//!
//! A chart is drawn as an SVG, with the numbers written into it as pixels, and
//! carries the same numbers as a table of the page's own words (`₹30,70,972`)
//! for assistive technology. Neither gives a native client the values. So a
//! chart built while a native client asks keeps its typed values, its labels and
//! its series, with the request, by the id of its figure, and that is what is
//! read: never the words of the table, never a pixel. The table is read only for
//! the words a series is shown in. The page's markup holds none of it, so a chart
//! built for a native client and written for a browser is the markup it always was.
//!
//! A chart built outside such a request has no values to read, and is a miss.

use navree_native::node as n;

use super::super::lower::{
    Cx, N, Parts, Read, attr, elements, fail, fail_on, flat, has, hooks, id, text_of,
};
use crate::node::{Element, Node};

/// The hooks the figure of a chart carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &["data-ui-chart"];

/// The typed values of a chart, written for a native client: its labels, and for each
/// series its name and its values, one a label, a gap where there is none. `None`
/// when no native client is asking, so a browser gets a page with none of it.
pub(crate) fn written<'a>(
    labels: impl Iterator<Item = &'a str>,
    series: impl Iterator<Item = (&'a str, &'a [Option<f64>])>,
    extras: Option<serde_json::Value>,
) -> Option<String> {
    if !crate::native::Client::current().is_native() {
        return None;
    }
    let number = |value: f64| {
        // A whole number is written whole; a fraction, as it is.
        if value.fract() == 0.0 && value.abs() < 9.0e15 {
            serde_json::Number::from(value as i64)
        } else {
            serde_json::Number::from_f64(value).unwrap_or_else(|| serde_json::Number::from(0))
        }
    };
    let lines: Vec<serde_json::Value> = series
        .map(|(label, values)| {
            serde_json::json!({
                "label": label,
                "values": values
                    .iter()
                    .map(|value| value.map(number))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    Some(
        serde_json::json!({
            "labels": labels.collect::<Vec<_>>(),
            "series": lines,
            "extras": extras,
        })
        .to_string(),
    )
}

/// `Chart`, `Donut`, `Funnel`: a figure.
pub(crate) fn figure(cx: &mut Cx, element: &Element) -> Read {
    // The charts the toolkit has beyond the lines, bars, donut, funnel and sparkline: the
    // protocol has a kind for each of them and none has a reader yet. Named, never guessed.
    if let Some(kind) = attr(element, "data-kind")
        && let Some(waits) = waiting(&kind)
    {
        return fail_on(
            format!("chart:{kind}"),
            format!("a chart with no reader here yet: the protocol has {waits} for it"),
        );
    }
    let mut parts = Parts::of(element);
    let header = parts.slot("chart-header");
    let state = parts.slot("chart-state");
    // The legend of a chart that has one apart from its drawing, and the drawing itself,
    // with the tooltip a script fills: the values are in the figure's attribute.
    let _legend = parts.slot("chart-legend");
    let body = parts.slot("chart-body");
    let _note = parts.slot("chart-note");
    // The summary is written for assistive technology, in the words of the page.
    let _summary = parts.slot("chart-summary");
    let _announce = parts.slot("chart-announce");
    let data = parts.slot("chart-data");
    if let Some(body) = body {
        hooks(body, &[])?;
    }
    // The words of the keys, hidden, for a script.
    for node in parts.left() {
        match node {
            Node::Element(span) if span.tag() == "span" && has(span, "hidden") => {}
            Node::Text(text) if text.trim().is_empty() => {}
            _ => return fail("a part of a chart no reader here knows"),
        }
    }
    let Some(header) = header else {
        return fail("a chart with no header");
    };
    let title = title_of(header)?;
    // A title the page shows to assistive technology only (a card around the chart names
    // it) is no visible title: the chart is not drawn with a second one.
    let title = if has(header, "data-sr") {
        cx.dropped.push("chart-title-hidden".to_owned());
        None
    } else {
        Some(title)
    };
    if let Some(state) = state {
        return unready(state).map(|node| vec![node]);
    }
    let Some(values) = written_for(element) else {
        return fail("a chart built outside a native request, with no numbers to read");
    };
    let (labels, series, extras) = numbers(&values)?;
    let kind = match attr(element, "data-kind").as_deref() {
        Some("line") => n::ChartKind::Line,
        Some("area") => n::ChartKind::Area,
        Some("bar" | "bar-horizontal") => n::ChartKind::Bar,
        Some("donut") => n::ChartKind::Donut,
        Some("funnel") => n::ChartKind::Funnel,
        Some("sparkline") => n::ChartKind::Sparkline,
        Some(other) => return fail(format!("a chart of the `{other}` kind")),
        None => return fail("a chart of no kind"),
    };
    let shown = match data {
        Some(data) => shown_in(data)?,
        None => Vec::new(),
    };
    let series = series
        .into_iter()
        .enumerate()
        .map(|(at, (label, values))| {
            let words = shown
                .iter()
                .find(|(name, _)| *name == label)
                .map(|(_, words)| words.clone())
                .filter(|words| words.len() == values.len())
                .unwrap_or_default();
            n::Series {
                label,
                values,
                shown: words,
                unit: extras.units.get(at).cloned().flatten(),
                colour: extras
                    .hues
                    .get(at)
                    .copied()
                    .flatten()
                    .map(n::Colour::new),
                ..n::Series::default()
            }
        })
        .collect();
    Ok(vec![N::Chart(n::Chart {
        id: id(element),
        kind,
        title,
        labels,
        series,
        x: extras.x,
        y: extras.y,
        references: extras.references,
        zones: extras.zones,
        levels: extras.levels,
        curve: extras.curve,
        stacking: extras.stacking,
        horizontal: extras.horizontal,
        summary: extras.summary.filter(|summary| !summary.is_empty()),
        ..n::Chart::default()
    })])
}

/// What the protocol has for each chart the toolkit draws that no reader reads yet.
fn waiting(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "ring" => "a `ring`",
        "rings" => "`rings`",
        "gauge" => "a `gauge` with zones",
        "day-clock" => "a `day_clock`",
        "meter" => "a `meter`",
        "mini-columns" => "`columns`",
        "color-strip" => "a `strip`",
        "range-scale" => "a `range`",
        "calendar-heatmap" => "a `calendar_heatmap`",
        "matrix-heatmap" | "relation-list" => "a `matrix_heatmap`",
        "scatter" => "a `scatter`",
        _ => return None,
    })
}

/// What the chart with this figure wrote during the request that is current.
fn written_for(figure: &Element) -> Option<String> {
    let id = id(figure)?;
    crate::native::scope::native()?.chart(&id)
}

/// The chart's name.
fn title_of(header: &Element) -> Read<String> {
    let mut parts = Parts::of(header);
    let heading = parts.slot("chart-heading");
    // What the page puts beside the title, such as a way to change the period.
    let _action = parts.slot("chart-action");
    parts.done()?;
    let Some(heading) = heading else {
        return fail("a chart with no title");
    };
    let mut inside = Parts::of(heading);
    let title = inside.slot("chart-title");
    let _description = inside.slot("chart-description");
    inside.done()?;
    match title {
        Some(title) => Ok(text_of(title)),
        None => fail("a chart with no title"),
    }
}

/// A chart that is not drawn: it is loading, it failed, or it has nothing to draw.
fn unready(state: &Element) -> Read<N> {
    let words: Vec<String> = flat(state.child_nodes())
        .iter()
        .filter_map(|node| match node {
            Node::Element(part) => Some(text_of(part)),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect();
    match attr(state, "data-state").as_deref() {
        Some("loading") => Ok(N::Progress(n::Progress::default())),
        Some("failed") => Ok(N::Banner(n::Banner {
            tone: Some(n::Tone::Danger),
            title: words.first().cloned(),
            text: words.get(1).cloned(),
            ..n::Banner::default()
        })),
        Some("empty") => Ok(N::Empty(n::Empty {
            title: words.first().cloned().unwrap_or_default(),
            text: words.get(1).cloned(),
            ..n::Empty::default()
        })),
        _ => fail("a chart in a state no reader here knows"),
    }
}

/// What a chart says beside its numbers, in the protocol's own words: written by the
/// chart (see `Chart::native_extras`), read here, every part optional.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Extras {
    x: Option<n::Measure>,
    y: Option<n::Measure>,
    references: Vec<n::Reference>,
    zones: Vec<n::Zone>,
    levels: Vec<String>,
    curve: n::Curve,
    stacking: n::Stacking,
    horizontal: bool,
    summary: Option<String>,
    units: Vec<Option<String>>,
    hues: Vec<Option<n::Hue>>,
}

/// The labels and series a chart wrote for a native client, with what it says beside them.
type Numbers = (
    Vec<String>,
    Vec<(String, Vec<Option<serde_json::Number>>)>,
    Extras,
);

fn numbers(json: &str) -> Read<Numbers> {
    #[derive(serde::Deserialize)]
    struct Written {
        labels: Vec<String>,
        series: Vec<Line>,
        #[serde(default)]
        extras: Option<Extras>,
    }
    #[derive(serde::Deserialize)]
    struct Line {
        label: String,
        values: Vec<Option<serde_json::Number>>,
    }
    let Ok(written) = serde_json::from_str::<Written>(json) else {
        return fail("numbers of a chart that are not readable");
    };
    for line in &written.series {
        if line.values.len() != written.labels.len() {
            return fail("a series with more or fewer values than the chart has labels");
        }
    }
    Ok((
        written.labels,
        written
            .series
            .into_iter()
            .map(|line| (line.label, line.values))
            .collect(),
        written.extras.unwrap_or_default(),
    ))
}

/// The words each series is shown in, from the table of the page's own words:
/// by the heading of its column.
fn shown_in(data: &Element) -> Read<Vec<(String, Vec<String>)>> {
    let Some(table) = crate::native::page::find(data.child_nodes(), "chart-table") else {
        return Ok(Vec::new());
    };
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let Some(Node::Element(markup)) = flat(table.child_nodes()).first().copied() else {
        return Ok(Vec::new());
    };
    let mut columns: Vec<String> = Vec::new();
    for part in elements(markup)? {
        match part.tag() {
            "caption" => {}
            "thead" => {
                for row in elements(part)? {
                    for (at, cell) in elements(row)?.into_iter().enumerate() {
                        if at > 0 {
                            columns.push(text_of(cell));
                            out.push((text_of(cell), Vec::new()));
                        }
                    }
                }
            }
            "tbody" => {
                for row in elements(part)? {
                    for (at, cell) in elements(row)?.into_iter().skip(1).enumerate() {
                        if let Some((_, words)) = out.get_mut(at) {
                            words.push(text_of(cell));
                        }
                    }
                }
            }
            _ => return fail(format!("a <{}> in the table of a chart", part.tag())),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{misses, tamper};
    use super::*;
    use crate::View;
    use crate::charts::{Chart, Donut, Funnel, Series, Sparkline, Stage};
    use crate::native::TestNative;
    use crate::native::scope::{Asked, Request};
    use crate::node::Node;
    use navree_native::{Client, FormFactor, Platform};

    fn phone() -> Client {
        Client::new(Platform::Android, FormFactor::Phone)
    }

    /// The first chart of the page `build` makes while a phone asks.
    fn chart_of(build: impl FnOnce() -> Node) -> n::Chart {
        let page = TestNative::build(phone(), || View::new("x").body(build()));
        assert!(page.is_native(), "{:?}", page.why());
        let mut stack = vec![&page.screen().body];
        while let Some(node) = stack.pop() {
            if let N::Chart(chart) = node {
                return chart.clone();
            }
            stack.extend(node.children());
        }
        panic!("no chart on the page");
    }

    fn numbers_of(series: &n::Series) -> Vec<Option<f64>> {
        series
            .values
            .iter()
            .map(|value| value.as_ref().and_then(serde_json::Number::as_f64))
            .collect()
    }

    #[test]
    fn a_title_only_assistive_technology_hears_is_not_drawn_as_a_second_title() {
        let hidden = chart_of(|| {
            Chart::line("Revenue by day")
                .hide_title()
                .categories(["Mon", "Tue"])
                .series(Series::new("Revenue", [1, 2]))
                .into()
        });
        assert_eq!(hidden.title, None);
        let shown = chart_of(|| {
            Chart::line("Revenue by day")
                .categories(["Mon", "Tue"])
                .series(Series::new("Revenue", [1, 2]))
                .into()
        });
        assert_eq!(shown.title.as_deref(), Some("Revenue by day"));
        let page = TestNative::build(phone(), || {
            View::new("x").body(
                Chart::line("Revenue by day")
                    .hide_title()
                    .series(Series::new("R", [1])),
            )
        });
        assert!(
            page.page()
                .dropped
                .iter()
                .any(|name| name == "chart-title-hidden")
        );
    }

    #[test]
    fn what_the_chart_says_beside_its_numbers_is_what_the_protocols_chart_carries() {
        use crate::charts::{Annotation, Curve, Reference, Zone};
        use navree_native::node as nn;
        let chart = chart_of(|| {
            Chart::line("Sleep")
                .categories(["Mon", "Tue", "Wed"])
                .series(Series::new("Heart rate", [52, 58, 61]).unit("bpm").color(3))
                .series(Series::new("Resting", [48, 49, 47]).unit("bpm"))
                .curve(Curve::Monotone)
                .x_label("Night")
                .y_min(40)
                .y_max(80)
                .reference(Reference::level(60).label("Limit"))
                .reference(Reference::average())
                .annotation(Annotation::target(55, "Target"))
                .zone(Zone::new(50, 60).label("Optimal"))
                .summary("Heart rate over three nights")
                .into()
        });
        assert_eq!(chart.curve, nn::Curve::Smooth);
        assert_eq!(chart.stacking, nn::Stacking::Apart);
        assert!(!chart.horizontal);
        assert_eq!(chart.x.as_ref().and_then(|x| x.name.as_deref()), Some("Night"));
        let y = chart.y.as_ref().expect("a value axis");
        // Both series count the same, so the axis does too; its bounds are the page's.
        assert_eq!(y.unit.as_deref(), Some("bpm"));
        assert_eq!(
            (y.min.as_ref().map(ToString::to_string), y.max.as_ref().map(ToString::to_string)),
            (Some("40".into()), Some("80".into()))
        );
        let levels: Vec<(Option<String>, Option<&str>)> = chart
            .references
            .iter()
            .map(|r| (r.value.as_ref().map(ToString::to_string), r.label.as_deref()))
            .collect();
        // A level, the average worked out into a level, and a target.
        assert_eq!(levels.len(), 3, "{levels:?}");
        assert_eq!(levels[0], (Some("60".into()), Some("Limit")));
        assert!(levels[1].0.is_some());
        assert_eq!(levels[2], (Some("55".into()), Some("Target")));
        assert!(chart.references.iter().all(|r| r.kind == nn::ReferenceKind::Level));
        let [zone] = chart.zones.as_slice() else {
            panic!("not one zone: {:?}", chart.zones);
        };
        assert_eq!((zone.from.to_string(), zone.to.to_string()), ("50".into(), "60".into()));
        assert_eq!(zone.label.as_deref(), Some("Optimal"));
        assert_eq!(chart.summary.as_deref(), Some("Heart rate over three nights"));
        let [first, second] = chart.series.as_slice() else {
            panic!("not two series");
        };
        assert_eq!(first.unit.as_deref(), Some("bpm"));
        assert_eq!(first.colour.as_ref().map(|c| c.hue), Some(nn::Hue::Green));
        assert!(second.colour.is_some());

        // Bars that lie along rows, one on another as shares, and the names of a value axis.
        let bars = chart_of(|| {
            Chart::bar("Stages")
                .categories(["Mon", "Tue"])
                .series(Series::new("Deep", [1, 2]))
                .series(Series::new("Light", [3, 2]))
                .stacked_percent()
                .horizontal()
                .into()
        });
        assert_eq!(bars.stacking, nn::Stacking::Shares);
        assert!(bars.horizontal);
        let levels = chart_of(|| {
            Chart::line("Stages")
                .categories(["22:00", "23:00"])
                .series(Series::new("Stage", [0, 2]))
                .curve(Curve::Step)
                .y_categories(["Awake", "Light", "Deep"])
                .into()
        });
        assert_eq!(levels.levels, ["Awake", "Light", "Deep"]);
        assert_eq!(levels.curve, nn::Curve::Step);
        // A chart that says none of this carries none of it.
        let plain = chart_of(|| {
            Chart::line("Plain")
                .categories(["Mon", "Tue"])
                .series(Series::new("A", [1, 2]))
                .into()
        });
        assert!(plain.x.is_none() && plain.references.is_empty() && plain.zones.is_empty());
        assert_eq!((plain.curve, plain.stacking), (nn::Curve::Linear, nn::Stacking::Apart));
    }

    #[test]
    fn a_line_chart_gives_its_typed_values_labels_and_series() {
        let chart = chart_of(|| {
            Chart::line("Sign-ups")
                .categories(["Mon", "Tue", "Wed"])
                .series(Series::new("Trial", [12, 18, 9]))
                .series(Series::new("Paid", [Some(3.5), None, Some(2.0)]))
                .into()
        });
        assert_eq!(chart.kind, n::ChartKind::Line);
        assert_eq!(chart.title.as_deref(), Some("Sign-ups"));
        assert_eq!(chart.labels, ["Mon", "Tue", "Wed"]);
        let [trial, paid] = chart.series.as_slice() else {
            panic!("not two series");
        };
        assert_eq!(trial.label, "Trial");
        assert_eq!(numbers_of(trial), [Some(12.0), Some(18.0), Some(9.0)]);
        assert_eq!(numbers_of(paid), [Some(3.5), None, Some(2.0)]);
        // The words the page shows the numbers in come from its table.
        assert_eq!(trial.shown, ["12", "18", "9"]);
    }

    #[test]
    fn the_numbers_are_exact_whatever_the_page_formats_them_as() {
        let chart = chart_of(|| {
            Chart::bar("Revenue")
                .categories(["Q1", "Q2"])
                .series(Series::new("Sales", [3_070_972, 2_500_000]))
                .format(crate::charts::NumberFormat::currency("₹"))
                .into()
        });
        assert_eq!(chart.kind, n::ChartKind::Bar);
        assert_eq!(
            numbers_of(&chart.series[0]),
            [Some(3_070_972.0), Some(2_500_000.0)]
        );
        // The page's words stay as the page wrote them.
        assert!(
            chart.series[0].shown[0].contains('₹'),
            "{:?}",
            chart.series[0].shown
        );
    }

    #[test]
    fn a_donut_a_funnel_and_a_sparkline_give_theirs() {
        let donut = chart_of(|| {
            Donut::new("Sales")
                .part("Online", 70)
                .part("Store", 30)
                .into()
        });
        assert_eq!(donut.kind, n::ChartKind::Donut);
        assert_eq!(donut.labels, ["Online", "Store"]);
        assert_eq!(numbers_of(&donut.series[0]), [Some(70.0), Some(30.0)]);
        let funnel = chart_of(|| {
            Funnel::new("Checkout")
                .stage(Stage::new("Visited", 100))
                .stage(Stage::new("Bought", 20))
                .into()
        });
        assert_eq!(funnel.kind, n::ChartKind::Funnel);
        assert_eq!(funnel.labels, ["Visited", "Bought"]);
        assert_eq!(numbers_of(&funnel.series[0]), [Some(100.0), Some(20.0)]);
        let spark = chart_of(|| Sparkline::new("Trend", [1, 3, 2, 5]).into());
        assert_eq!(spark.kind, n::ChartKind::Sparkline);
        assert_eq!(
            numbers_of(&spark.series[0]),
            [Some(1.0), Some(3.0), Some(2.0), Some(5.0)]
        );
    }

    #[test]
    fn a_chart_that_is_loading_failed_or_empty_says_so() {
        let page = TestNative::build(phone(), || {
            View::new("x").body((
                Chart::line("A").loading(),
                Chart::line("B").failed(Some("Could not load".to_owned())),
                Chart::line("C"),
            ))
        });
        assert!(page.is_native(), "{:?}", page.why());
        let nodes = page.screen().body.children();
        assert!(matches!(nodes[0], N::Progress(_)));
        assert!(matches!(nodes[1], N::Banner(banner) if banner.tone == Some(n::Tone::Danger)));
        assert!(matches!(nodes[2], N::Empty(_)));
    }

    #[test]
    fn a_chart_built_outside_a_native_request_has_no_numbers_and_is_a_miss() {
        // Not a pixel and not a formatted word is read in their place.
        let node: Node = Chart::line("Sign-ups")
            .categories(["Mon", "Tue"])
            .series(Series::new("Trial", [12, 18]))
            .into();
        assert_eq!(misses(node), ["slot:chart"]);
    }

    #[test]
    fn a_chart_built_for_a_native_client_and_written_for_a_browser_is_the_chart_it_always_was() {
        let build = || {
            Chart::line("Sign-ups")
                .categories(["Mon", "Tue"])
                .series(Series::new("Trial", [12, 18]))
        };
        let plain = crate::render(&build().into());
        let asked = Asked::Native(std::sync::Arc::new(Request::new(phone(), "")));
        let during = crate::native::scope::sync(asked, || crate::render(&build().into()));
        assert_eq!(plain, during);
        assert!(!plain.contains("data-native"));
    }

    #[test]
    fn a_hook_no_reader_knows_inside_a_chart_is_still_a_miss() {
        for name in ["chart", "chart-body"] {
            let page = TestNative::build(phone(), || {
                View::new("x").body(tamper(
                    Chart::line("Sign-ups")
                        .categories(["Mon", "Tue"])
                        .series(Series::new("Trial", [12, 18])),
                    name,
                    "data-ui-wizard",
                ))
            });
            assert_eq!(page.why(), ["hook:data-ui-wizard"], "{name}");
        }
    }
}
