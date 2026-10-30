//! The examples of `content/docs/digging-deeper/batches-and-operations.mdx`,
//! written as an app writes them (the facade, the prelude and the macros),
//! compiled and run.
#![cfg(feature = "sqlite")]

use navree::contracts::{BoxError, Job, JobContext, JobOptions};
use navree::http::CurrentPrincipal;
use navree::prelude::*;
use navree::queue::{
    Batch, JobBatch, OperationRecord, OperationStatus, Queue, QueueDriver, QueueFake, QueueOptions,
};
use navree::testing::{TestApp, principal};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

/// Imports one chunk of a contacts file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportChunk {
    /// The import it belongs to.
    pub import_id: Uuid,
    /// The rows of this chunk.
    pub rows: Vec<String>,
}

impl Job for ImportChunk {
    const NAME: &'static str = "contacts.import_chunk";

    /// A bad row fails the same way twice: no retries.
    fn options() -> JobOptions {
        JobOptions {
            max_attempts: 1,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let total = self.rows.len() as u64;
        for (done, email) in self.rows.iter().enumerate() {
            if email.is_empty() {
                return Err("a row has no email".into());
            }
            // ... insert the contact
            ctx.progress(done as u64 + 1, total, format!("Imported {email}"))
                .await?;
        }
        Ok(())
    }
}

/// Runs once every chunk succeeded: records the result on the operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FinishImport {
    /// The import's operation.
    pub operation: Uuid,
    /// How many rows were imported.
    pub rows: u64,
}

impl Job for FinishImport {
    const NAME: &'static str = "contacts.import_finished";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let operations = ctx.app.get::<Operations>().await?;
        operations
            .complete(self.operation, json!({ "imported": self.rows }))
            .await?;
        Ok(())
    }
}

/// Runs as soon as one chunk fails.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReportImportFailure {
    /// The import that failed.
    pub import_id: Uuid,
}

impl Job for ReportImportFailure {
    const NAME: &'static str = "contacts.import_failed";

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        // ... tell whoever started the import
        Ok(())
    }
}

/// The body of `POST /api/v1/contacts/imports`.
#[derive(Debug, Deserialize)]
pub struct ImportInput {
    /// The contacts to import.
    pub emails: Vec<String>,
}

/// `POST /api/v1/contacts/imports`.
#[injectable]
pub struct ImportController {
    operations: Arc<Operations>,
}

#[controller("/contacts/imports", version = "1")]
impl ImportController {
    /// Starts the import and answers at once with its operation.
    #[post("/")]
    #[authenticated]
    async fn store(
        &self,
        uow: Uow,
        CurrentPrincipal(me): CurrentPrincipal,
        Json(input): Json<ImportInput>,
    ) -> Result<Json<OperationRecord>> {
        let import_id = Uuid::now_v7();
        let total = input.emails.len() as u64;
        let op = self
            .operations
            .start(
                &uow,
                Operation::new("contacts.import")
                    .owner(me.user_id)
                    .total(total)
                    .note("Waiting for a worker"),
            )
            .await?;
        let chunks = input.emails.chunks(2).map(|rows| ImportChunk {
            import_id,
            rows: rows.to_vec(),
        });
        let batch = Batch::new("contacts.import")
            .add(chunks)
            .then(FinishImport {
                operation: op.id,
                rows: total,
            })
            .catch(ReportImportFailure { import_id });
        self.operations.dispatch_batch(&uow, &op, batch).await?;
        Ok(Json(op))
    }
}

/// The contacts module: the import route and its jobs.
#[module(controllers = [ImportController], register = Self::wire)]
pub struct ContactsModule;

impl ContactsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.job::<ImportChunk>()
            .job::<FinishImport>()
            .job::<ReportImportFailure>();
    }
}

async fn app() -> TestApp {
    TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Database,
            ..QueueOptions::default()
        }))
        .module(ContactsModule)
        .build()
        .await
}

#[tokio::test]
async fn an_import_runs_as_a_batch_behind_an_operation() {
    let app = app().await;
    let me = Uuid::now_v7();
    let emails = [
        "ada@example.test",
        "grace@example.test",
        "alan@example.test",
    ];

    let op = app
        .acting_as(principal(me))
        .post("/api/v1/contacts/imports")
        .json(&json!({ "emails": emails }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("pending"))
        .assert_json_path("progress.note", json!("Waiting for a worker"))
        .json_value();
    let id = op["id"].as_str().unwrap().to_owned();

    let stats = app.get::<Queue>().await.run_pending().await.unwrap();
    assert_eq!(stats.failed, 0);

    let done = app
        .acting_as(principal(me))
        .get(&format!("/api/operations/{id}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(done["status"], "succeeded");
    assert_eq!(done["result"], json!({ "imported": 3 }));
    assert_eq!(done["progress"]["percent"], 100);

    // The SSE stream of a finished operation sends it once, then ends.
    let stream = app
        .acting_as(principal(me))
        .get(&format!("/api/operations/{id}/events"))
        .send()
        .await
        .assert_status(200);
    assert!(
        stream.text().contains("event: operation"),
        "{}",
        stream.text()
    );

    let batch_id = done["batch_id"].as_str().expect("the batch is recorded");
    let batch: JobBatch = app
        .get::<Queue>()
        .await
        .find_batch(batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((batch.total_jobs, batch.failed_jobs), (2, 0));
    assert!(batch.finished());

    // Someone else gets a 404.
    app.acting_as(principal(Uuid::now_v7()))
        .get(&format!("/api/operations/{id}"))
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn a_failing_chunk_fails_the_operation() {
    let app = app().await;
    let me = Uuid::now_v7();

    let op = app
        .acting_as(principal(me))
        .post("/api/v1/contacts/imports")
        .json(&json!({ "emails": ["ada@example.test", "", "alan@example.test"] }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    app.get::<Queue>().await.run_pending().await.unwrap();

    let id: Uuid = serde_json::from_value(op["id"].clone()).unwrap();
    let failed = app
        .get::<Operations>()
        .await
        .find(id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed.status, OperationStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("1 of 2 jobs failed."));
}

#[tokio::test]
async fn the_fake_records_the_batch() {
    let fake = QueueFake::new();
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(ContactsModule)
        .override_value(Arc::new(fake.queue()))
        .build()
        .await;
    let me = Uuid::now_v7();

    app.acting_as(principal(me))
        .post("/api/v1/contacts/imports")
        .json(&json!({ "emails": ["ada@example.test", "grace@example.test", "alan@example.test"] }))
        .send()
        .await
        .assert_status(200);

    fake.assert_batched(|batch| batch.name() == "contacts.import" && batch.jobs.len() == 2);
    let batch = &fake.batches()[0];
    assert_eq!(
        batch.jobs_of::<ImportChunk>()[1].rows,
        ["alan@example.test"]
    );
    assert_eq!(batch.then_of::<FinishImport>().unwrap().rows, 3);
    assert!(batch.catch_of::<ReportImportFailure>().is_some());
    fake.assert_nothing_dispatched();
}
