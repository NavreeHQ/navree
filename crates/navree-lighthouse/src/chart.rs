//! The rows of a trace's waterfall: spans placed on a track. Computed here
//! so the pages need no inline styles (their Content Security Policy forbids
//! them).

use crate::engine::Span;

/// One row of a waterfall: a span, or a run of identical consecutive spans
/// (`SELECT ... x 11`).
#[derive(Clone, Debug, PartialEq)]
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
