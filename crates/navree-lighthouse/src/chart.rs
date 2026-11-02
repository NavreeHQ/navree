//! Chart geometry for the dashboard's SVG charts: bars of a timeline and
//! the rows of a trace's waterfall. Computed here so the pages need no
//! inline styles (their Content Security Policy forbids them).

use chrono::{DateTime, Timelike as _, Utc};
use serde::Serialize;

use crate::engine::Span;
use crate::options::Period;
use crate::release::ReleaseMarker;
use crate::store::TimelinePoint;

/// One bar of a bar chart, in the SVG's user units.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Bar {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) w: f64,
    pub(crate) h: f64,
    pub(crate) value: i64,
}

/// Bars for `values` in a `width` × `height` box: one slot per value, the
/// tallest value filling the height, a non-zero value at least 1 high.
pub(crate) fn bars(values: &[i64], width: f64, height: f64) -> Vec<Bar> {
    if values.is_empty() {
        return Vec::new();
    }
    let slot = width / values.len() as f64;
    let w = round2((slot * 0.75).max(1.0));
    let max = values.iter().copied().max().unwrap_or(0).max(1) as f64;
    values
        .iter()
        .enumerate()
        .map(|(i, &value)| {
            let h = if value > 0 {
                (value as f64 / max * height).max(1.0)
            } else {
                0.0
            };
            Bar {
                x: round2(i as f64 * slot + (slot - w) / 2.0),
                y: round2(height - h),
                w,
                h: round2(h),
                value,
            }
        })
        .collect()
}

/// The path of a sparkline for `values` in a `width` × `height` box: the
/// line runs from the first value to the last, the largest value reaching
/// the top and zero the bottom. `None` for fewer than two values.
pub(crate) fn sparkline(values: &[f64], width: f64, height: f64) -> Option<String> {
    if values.len() < 2 {
        return None;
    }
    let max = values.iter().copied().fold(0.0_f64, f64::max);
    let step = width / (values.len() - 1) as f64;
    let mut path = String::new();
    for (i, value) in values.iter().enumerate() {
        let y = if max > 0.0 {
            height - (value / max * height).clamp(0.0, height)
        } else {
            height
        };
        // Keep a line at the very top or bottom inside the box.
        let y = y.clamp(1.0, height - 1.0);
        path.push_str(if i == 0 { "M" } else { " L" });
        path.push_str(&format!("{} {}", round2(i as f64 * step), round2(y)));
    }
    Some(path)
}

/// The time labels under a timeline: five evenly spaced, the last `Now`.
/// Times are UTC (the server has no viewer's zone): hours and minutes for
/// an hour or a day, the date for a week.
pub(crate) fn axis_labels(timeline: &[TimelinePoint], period: Period) -> Vec<String> {
    let n = timeline.len();
    if n < 4 {
        return Vec::new();
    }
    let mut labels: Vec<String> = (0..4)
        .map(|i| {
            let at = timeline[i * n / 4].at;
            match period {
                Period::Hour | Period::Day => {
                    format!("{:02}:{:02}", at.hour(), at.minute())
                }
                Period::Week => at.format("%-d %b").to_string(),
            }
        })
        .collect();
    labels.push("Now".to_owned());
    labels
}

/// A deploy drawn on a chart as a vertical line.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct ReleaseLine {
    /// The position along the chart, in the SVG's user units.
    pub(crate) x: f64,
    /// The release label.
    pub(crate) name: String,
    /// When it was deployed, as `2026-11-02 10:15 UTC`.
    pub(crate) at: String,
}

/// Lines for the deploys within a chart that starts at `first`, whose
/// buckets are `step_seconds` long and `buckets` many, drawn over `width`.
pub(crate) fn release_lines(
    markers: &[ReleaseMarker],
    first: DateTime<Utc>,
    step_seconds: i64,
    buckets: usize,
    width: f64,
) -> Vec<ReleaseLine> {
    let span = (step_seconds * buckets as i64).max(1) as f64;
    markers
        .iter()
        .filter_map(|marker| {
            let offset = (marker.at - first).num_seconds() as f64;
            (offset >= 0.0 && offset <= span).then(|| ReleaseLine {
                x: round2(offset / span * width),
                name: marker.name.clone(),
                at: marker.at.format("%Y-%m-%d %H:%M UTC").to_string(),
            })
        })
        .collect()
}

/// One row of a waterfall: a span, or a run of identical consecutive spans
/// (`SELECT ... x 11`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct WaterfallRow {
    pub(crate) kind: String,
    pub(crate) label: String,
    pub(crate) count: usize,
    pub(crate) start_ms: f64,
    pub(crate) duration_ms: f64,
    pub(crate) failed: bool,
    pub(crate) x: f64,
    pub(crate) w: f64,
}

/// The waterfall of `spans` over a `width`-wide track: consecutive spans
/// of the same kind and label are one row, positioned from the first's
/// start to the last's end, with their summed duration.
pub(crate) fn waterfall(spans: &[Span], total_ms: f64, width: f64) -> Vec<WaterfallRow> {
    let end = spans
        .iter()
        .map(|span| span.start_ms + span.duration_ms)
        .fold(total_ms, f64::max)
        .max(1.0);
    let mut rows: Vec<(WaterfallRow, f64)> = Vec::new();
    for span in spans {
        let span_end = span.start_ms + span.duration_ms;
        if let Some((row, last_end)) = rows.last_mut()
            && row.kind == span.kind
            && row.label == span.label
        {
            row.count += 1;
            row.duration_ms += span.duration_ms;
            row.failed |= span.failed;
            *last_end = last_end.max(span_end);
            continue;
        }
        rows.push((
            WaterfallRow {
                kind: span.kind.clone(),
                label: span.label.clone(),
                count: 1,
                start_ms: span.start_ms,
                duration_ms: span.duration_ms,
                failed: span.failed,
                x: 0.0,
                w: 0.0,
            },
            span_end,
        ));
    }
    rows.into_iter()
        .map(|(mut row, last_end)| {
            row.x = round2(row.start_ms / end * width);
            row.w = round2(((last_end - row.start_ms) / end * width).max(2.0));
            row.start_ms = round2(row.start_ms);
            row.duration_ms = round2(row.duration_ms);
            row
        })
        .collect()
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(kind: &str, label: &str, start_ms: f64, duration_ms: f64) -> Span {
        Span {
            kind: kind.into(),
            label: label.into(),
            start_ms,
            duration_ms,
            failed: false,
        }
    }

    #[test]
    fn bars_scale_to_the_tallest_value() {
        let bars = bars(&[0, 5, 10], 300.0, 100.0);
        assert_eq!(bars.len(), 3);
        assert_eq!(bars[0].h, 0.0);
        assert_eq!(bars[1].h, 50.0);
        assert_eq!(bars[2].h, 100.0);
        assert_eq!(bars[2].y, 0.0);
        assert!(bars[1].x > bars[0].x);
        assert!(super::bars(&[], 10.0, 10.0).is_empty());
        assert_eq!(super::bars(&[0, 0], 10.0, 10.0)[1].h, 0.0);
    }

    #[test]
    fn sparklines_run_from_the_first_value_to_the_last_inside_their_box() {
        let path = sparkline(&[0.0, 5.0, 10.0], 80.0, 20.0).unwrap();
        assert_eq!(path, "M0 19 L40 10 L80 1");
        assert_eq!(sparkline(&[0.0, 0.0], 80.0, 20.0).unwrap(), "M0 19 L80 19");
        assert!(sparkline(&[3.0], 80.0, 20.0).is_none());
        assert!(sparkline(&[], 80.0, 20.0).is_none());
    }

    #[test]
    fn labels_are_five_evenly_spaced_times_ending_in_now() {
        let start: DateTime<Utc> = "2026-11-02T09:00:00Z".parse().unwrap();
        let day: Vec<TimelinePoint> = (0..24)
            .map(|i| TimelinePoint {
                at: start + chrono::TimeDelta::hours(i),
                count: 0,
                errors: 0,
            })
            .collect();
        assert_eq!(
            axis_labels(&day, Period::Day),
            ["09:00", "15:00", "21:00", "03:00", "Now"]
        );
        let week: Vec<TimelinePoint> = (0..168)
            .map(|i| TimelinePoint {
                at: start + chrono::TimeDelta::hours(i),
                count: 0,
                errors: 0,
            })
            .collect();
        assert_eq!(
            axis_labels(&week, Period::Week),
            ["2 Nov", "4 Nov", "5 Nov", "7 Nov", "Now"]
        );
        assert!(axis_labels(&day[..3], Period::Day).is_empty());
    }

    #[test]
    fn deploys_are_lines_only_inside_the_chart() {
        let first: DateTime<Utc> = "2026-11-02T09:00:00Z".parse().unwrap();
        let marker = |at: &str| ReleaseMarker {
            name: "r2".into(),
            at: at.parse().unwrap(),
        };
        let lines = release_lines(
            &[
                marker("2026-11-02T08:00:00Z"),
                marker("2026-11-02T21:00:00Z"),
                marker("2026-11-03T10:00:00Z"),
            ],
            first,
            3600,
            24,
            960.0,
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].x, 480.0);
        assert_eq!(lines[0].at, "2026-11-02 21:00 UTC");
    }

    #[test]
    fn identical_consecutive_spans_are_one_row() {
        let spans = vec![
            span("db", "SELECT deals", 0.0, 38.0),
            span("db", "SELECT contacts", 52.0, 100.0),
            span("db", "SELECT contacts", 152.0, 100.0),
            span("db", "SELECT contacts", 252.0, 100.0),
            span("http", "GET https://api.example.com/latest", 400.0, 150.0),
        ];
        let rows = waterfall(&spans, 600.0, 1000.0);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].count, 3);
        assert_eq!(rows[1].duration_ms, 300.0);
        assert_eq!(rows[1].x, round2(52.0 / 600.0 * 1000.0));
        assert_eq!(rows[1].w, round2(300.0 / 600.0 * 1000.0));
        assert_eq!(rows[2].kind, "http");
        assert!(rows[0].w >= 2.0);
    }
}
