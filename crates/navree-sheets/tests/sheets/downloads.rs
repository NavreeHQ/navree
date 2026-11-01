//! Downloads of rows a handler has: `Download` and `Exports`, CSV and XLSX.

use chrono::NaiveDate;
use futures_util::stream;
use navree_http::response::IntoResponse;
use navree_kernel::{Module, ModuleBuilder};
use navree_sheets::{
    CSV_CONTENT_TYPE, Cell, Download, Exportable, Exports, HeaderCatalog, ModuleBuilderSheetsExt,
    SheetFormat, XLSX_CONTENT_TYPE,
};

use crate::support::{self, csv_lines};
use crate::xlsx::{Data, read_xlsx};

#[derive(Exportable)]
#[export(sheet = "Deals")]
struct DealRow {
    #[export(header = "deals.title")]
    title: String,
    #[export(header = "Amount", format = "#,##0.00")]
    amount: f64,
    closed_on: Option<NaiveDate>,
    won: bool,
    #[export(skip)]
    #[allow(dead_code)]
    internal: String,
}

fn deals() -> Vec<DealRow> {
    vec![
        DealRow {
            title: "=HYPERLINK(\"http://evil\")".into(),
            amount: 1200.5,
            closed_on: NaiveDate::from_ymd_opt(2026, 10, 1),
            won: true,
            internal: "x".into(),
        },
        DealRow {
            title: "تجديد العقد".into(),
            amount: -5.0,
            closed_on: None,
            won: false,
            internal: String::new(),
        },
    ]
}

async fn body(download: Download) -> (http::response::Parts, Vec<u8>) {
    let (parts, body) = download.into_response().into_parts();
    let bytes = navree_http::body::to_bytes(body, usize::MAX).await.unwrap();
    (parts, bytes.to_vec())
}

#[tokio::test]
async fn a_csv_download_has_a_bom_escaped_formulas_and_attachment_headers() {
    let download = Download::csv("deals", deals());
    assert_eq!(download.filename(), "deals.csv");
    let (parts, bytes) = body(download).await;
    assert_eq!(parts.status, 200);
    assert_eq!(parts.headers["content-type"], CSV_CONTENT_TYPE);
    assert_eq!(
        parts.headers["content-disposition"],
        "attachment; filename=\"deals.csv\"; filename*=UTF-8''deals.csv"
    );
    assert_eq!(parts.headers["cache-control"], "no-store");
    assert_eq!(parts.headers["x-content-type-options"], "nosniff");
    assert_eq!(parts.headers["content-length"], bytes.len().to_string());
    assert!(bytes.starts_with(b"\xEF\xBB\xBF"));
    assert_eq!(
        csv_lines(&bytes),
        [
            "deals.title,Amount,Closed on,Won",
            "\"'=HYPERLINK(\"\"http://evil\"\")\",1200.5,2026-10-01,true",
            "تجديد العقد,-5,,false",
        ]
    );
}

#[tokio::test]
async fn an_xlsx_download_has_typed_cells_and_the_sheet_name() {
    let download = Download::xlsx("deals", deals()).unwrap();
    assert_eq!(download.filename(), "deals.xlsx");
    let (parts, bytes) = body(download).await;
    assert_eq!(parts.headers["content-type"], XLSX_CONTENT_TYPE);
    let (names, rows) = read_xlsx(&bytes);
    assert_eq!(names, ["Deals"]);
    assert_eq!(
        rows[0],
        [
            Data::String("deals.title".into()),
            Data::String("Amount".into()),
            Data::String("Closed on".into()),
            Data::String("Won".into())
        ]
    );
    // Text is never a formula in XLSX: it is written as it is.
    assert_eq!(
        rows[1][0],
        Data::String("=HYPERLINK(\"http://evil\")".into())
    );
    assert_eq!(rows[1][1], Data::Float(1200.5));
    match &rows[1][2] {
        Data::DateTime(at) => {
            assert_eq!(
                at.date(),
                NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
            );
        }
        other => panic!("a date cell, not {other:?}"),
    }
    assert_eq!(rows[1][3], Data::Bool(true));
    assert_eq!(rows[2][0], Data::String("تجديد العقد".into()));
    assert_eq!(rows[2][2], Data::Empty);
}

#[tokio::test]
async fn large_downloads_keep_every_row() {
    let many = || {
        (0u32..50_000).map(|n| DealRow {
            title: format!("صفقة {n}"),
            amount: f64::from(n) / 4.0,
            closed_on: NaiveDate::from_ymd_opt(2026, 1, 1)
                .map(|day| day + chrono::Days::new(u64::from(n % 365))),
            won: n % 3 == 0,
            internal: String::new(),
        })
    };

    let (_, bytes) = body(Download::xlsx("many", many()).unwrap()).await;
    let (_, rows) = read_xlsx(&bytes);
    assert_eq!(rows.len(), 50_001, "the header and every row");
    assert_eq!(rows[50_000][0], Data::String("صفقة 49999".into()));
    assert_eq!(rows[50_000][1], Data::Float(12_499.75));
    assert_eq!(rows[50_000][3], Data::Bool(false));

    let (_, bytes) = body(Download::csv("many", many())).await;
    let lines = csv_lines(&bytes);
    assert_eq!(lines.len(), 50_001);
    assert_eq!(lines[50_000], "صفقة 49999,12499.75,2026-12-26,false");
}

#[tokio::test]
async fn a_streamed_csv_download_writes_rows_as_they_come() {
    let rows = stream::iter((1..=600).map(|n| {
        Ok::<_, std::io::Error>(DealRow {
            title: format!("Deal {n}"),
            amount: f64::from(n),
            closed_on: None,
            won: n % 2 == 0,
            internal: String::new(),
        })
    }));
    let download = Download::csv_stream("big", rows);
    let (parts, bytes) = body(download).await;
    assert!(parts.headers.get("content-length").is_none(), "streamed");
    let lines = csv_lines(&bytes);
    assert_eq!(lines.len(), 601);
    assert_eq!(lines[600], "Deal 600,600,,true");
}

#[tokio::test]
async fn a_failing_stream_cuts_the_download_short() {
    let rows = stream::iter(vec![
        Ok(DealRow {
            title: "First".into(),
            amount: 1.0,
            closed_on: None,
            won: true,
            internal: String::new(),
        }),
        Err(std::io::Error::other("database went away")),
    ]);
    let (_, body) = Download::csv_stream("cut", rows)
        .into_response()
        .into_parts();
    let error = navree_http::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("database went away"), "{error}");
}

/// The app's export headers, from a catalog.
struct Headers;

impl Module for Headers {
    fn register(&self, m: &mut ModuleBuilder) {
        m.header_catalog(
            HeaderCatalog::new("en")
                .locale(
                    "en",
                    [("deals.title", "Title"), ("contacts.email", "Email")],
                )
                .locale("ar", [("deals.title", "العنوان")]),
        );
    }
}

#[tokio::test]
async fn exports_use_the_configured_csv_options_and_translated_headers() {
    let app = support::builder()
        .config("sheets.csv.delimiter", ";")
        .config("sheets.csv.bom", false)
        .config("sheets.csv.escape_formulas", false)
        .module(Headers)
        .build()
        .await;
    let exports = app.get::<Exports>().await;
    assert_eq!(
        exports.headers::<DealRow>(),
        ["Title", "Amount", "Closed on", "Won"]
    );
    let (_, bytes) = body(exports.csv("deals", deals())).await;
    assert!(!bytes.starts_with(b"\xEF\xBB\xBF"));
    let lines = csv_lines(&bytes);
    assert_eq!(lines[0], "Title;Amount;Closed on;Won");
    assert_eq!(
        lines[1],
        "\"=HYPERLINK(\"\"http://evil\"\")\";1200.5;2026-10-01;true"
    );

    let (parts, bytes) = body(exports.file(SheetFormat::Xlsx, "deals", deals()).unwrap()).await;
    assert_eq!(parts.headers["content-type"], XLSX_CONTENT_TYPE);
    let (_, rows) = read_xlsx(&bytes);
    assert_eq!(rows[0][0], Data::String("Title".into()));

    let rows = stream::iter(deals().into_iter().map(Ok::<_, std::io::Error>));
    let (_, bytes) = body(exports.csv_stream("deals", rows)).await;
    assert_eq!(csv_lines(&bytes)[0], "Title;Amount;Closed on;Won");
}

#[test]
fn cells_come_from_fields() {
    let row = &deals()[1];
    assert_eq!(
        row.cells(),
        [
            Cell::Text("تجديد العقد".into()),
            Cell::Float(-5.0),
            Cell::Empty,
            Cell::Bool(false)
        ]
    );
    let columns = DealRow::columns();
    assert_eq!(columns[1].format.as_deref(), Some("#,##0.00"));
    assert_eq!(DealRow::sheet_name(), "Deals");
}
