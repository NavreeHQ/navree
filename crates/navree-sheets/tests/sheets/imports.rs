//! Imports end to end: upload, staging, the preview, mappings, applying,
//! duplicates and error files.

use std::sync::Arc;
use std::time::Duration;

use encoding_rs::{WINDOWS_1252, WINDOWS_1256};
use navree_contracts::{Clock, FixedClock};
use navree_db::Db;
use navree_queue::{OperationStatus, Operations};
use navree_sheets::{
    Cell, DuplicateMode, ImportStatus, Imports, Mapping, RowQuery, RowStatus, SheetsError, Upload,
    UploadOptions, write_xlsx,
};
use navree_testing::{TestApp, TestClient, TestResponse};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{
    self, ADMIN, OTHER, completed, contact, csv_lines, id_of, import, operation, principal,
    run_queue, seed, stored_contacts, upload,
};

/// Uploads, runs the staging job and returns the import as the API shows it.
async fn staged(app: &TestApp, client: &TestClient, name: &str, bytes: &[u8], extra: &[(&str, &str)]) -> Value {
    let uploaded = upload(client, name, bytes, extra).await.assert_status(202).json_value();
    run_queue(app).await;
    show(client, id_of(&uploaded)).await
}

async fn show(client: &TestClient, id: Uuid) -> Value {
    client
        .get(&format!("/api/imports/{id}"))
        .send()
        .await
        .assert_status(200)
        .json_value()
}

async fn apply(app: &TestApp, client: &TestClient, id: Uuid, duplicates: &str) -> Value {
    client
        .post(&format!("/api/imports/{id}/apply"))
        .json(&json!({ "duplicates": duplicates }))
        .send()
        .await
        .assert_status(202);
    run_queue(app).await;
    show(client, id).await
}

async fn error_file(client: &TestClient, id: Uuid) -> TestResponse {
    client
        .get(&format!("/api/imports/{id}/errors"))
        .send()
        .await
}

#[tokio::test]
async fn an_upload_is_staged_previewed_and_applied() {
    let app = support::app().await;
    let admin = app.acting_as(principal(ADMIN));
    let file = b"Email,Name,Age,VIP\nada@example.com,Ada Lovelace,36,yes\nnot-an-email,Bob,x,no\nalan@example.com,Alan Turing,,0\n";
    let uploaded = upload(&admin, "contacts.csv", file, &[]).await.assert_status(202).json_value();
    assert_eq!(uploaded["status"], "uploaded");
    assert_eq!(uploaded["importer"], "contacts");
    assert_eq!(uploaded["filename"], "contacts.csv");
    assert_eq!(uploaded["owner_id"], ADMIN.to_string());
    let id = id_of(&uploaded);
    let staging = Uuid::parse_str(uploaded["operation_id"].as_str().unwrap()).unwrap();

    run_queue(&app).await;
    let record = show(&admin, id).await;
    assert_eq!(record["status"], "staged", "{record}");
    assert_eq!(record["format"], "csv");
    assert_eq!(record["encoding"], "UTF-8");
    assert_eq!(record["headers"], json!(["Email", "Name", "Age", "VIP"]));
    assert_eq!(
        record["mapping"],
        json!({ "age": "Age", "email": "Email", "name": "Name", "vip": "VIP" })
    );
    assert_eq!(record["counts"]["total"], 3);
    assert_eq!(record["counts"]["valid"], 2);
    assert_eq!(record["counts"]["invalid"], 1);
    assert_eq!(record["has_error_file"], true);
    let op = operation(&app, staging).await;
    assert_eq!(op.status, OperationStatus::Succeeded);
    assert_eq!(op.kind, "import.contacts");
    assert_eq!(op.result.unwrap()["status"], "staged");

    // The preview.
    let page = admin
        .get(&format!("/api/imports/{id}/rows"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(page["meta"]["total"], 3);
    assert_eq!(page["data"][0]["row"], 2);
    assert_eq!(page["data"][0]["status"], "valid");
    assert_eq!(
        page["data"][0]["values"],
        json!({ "age": "36", "email": "ada@example.com", "name": "Ada Lovelace", "vip": "yes" })
    );
    let invalid = admin
        .get(&format!("/api/imports/{id}/rows?status=invalid"))
        .send()
        .await
        .json_value();
    assert_eq!(invalid["meta"]["total"], 1);
    assert_eq!(invalid["data"][0]["row"], 3);
    assert_eq!(
        invalid["data"][0]["errors"],
        json!({ "age": ["age must be an integer number"] })
    );

    let record = apply(&app, &admin, id, "skip").await;
    assert_eq!(record["status"], "completed", "{record}");
    assert_eq!(record["duplicates"], "skip");
    assert_eq!(record["counts"]["created"], 2);
    assert_eq!(record["counts"]["invalid"], 1);
    assert!(record["finished_at"].is_string());
    assert!(completed(id));
    assert_eq!(
        stored_contacts(&app).await,
        [
            contact("ada@example.com", "Ada Lovelace", Some(36), true),
            contact("alan@example.com", "Alan Turing", None, false),
        ]
    );

    let errors = error_file(&admin, id).await.assert_status(200);
    assert_eq!(
        errors.header("content-disposition"),
        Some("attachment; filename=\"contacts-errors.csv\"; filename*=UTF-8''contacts-errors.csv")
    );
    assert_eq!(
        csv_lines(errors.bytes()),
        [
            "Row,Email,Name,Age,VIP,Errors",
            "3,not-an-email,Bob,x,no,age must be an integer number",
        ]
    );
}

#[tokio::test]
async fn windows_1256_arabic_files_are_detected_and_read() {
    let app = support::app().await;
    let admin = app.acting_as(principal(ADMIN));
    let text = "البريد الإلكتروني,الاسم,Age\n\
        mohammed@example.com,محمد عبد الله,40\n\
        fatima@example.com,فاطمة الزهراء,31\n\
        noura@example.com,نورة القحطاني,28\n";
    let (bytes, _, unmappable) = WINDOWS_1256.encode(text);
    assert!(!unmappable);
    let record = staged(&app, &admin, "عملاء.csv", &bytes, &[]).await;
    assert_eq!(record["encoding"], "windows-1256", "{record}");
    assert_eq!(record["filename"], "عملاء.csv");
    assert_eq!(record["mapping"]["email"], "البريد الإلكتروني");
    assert_eq!(record["mapping"]["name"], "الاسم");
    assert_eq!(record["counts"]["valid"], 3);
    let record = apply(&app, &admin, id_of(&record), "skip").await;
    assert_eq!(record["counts"]["created"], 3);
    let names: Vec<String> = stored_contacts(&app).await.into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["فاطمة الزهراء", "محمد عبد الله", "نورة القحطاني"]);
}

#[tokio::test]
async fn utf16_tab_separated_files_are_read() {
    let app = support::app().await;
    let admin = app.acting_as(principal(ADMIN));
    // Excel's "Unicode Text": UTF-16LE with a byte-order mark, tabs.
    let text = "Email\tName\r\nyuki@example.jp\t山田 由紀\r\nrenee@example.fr\tRenée Côté\r\n";
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    let record = staged(&app, &admin, "contacts.txt", &bytes, &[]).await;
    assert_eq!(record["encoding"], "UTF-16LE", "{record}");
    assert_eq!(record["counts"]["valid"], 2);
    apply(&app, &admin, id_of(&record), "skip").await;
    let names: Vec<String> = stored_contacts(&app).await.into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["Renée Côté", "山田 由紀"]);
}

#[tokio::test]
async fn a_given_encoding_wins_over_detection() {
    let app = support::app().await;
    let admin = app.acting_as(principal(ADMIN));
    let (bytes, _, _) = WINDOWS_1252.encode("Email;Name\nrenee@example.fr;Renée\n");
    let record = staged(&app, &admin, "x.csv", &bytes, &[("encoding", "windows-1252")]).await;
    assert_eq!(record["encoding"], "windows-1252");
    let rows = admin
        .get(&format!("/api/imports/{}/rows", id_of(&record)))
        .send()
        .await
        .json_value();
    assert_eq!(rows["data"][0]["values"]["name"], "Renée");
}

#[tokio::test]
async fn xlsx_files_are_read_with_typed_cells() {
    let app = support::app().await;
    let admin = app.acting_as(principal(ADMIN));
    let file = write_xlsx(
        "People",
        &[],
        &["Email".into(), "Name".into(), "Age".into(), "Vip".into()],
        &[
            vec![
                Cell::Text("ada@example.com".into()),
                Cell::Text("Ada".into()),
                Cell::Int(36),
                Cell::Bool(true),
            ],
            vec![
                Cell::Text("alan@example.com".into()),
                Cell::Text("Alan".into()),
                Cell::Float(41.0),
                Cell::Empty,
            ],
        ],
    )
    .unwrap();
    let record = staged(&app, &admin, "people.xlsx", &file, &[]).await;
    assert_eq!(record["format"], "xlsx", "{record}");
    assert_eq!(record["encoding"], Value::Null);
    let record = apply(&app, &admin, id_of(&record), "skip").await;
    assert_eq!(record["counts"]["created"], 2);
    assert_eq!(
        stored_contacts(&app).await,
        [
            contact("ada@example.com", "Ada", Some(36), true),
            contact("alan@example.com", "Alan", Some(41), false),
        ]
    );
}

#[tokio::test]
async fn unmatched_columns_wait_for_a_mapping_that_can_be_saved() {
    let app = support::app().await;
    let admin = app.acting_as(principal(ADMIN));
    let record = staged(
        &app,
        &admin,
        "export.csv",
        b"Mail;Full name\nada@example.com;Ada\nalan@example.com;Alan\n",
        &[],
    )
    .await;
    assert_eq!(record["status"], "needs_mapping", "{record}");
    assert_eq!(record["missing"], json!(["email", "name"]));
    assert_eq!(record["counts"]["total"], 2);
    let id = id_of(&record);
    admin
        .post(&format!("/api/imports/{id}/apply"))
        .json(&json!({}))
        .send()
        .await
        .assert_error_code(409, "import_not_ready");

    let invalid = admin
        .put(&format!("/api/imports/{id}/mapping"))
        .json(&json!({ "mapping": { "email": "Email", "name": "Full name", "nickname": "Mail" } }))
        .send()
        .await
        .assert_error_code(400, "validation_failed")
        .json_value();
    assert_eq!(
        invalid["errors"]["email"],
        json!([
            "email is mapped to `Email`, which is not a column of the file",
            "email should not be empty"
        ])
    );
    assert_eq!(invalid["errors"]["nickname"], json!(["nickname is not a column of this import"]));

    let remapped = admin
        .put(&format!("/api/imports/{id}/mapping"))
        .json(&json!({ "mapping": { "email": "Mail", "name": "Full name" }, "save": true }))
        .send()
        .await
        .assert_status(202)
        .json_value();
    assert_eq!(remapped["status"], "uploaded");
    run_queue(&app).await;
    let record = show(&admin, id).await;
    assert_eq!(record["status"], "staged");
    assert_eq!(record["counts"]["valid"], 2);

    // A later file with the same columns, in another order and case,
    // reuses the saved mapping.
    let later = staged(&app, &admin, "export.csv", b"FULL NAME;mail\nGrace;grace@example.com\n", &[]).await;
    assert_eq!(later["status"], "staged", "{later}");
    assert_eq!(later["mapping"], json!({ "email": "mail", "name": "FULL NAME" }));
}

#[tokio::test]
async fn a_mapping_can_come_with_the_upload() {
    let app = support::app().await;
    let admin = app.acting_as(principal(ADMIN));
    let record = staged(
        &app,
        &admin,
        "x.csv",
        b"A,B\nada@example.com,Ada\n",
        &[("mapping", r#"{"email": "A", "name": "B"}"#)],
    )
    .await;
    assert_eq!(record["status"], "staged", "{record}");
    assert_eq!(record["counts"]["valid"], 1);
    upload(&admin, "x.csv", b"A\n1\n", &[("mapping", "not json")])
        .await
        .assert_validation_error("mapping");
}

#[tokio::test]
async fn duplicates_are_updated_skipped_or_refused() {
    let app = support::app().await;
    seed(&app, &[("ada@example.com", "Ada", Some(30), false)]).await;
    let admin = app.acting_as(principal(ADMIN));
    let file = b"Email,Name\nADA@example.com,Ada Byron\nbob@example.com,Bob\nbob@example.com,Bobby\n";
    let record = staged(&app, &admin, "c.csv", file, &[]).await;
    assert_eq!(record["counts"]["valid"], 1, "{record}");
    assert_eq!(record["counts"]["duplicate"], 1);
    assert_eq!(record["counts"]["repeated"], 1);
    let id = id_of(&record);
    let repeated = admin
        .get(&format!("/api/imports/{id}/rows?status=repeated"))
        .send()
        .await
        .json_value();
    assert_eq!(repeated["data"][0]["row"], 4);
    assert_eq!(repeated["data"][0]["errors"], json!({ "row": ["Repeats row 3"] }));

    admin
        .post(&format!("/api/imports/{id}/apply"))
        .json(&json!({ "duplicates": "fail" }))
        .send()
        .await
        .assert_error_code(409, "import_has_duplicates");

    let record = apply(&app, &admin, id, "update").await;
    assert_eq!(record["status"], "completed", "{record}");
    assert_eq!(record["counts"]["updated"], 1);
    assert_eq!(record["counts"]["created"], 1);
    assert_eq!(record["counts"]["repeated"], 1);
    assert_eq!(
        stored_contacts(&app).await,
        [
            contact("ada@example.com", "Ada Byron", None, false),
            contact("bob@example.com", "Bob", None, false),
        ]
    );

    // The same file again: both keys exist now, and are left alone.
    let again = staged(&app, &admin, "c.csv", file, &[]).await;
    assert_eq!(again["counts"]["duplicate"], 2, "{again}");
    let again = apply(&app, &admin, id_of(&again), "skip").await;
    assert_eq!(again["counts"]["skipped"], 2);
    let errors = error_file(&admin, id_of(&again)).await.assert_status(200);
    assert_eq!(
        csv_lines(errors.bytes()),
        [
            "Row,Email,Name,Errors",
            "2,ADA@example.com,Ada Byron,A record with this key already exists",
            "3,bob@example.com,Bob,A record with this key already exists",
            "4,bob@example.com,Bobby,Repeats row 3",
        ]
    );
}

#[tokio::test]
async fn rows_the_import_refuses_are_reported() {
    let app = support::app().await;
    let admin = app.acting_as(principal(ADMIN));
    let record = staged(
        &app,
        &admin,
        "c.csv",
        b"Email,Name\nr2@example.com,Robot\nx@example.com,Explode\nok@example.com,Fine\n",
        &[],
    )
    .await;
    let record = apply(&app, &admin, id_of(&record), "skip").await;
    assert_eq!(record["counts"]["created"], 1, "{record}");
    assert_eq!(record["counts"]["failed"], 2);
    let failed = admin
        .get(&format!("/api/imports/{}/rows?status=failed", id_of(&record)))
        .send()
        .await
        .json_value();
    assert_eq!(failed["data"][0]["errors"], json!({ "row": ["Robots cannot be contacts"] }));
    assert_eq!(failed["data"][1]["errors"], json!({ "row": ["This row could not be saved"] }));
    // A failed row rolled back alone.
    assert_eq!(stored_contacts(&app).await.len(), 1);
}

#[tokio::test]
async fn large_files_are_staged_and_applied_in_batches() {
    let app = support::builder()
        .config("sheets.imports.chunk_size", 400)
        .build()
        .await;
    let admin = app.acting_as(principal(ADMIN));
    let mut file = String::from("\u{FEFF}Email,Name,Age\n");
    for n in 0..2_000 {
        file.push_str(&format!("person{n}@example.com,Person {n},{}\n", n % 90));
    }
    let record = staged(&app, &admin, "big.csv", file.as_bytes(), &[]).await;
    assert_eq!(record["counts"]["valid"], 2_000, "{record}");
    assert_eq!(record["encoding"], "UTF-8");
    let id = id_of(&record);
    let applying = admin
        .post(&format!("/api/imports/{id}/apply"))
        .json(&json!({}))
        .send()
        .await
        .assert_status(202)
        .json_value();
    let op_id = Uuid::parse_str(applying["operation_id"].as_str().unwrap()).unwrap();
    run_queue(&app).await;
    let record = show(&admin, id).await;
    assert_eq!(record["status"], "completed", "{record}");
    assert_eq!(record["counts"]["created"], 2_000);
    assert_eq!(record["has_error_file"], false);
    let op = operation(&app, op_id).await;
    assert_eq!(op.status, OperationStatus::Succeeded);
    assert_eq!(op.progress.total, 5, "one job per 400 rows");
    assert_eq!(op.progress.done, 5);
    assert!(op.batch_id.is_some());
    assert_eq!(stored_contacts(&app).await.len(), 2_000);
}

#[tokio::test]
async fn uploads_are_refused_with_a_reason() {
    let app = support::builder()
        .config("sheets.imports.max_upload_bytes", 64)
        .build()
        .await;
    let admin = app.acting_as(principal(ADMIN));
    upload(&admin, "big.csv", &[b'a'; 65], &[])
        .await
        .assert_error_code(413, "payload_too_large");
    upload(&admin, "c.csv", b"%PDF-1.7 ...", &[])
        .await
        .assert_error_code(400, "unsupported_file");
    upload(&admin, "c.csv", b"  \n", &[])
        .await
        .assert_error_code(400, "unsupported_file");
    let (content_type, body) = support::multipart(&[("importer", "deals")], Some(("c.csv", b"a\n1\n")));
    admin
        .post("/api/imports")
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .assert_status(404);
    let (content_type, body) = support::multipart(&[], Some(("c.csv", b"a\n1\n")));
    admin
        .post("/api/imports")
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .assert_validation_error("importer");
    let (content_type, body) = support::multipart(&[("importer", "contacts")], None);
    admin
        .post("/api/imports")
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .assert_validation_error("file");
    upload(&app.acting_as(principal(OTHER)), "c.csv", b"a\n1\n", &[])
        .await
        .assert_status(403);
    upload(&app.client(), "c.csv", b"a\n1\n", &[]).await.assert_status(401);
}

#[tokio::test]
async fn files_that_cannot_be_read_fail_the_import() {
    let app = support::builder()
        .config("sheets.imports.max_rows", 2)
        .build()
        .await;
    let admin = app.acting_as(principal(ADMIN));
    let record = staged(&app, &admin, "c.csv", b"Email,Name\na@x.io,A\nb@x.io,B\nc@x.io,C\n", &[]).await;
    assert_eq!(record["status"], "failed");
    assert_eq!(record["error"], "The file has more than 2 rows.");
    let op = operation(&app, Uuid::parse_str(record["operation_id"].as_str().unwrap()).unwrap()).await;
    assert_eq!(op.status, OperationStatus::Failed);
    assert_eq!(op.error.as_deref(), Some("The file has more than 2 rows."));

    let record = staged(&app, &admin, "book.xlsx", b"PK\x03\x04 not really a workbook", &[]).await;
    assert_eq!(record["status"], "failed");
    assert!(
        record["error"].as_str().unwrap().starts_with("The workbook cannot be read"),
        "{record}"
    );
}

#[tokio::test]
async fn imports_are_listed_shown_to_permitted_users_and_deleted() {
    let app = support::app().await;
    let admin = app.acting_as(principal(ADMIN));
    let record = staged(&app, &admin, "c.csv", b"Email,Name\na@x.io,A\n", &[]).await;
    let id = id_of(&record);
    let list = admin.get("/api/imports").send().await.assert_status(200).json_value();
    assert_eq!(list["data"][0]["id"], id.to_string());
    let other = app.acting_as(principal(OTHER));
    other.get(&format!("/api/imports/{id}")).send().await.assert_status(404);
    other.get("/api/imports").send().await.assert_json_path("data", json!([]));
    admin.get("/api/imports/not-a-uuid").send().await.assert_status(404);

    let stored = import(&app, id).await;
    assert!(!stored.has_error_file);
    admin.delete(&format!("/api/imports/{id}")).send().await.assert_status(204);
    admin.get(&format!("/api/imports/{id}")).send().await.assert_status(404);
    let imports = app.get::<Imports>().await;
    assert!(matches!(imports.find(id).await, Err(SheetsError::NotFound)));
}

#[tokio::test]
async fn importers_and_templates_are_listed() {
    let app = support::app().await;
    let admin = app.acting_as(principal(ADMIN));
    let importers = admin
        .get("/api/imports/importers")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(importers["data"][0]["name"], "contacts");
    assert_eq!(
        importers["data"][0]["columns"][0],
        json!({ "field": "email", "header": "Email", "aliases": ["E-mail", "البريد الإلكتروني"], "required": true })
    );
    assert_eq!(importers["data"][0]["columns"][2]["required"], false);
    app.acting_as(principal(OTHER))
        .get("/api/imports/importers")
        .send()
        .await
        .assert_json_path("data", json!([]));

    let template = admin
        .get("/api/imports/templates/contacts")
        .send()
        .await
        .assert_status(200);
    assert!(template.header("content-disposition").unwrap().contains("contacts-template.csv"));
    assert_eq!(csv_lines(template.bytes()), ["Email,Name,Age,Vip"]);
    let xlsx = admin
        .get("/api/imports/templates/contacts?format=xlsx")
        .send()
        .await
        .assert_status(200);
    assert!(xlsx.bytes().starts_with(b"PK"));
    admin
        .get("/api/imports/templates/deals")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn cancelling_apply_puts_the_import_back_to_staged() {
    let app = support::app().await;
    let admin = app.acting_as(principal(ADMIN));
    let record = staged(&app, &admin, "c.csv", b"Email,Name\na@x.io,A\nb@x.io,B\n", &[]).await;
    let id = id_of(&record);
    let applying = admin
        .post(&format!("/api/imports/{id}/apply"))
        .json(&json!({}))
        .send()
        .await
        .assert_status(202)
        .json_value();
    let op = Uuid::parse_str(applying["operation_id"].as_str().unwrap()).unwrap();
    app.get::<Operations>().await.cancel(op).await.unwrap();
    run_queue(&app).await;
    let record = show(&admin, id).await;
    assert_eq!(record["status"], "staged", "{record}");
    assert_eq!(
        record["error"],
        "Applying was cancelled after 0 rows were saved; apply again to save the rest."
    );
    assert!(stored_contacts(&app).await.is_empty());

    let record = apply(&app, &admin, id, "skip").await;
    assert_eq!(record["status"], "completed");
    assert_eq!(record["counts"]["created"], 2);
}

#[tokio::test]
async fn imports_run_from_code() {
    let app = support::app().await;
    let imports = app.get::<Imports>().await;
    assert_eq!(imports.importers(), ["contacts"]);
    let db = app.get::<Db>().await;
    let uow = db.uow(Some(ADMIN), None).await;
    let record = imports
        .upload(
            &uow,
            "contacts",
            Upload::new("c.csv", "Mail,Who\nada@example.com,Ada\n"),
            UploadOptions::default()
                .owner(ADMIN)
                .mapping(Mapping::new().map("email", "Mail").map("name", "Who")),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();
    run_queue(&app).await;
    let staged = imports.find(record.id).await.unwrap();
    assert_eq!(staged.status, ImportStatus::Staged);
    let page = imports
        .rows(record.id, &RowQuery { status: Some(RowStatus::Valid), ..RowQuery::default() })
        .await
        .unwrap();
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.meta.last_page, 1);

    let uow = db.uow(Some(ADMIN), None).await;
    imports.apply(&uow, record.id, DuplicateMode::Skip).await.unwrap();
    uow.commit().await.unwrap();
    run_queue(&app).await;
    let done = imports.find(record.id).await.unwrap();
    assert_eq!(done.status, ImportStatus::Completed);
    assert_eq!(done.counts.created, 1);

    let uow = db.uow(None, None).await;
    let again = imports.apply(&uow, record.id, DuplicateMode::Skip).await.unwrap_err();
    assert!(matches!(again, SheetsError::NotReady(_)), "{again}");
    let missing = imports
        .upload(&uow, "deals", Upload::new("c.csv", "a\n1\n"), UploadOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(missing, SheetsError::UnknownImporter(_)));
}

#[tokio::test]
async fn prune_deletes_old_imports_with_their_files() {
    let clock = Arc::new(FixedClock::new(chrono::Utc::now()));
    let app = support::builder()
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    let admin = app.acting_as(principal(ADMIN));
    let record = staged(&app, &admin, "c.csv", b"Email,Name\nbad,A\n", &[]).await;
    let id = id_of(&record);
    let stored = import(&app, id).await;
    assert!(stored.has_error_file);
    let imports = app.get::<Imports>().await;
    assert_eq!(imports.prune().await.unwrap(), 0);
    clock.advance(Duration::from_secs(8 * 86_400));
    assert_eq!(imports.prune().await.unwrap(), 1);
    assert!(matches!(imports.find(id).await, Err(SheetsError::NotFound)));
    app.run_command("sheets:prune", &[]).await.unwrap();
}
