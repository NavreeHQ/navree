//! The companies and contacts API over HTTP, against a booted CRM
//! (`crm_testkit::TestCrm`): one test binary, one module per resource.

mod companies;
mod contacts;
mod openapi;
mod support;

use crm_testkit::Role;

#[tokio::test]
async fn boots_with_companies_and_contacts() {
    let crm = support::crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    rep.get("/api/auth/me").send().await.assert_status(200);
    crm.shutdown().await;
}
