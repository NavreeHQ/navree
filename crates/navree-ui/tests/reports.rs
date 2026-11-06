//! What a report adds to its chart: its numbers as a file that equals the chart's own
//! table, links from a period to the rows behind it, and the period before to draw beside
//! it, across a clock change.

use chrono::{NaiveDate, Utc};
use navree_ui::charts::{Chart, Grain, Report, Series};
use navree_ui::dates::DateRange;
use navree_ui::render;

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("a real day")
}

fn html(chart: Chart) -> String {
    render(&chart.into())
}

fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

/// The numbers in the cells of the chart's table, row by row, written as a file writes them.
fn twin_cells(out: &str) -> Vec<Vec<String>> {
    out.split("<tr>")
        .skip(2)
        .map(|row| {
            row.split("<td data-num=\"\">")
                .skip(1)
                .map(|cell| {
                    cell.split("</td>")
                        .next()
                        .unwrap_or_default()
                        .replace(',', "")
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_report_downloads_exactly_the_numbers_its_chart_tables() {
    let report = Report::new(
        navree_ui::dates::DateRange::new(day(2026, 11, 2), day(2026, 11, 6)),
        Grain::Day,
        &Utc,
    );
    let sales: &[Option<f64>] = &[Some(12_000.0), Some(1_500.0), Some(0.0), Some(250.0), Some(98_765.0)];
    let orders: &[Option<f64>] = &[Some(4.0), Some(3.0), Some(0.0), Some(1.0), Some(17.0)];
    let chart = report
        .chart(Chart::bar("Sales"))
        .series(Series::new("Sales", sales.iter().copied()))
        .series(Series::new("Orders", orders.iter().copied()));
    let page = html(chart);
    let table = report.table("Day", &[("Sales", sales), ("Orders", orders)]);
    let csv = table.csv();

    assert!(csv.starts_with("\u{feff}Day,Sales,Orders\r\n"), "{csv}");
    let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').split("\r\n").collect();
    assert_eq!(lines.len(), 1 + 5 + 1, "a heading, five days and the empty end: {lines:?}");
    let twin = twin_cells(&page);
    assert_eq!(twin.len(), 5, "{page}");
    for (line, cells) in lines[1..6].iter().zip(&twin) {
        let fields: Vec<&str> = line.split(',').collect();
        assert_eq!(fields[1..], cells[..], "a row of the file is a row of the table: {line}");
    }
    assert_eq!(lines[1], "2026-11-02,12000,4");
    assert_eq!(table.head(), ["Day", "Sales", "Orders"]);
    assert_eq!(table.periods().len(), 5);
}

#[test]
fn a_file_names_its_periods_by_the_grain_and_keeps_gaps_empty() {
    let months = Report::new(
        DateRange::new(day(2026, 1, 15), day(2026, 6, 20)),
        Grain::Month,
        &Utc,
    );
    assert_eq!(months.table("Month", &[]).periods()[0], "2026-01");
    let quarters = Report::new(
        DateRange::new(day(2025, 11, 1), day(2026, 8, 1)),
        Grain::Quarter,
        &Utc,
    );
    assert_eq!(quarters.table("Quarter", &[]).periods()[0], "2025-Q4");
    let years = Report::new(
        DateRange::new(day(2024, 3, 1), day(2026, 8, 1)),
        Grain::Year,
        &Utc,
    );
    assert_eq!(
        years.table("Year", &[]).periods(),
        ["2024", "2025", "2026"]
    );
    let hours = Report::new(
        DateRange::single(day(2026, 11, 2)),
        Grain::Hour,
        &Utc,
    );
    assert_eq!(hours.table("Hour", &[]).periods()[13], "2026-11-02 13:00");

    // A gap is an empty field; a short series is padded and a long one cut.
    let short: &[Option<f64>] = &[Some(1.5), None];
    let long: &[Option<f64>] = &[Some(1.0), Some(2.0), Some(3.0), Some(4.0)];
    let report = Report::new(DateRange::new(day(2026, 11, 2), day(2026, 11, 4)), Grain::Day, &Utc);
    let table = report.table("Day", &[("A", short), ("B", long)]);
    assert_eq!(
        table.csv(),
        "\u{feff}Day,A,B\r\n2026-11-02,1.5,1\r\n2026-11-03,,2\r\n2026-11-04,,3\r\n"
    );
}

#[test]
fn a_file_never_holds_a_formula_and_quotes_what_it_must() {
    let report = Report::new(DateRange::new(day(2026, 11, 2), day(2026, 11, 2)), Grain::Day, &Utc);
    let values: &[Option<f64>] = &[Some(1.0)];
    let table = report.table("Day", &[("=HYPERLINK(\"x\")", values), ("Net, after tax", values)]);
    let csv = table.csv();
    assert!(csv.contains("\"'=HYPERLINK(\"\"x\"\")\""), "{csv}");
    assert!(csv.contains("\"Net, after tax\""), "{csv}");
}

#[test]
fn the_download_saves_with_a_safe_name_and_is_not_cached_or_sniffed() {
    let report = Report::new(DateRange::new(day(2026, 11, 2), day(2026, 11, 3)), Grain::Day, &Utc);
    let table = report.table("Day", &[]);
    let response = table.csv_response("../orders \"november\".csv; x");
    let headers = response.headers();
    assert_eq!(headers["content-type"], "text/csv; charset=utf-8");
    assert_eq!(
        headers["content-disposition"],
        "attachment; filename=\"..ordersnovember.csvx\""
    );
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    let nameless = table.csv_response("///");
    assert_eq!(
        nameless.headers()["content-disposition"],
        "attachment; filename=\"report\""
    );
}

#[test]
fn a_period_links_to_the_rows_behind_it() {
    let days = Report::new(DateRange::new(day(2026, 11, 2), day(2026, 11, 4)), Grain::Day, &Utc);
    assert_eq!(
        days.drill_links("/orders", "created_at"),
        [
            "/orders?filter[created_at][gte]=2026-11-02&filter[created_at][lte]=2026-11-02",
            "/orders?filter[created_at][gte]=2026-11-03&filter[created_at][lte]=2026-11-03",
            "/orders?filter[created_at][gte]=2026-11-04&filter[created_at][lte]=2026-11-04",
        ]
    );

    // A week covers its days, and the last one stops at the window's last day.
    let weeks = Report::new(DateRange::new(day(2026, 11, 2), day(2026, 11, 12)), Grain::Week, &Utc);
    let links = weeks.drill_links("/orders?status=paid", "created_at");
    assert_eq!(links.len(), 2);
    assert_eq!(
        links[0],
        "/orders?status=paid&filter[created_at][gte]=2026-11-02&filter[created_at][lte]=2026-11-08"
    );
    assert_eq!(
        links[1],
        "/orders?status=paid&filter[created_at][gte]=2026-11-09&filter[created_at][lte]=2026-11-12"
    );

    // An hour is listed with its day; a column that is no plain name gives no link.
    let hours = Report::new(DateRange::single(day(2026, 11, 2)), Grain::Hour, &Utc);
    assert!(hours.drill_links("/o", "at").iter().all(|link| link.contains("gte]=2026-11-02&filter[at][lte]=2026-11-02")));
    let long = "c".repeat(65);
    for bad in ["", "a b", "a]&x=1", "a[b]", long.as_str()] {
        assert!(days.drill_links("/orders", bad).is_empty(), "{bad:?}");
    }
}

#[test]
fn a_bar_and_a_period_name_become_links_when_a_chart_drills() {
    let report = Report::new(DateRange::new(day(2026, 11, 2), day(2026, 11, 4)), Grain::Day, &Utc);
    let links = report.drill_links("/orders", "created_at");
    let out = html(
        report
            .chart(Chart::bar("Orders"))
            .series(Series::new("Orders", [4, 7, 5]))
            .series(Series::new("Refunds", [1, 3, 2]))
            .drill(links.clone()),
    );
    assert_eq!(count(&out, r#"data-slot="chart-drill""#), 6, "a link round each bar: {out}");
    assert!(out.contains(r#"aria-label="Orders: "#), "{out}");
    assert!(out.contains(r#"tabindex="-1""#), "the chart stays one tab stop: {out}");
    let first = links[0].replace('&', "&amp;");
    assert!(out.contains(&format!(r#"<th scope="row"><a href="{first}">"#)), "the period's name in the table links too: {out}");
    assert_eq!(count(&out, r#"<th scope="row"><a href"#), 3, "{out}");

    // A line has no bars: the table's names link, and nothing else does.
    let line = html(
        report
            .chart(Chart::line("Orders"))
            .series(Series::new("Orders", [4, 7, 5]))
            .drill(links),
    );
    assert_eq!(count(&line, r#"data-slot="chart-drill""#), 0, "{line}");
    assert_eq!(count(&line, r#"<th scope="row"><a href"#), 3, "{line}");

    // Without links the chart is as it was.
    let plain = html(report.chart(Chart::bar("Orders")).series(Series::new("Orders", [4, 7, 5])));
    assert!(!plain.contains("chart-drill") && !plain.contains(r#"<th scope="row"><a"#), "{plain}");
}

#[test]
fn merged_bars_have_no_links_because_a_bar_stands_for_several_rows() {
    let report = Report::new(DateRange::new(day(2026, 1, 1), day(2026, 3, 31)), Grain::Day, &Utc);
    let values: Vec<u32> = (0..report.buckets().len() as u32).collect();
    let out = html(
        report
            .chart(Chart::bar("Orders"))
            .max_bars(20)
            .series(Series::new("Orders", values))
            .drill(report.drill_links("/orders", "created_at")),
    );
    assert_eq!(count(&out, "chart-drill"), 0, "{}", &out[..300.min(out.len())]);
}

#[test]
fn the_period_before_has_as_many_days_across_a_clock_change() {
    use chrono_tz::Europe::London;
    // London's clocks went back on Sunday 25 October 2026: that day is 25 hours long.
    let now = Report::new(DateRange::new(day(2026, 10, 26), day(2026, 11, 1)), Grain::Day, &London);
    let before = now.previous(&London);
    assert_eq!(before.range(), DateRange::new(day(2026, 10, 19), day(2026, 10, 25)));
    assert_eq!(before.buckets().len(), now.buckets().len(), "seven days against seven");
    let long_day = before.buckets()[6];
    assert_eq!((long_day.end - long_day.start).num_hours(), 25);
    assert!(now.buckets().iter().all(|b| (b.end - b.start).num_hours() == 24));
    assert_eq!(before.grain(), now.grain());

    // The window before a window that holds the change is the same length too.
    let across = Report::new(DateRange::new(day(2026, 10, 23), day(2026, 10, 29)), Grain::Day, &London);
    assert_eq!(across.previous(&London).range(), DateRange::new(day(2026, 10, 16), day(2026, 10, 22)));

    // Weeks and months keep their grain; a first window has a before.
    let month = Report::new(DateRange::new(day(2026, 3, 1), day(2026, 3, 31)), Grain::Day, &Utc);
    assert_eq!(month.previous(&Utc).range(), DateRange::new(day(2026, 1, 29), day(2026, 2, 28)));
}

#[test]
fn the_period_before_is_laid_on_this_periods_by_position_and_drawn_quiet() {
    let now = Report::new(DateRange::new(day(2026, 11, 2), day(2026, 11, 6)), Grain::Day, &Utc);
    let before = now.previous(&Utc);
    // The figures of the days before: one fewer than this window has periods.
    let found: &[Option<f64>] = &[Some(3.0), Some(5.0), None, Some(9.0)];
    let series = now.previous_series("Previous period", found);
    let out = html(
        now.chart(Chart::line("Orders"))
            .series(Series::new("This period", [4, 7, 5, 6, 8]))
            .series(series),
    );
    assert_eq!(before.buckets().len(), 5);
    assert!(out.contains(r#"data-color="other""#), "the earlier period is in the quiet ink: {out}");
    assert!(out.contains("Previous period"), "{out}");
    let current: &[Option<f64>] = &[Some(4.0), Some(7.0), Some(5.0), Some(6.0), Some(8.0)];
    let table = now.table("Day", &[("This period", current), ("Previous period", found)]);
    assert_eq!(
        table.csv().lines().nth(5).unwrap_or_default(),
        "2026-11-06,8,",
        "the fifth day has no earlier figure"
    );
}
