//! Registered exports: `/api/exports`, at once and queued.

use std::sync::Arc;
use std::time::Duration;

use navree_contracts::{Clock, FixedClock};
use navree_db::Db;
use navree_http::response::IntoResponse;
use navree_kernel::{Module, ModuleBuilder};
use navree_queue::OperationStatus;
use navree_sheets::{
    ExportRequest, ExportResult, Exports, HeaderCatalog, ModuleBuilderSheetsExt, SheetFormat,
};
use navree_testing::TestApp;
use serde_json::{Value, json};

use crate::support::{
    self, ADMIN, ContactFilter, ContactsExport, OTHER, csv_lines, file_on_disk, id_of, operation,
    principal, run_queue, seed,
};
use crate::xlsx::{Data, read_xlsx};

/// Header texts for the export's translation keys.
struct Headers;

impl Module for Headers {
    fn register(&self, m: &mut ModuleBuilder) {
        m.header_catalog(HeaderCatalog::new("en").locale("en", [("contacts.email", "Email")]));
    }
}

async fn seeded(
    configure: impl FnOnce(navree_testing::TestAppBuilder) -> navree_testing::TestAppBuilder,
) -> TestApp {
    let app = configure(support::builder().module(Headers)).build().await;
    seed(
        &app,
        &[
            ("ada@example.com", "Ada Lovelace", Some(36), true),
            ("alan@example.com", "Alan Turing", Some(41), false),
            ("grace@example.com", "=cmd|' /C calc'!A0", None, true),
        ],
    )
    .await;
    app
}

#[tokio::test]
async fn the_index_lists_the_exports_a_user_may_run() {
    let app = seeded(|b| b).await;
    let body = app
        .acting_as(principal(ADMIN))
        .get("/api/exports")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["data"][0]["name"], "contacts");
    assert_eq!(
        body["data"][0]["columns"],
        json!([
            { "key": "email", "header": "Email" },
            { "key": "name", "header": "Name" },
            { "key": "age", "header": "Age" },
            { "key": "vip", "header": "Vip" },
        ])
    );
    app.acting_as(principal(OTHER))
        .get("/api/exports")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));
    app.client()
        .get("/api/exports")
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn a_csv_export_streams_every_page_with_its_parameters() {
    let app = seeded(|b| b.config("sheets.exports.page_size", 2)).await;
    let admin = app.acting_as(principal(ADMIN));
    let response = admin
        .get("/api/exports/contacts")
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        response.header("content-disposition"),
        Some("attachment; filename=\"contacts.csv\"; filename*=UTF-8''contacts.csv")
    );
    assert_eq!(
        response.header("content-type"),
        Some("text/csv; charset=utf-8")
    );
    assert!(response.bytes().starts_with(b"\xEF\xBB\xBF"));
    assert_eq!(
        csv_lines(response.bytes()),
        [
            "Email,Name,Age,Vip",
            "ada@example.com,Ada Lovelace,36,true",
            "alan@example.com,Alan Turing,41,false",
            "grace@example.com,'=cmd|' /C calc'!A0,,true",
        ]
    );

    let response = admin
        .get("/api/exports/contacts?vip=true")
        .send()
        .await
        .assert_status(200);
    assert!(
        response
            .header("content-disposition")
            .unwrap()
            .contains("vip-contacts.csv")
    );
    assert_eq!(csv_lines(response.bytes()).len(), 3);
}

#[tokio::test]
async fn an_xlsx_export_is_built_page_by_page() {
    let app = seeded(|b| b.config("sheets.exports.page_size", 1)).await;
    let response = app
        .acting_as(principal(ADMIN))
        .get("/api/exports/contacts?format=xlsx&vip=false")
        .send()
        .await
        .assert_status(200);
    assert!(
        response
            .header("content-disposition")
            .unwrap()
            .contains("contacts.xlsx")
    );
    let (names, rows) = read_xlsx(response.bytes());
    assert_eq!(names, ["Contacts"]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1][0], Data::String("alan@example.com".into()));
    assert_eq!(rows[1][2], Data::Float(41.0));
    assert_eq!(rows[1][3], Data::Bool(false));
}

#[tokio::test]
async fn exports_are_refused_with_a_reason() {
    let app = seeded(|b| b.config("sheets.exports.max_rows", 2)).await;
    let admin = app.acting_as(principal(ADMIN));
    admin
        .get("/api/exports/contacts")
        .send()
        .await
        .assert_error_code(400, "too_many_rows");
    admin
        .get("/api/exports/contacts?vip=false")
        .send()
        .await
        .assert_status(200);
    admin
        .get("/api/exports/contacts?format=pdf")
        .send()
        .await
        .assert_status(400);
    admin
        .get("/api/exports/contacts?vip=maybe")
        .send()
        .await
        .assert_status(400);
    admin
        .get("/api/exports/deals")
        .send()
        .await
        .assert_status(404);
    app.acting_as(principal(OTHER))
        .get("/api/exports/contacts")
        .send()
        .await
        .assert_status(403);
    app.client()
        .get("/api/exports/contacts")
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn a_queued_export_writes_a_file_and_finishes_with_a_signed_link() {
    let app = seeded(|b| b.config("sheets.exports.page_size", 2)).await;
    let admin = app.acting_as(principal(ADMIN));
    let pending = admin
        .post("/api/exports/contacts")
        .json(&json!({ "format": "xlsx", "params": { "vip": true } }))
        .send()
        .await
        .assert_status(202)
        .json_value();
    assert_eq!(pending["kind"], "export.contacts");
    assert_eq!(pending["status"], "pending");
    assert_eq!(pending["owner_id"], ADMIN.to_string());

    run_queue(&app).await;
    let done = operation(&app, id_of(&pending)).await;
    assert_eq!(done.status, OperationStatus::Succeeded, "{:?}", done.error);
    let result: ExportResult = serde_json::from_value(done.result.clone().unwrap()).unwrap();
    assert_eq!(result.filename, "vip-contacts.xlsx");
    assert_eq!(result.format, SheetFormat::Xlsx);
    assert_eq!(result.rows, 2);
    assert_eq!(result.disk, "files");
    assert!(result.path.starts_with("exports/"), "{}", result.path);
    assert!(
        result.path.ends_with("/vip-contacts.xlsx"),
        "{}",
        result.path
    );
    let file = file_on_disk(&app, "files", &result.path).await;
    assert_eq!(file.len() as u64, result.bytes);
    assert!(
        result.url.starts_with("/storage/files/exports/"),
        "{}",
        result.url
    );

    // The signed link downloads the file, without signing in.
    let fetched = app
        .client()
        .get(&result.url)
        .send()
        .await
        .assert_status(200);
    assert_eq!(fetched.bytes().to_vec(), file);
}

#[tokio::test]
async fn a_queued_export_over_the_limit_fails_its_operation() {
    let app = seeded(|b| b.config("sheets.exports.max_rows", 1)).await;
    let pending = app
        .acting_as(principal(ADMIN))
        .post("/api/exports/contacts")
        .json(&json!({}))
        .send()
        .await
        .assert_status(202)
        .json_value();
    run_queue(&app).await;
    let done = operation(&app, id_of(&pending)).await;
    assert_eq!(done.status, OperationStatus::Failed);
    assert_eq!(
        done.error.as_deref(),
        Some("The export has more than 1 rows.")
    );
}

/// Every contact's email, for every signed-in user, without a count.
#[navree_macros::injectable]
struct Everyone;

#[derive(navree_sheets::Exportable)]
struct EmailRow {
    email: String,
}

impl navree_sheets::Export for Everyone {
    const NAME: &'static str = "everyone";
    type Row = EmailRow;
    type Params = ();

    fn permission() -> Option<String> {
        None
    }

    async fn rows(
        &self,
        ctx: &navree_sheets::ExportContext,
        _params: &(),
        page: navree_sheets::ExportPage,
    ) -> Result<Vec<EmailRow>, navree_contracts::BoxError> {
        let all: Vec<EmailRow> = support::contacts(&*ctx.app.get::<Db>().await?)
            .await
            .into_iter()
            .map(|c| EmailRow { email: c.email })
            .collect();
        Ok(all
            .into_iter()
            .skip(usize::try_from(page.offset)?)
            .take(usize::try_from(page.limit)?)
            .collect())
    }
}

struct EveryoneModule;

impl Module for EveryoneModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.export::<Everyone>();
    }
}

#[tokio::test]
async fn an_export_without_a_count_is_stopped_at_the_limit() {
    let app = seeded(|b| {
        b.module(EveryoneModule)
            .config("sheets.exports.max_rows", 2)
            .config("sheets.exports.page_size", 2)
    })
    .await;
    // Without a permission, every signed-in user may run it.
    let other = app.acting_as(principal(OTHER));
    let body = other.get("/api/exports").send().await.json_value();
    assert_eq!(body["data"][0]["name"], "everyone");
    let pending = other
        .post("/api/exports/everyone")
        .json(&json!({ "format": "csv" }))
        .send()
        .await
        .assert_status(202)
        .json_value();
    run_queue(&app).await;
    let done = operation(&app, id_of(&pending)).await;
    assert_eq!(done.status, OperationStatus::Failed);
    assert_eq!(
        done.error.as_deref(),
        Some("The export has more than 2 rows.")
    );

    // Downloaded at once, the stream stops after the rows that fit.
    let (_, body) = app
        .get::<Exports>()
        .await
        .download::<Everyone>((), ExportRequest::new(SheetFormat::Csv))
        .await
        .unwrap()
        .into_response()
        .into_parts();
    let error = navree_http::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("more than 2 rows"), "{error}");
}

#[tokio::test]
async fn exports_run_from_code() {
    let app = seeded(|b| b).await;
    let exports = app.get::<Exports>().await;
    assert_eq!(exports.names(), ["contacts"]);

    let download = exports
        .download::<ContactsExport>(
            ContactFilter { vip: Some(false) },
            ExportRequest::new(SheetFormat::Csv),
        )
        .await
        .unwrap();
    assert_eq!(download.filename(), "contacts.csv");

    let db = app.get::<Db>().await;
    let uow = db.uow(Some(ADMIN), None).await;
    let operation_record = exports
        .queue::<ContactsExport>(
            &uow,
            ContactFilter::default(),
            ExportRequest::new(SheetFormat::Csv).user(ADMIN),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();
    run_queue(&app).await;
    let done = operation(&app, operation_record.id).await;
    let result: ExportResult = serde_json::from_value(done.result.unwrap()).unwrap();
    let file = file_on_disk(&app, "files", &result.path).await;
    assert_eq!(csv_lines(&file).len(), 4);
}

#[tokio::test]
async fn prune_deletes_export_files_whose_link_expired() {
    let clock = Arc::new(FixedClock::new(chrono::Utc::now()));
    let app = seeded(|b| {
        b.config("sheets.exports.link_ttl_secs", 60)
            .override_value::<dyn Clock>(clock.clone())
    })
    .await;
    let pending = app
        .acting_as(principal(ADMIN))
        .post("/api/exports/contacts")
        .json(&json!({ "format": "csv" }))
        .send()
        .await
        .assert_status(202)
        .json_value();
    run_queue(&app).await;
    let result: Value = operation(&app, id_of(&pending)).await.result.unwrap();
    let path = result["path"].as_str().unwrap().to_owned();
    let exports = app.get::<Exports>().await;
    assert_eq!(exports.prune_files().await.unwrap(), 0);
    // Files carry the disk's own write time: a few minutes on, the link of
    // a minute has long expired.
    clock.advance(Duration::from_secs(180));
    assert_eq!(exports.prune_files().await.unwrap(), 1);
    let storage = app.get::<navree_storage::Storage>().await;
    assert!(!storage.disk("files").unwrap().exists(&path).await.unwrap());
}
