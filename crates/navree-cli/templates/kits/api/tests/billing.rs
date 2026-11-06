//! Plans, checkout, trials and seats with the fake provider (no payment
//! account needed): what the workspace's billing settings do.

mod support;

use navree::queue::Queue;
use navree::workspace::WorkspaceRole;
use serde_json::{Value, json};
use support::{add_member, boot, in_workspace, sign_in, user, workspace};

#[tokio::test]
async fn owners_check_out_a_plan_and_start_its_trial() {
    let (app, _guard) = boot().await;
    let owner = user(&app, "Ojaswi Owner", "ojaswi@example.com").await;
    let team = workspace(&app, &owner, "Agra").await;
    let token = sign_in(&app, "ojaswi@example.com").await;

    // The plans of config/app.toml; no subscription yet.
    let overview = in_workspace(app.client().get("/api/billing"), &token, &team.slug)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("subscription", Value::Null)
        .json_value();
    let plans: Vec<&str> = overview["plans"]
        .as_array()
        .expect("plans")
        .iter()
        .filter_map(|plan| plan["key"].as_str())
        .collect();
    assert_eq!(plans, ["pro", "starter"]);

    // Checkout: the fake provider's URL, completed as if the customer paid.
    let checkout = in_workspace(
        app.client().post("/api/billing/checkout"),
        &token,
        &team.slug,
    )
    .json(&json!({ "plan": "pro", "interval": "monthly" }))
    .send()
    .await
    .assert_status(200)
    .json_value();
    let url = checkout["url"].as_str().expect("a checkout URL");
    let id = url
        .strip_prefix("https://checkout.fake.test/c/")
        .expect("the fake provider's checkout");
    in_workspace(
        app.client()
            .post(&format!("/api/v1/settings/billing/fake-checkout/{id}")),
        &token,
        &team.slug,
    )
    .send()
    .await
    .assert_status(204);

    // The plan has a 14-day trial and one seat per member.
    in_workspace(app.client().get("/api/billing"), &token, &team.slug)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("subscription.plan", json!("pro"))
        .assert_json_path("subscription.status", json!("trialing"))
        .assert_json_path("subscription.on_trial", json!(true))
        .assert_json_path("subscription.quantity", json!(1));
    in_workspace(
        app.client().get("/api/billing/invoices"),
        &token,
        &team.slug,
    )
    .send()
    .await
    .assert_status(200);
}

#[tokio::test]
async fn seats_follow_the_member_count() {
    let (app, _guard) = boot().await;
    let owner = user(&app, "Ojaswi Owner", "ojaswi@example.com").await;
    let team = workspace(&app, &owner, "Agra").await;
    let token = sign_in(&app, "ojaswi@example.com").await;
    let checkout = in_workspace(
        app.client().post("/api/billing/checkout"),
        &token,
        &team.slug,
    )
    .json(&json!({ "plan": "starter" }))
    .send()
    .await
    .assert_status(200)
    .json_value();
    let id = checkout["url"]
        .as_str()
        .and_then(|url| url.rsplit('/').next())
        .expect("a checkout id")
        .to_owned();
    in_workspace(
        app.client()
            .post(&format!("/api/v1/settings/billing/fake-checkout/{id}")),
        &token,
        &team.slug,
    )
    .send()
    .await
    .assert_status(204);

    let grace = user(&app, "Gauri Hundal", "gauri@example.com").await;
    add_member(&app, &team, &grace, WorkspaceRole::Member).await;
    // The seat count is updated by a queued listener.
    app.get::<Queue>()
        .await
        .run_pending()
        .await
        .expect("the jobs run");

    in_workspace(app.client().get("/api/billing"), &token, &team.slug)
        .send()
        .await
        .assert_json_path("subscription.quantity", json!(2));
}

#[tokio::test]
async fn only_billing_managers_reach_billing() {
    let (app, _guard) = boot().await;
    let owner = user(&app, "Ojaswi Owner", "ojaswi@example.com").await;
    let team = workspace(&app, &owner, "Agra").await;
    let viewer = user(&app, "Vera Viewer", "vera@example.com").await;
    add_member(&app, &team, &viewer, WorkspaceRole::Viewer).await;
    let token = sign_in(&app, "vera@example.com").await;

    in_workspace(app.client().get("/api/billing"), &token, &team.slug)
        .send()
        .await
        .assert_status(403);
    in_workspace(
        app.client()
            .post("/api/v1/settings/billing/fake-checkout/cs_1"),
        &token,
        &team.slug,
    )
    .send()
    .await
    .assert_status(403);
}
