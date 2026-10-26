//! Metered usage: recording, the period total and reporting.

use navree_billing::BillingError;
use navree_db::Db;

use crate::support::World;

#[tokio::test]
async fn usage_is_recorded_locally_and_reported_in_batches() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    world.subscribe(&acme, "pro").await;

    billing
        .report_usage(&acme, "ai_summaries", 1)
        .await
        .unwrap();
    billing
        .report_usage(&acme, "ai_summaries", 2)
        .await
        .unwrap();
    assert_eq!(billing.usage(&acme, "ai_summaries").await.unwrap(), 3);
    let fake = world.fake().await;
    assert_eq!(
        fake.reported_usage(&acme, "ai_summaries"),
        0,
        "not sent yet"
    );

    let summary = billing.report_pending_usage().await.unwrap();
    assert_eq!(summary.reported, 1);
    assert_eq!(summary.units, 3);
    assert_eq!(summary.failed, 0);
    assert_eq!(fake.reported_usage(&acme, "ai_summaries"), 3);
    // Reported rows are not sent again, but still count for the period.
    let summary = billing.report_pending_usage().await.unwrap();
    assert_eq!(summary.reported, 0);
    assert_eq!(billing.usage(&acme, "ai_summaries").await.unwrap(), 3);

    billing
        .report_usage(&acme, "ai_summaries", 4)
        .await
        .unwrap();
    world
        .app
        .run_command("billing:report-usage", &[])
        .await
        .unwrap();
    assert_eq!(fake.reported_usage(&acme, "ai_summaries"), 7);

    // A new period starts the count again.
    world.advance_days(15);
    world.fake().await.end_trial(&acme).await.unwrap();
    world.run_queue().await;
    assert_eq!(billing.usage(&acme, "ai_summaries").await.unwrap(), 0);
}

#[tokio::test]
async fn unknown_meters_and_billables_without_the_meter_are_refused() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;

    let error = billing.report_usage(&acme, "emails", 1).await.unwrap_err();
    assert!(matches!(&error, BillingError::UnknownMeter(meter) if meter == "emails"));
    assert_eq!(error.to_app_error().status.as_u16(), 400);
    let error = billing.usage(&acme, "emails").await.unwrap_err();
    assert!(matches!(error, BillingError::UnknownMeter(_)));

    let error = billing
        .report_usage(&acme, "ai_summaries", 1)
        .await
        .unwrap_err();
    assert!(matches!(error, BillingError::NoSubscription), "{error}");

    world.subscribe(&acme, "business").await;
    let error = billing
        .report_usage(&acme, "ai_summaries", 1)
        .await
        .unwrap_err();
    assert!(
        matches!(error, BillingError::UnknownMeter(_)),
        "business has no meter"
    );
    let error = billing
        .report_usage(&acme, "ai_summaries", 0)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        BillingError::Invalid {
            field: "quantity",
            ..
        }
    ));
}

#[tokio::test]
async fn usage_joins_the_unit_of_work_and_a_rollback_records_nothing() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    world.subscribe(&acme, "pro").await;

    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    uow.scope(billing.report_usage(&acme, "ai_summaries", 5))
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(billing.usage(&acme, "ai_summaries").await.unwrap(), 0);

    let uow = db.uow(None, None).await;
    uow.scope(billing.report_usage(&acme, "ai_summaries", 5))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(billing.usage(&acme, "ai_summaries").await.unwrap(), 5);
}

#[tokio::test]
async fn usage_of_a_billable_whose_subscription_ended_stays_unreported() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    world.subscribe(&acme, "pro").await;
    billing
        .report_usage(&acme, "ai_summaries", 2)
        .await
        .unwrap();
    billing.cancel_now(&acme).await.unwrap();
    let summary = billing.report_pending_usage().await.unwrap();
    assert_eq!(summary.reported, 0);
    assert_eq!(summary.skipped, 1);
    assert_eq!(world.fake().await.reported_usage(&acme, "ai_summaries"), 0);
}
