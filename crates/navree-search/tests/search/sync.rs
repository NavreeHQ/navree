//! Keeping indexes in sync: the change observer, `search.sync` modes, the
//! sync job, soft deletes, `reindex` and `without_syncing`.

use std::sync::Arc;

use navree_contracts::{Dispatcher, JobEnvelope};
use navree_db::{DatabaseModule, Db, Repo};
use navree_kernel::{BoxError, BoxFuture};
use navree_queue::{Queue, QueueFake};
use navree_search::{Search, SearchModule, SyncDocuments};
use navree_testing::{TestApp, TestAppBuilder};
use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter};
use uuid::Uuid;

use crate::support::{
    self, Company, Contact, Deal, company, deal, insert_contacts, new_contact, settle,
};

async fn reload(db: &Db, id: Uuid) -> crate::support::contact::Model {
    Contact::find_by_id(id).one(db.conn()).await.unwrap().unwrap()
}

async fn found(search: &Search, text: &str) -> Vec<String> {
    search
        .query::<Contact>(text)
        .ids(10)
        .await
        .expect("the query runs")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inline_sync_indexes_committed_writes() {
    let app = support::app("memory", "inline").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let [ada] = insert_contacts(&db, vec![new_contact("Ada", "Lovelace")])
        .await
        .try_into()
        .unwrap();
    settle(&app).await;
    search.assert_indexed::<Contact>(ada.id).await;
    assert_eq!(found(&search, "lovelace").await, [ada.id.to_string()]);

    // An update replaces the document.
    let mut row = ada.clone().into_active_model();
    row.last_name = Set("Byron".into());
    support::in_uow(&db, async |uow| Repo::<Contact>::update(uow, row).await.unwrap()).await;
    settle(&app).await;
    assert!(found(&search, "lovelace").await.is_empty());
    assert_eq!(found(&search, "byron").await, [ada.id.to_string()]);

    // A soft delete removes it.
    let current = reload(&db, ada.id).await;
    let mut row = current.into_active_model();
    row.deleted_at = Set(Some(support::now()));
    support::in_uow(&db, async |uow| Repo::<Contact>::update(uow, row).await.unwrap()).await;
    settle(&app).await;
    search.assert_not_indexed::<Contact>(ada.id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deletes_and_rollbacks() {
    let app = support::app("memory", "inline").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let [ada, grace] = insert_contacts(
        &db,
        vec![new_contact("Ada", "Lovelace"), new_contact("Grace", "Hopper")],
    )
    .await
    .try_into()
    .unwrap();
    settle(&app).await;

    support::in_uow(&db, async |uow| Repo::<Contact>::delete(uow, ada.clone()).await.unwrap()).await;
    settle(&app).await;
    search.assert_not_indexed::<Contact>(ada.id).await;

    let uow = db.uow(None, None).await;
    let mut row = grace.clone().into_active_model();
    row.last_name = Set("Rolled back".into());
    Repo::<Contact>::update(&uow, row).await.unwrap();
    uow.rollback().await.unwrap();
    settle(&app).await;
    assert_eq!(found(&search, "hopper").await, [grace.id.to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_sync_records_a_search_sync_job_in_the_commit() {
    let fake = QueueFake::new();
    let app = support::builder("memory", "queued")
        .config("search.queue", "search")
        .override_value(Arc::new(fake.queue()))
        .build()
        .await;
    let db = app.get::<Db>().await;
    let [ada] = insert_contacts(&db, vec![new_contact("Ada", "Lovelace")])
        .await
        .try_into()
        .unwrap();
    fake.assert_dispatched::<SyncDocuments>(|job| {
        job.index == "contacts" && job.ids == [ada.id.to_string()] && job.workspace.is_none()
    });
    let envelope = &fake.envelopes()[0];
    assert_eq!(envelope.job_type, "search.sync");
    assert_eq!(envelope.options.queue, "search");
    assert_eq!(envelope.options.unique_for_secs, Some(5));

    // A rolled-back unit of work queues nothing.
    fake.clear();
    let uow = db.uow(None, None).await;
    Repo::<Contact>::insert(&uow, new_contact("Grace", "Hopper"))
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    fake.assert_nothing_dispatched();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_sync_jobs_run_on_the_queue_and_collapse() {
    let app = support::app("memory", "queued").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let queue = app.get::<Queue>().await;
    let [ada] = insert_contacts(&db, vec![new_contact("Ada", "Lovelace")])
        .await
        .try_into()
        .unwrap();
    search.assert_not_indexed::<Contact>(ada.id).await;
    assert_eq!(queue.pending_count("default").await.unwrap(), 1);

    // Two more writes to the same record collapse into the pending job.
    for name in ["Byron", "King"] {
        let current = reload(&db, ada.id).await;
        let mut row = current.into_active_model();
        row.last_name = Set(name.into());
        support::in_uow(&db, async |uow| Repo::<Contact>::update(uow, row).await.unwrap()).await;
    }
    assert_eq!(queue.pending_count("default").await.unwrap(), 1);

    queue.run_pending().await.unwrap();
    assert_eq!(found(&search, "king").await, [ada.id.to_string()]);
    assert!(found(&search, "lovelace").await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_off_leaves_indexes_to_reindex_and_import() {
    let app = support::app("memory", "off").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let [ada, grace] = insert_contacts(
        &db,
        vec![new_contact("Ada", "Lovelace"), new_contact("Grace", "Hopper")],
    )
    .await
    .try_into()
    .unwrap();
    search.assert_not_indexed::<Contact>(ada.id).await;
    search.reindex::<Contact>([ada.id]).await.unwrap();
    search.assert_indexed::<Contact>(ada.id).await;
    search.assert_not_indexed::<Contact>(grace.id).await;
    let report = search.import::<Contact>().await.unwrap();
    assert_eq!((report.rows, report.documents, report.skipped), (2, 2, 0));
    search.assert_indexed::<Contact>(grace.id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_syncing_pauses_the_observer() {
    let app = support::app("memory", "inline").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let rows = search
        .without_syncing(insert_contacts(&db, vec![new_contact("Ada", "Lovelace")]))
        .await;
    settle(&app).await;
    search.assert_not_indexed::<Contact>(rows[0].id).await;
    search.import::<Contact>().await.unwrap();
    search.assert_indexed::<Contact>(rows[0].id).await;

    // Outside the block syncing is back.
    let [grace] = insert_contacts(&db, vec![new_contact("Grace", "Hopper")])
        .await
        .try_into()
        .unwrap();
    settle(&app).await;
    search.assert_indexed::<Contact>(grace.id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn raw_writes_need_a_reindex() {
    let app = support::app("memory", "inline").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let [ada] = insert_contacts(&db, vec![new_contact("Ada", "Lovelace")])
        .await
        .try_into()
        .unwrap();
    settle(&app).await;
    // A bulk update bypasses `Repo`.
    Contact::update_many()
        .col_expr(
            crate::support::contact::Column::LastName,
            sea_orm::sea_query::Expr::value("Byron"),
        )
        .filter(crate::support::contact::Column::Id.eq(ada.id))
        .exec(db.conn())
        .await
        .unwrap();
    settle(&app).await;
    assert!(found(&search, "byron").await.is_empty(), "raw SQL is not observed");
    search.reindex::<Contact>([ada.id]).await.unwrap();
    settle(&app).await;
    assert_eq!(found(&search, "byron").await, [ada.id.to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hand_written_documents_read_related_rows_and_can_opt_out() {
    let app = support::app("memory", "inline").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let workspace = Uuid::now_v7();
    let acme = support::in_uow(&db, async |uow| {
        Repo::<Company>::insert(
            uow,
            company::ActiveModel {
                id: Set(Uuid::now_v7()),
                workspace_id: Set(workspace),
                name: Set("Acme Rockets".into()),
                domain: Set(None),
            },
        )
        .await
        .unwrap()
    })
    .await;
    let renewal = support::in_uow(&db, async |uow| {
        Repo::<Deal>::insert(
            uow,
            deal::ActiveModel {
                id: Set(Uuid::now_v7()),
                title: Set("Renewal".into()),
                company_id: Set(Some(acme.id)),
                stage: Set("negotiation".into()),
                amount_cents: Set(1_000_000),
                closes_on: Set(None),
                archived: Set(false),
            },
        )
        .await
        .unwrap()
    })
    .await;
    settle(&app).await;
    let ids = search.query::<Deal>("rockets").ids(10).await.unwrap();
    assert_eq!(ids, [renewal.id.to_string()], "found by its company's name");

    let mut row = renewal.clone().into_active_model();
    row.archived = Set(true);
    support::in_uow(&db, async |uow| Repo::<Deal>::update(uow, row).await.unwrap()).await;
    settle(&app).await;
    search.assert_not_indexed::<Deal>(renewal.id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_scoped_changes_carry_their_workspace() {
    let fake = QueueFake::new();
    let app = support::builder("memory", "queued")
        .override_value(Arc::new(fake.queue()))
        .build()
        .await;
    let db = app.get::<Db>().await;
    let (w1, w2) = (Uuid::now_v7(), Uuid::now_v7());
    support::in_uow(&db, async |uow| {
        for (workspace, name) in [(w1, "Acme"), (w2, "Globex"), (w1, "Initech")] {
            Repo::<Company>::insert(
                uow,
                company::ActiveModel {
                    id: Set(Uuid::now_v7()),
                    workspace_id: Set(workspace),
                    name: Set(name.into()),
                    domain: Set(None),
                },
            )
            .await
            .unwrap();
        }
    })
    .await;
    let jobs = fake.dispatched::<SyncDocuments>();
    assert_eq!(jobs.len(), 2, "one job per workspace: {jobs:?}");
    let first = jobs
        .iter()
        .find(|j| j.workspace == Some(w1.to_string()))
        .unwrap();
    assert_eq!(first.ids.len(), 2);
    assert_eq!(first.index, "companies");
}

/// A `dyn Dispatcher` that records what it is given.
#[derive(Default)]
struct RecordingDispatcher(std::sync::Mutex<Vec<JobEnvelope>>);

impl RecordingDispatcher {
    fn envelopes(&self) -> Vec<JobEnvelope> {
        self.0.lock().unwrap().clone()
    }
}

impl Dispatcher for RecordingDispatcher {
    fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(env);
            Ok("1".to_owned())
        })
    }
}

/// An app with `sync = "queued"` and no `QueueModule`.
fn without_a_queue() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("search.engine", "memory")
        .config("search.sync", "queued")
        .module(DatabaseModule::from_config())
        .module(SearchModule::from_config())
        .module(support::CrmModule)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_sync_without_a_queue_goes_to_the_dispatcher() {
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let app = without_a_queue()
        .override_value(dispatcher.clone() as Arc<dyn Dispatcher>)
        .build()
        .await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let [ada] = insert_contacts(&db, vec![new_contact("Ada", "Lovelace")])
        .await
        .try_into()
        .unwrap();
    let envelopes = dispatcher.envelopes();
    assert_eq!(envelopes.len(), 1, "dispatched after the commit");
    assert_eq!(envelopes[0].job_type, "search.sync");
    let job: SyncDocuments = serde_json::from_value(envelopes[0].payload.clone()).unwrap();
    assert_eq!(job.ids, [ada.id.to_string()]);
    search.assert_not_indexed::<Contact>(ada.id).await;

    // A rolled-back unit of work dispatches nothing.
    let uow = db.uow(None, None).await;
    Repo::<Contact>::insert(&uow, new_contact("Grace", "Hopper"))
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(dispatcher.envelopes().len(), 1);

    // `reindex` goes the same way.
    search.reindex::<Contact>([ada.id]).await.unwrap();
    assert_eq!(dispatcher.envelopes().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_sync_without_a_queue_or_dispatcher_syncs_inline() {
    let app = without_a_queue().build().await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let [ada] = insert_contacts(&db, vec![new_contact("Ada", "Lovelace")])
        .await
        .try_into()
        .unwrap();
    settle(&app).await;
    search.assert_indexed::<Contact>(ada.id).await;
}
