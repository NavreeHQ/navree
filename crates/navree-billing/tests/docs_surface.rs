//! Names every public item the billing docs page documents
//! (`apps/website/content/docs/packages/billing.mdx`), so a rename fails the
//! build: the module, the trait, the service and builder methods, the
//! subscription checks, the extractor, the events and their names, the
//! fake, the config keys, the routes, the tables and the commands.

#![allow(clippy::type_complexity)]

use std::collections::BTreeSet;

use navree_auth::{CurrentUser, User};
use navree_billing::{
    Billable, Billing, BillingError, BillingFake, BillingModule, BillingOptions, CheckoutUrl,
    FakeCheckout, Interval, Invoice, InvoicePaid, PaymentFailed, Proration, Subscribed,
    Subscription, SubscriptionCanceled, SubscriptionCreated, SubscriptionStatus,
    SubscriptionUpdated, TrialEnding,
};
use navree_events::Event;
use navree_kernel::Module;
use navree_workspace::{CurrentWorkspace, Workspace};
use sea_orm::{ConnectionTrait, Statement};
use serde_json::json;

fn billable<B: Billable>() -> &'static str {
    B::BILLABLE_TYPE
}

#[test]
fn the_documented_types_and_traits_exist() {
    let _: fn() -> _ = BillingModule::from_config;
    let _: fn(BillingOptions) -> _ = BillingModule::for_root;
    let _ = BillingModule::for_feature::<Workspace>();
    let _ = BillingModule::for_feature::<User>();
    fn is_module(_: &impl Module) {}
    is_module(&BillingModule::for_feature::<CurrentWorkspace>());
    assert_eq!(billable::<Workspace>(), "workspace");
    assert_eq!(billable::<CurrentWorkspace>(), "workspace");
    assert_eq!(billable::<User>(), "user");
    assert_eq!(billable::<CurrentUser>(), "user");

    let _ = Proration::InvoiceNow;
    let _ = Proration::None;
    for status in [
        SubscriptionStatus::Trialing,
        SubscriptionStatus::Active,
        SubscriptionStatus::PastDue,
        SubscriptionStatus::Paused,
        SubscriptionStatus::Canceled,
        SubscriptionStatus::Incomplete,
    ] {
        assert!(!status.as_str().is_empty());
    }
    let _ = (Interval::Monthly, Interval::Yearly);
    let _ = BillingError::UnknownPlan(String::new());
    let _ = BillingError::UnknownMeter(String::new());
    let url = CheckoutUrl {
        url: "https://checkout.stripe.com/c/pay/cs_live_a1B2c3".into(),
    };
    assert_eq!(
        serde_json::to_value(&url).unwrap(),
        json!({ "url": "https://checkout.stripe.com/c/pay/cs_live_a1B2c3" })
    );

    // The subscription's documented fields and checks.
    let read = |s: &Subscription| {
        (
            s.plan.clone(),
            s.interval,
            s.status,
            s.quantity,
            s.trial_ends_at,
            s.ends_at,
            s.current_period_end,
            s.valid(),
            s.on_trial(),
            s.on_grace_period(),
            s.canceled(),
            s.past_due(),
            s.paused(),
            s.on_plan("pro"),
        )
    };
    let _ = read;
    let deref = |s: &Subscribed| s.on_plan("pro");
    let _ = deref;

    // Invoices.
    let fields = |i: &Invoice| {
        (
            i.number.clone(),
            i.status.to_string(),
            i.total_cents,
            i.currency.clone(),
            i.pdf_url.clone(),
            i.hosted_url.clone(),
        )
    };
    let _ = fields;

    // The fake's documented API and the checkout fields tests assert on.
    let fake = |f: &BillingFake, w: &Workspace| {
        let _ = f.checkouts();
        let _: fn(&FakeCheckout) -> (&String, Option<u32>, &String, &String) =
            |c| (&c.plan, c.trial_days, &c.url, &c.id);
        f.fail_next_payment(w);
        let _ = f.reported_usage(w, "ai_summaries");
        drop(f.complete_checkout("cs_1"));
        drop(f.end_trial(w));
        drop(f.renew(w));
        drop(f.send_webhook("invoice.paid", json!({})));
    };
    let _ = fake;
}

#[test]
fn the_documented_event_names() {
    assert_eq!(SubscriptionCreated::NAME, "subscription.created");
    assert_eq!(SubscriptionUpdated::NAME, "subscription.updated");
    assert_eq!(SubscriptionCanceled::NAME, "subscription.canceled");
    assert_eq!(TrialEnding::NAME, "subscription.trial_ending");
    assert_eq!(InvoicePaid::NAME, "invoice.paid");
    assert_eq!(PaymentFailed::NAME, "invoice.payment_failed");
    let carried = |e: &PaymentFailed| {
        (
            e.billable_type.clone(),
            e.billable_id,
            e.subscription_id,
            e.plan.clone(),
            e.invoice_id.clone(),
            e.amount_cents,
        )
    };
    let _ = carried;
    let carried = |e: &SubscriptionCreated| {
        (
            e.billable_type.clone(),
            e.billable_id,
            e.subscription_id,
            e.plan.clone(),
        )
    };
    let _ = carried;
}

#[test]
fn the_documented_config_keys_and_defaults() {
    let options = BillingOptions::default();
    assert_eq!(options.provider.as_str(), "stripe");
    assert_eq!(options.currency, "usd");
    assert_eq!(options.return_url, None, "falls back to app.url");
    assert_eq!(options.queue, "default");
    assert_eq!(options.past_due_grace_days, 7);
    assert_eq!(options.trial_plan, None);
    assert_eq!(options.invoice_cache_secs, 300);
    assert!(options.routes);
    assert_eq!(options.routes_billable, None);
    assert!(options.webhook_route);
    assert_eq!(options.webhook_tolerance_secs, 300);
    let _ = (
        &options.stripe.secret_key,
        &options.stripe.webhook_secret,
        &options.paddle.api_key,
        &options.paddle.webhook_secret,
        options.paddle.environment,
    );
    let plan = navree_billing::PlanOptions::default();
    let _ = (
        &plan.name,
        &plan.prices.monthly,
        &plan.prices.yearly,
        plan.trial_days,
        plan.seats,
        &plan.meters,
    );
}

/// The documented service methods, with the documented argument shapes.
#[allow(dead_code)]
async fn the_documented_methods(billing: &Billing, workspace: &CurrentWorkspace, user: &User) {
    let mut checkout = billing
        .checkout(workspace, "pro")
        .customer_email(&user.email)
        .success_url("https://app.acme.test/settings/billing?upgraded=1")
        .cancel_url("https://app.acme.test/settings/billing")
        .trial_days(14)
        .quantity(3)
        .coupon("LAUNCH")
        .allow_promotion_codes()
        .metadata("source", "upgrade")
        .monthly();
    checkout = checkout.yearly();
    let _: Result<CheckoutUrl, BillingError> = checkout.create().await.map(Into::into);
    let _ = billing.subscription(workspace).await;
    let _ = billing.subscribed(workspace, "pro").await;
    let _ = billing.swap(workspace, "business").await;
    let _ = billing.swap_yearly(workspace, "pro").await;
    let _ = billing.update_quantity(workspace, 12).await;
    let _ = billing.cancel(workspace).await;
    let _ = billing.cancel_now(workspace).await;
    let _ = billing.resume(workspace).await;
    let _ = billing.pause(workspace).await;
    let _ = billing.unpause(workspace).await;
    let _ = billing
        .swap_with(workspace, "business", Proration::InvoiceNow)
        .await;
    let _ = billing.start_trial(workspace, 14).await;
    let _ = billing.on_trial(workspace).await;
    let _ = billing.trial_ends_at(workspace).await;
    let _ = billing.report_usage(workspace, "ai_summaries", 1).await;
    let _ = billing.usage(workspace, "ai_summaries").await;
    let _ = billing.invoices(workspace).await;
    let _ = billing
        .portal_url(workspace, "https://app.acme.test/settings/billing")
        .await;
    let _ = billing.fake();
}

#[tokio::test]
async fn the_documented_routes_tables_and_commands() {
    let app = navree_testing::TestApp::builder()
        .temp_database()
        .fake_auth()
        .config("billing.provider", "fake")
        .config("billing.routes_billable", "user")
        .module(navree_db::DatabaseModule::from_config())
        .module(BillingModule::from_config())
        .module(Users)
        .build()
        .await;

    let routes: BTreeSet<String> = navree_http::route_table(app.ctx())
        .await
        .unwrap()
        .into_iter()
        .map(|route| format!("{} {}", route.method, route.full_path))
        .collect();
    for route in [
        "GET /api/billing",
        "GET /api/billing/plans",
        "POST /api/billing/checkout",
        "POST /api/billing/swap",
        "POST /api/billing/cancel",
        "POST /api/billing/resume",
        "GET /api/billing/invoices",
        "POST /api/billing/portal",
        "POST /api/billing/webhook",
    ] {
        assert!(routes.contains(route), "{route} in {routes:#?}");
    }

    let db = app.get::<navree_db::Db>().await;
    let tables: BTreeSet<String> = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            "SELECT name FROM sqlite_master WHERE type = 'table'",
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect();
    for table in [
        "billing_customers",
        "subscriptions",
        "subscription_items",
        "billing_usage",
        "billing_events",
    ] {
        assert!(tables.contains(table), "{table} in {tables:?}");
    }

    let commands: BTreeSet<&str> = app.ctx().commands().iter().map(|c| c.name).collect();
    for command in [
        "billing:plans",
        "billing:webhook",
        "billing:report-usage",
        "billing:sync",
        "billing:events",
        "billing:prune-events",
    ] {
        assert!(commands.contains(command), "{command} in {commands:?}");
    }
}

/// Registers users as the billable.
struct Users;

impl Module for Users {
    fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
        m.import(BillingModule::for_feature::<User>());
    }
}
