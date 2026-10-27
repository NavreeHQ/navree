//! Records in queued jobs: identities that serialize into a job and load
//! the records again when it runs.

use chrono::{DateTime, Utc};
use navree_orm::attributes::{ModelRef, ModelRefs};
use navree_orm::{Model, OrmError, Record};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::support::sqlite;

const SCHEMA: &str = r#"
CREATE TABLE invoices (
    id BLOB PRIMARY KEY,
    number TEXT NOT NULL,
    deleted_at TEXT NULL
);
CREATE TABLE lines (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item TEXT NOT NULL
)
"#;

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "invoices", soft_deletes)]
pub struct Invoice {
    pub id: Uuid,
    pub number: String,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "lines")]
pub struct Line {
    pub id: i64,
    pub item: String,
}

/// A job as the queue stores it.
#[derive(Serialize, Deserialize)]
struct SendInvoice {
    invoice: ModelRef<Invoice>,
    lines: ModelRefs<Line>,
}

#[tokio::test]
async fn identities_round_trip_through_a_job_and_restore_records() {
    let t = sqlite(SCHEMA).await;
    let invoice = Invoice::create(&t.db, Invoice { number: "INV-1".into(), ..Invoice::default() })
        .await
        .unwrap();
    let mut lines = Vec::new();
    for item in ["a", "b", "c"] {
        lines.push(Line::create(&t.db, Line { item: item.into(), ..Line::default() }).await.unwrap());
    }
    let mut invoice_with_relation = invoice.clone();
    invoice_with_relation.relations_mut().set("lines", Vec::<String>::new());
    let reversed: navree_orm::Collection<Line> = lines.iter().rev().cloned().collect();
    let job = SendInvoice {
        invoice: invoice_with_relation.to_ref().unwrap(),
        lines: reversed.to_refs().unwrap(),
    };
    let payload = serde_json::to_value(&job).unwrap();
    assert_eq!(
        payload,
        json!({
            "invoice": {"model": "Invoice", "key": invoice.id.to_string(), "relations": ["lines"]},
            "lines": {"model": "Line", "keys": [3, 2, 1]},
        })
    );

    let job: SendInvoice = serde_json::from_value(payload).unwrap();
    assert_eq!(job.invoice.key().unwrap(), invoice.id);
    assert_eq!(job.invoice.relations(), ["lines"]);
    assert_eq!(job.invoice.connection(), None);
    let restored = job.invoice.restore(&t.db).await.unwrap();
    assert_eq!(*restored, *invoice);

    Line::destroy(&t.db, [2_i64]).await.unwrap();
    let restored = job.lines.restore(&t.db).await.unwrap();
    assert_eq!(restored.map(|line| line.item.clone()), ["c", "a"], "in order, the deleted one left out");
    assert_eq!(job.lines.keys().unwrap(), [3, 2, 1]);
    assert_eq!((job.lines.len(), job.lines.is_empty()), (3, false));
}

#[tokio::test]
async fn missing_records_fail_clearly_or_are_skipped_on_request() {
    let t = sqlite(SCHEMA).await;
    let mut invoice = Invoice::create(&t.db, Invoice { number: "INV-2".into(), ..Invoice::default() })
        .await
        .unwrap();
    let reference = invoice.to_ref().unwrap();
    invoice.delete(&t.db).await.unwrap();
    let trashed = reference.restore(&t.db).await.unwrap();
    assert!(trashed.trashed(), "soft-deleted records restore too");

    navree_orm::table("invoices").delete(&t.db).await.unwrap();
    let error = reference.restore(&t.db).await.unwrap_err();
    assert!(error.is_not_found(), "{error:?}");
    assert_eq!(error.to_string(), format!("no Invoice found for key {}", invoice.id));
    assert!(reference.restore_or_skip(&t.db).await.unwrap().is_none());

    let error = Record::new(Line::default()).to_ref().unwrap_err();
    assert_eq!(error.to_string(), "cannot refer to a Line that has not been saved");

    let wrong: ModelRef<Line> = serde_json::from_value(json!({"model": "Invoice", "key": 1})).unwrap();
    let error = wrong.restore(&t.db).await.unwrap_err();
    assert_eq!(error.to_string(), "invalid query: this reference names model Invoice, not Line");
    let malformed: ModelRef<Invoice> = serde_json::from_value(json!({"model": "Invoice", "key": "nope"})).unwrap();
    assert!(matches!(malformed.key().unwrap_err(), OrmError::Decode { .. }));
    let none: ModelRefs<Line> = serde_json::from_value(json!({"model": "Line", "keys": []})).unwrap();
    assert!(none.restore(&t.db).await.unwrap().is_empty());
}
