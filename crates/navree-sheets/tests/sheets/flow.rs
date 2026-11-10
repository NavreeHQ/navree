//! The import and export flow end to end, so the staging SQL (multi-row
//! inserts, `IN` lists, `LIMIT ? OFFSET ?`, `GROUP BY`, `IS NULL` lookups of
//! saved mappings) runs on whichever database the suite runs on (see
//! `servers`).

use encoding_rs::WINDOWS_1256;
use serde_json::json;

use crate::support::{
    self, ADMIN, contact, csv_lines, id_of, principal, run_queue, seed, stored_contacts, upload,
};

/// Upload, stage, preview, map, apply with updates, the error file, and an
/// export.
#[tokio::test]
async fn uploads_stages_applies_and_exports() {
    let app = support::builder()
        .config("sheets.imports.chunk_size", 2)
        .build()
        .await;
    seed(&app, &[("anu@example.com", "Anu", Some(30), false)]).await;
    let admin = app.acting_as(principal(ADMIN));

    let text = "البريد الإلكتروني,الاسم,Age\n\
        ANU@example.com,آدا لوفليس,36\n\
        mohammed@example.com,محمد عبد الله,40\n\
        not-an-email,فاطمة الزهراء,31\n\
        mohammed@example.com,محمد,41\n\
        noura@example.com,نورة القحطاني,28\n";
    let (bytes, _, _) = WINDOWS_1256.encode(text);
    let uploaded = upload(&admin, "عملاء.csv", &bytes, &[])
        .await
        .assert_status(202)
        .json_value();
    let id = id_of(&uploaded);
    run_queue(&app).await;
    let record = admin
        .get(&format!("/api/imports/{id}"))
        .send()
        .await
        .json_value();
    assert_eq!(record["status"], "staged", "{record}");
    assert_eq!(record["encoding"], "windows-1256");
    assert_eq!(record["counts"]["valid"], 2);
    assert_eq!(record["counts"]["duplicate"], 1);
    assert_eq!(record["counts"]["invalid"], 1);
    assert_eq!(record["counts"]["repeated"], 1);
    let page = admin
        .get(&format!("/api/imports/{id}/rows?per_page=2&page=2"))
        .send()
        .await
        .json_value();
    assert_eq!(
        page["meta"],
        json!({ "current_page": 2, "per_page": 2, "total": 5, "last_page": 3 })
    );
    assert_eq!(page["data"][0]["row"], 4);

    admin
        .post(&format!("/api/imports/{id}/apply"))
        .json(&json!({ "duplicates": "update" }))
        .send()
        .await
        .assert_status(202);
    run_queue(&app).await;
    let record = admin
        .get(&format!("/api/imports/{id}"))
        .send()
        .await
        .json_value();
    assert_eq!(record["status"], "completed", "{record}");
    assert_eq!(record["counts"]["created"], 2);
    assert_eq!(record["counts"]["updated"], 1);
    let contacts = stored_contacts(&app).await;
    assert_eq!(
        contacts[0],
        contact("anu@example.com", "آدا لوفليس", Some(36), false)
    );
    assert_eq!(contacts.len(), 3);
    let errors = admin
        .get(&format!("/api/imports/{id}/errors"))
        .send()
        .await
        .assert_status(200);
    let lines = csv_lines(errors.bytes());
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[2].ends_with("Repeats row 3"), "{lines:?}");

    // A chosen, saved mapping is found again for the next file.
    let uploaded = upload(&admin, "x.csv", b"Mail,Who\nzed@example.com,Zed\n", &[])
        .await
        .json_value();
    run_queue(&app).await;
    let next = id_of(&uploaded);
    admin
        .put(&format!("/api/imports/{next}/mapping"))
        .json(&json!({ "mapping": { "email": "Mail", "name": "Who" }, "save": true }))
        .send()
        .await
        .assert_status(202);
    run_queue(&app).await;
    let again = upload(&admin, "y.csv", b"WHO,mail\nyan@example.com,Yan\n", &[])
        .await
        .json_value();
    run_queue(&app).await;
    let record = admin
        .get(&format!("/api/imports/{}", id_of(&again)))
        .send()
        .await
        .json_value();
    assert_eq!(record["status"], "staged", "{record}");

    // An export pages through the server.
    let export = admin
        .get("/api/exports/contacts?vip=false")
        .send()
        .await
        .assert_status(200);
    assert_eq!(csv_lines(export.bytes()).len(), 4);
}
