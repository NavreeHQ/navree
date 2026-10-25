//! The activities and dashboard modules boot inside a CRM.

use crm_activities::{ActivitiesModule, DashboardModule};
use crm_testkit::{Role, TestCrm};

#[tokio::test]
async fn boots_with_activities_and_dashboard() {
    let crm = TestCrm::builder()
        .module(ActivitiesModule)
        .module(DashboardModule)
        .build()
        .await;
    let rep = crm.login_as(Role::Rep1).await;
    rep.get("/api/auth/me").send().await.assert_status(200);
    crm.shutdown().await;
}
