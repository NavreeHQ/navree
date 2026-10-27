//! Operations: user-visible long work, with either queue driver (the
//! operations themselves are always in the database).

use std::sync::Arc;
use std::time::Duration;

use navree_contracts::{AuthVia, Principal};
use navree_db::Db;
use navree_queue::{
    Batch, FAILED_MESSAGE, OPERATION_UPDATED_EVENT, Operation, OperationStatus, QueueDriver,
    STALLED_MESSAGE, SweepReport,
};
use sea_orm::ConnectionTrait;
use serde_json::json;
use uuid::Uuid;

use crate::support::{
    CompleteOperation, Cooperative, Export, Grant, GrantsModule, Harness, Record, Toggle,
    harness_using, harness_with, one_worker,
};

async fn started(h: &Harness, operation: Operation) -> navree_queue::OperationRecord {
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let record = h.operations.start(&uow, operation).await.unwrap();
    uow.commit().await.unwrap();
    record
}

fn principal(user_id: Uuid) -> Principal {
    Principal {
        user_id,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    }
}

pub async fn an_operation_follows_its_job(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    let db = h.app.get::<Db>().await;
    let owner = Uuid::now_v7();
    let workspace = Uuid::now_v7();

    let uow = db.uow(None, None).await;
    let op = h
        .operations
        .start(
            &uow,
            Operation::new("deals.export")
                .owner(owner)
                .workspace(workspace)
                .note("Waiting for a worker"),
        )
        .await
        .unwrap();
    h.operations
        .dispatch(
            &uow,
            &op,
            Export {
                operation: op.id,
                rows: 4,
            },
        )
        .await
        .unwrap();
    assert_eq!(h.operations.find(op.id).await.unwrap(), None);
    uow.commit().await.unwrap();

    let pending = h.operations.find(op.id).await.unwrap().unwrap();
    assert_eq!(pending.status, OperationStatus::Pending);
    assert_eq!(pending.kind, "deals.export");
    assert_eq!(pending.owner_id, Some(owner));
    assert_eq!(pending.workspace_id, Some(workspace));
    assert_eq!(pending.progress.note, "Waiting for a worker");
    assert_eq!(pending.channel(), format!("private-operations.{}", op.id));

    h.drain().await;

    let done = h.operations.find(op.id).await.unwrap().unwrap();
    assert_eq!(done.status, OperationStatus::Succeeded);
    assert_eq!(
        (done.progress.done, done.progress.total, done.progress.percent),
        (4, 4, 100)
    );
    assert_eq!(done.progress.note, "Exported 4 of 4");
    assert_eq!(done.result, Some(json!({ "rows": 4 })));
    assert!(done.started_at.is_some() && done.finished_at.is_some());
    assert!(done.heartbeat_at.is_some());

    // Every change went out on the operation's channel.
    let broadcasts = h.broadcasts.all();
    assert!(broadcasts.len() >= 3, "{broadcasts:?}");
    for (channels, event, _) in &broadcasts {
        assert_eq!(channels, &[done.channel()]);
        assert_eq!(event, OPERATION_UPDATED_EVENT);
    }
    let statuses: Vec<String> = broadcasts
        .iter()
        .map(|(_, _, data)| data["status"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(statuses.first().map(String::as_str), Some("running"));
    assert_eq!(statuses.last().map(String::as_str), Some("succeeded"));
    let last = &broadcasts.last().unwrap().2;
    assert_eq!(last["result"], json!({ "rows": 4 }));
    assert_eq!(last["progress"]["percent"], 100);
    assert_eq!(last["owner_id"], json!(owner));
}

pub async fn the_jobs_outcome_finishes_the_operation(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    let db = h.app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    let quiet = h
        .operations
        .start(&uow, Operation::new("quiet"))
        .await
        .unwrap();
    h.operations
        .dispatch(&uow, &quiet, Record::new("quiet"))
        .await
        .unwrap();
    let broken = h
        .operations
        .start(&uow, Operation::new("broken"))
        .await
        .unwrap();
    h.operations
        .dispatch(&uow, &broken, Toggle { label: "x".into() })
        .await
        .unwrap();
    uow.commit().await.unwrap();
    h.journal.set_failing(true);

    h.drain().await;

    let quiet = h.operations.find(quiet.id).await.unwrap().unwrap();
    assert_eq!(quiet.status, OperationStatus::Succeeded);
    assert_eq!(quiet.result, None);
    let broken = h.operations.find(broken.id).await.unwrap().unwrap();
    assert_eq!(broken.status, OperationStatus::Failed);
    assert_eq!(broken.error.as_deref(), Some(FAILED_MESSAGE));
    assert_eq!(h.queue.failed().await.unwrap().len(), 1);
}

pub async fn a_finished_operation_never_changes(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    let op = started(&h, Operation::new("export").total(10)).await;

    let running = h
        .operations
        .progress(op.id, 3, 10, "Three rows")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(running.status, OperationStatus::Running);
    assert_eq!(running.progress.percent, 30);
    let done = h
        .operations
        .complete(op.id, json!({"url": "/exports/1.csv"}))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(done.status, OperationStatus::Succeeded);
    assert_eq!(done.progress.done, 10, "a success fills the progress");

    for after in [
        h.operations.fail(op.id, "late").await.unwrap(),
        h.operations.cancel(op.id).await.unwrap(),
        h.operations
            .progress(op.id, 1, 10, "late")
            .await
            .unwrap(),
    ] {
        assert_eq!(after, Some(done.clone()));
    }
    assert_eq!(h.operations.find(Uuid::now_v7()).await.unwrap(), None);
}

pub async fn cancel_skips_work_not_started(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let op = h
        .operations
        .start(&uow, Operation::new("export"))
        .await
        .unwrap();
    h.operations
        .dispatch(&uow, &op, Record::new("never"))
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let cancelled = h.operations.cancel(op.id).await.unwrap().unwrap();
    assert_eq!(cancelled.status, OperationStatus::Cancelled);
    assert!(cancelled.finished_at.is_some());

    let stats = h.drain().await;
    assert_eq!((stats.cancelled, stats.succeeded), (1, 0));
    assert!(h.journal.runs().is_empty());
    assert!(h.queue.failed().await.unwrap().is_empty());
}

pub async fn cancel_stops_a_running_job_at_its_next_report(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let op = h
        .operations
        .start(&uow, Operation::new("long"))
        .await
        .unwrap();
    h.operations
        .dispatch(&uow, &op, Cooperative { label: "c".into() })
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let queue = h.queue.clone();
    let worker = tokio::spawn(async move { queue.run_pending().await });
    tokio::time::timeout(Duration::from_secs(10), h.journal.started.notified())
        .await
        .expect("the job starts");
    h.operations.cancel(op.id).await.unwrap();
    let stats = tokio::time::timeout(Duration::from_secs(10), worker)
        .await
        .expect("the job stops")
        .unwrap()
        .unwrap();

    assert_eq!((stats.cancelled, stats.failed, stats.released), (1, 0, 0));
    assert!(h.queue.failed().await.unwrap().is_empty());
    let op = h.operations.find(op.id).await.unwrap().unwrap();
    assert_eq!(op.status, OperationStatus::Cancelled);
    assert!(op.progress.done > 0, "it reported progress before");
}

pub async fn a_batch_drives_its_operation(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let op = h
        .operations
        .start(&uow, Operation::new("recipients.import"))
        .await
        .unwrap();
    let batch = h
        .operations
        .dispatch_batch(
            &uow,
            &op,
            Batch::new("recipients.import")
                .add(["a", "b", "c"].map(Record::new))
                .then(CompleteOperation {
                    operation: op.id,
                    result: json!({"imported": 3}),
                }),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(batch.operation_id, Some(op.id));
    let pending = h.operations.find(op.id).await.unwrap().unwrap();
    assert_eq!(pending.batch_id.as_deref(), Some(batch.id.as_str()));
    assert_eq!(pending.progress.total, 3);

    h.drain().await;

    let done = h.operations.find(op.id).await.unwrap().unwrap();
    assert_eq!(done.status, OperationStatus::Succeeded);
    assert_eq!(done.result, Some(json!({"imported": 3})));
    assert_eq!((done.progress.done, done.progress.total), (3, 3));
    // Progress moved with the batch: 1/3, 2/3, 3/3, then the result.
    let percents: Vec<u64> = h
        .broadcasts
        .all()
        .iter()
        .map(|(_, _, data)| data["progress"]["percent"].as_u64().unwrap())
        .collect();
    assert!(percents.contains(&33) && percents.contains(&66), "{percents:?}");
}

pub async fn a_failing_batch_fails_its_operation(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    h.journal.set_failing(true);
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let op = h
        .operations
        .start(&uow, Operation::new("import"))
        .await
        .unwrap();
    h.operations
        .dispatch_batch(
            &uow,
            &op,
            Batch::new("import")
                .push(Toggle { label: "x".into() })
                .push(Record::new("a"))
                .finally(Record::new("finally")),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();

    h.drain().await;

    let done = h.operations.find(op.id).await.unwrap().unwrap();
    assert_eq!(done.status, OperationStatus::Failed);
    assert_eq!(done.error.as_deref(), Some("1 of 2 jobs failed."));
    assert_eq!(h.journal.labels(), ["finally"]);
}

pub async fn cancelling_a_batch_operation_cancels_the_batch(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let op = h
        .operations
        .start(&uow, Operation::new("import"))
        .await
        .unwrap();
    let batch = h
        .operations
        .dispatch_batch(
            &uow,
            &op,
            Batch::new("import")
                .add(["a", "b"].map(Record::new))
                .finally(Record::new("finally")),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();

    h.operations.cancel(op.id).await.unwrap();
    assert!(
        h.queue
            .find_batch(&batch.id)
            .await
            .unwrap()
            .unwrap()
            .cancelled()
    );
    h.drain().await;

    assert_eq!(h.journal.labels(), ["finally"]);
    let done = h.operations.find(op.id).await.unwrap().unwrap();
    assert_eq!(done.status, OperationStatus::Cancelled);
}

pub async fn the_watchdog_fails_stalled_operations(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    let db = h.app.get::<Db>().await;

    let stalled = started(&h, Operation::new("stalled")).await;
    let uow = db.uow(None, None).await;
    let queued = h
        .operations
        .start(&uow, Operation::new("queued"))
        .await
        .unwrap();
    h.operations
        .dispatch(&uow, &queued, Record::new("queued"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let fresh_at_start = started(&h, Operation::new("fresh")).await;

    // Nothing is stale yet.
    assert_eq!(h.operations.sweep().await.unwrap(), SweepReport::default());

    h.advance(200);
    h.operations
        .heartbeat(fresh_at_start.id)
        .await
        .unwrap();
    h.advance(150);
    let report = h.operations.sweep().await.unwrap();

    assert_eq!(
        report,
        SweepReport {
            failed: 1,
            ..SweepReport::default()
        }
    );
    let stalled = h.operations.find(stalled.id).await.unwrap().unwrap();
    assert_eq!(stalled.status, OperationStatus::Failed);
    assert_eq!(stalled.error.as_deref(), Some(STALLED_MESSAGE));
    // Its job is still in the queue: the queue owns the work.
    let queued = h.operations.find(queued.id).await.unwrap().unwrap();
    assert_eq!(queued.status, OperationStatus::Pending);
    // Its heartbeat is recent.
    let fresh = h.operations.find(fresh_at_start.id).await.unwrap().unwrap();
    assert_eq!(fresh.status, OperationStatus::Pending);

    // The command runs the same sweep.
    h.advance(400);
    h.app.run_command("queue:watchdog", &[]).await.unwrap();
    let fresh = h.operations.find(fresh_at_start.id).await.unwrap().unwrap();
    assert_eq!(fresh.status, OperationStatus::Failed);
}

pub async fn access_is_the_owner_or_the_permission(driver: QueueDriver) {
    let owner = Uuid::now_v7();
    let viewer = Uuid::now_v7();
    let member = Uuid::now_v7();
    let stranger = Uuid::now_v7();
    let acme = Uuid::now_v7();
    let grants = vec![
        Arc::new(Grant {
            user: viewer,
            permission: "operations.view",
            workspace: None,
        }),
        Arc::new(Grant {
            user: member,
            permission: "operations.cancel",
            workspace: Some(acme),
        }),
    ];
    let h = harness_using(one_worker(driver), |builder| {
        builder.module(GrantsModule(grants))
    })
    .await;
    let op = started(&h, Operation::new("export").owner(owner).workspace(acme)).await;

    let view = |user| {
        let ops = h.operations.clone();
        let op = op.clone();
        async move { ops.can_view(&principal(user), &op).await.unwrap() }
    };
    let cancel = |user| {
        let ops = h.operations.clone();
        let op = op.clone();
        async move { ops.can_cancel(&principal(user), &op).await.unwrap() }
    };
    assert!(view(owner).await && cancel(owner).await);
    assert!(view(viewer).await && !cancel(viewer).await);
    assert!(!view(member).await && cancel(member).await);
    assert!(!view(stranger).await && !cancel(stranger).await);

    // In another workspace the member's grant does not apply.
    let elsewhere = started(
        &h,
        Operation::new("export")
            .owner(owner)
            .workspace(Uuid::now_v7()),
    )
    .await;
    assert!(
        !h.operations
            .can_cancel(&principal(member), &elsewhere)
            .await
            .unwrap()
    );
}

pub async fn owners_list_and_prune_operations(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    let owner = Uuid::now_v7();
    let first = started(&h, Operation::new("first").owner(owner)).await;
    h.advance(1);
    let second = started(&h, Operation::new("second").owner(owner)).await;
    started(&h, Operation::new("someone else's").owner(Uuid::now_v7())).await;

    let mine: Vec<Uuid> = h
        .operations
        .for_owner(owner, 10)
        .await
        .unwrap()
        .into_iter()
        .map(|op| op.id)
        .collect();
    assert_eq!(mine, [second.id, first.id]);
    assert_eq!(h.operations.for_owner(owner, 1).await.unwrap().len(), 1);

    h.operations.complete(first.id, json!(null)).await.unwrap();
    h.advance(2 * 3600);
    assert_eq!(
        h.operations
            .prune(Duration::from_secs(3600))
            .await
            .unwrap(),
        1
    );
    assert_eq!(h.operations.find(first.id).await.unwrap(), None);
    assert!(h.operations.find(second.id).await.unwrap().is_some());
}

/// Database driver: an idempotent operation whose job vanished is
/// restarted by the watchdog, up to `max_redispatches` times.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_watchdog_restarts_idempotent_operations() {
    let mut options = one_worker(QueueDriver::Database);
    options.operations.max_redispatches = 1;
    let h = harness_with(options).await;
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let op = h
        .operations
        .start(&uow, Operation::new("export").idempotent())
        .await
        .unwrap();
    h.operations
        .dispatch(
            &uow,
            &op,
            Export {
                operation: op.id,
                rows: 2,
            },
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();
    // The job is lost (a crashed process, a flushed queue).
    let lose_jobs = || async {
        db.conn()
            .execute_unprepared("DELETE FROM jobs")
            .await
            .unwrap();
    };
    lose_jobs().await;

    h.advance(301);
    assert_eq!(
        h.operations.sweep().await.unwrap(),
        SweepReport {
            redispatched: 1,
            ..SweepReport::default()
        }
    );
    let restarted = h.operations.find(op.id).await.unwrap().unwrap();
    assert_eq!(restarted.status, OperationStatus::Pending);
    assert_eq!(
        restarted.progress.note,
        "Restarted after it stopped responding"
    );

    // Lost again: the restarts are used up, so it fails.
    lose_jobs().await;
    h.advance(301);
    assert_eq!(h.operations.sweep().await.unwrap().failed, 1);

    // A restarted job that runs finishes the operation as usual.
    let uow = db.uow(None, None).await;
    let op = h
        .operations
        .start(&uow, Operation::new("export").idempotent())
        .await
        .unwrap();
    h.operations
        .dispatch(
            &uow,
            &op,
            Export {
                operation: op.id,
                rows: 2,
            },
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();
    lose_jobs().await;
    h.advance(301);
    h.operations.sweep().await.unwrap();
    h.drain().await;
    let done = h.operations.find(op.id).await.unwrap().unwrap();
    assert_eq!(done.status, OperationStatus::Succeeded);
}

/// A batch whose operation missed its finish (the worker stopped right
/// after the batch's last job) is finished by the watchdog.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_watchdog_finishes_operations_of_finished_batches() {
    let h = harness_with(one_worker(QueueDriver::Database)).await;
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let op = h
        .operations
        .start(&uow, Operation::new("import"))
        .await
        .unwrap();
    h.operations
        .dispatch_batch(&uow, &op, Batch::new("import").push(Record::new("a")))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    h.drain().await;
    // Undo the operation's finish, as if the worker had stopped first.
    db.conn()
        .execute_unprepared(
            "UPDATE operations SET status = 'running', finished_at = NULL",
        )
        .await
        .unwrap();

    h.advance(301);
    assert_eq!(
        h.operations.sweep().await.unwrap(),
        SweepReport {
            finished: 1,
            ..SweepReport::default()
        }
    );
    assert_eq!(
        h.operations.find(op.id).await.unwrap().unwrap().status,
        OperationStatus::Succeeded
    );
}
