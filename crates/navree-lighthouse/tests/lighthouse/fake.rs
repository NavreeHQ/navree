//! `LighthouseFake`: reports your code makes and exceptions the framework
//! captures, without a store.

use std::sync::Arc;

use navree_lighthouse::{Level, LighthouseFake, LighthouseModule};
use navree_testing::TestApp;

use crate::support::{DemoModule, InvoiceError};

#[tokio::test]
async fn the_fake_records_reports_and_captured_exceptions() {
    let fake = LighthouseFake::new();
    let app = TestApp::builder()
        .temp_database()
        .module(navree_db::DatabaseModule::from_config())
        .module(LighthouseModule::from_config())
        .module(DemoModule)
        .override_value(Arc::new(fake.lighthouse()))
        .build()
        .await;
    let client = app.client();
    client
        .post("/api/demo/report")
        .send()
        .await
        .assert_status(204);
    fake.assert_reported::<InvoiceError>(|report| {
        report
            .debug
            .as_deref()
            .is_some_and(|debug| debug.starts_with("Charge("))
    });
    fake.assert_reported_message("cannot charge invoice");
    fake.assert_reported_times(1);
    assert_eq!(fake.reports()[0].level, Level::Error);
    assert_eq!(
        fake.reports()[0].chain,
        ["cannot charge invoice", "card declined"]
    );

    client
        .get("/api/demo/boom/9")
        .send()
        .await
        .assert_status(500);
    fake.assert_reported_times(2);
    let captured = &fake.reports()[1];
    assert_eq!(captured.message, "invoice inv_9 not found");
    assert_eq!(captured.route.as_deref(), Some("GET /api/demo/boom/{n}"));

    client
        .post("/api/demo/tagged")
        .send()
        .await
        .assert_status(204);
    let tagged = fake.reports().pop().unwrap();
    assert_eq!(tagged.tags["plan"], "pro");
    assert_eq!(tagged.context["invoice_id"], 1042);

    fake.clear();
    fake.assert_nothing_reported();
    client.get("/api/demo/ok").send().await.assert_status(200);
    fake.assert_nothing_reported();
    app.shutdown().await.unwrap();
}

#[test]
#[should_panic(expected = "expected a report of a")]
fn a_missing_report_fails_the_assertion() {
    let fake = LighthouseFake::new();
    fake.lighthouse().report(&std::io::Error::other("x"));
    fake.assert_reported::<InvoiceError>(|_| true);
}
