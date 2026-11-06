//! What a report adds to its chart: its numbers as a file, links from its periods to the
//! rows behind them, and the period before it to draw beside it.

use bytes::Bytes;
use chrono::{Datelike, Duration, NaiveDateTime};
use http::HeaderValue;
use http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use navree_http::body::Body;
use navree_http::response::Response;

use super::report::{Report, ReportZone};
use super::scale::Grain;
use super::Series;
use crate::dates::DateRange;
use crate::table::write_csv_field;

/// A report's numbers as a table: a row for each period, a column for each series. It holds
/// what the chart's own table holds, as numbers, so a file and the page cannot differ.
///
/// ```
/// use navree_ui::charts::{Grain, Report};
/// use navree_ui::dates::DateRange;
/// use chrono::{NaiveDate, Utc};
///
/// let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
/// let report = Report::new(DateRange::new(day(1), day(3)), Grain::Day, &Utc);
/// let sales: &[Option<f64>] = &[Some(120.0), None, Some(1_20_000.5)];
/// let table = report.table("Day", &[("Sales", sales)]);
/// assert_eq!(table.csv(), "\u{feff}Day,Sales\r\n2026-11-01,120\r\n2026-11-02,\r\n2026-11-03,120000.5\r\n");
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ReportTable {
    head: Vec<String>,
    periods: Vec<String>,
    values: Vec<Vec<Option<f64>>>,
}

/// A period named the way a file writes it: sortable, and exact to the grain.
fn period_name(grain: Grain, local: NaiveDateTime) -> String {
    match grain {
        Grain::Minute | Grain::Hour => local.format("%Y-%m-%d %H:%M").to_string(),
        Grain::Day | Grain::Week => local.format("%Y-%m-%d").to_string(),
        Grain::Month => local.format("%Y-%m").to_string(),
        Grain::Quarter => format!("{}-Q{}", local.year(), local.month0() / 3 + 1),
        Grain::Year => local.format("%Y").to_string(),
    }
}

/// A number the way a file holds it: whole when it is whole, else as short as it can be
/// written and read back the same, with no grouping.
fn plain(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

impl Report {
    /// The report's numbers as a table. `period_header` heads the first column; each of
    /// `series` is a name and its values, one for each period, in the order the report's
    /// [`measure`](Self::measure) gives them. A series shorter than the report is filled
    /// with gaps, and a longer one is cut.
    pub fn table(&self, period_header: impl Into<String>, series: &[(&str, &[Option<f64>])]) -> ReportTable {
        let grain = self.grain();
        let count = self.buckets().len();
        let mut head = vec![period_header.into()];
        head.extend(series.iter().map(|(name, _)| (*name).to_owned()));
        ReportTable {
            head,
            periods: self
                .buckets()
                .iter()
                .map(|bucket| period_name(grain, bucket.local))
                .collect(),
            values: series
                .iter()
                .map(|(_, values)| (0..count).map(|at| values.get(at).copied().flatten()).collect())
                .collect(),
        }
    }

    /// A link for each period to the page that lists the rows behind it: `path` with a
    /// range filter on `column` for the days the period covers, in the grammar a
    /// [`DataTable`](crate::DataTable) reads (`filter[created_at][gte]=2026-11-02&filter[created_at][lte]=2026-11-08`).
    /// An hour or a minute is listed with its whole day. Give them to
    /// [`Chart::drill`](super::Chart::drill). A column that is not a plain name (letters,
    /// digits and `_`) gives no links.
    pub fn drill_links(&self, path: &str, column: &str) -> Vec<String> {
        let plain_column = !column.is_empty()
            && column.len() <= 64
            && column.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !plain_column {
            return Vec::new();
        }
        let buckets = self.buckets();
        let join = if path.contains('?') { '&' } else { '?' };
        (0..buckets.len())
            .map(|at| {
                let from = buckets[at].local.date();
                let to = match buckets.get(at + 1) {
                    Some(next) => next.local.date().pred_opt().unwrap_or(from),
                    None => self.range().to,
                };
                let to = to.max(from);
                format!("{path}{join}filter[{column}][gte]={from}&filter[{column}][lte]={to}")
            })
            .collect()
    }

    /// The window just before this one, as many days long, in the same zone and grain: the
    /// period to compare with. A window of seven days from the 8th has the seven days from
    /// the 1st. Pass the zone this report was cut in.
    pub fn previous<Tz: ReportZone>(&self, zone: &Tz) -> Report {
        let days = self.range().day_count();
        let to = self
            .range()
            .from
            .pred_opt()
            .unwrap_or(self.range().from);
        let from = to
            .checked_sub_signed(Duration::days(days - 1))
            .unwrap_or(to);
        Report::build(
            DateRange::new(from, to),
            self.asked_grain(),
            self.week_start(),
            zone,
        )
    }

    /// The previous window's figures as a series of this report's periods, drawn in the
    /// quiet ink: the period before, beside the one on show. `previous_values` are what
    /// [`measure`](Self::measure) gave for [`previous`](Self::previous); they are laid on
    /// this report's periods by position (the first period against the first), a shorter
    /// run leaving gaps and a longer one cut.
    pub fn previous_series(&self, name: impl Into<String>, previous_values: &[Option<f64>]) -> Series {
        let count = self.buckets().len();
        Series::new(
            name,
            (0..count).map(|at| previous_values.get(at).copied().flatten()),
        )
        .muted()
    }
}

impl ReportTable {
    /// The column headings: the period's, then each series' name.
    pub fn head(&self) -> &[String] {
        &self.head
    }

    /// The name of each period, as a file writes it (`2026-11-04`, `2026-11`, `2026-Q4`).
    pub fn periods(&self) -> &[String] {
        &self.periods
    }

    /// The values of each series, one for each period.
    pub fn values(&self) -> &[Vec<Option<f64>>] {
        &self.values
    }

    /// The table as CSV: a byte-order mark so a spreadsheet reads UTF-8, a line for the
    /// headings, one for each period, lines ended by CRLF. A gap is an empty field, a
    /// number has no grouping, and no text is read as a formula.
    pub fn csv(&self) -> String {
        let mut out = String::with_capacity(64 + self.periods.len() * 24);
        out.push('\u{feff}');
        for (at, name) in self.head.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            write_csv_field(&mut out, name);
        }
        out.push_str("\r\n");
        for (row, period) in self.periods.iter().enumerate() {
            write_csv_field(&mut out, period);
            for series in &self.values {
                out.push(',');
                if let Some(value) = series.get(row).copied().flatten() {
                    out.push_str(&plain(value));
                }
            }
            out.push_str("\r\n");
        }
        out
    }

    /// The CSV as a download named `file` (letters, digits, `-`, `_` and `.` are kept).
    ///
    /// A workbook is the app's to write, with `head`, `periods` and `values` and
    /// navree-sheets' `write_xlsx`: that crate sits above this one, so this one cannot
    /// call it.
    pub fn csv_response(&self, file: &str) -> Response {
        attachment(
            self.csv().into_bytes(),
            "text/csv; charset=utf-8",
            file,
        )
    }
}

/// A file as a response that saves rather than shows, never cached and never sniffed.
fn attachment(bytes: Vec<u8>, content_type: &'static str, file: &str) -> Response {
    let mut response = Response::new(Body::from(Bytes::from(bytes)));
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    let safe: String = file
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    let safe = if safe.is_empty() { "report".to_owned() } else { safe };
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{safe}\"")) {
        headers.insert(CONTENT_DISPOSITION, value);
    }
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}
