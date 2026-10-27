---
name: navree-billing
description: "Billing with navree-billing (facade feature `billing`, opt-in): `BillingModule` and the `billing` config section (plans, prices, trial days, seats, meters, Stripe and Paddle keys), the `Billable` trait for workspaces, users or your own types, the `Billing` service (checkout builder, `subscription`/`subscribed`, swap, quantity, cancel, resume, pause, trials without a subscription, metered usage, invoices, the customer portal), the `Subscribed` extractor (402), the `/api/billing` routes, signed webhooks (`Stripe-Signature`, `Paddle-Signature`) stored in `billing_events` and processed by the `billing.process_event` job, the billing events, the `billing:*` commands and `BillingFake` (`billing.provider = \"fake\"`). Use when adding subscriptions or paid plans, gating features by plan, recording usage, testing payment flows, or debugging a 400/402/409/502 from billing or a webhook that does nothing."
---

# Billing

`navree-billing` charges a **billable** (a workspace, a user, or a type of
your own) through Stripe or Paddle and keeps a local copy of its
subscriptions, so plan checks are local queries. It is `navree::billing`
(crate `navree-billing`, facade feature `billing`, **not** in `full`: add
`features = ["billing"]`). Everything below is in the code; when in doubt
read `crates/navree-billing/src` and its tests in
`crates/navree-billing/tests/` (`billing/` on the fake, `providers/` for
Stripe and Paddle against a mock server).

## Steps: bill workspaces

1. Turn the feature on and add the modules (CryptoModule for auth,
   AccessModule because the routes check `billing.manage`):

```toml
navree = { git = "https://github.com/NavreeHQ/navree", features = ["billing"] }
```

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())
    .module(DatabaseModule::from_config())
    .module(EventsModule::for_root())
    .module(QueueModule::from_config())       // webhook jobs (inline without it)
    .module(ScheduleModule::for_root())       // billing:report-usage every minute
    .module(HttpClientModule::from_config())  // optional; a default client otherwise
    .module(AuthModule::from_config())
    .module(AccessModule::for_root(AccessOptions::default()))
    .module(WorkspaceModule::from_config())
    .module(BillingModule::from_config())     // section `billing`
    .module(SettingsModule)
```

2. Configure plans and secrets (secrets only in `.env`):

```toml
[billing]
provider = "stripe"                        # stripe | paddle | fake
return_url = "https://app.acme.test/settings/billing"   # default: app.url

[billing.plans.pro]                        # key: lower-case letters, digits, _ -
name = "Pro"
prices = { monthly = "price_1Q8ProMonthly", yearly = "price_1Q8ProYearly" }
trial_days = 14
seats = true                               # quantity = member count

[billing.plans.pro.meters]
ai_summaries = "price_1Q8AiSummaries"     # a metered price
```

```env
APP_BILLING__STRIPE__SECRET_KEY=sk_live_...
APP_BILLING__STRIPE__WEBHOOK_SECRET=whsec_...
# Paddle: APP_BILLING__PROVIDER=paddle, APP_BILLING__PADDLE__API_KEY,
# APP_BILLING__PADDLE__WEBHOOK_SECRET, APP_BILLING__PADDLE__ENVIRONMENT=production
```

3. Register the billable in the module that owns billing, and write the
   upgrade route (or use the built-in `POST /api/billing/checkout`):

```rust
use navree::billing::{Billing, CheckoutUrl};
use navree::prelude::*;
use navree::workspace::{CurrentWorkspace, Workspace};

#[module(
    imports = [BillingModule::for_feature::<Workspace>()],
    controllers = [UpgradeController],
)]
pub struct SettingsModule;

#[injectable]
pub struct UpgradeController {
    billing: Arc<Billing>,
}

#[controller("/billing/upgrade", version = "1")]
#[guard(InWorkspace::guard())] // permissions are checked in the workspace
impl UpgradeController {
    #[post("/")]
    #[permissions("billing.manage")]
    async fn upgrade(
        &self,
        workspace: CurrentWorkspace,
        CurrentUser(user): CurrentUser,
        Valid(Json(input)): Valid<Json<Upgrade>>,
    ) -> Result<Json<CheckoutUrl>> {
        let checkout = self
            .billing
            .checkout(&workspace, &input.plan)
            .customer_email(&user.email)          // a workspace has no email
            .success_url("https://app.acme.test/settings/billing?upgraded=1")
            .cancel_url("https://app.acme.test/settings/billing");
        Ok(Json(checkout.create().await?.into()))
    }
}
```

4. Point the provider's webhooks at `POST /api/billing/webhook`
   (`navree billing:webhook https://api.acme.test/api/billing/webhook`
   creates the endpoint and prints the secret), run a queue worker
   (`navree queue:work`) and the scheduler.

5. Gate features: `Subscribed` (402 `subscription_required` without a
   valid subscription or trial) or `billing.subscribed(&ws, "pro").await?`.
   The front end maps the error body's `code` to its upgrade prompt.

```rust
#[post("/{id}/summary")]
#[authenticated]
async fn summarize(&self, subscription: Subscribed, Path(id): Path<Uuid>) -> Result<Json<Summary>> {
    if !subscription.on_plan("pro") {
        return Err(AppError::forbidden().with_detail("AI summaries are part of the Pro plan"));
    }
    // ...
}
```

Bill users instead: `BillingModule::for_feature::<navree::auth::User>()`
(routes then need only a signed-in user). Both registered: set
`billing.routes_billable = "workspace"` or `"user"` (the boot fails
otherwise). A type of your own: `impl Billable` (`BILLABLE_TYPE`,
`billable_id`, `billing_name`, `billing_email`), plus `from_request` to use
it on routes.

## The service (`Arc<Billing>`; every method takes `&impl AsBillable`)

```rust
billing.checkout(&ws, "pro")            // builder: yearly() monthly() interval(i) trial_days(n)
    .quantity(3).coupon("LAUNCH")       //   allow_promotion_codes() customer_email(e)
    .metadata("source", "pricing")      //   success_url(u) cancel_url(u)
    .create().await?;                   // Checkout { id, url, plan, interval, quantity, trial_days }
billing.subscription(&ws).await?;       // Option<Subscription>: valid one, else latest not ended
billing.subscriptions(&ws).await?;      // all, newest first
billing.subscribed(&ws, "pro").await?;  // valid && on_plan
billing.access(&ws).await?;             // what Subscribed sees (valid sub or trial w/o sub)
billing.swap(&ws, "business").await?;   // keeps interval; swap_yearly, swap_monthly,
billing.swap_with(&ws, "business", Proration::InvoiceNow).await?; // swap_to(&ws, plan, Some(i), p)
billing.update_quantity(&ws, 12).await?;
billing.cancel(&ws).await?;             // at period end (grace); cancel_now: at once
billing.resume(&ws).await?;             // only on the grace period (400 otherwise)
billing.pause(&ws).await?; billing.unpause(&ws).await?;
billing.start_trial(&ws, 14).await?;    // no provider call; billing_customers.trial_ends_at
billing.on_trial(&ws).await?; billing.trial_ends_at(&ws).await?;
billing.report_usage(&ws, "ai_summaries", 1).await?; // a row in the current Uow
billing.usage(&ws, "ai_summaries").await?;           // this period's total
billing.invoices(&ws).await?;           // provider list, cached invoice_cache_secs
billing.portal_url(&ws, "https://app.acme.test/settings/billing").await?;
billing.sync(&ws).await?;               // pull from the provider (billing:sync <type> <id>)
```

`Subscription`: `plan`, `interval`, `status` (`Trialing`, `Active`,
`PastDue`, `Paused`, `Canceled`, `Incomplete`), `quantity`,
`trial_ends_at`, `ends_at`, `current_period_start/end`, `past_due_since`,
`provider_id`; checks `valid()`, `on_trial()`, `on_grace_period()`,
`canceled()`, `past_due()` (valid for `past_due_grace_days`), `paused()`,
`ended()`, `on_plan(key)`, `is_trial_without_subscription()`.

Changes call the provider first, then write the local copy in the task's
unit of work (the request's in a handler) and emit events after commit.

## Webhooks and events

`POST /api/billing/webhook` (public): verify `Stripe-Signature` /
`Paddle-Signature` (HMAC-SHA256, 300 s tolerance, 400 otherwise) → store
once in `billing_events` (redelivery = 200 `{duplicate: true}`) → dispatch
`billing.process_event` on `billing.queue` (8 attempts, 10 s to 10 min).
The job applies snapshots (older provider states are ignored), links
customers and emits after commit:

| Event | `NAME` |
|---|---|
| `SubscriptionCreated` | `subscription.created` |
| `SubscriptionUpdated` | `subscription.updated` |
| `SubscriptionCanceled` | `subscription.canceled` |
| `TrialEnding` | `subscription.trial_ending` (Stripe only) |
| `InvoicePaid` | `invoice.paid` |
| `PaymentFailed` | `invoice.payment_failed` |

Listen with `m.listen::<PaymentFailed, MyListener>()`; fields
`billable_type`, `billable_id`, `subscription_id`, `plan` (`Option`s on the
invoice events), `invoice_id`, `amount_cents`, `currency`.

## Routes (`billing.routes = true`)

`GET /api/billing`, `GET /api/billing/plans`, `POST /api/billing/checkout
{plan, interval?}`, `POST /api/billing/swap {plan, interval?}`, `POST
/api/billing/cancel`, `POST /api/billing/resume`, `GET
/api/billing/invoices`, `POST /api/billing/portal {return_url?}` (same
origin as `billing.return_url`). Workspace billables: `billing.manage` in
the workspace (owner has it; grant admins via
`[workspace.roles.admin] permissions = ["billing.manage"]`) and the
`X-Workspace` header; users: signed in.

## Testing template

```rust
let app = TestApp::builder()
    .temp_database()
    .fake_auth()
    .config("app.key", navree::crypto::generate_key())
    .config("billing.provider", "fake")
    .config("billing.plans.pro.prices.monthly", "price_pro")
    .config("billing.plans.pro.trial_days", 14)
    .config("queue.driver", "memory")
    .module(CryptoModule::from_config())
    .module(DatabaseModule::from_config())
    .module(EventsModule::for_root())
    .module(QueueModule::from_config())
    .module(AuthModule::from_config())
    .module(AccessModule::for_root(AccessOptions::default()))
    .module(WorkspaceModule::from_config())
    .module(BillingModule::from_config())
    .module(SettingsModule)
    .override_value::<dyn Clock>(clock.clone()) // FixedClock for trials/grace
    .build()
    .await;
let billing = app.get::<Billing>().await;
let fake = billing.fake().expect("the fake provider");
let checkout = billing.checkout(&workspace, "pro").create().await?;
fake.complete_checkout(&checkout.id).await?;        // customer + subscription + invoice webhooks
app.get::<Queue>().await.run_pending().await?;      // process them
assert!(billing.subscription(&workspace).await?.unwrap().on_trial());
fake.fail_next_payment(&workspace);                 // next renewal / trial end fails
fake.end_trial(&workspace).await?;                  // or fake.renew(&workspace)
fake.assert_checkout(|c| c.plan == "pro" && c.trial_days == Some(14));
fake.reported_usage(&workspace, "ai_summaries");    // after report_pending_usage()
fake.send_webhook("invoice.paid", json!({ /* Stripe-shaped data.object */ })).await?;
```

## Checks

- `cargo test` with the fake; the provider clients are covered by
  `crates/navree-billing/tests/providers` (mock server, recorded shapes).
- `navree billing:plans` checks every price id against the provider.
- `navree billing:events --failed` after deploys; `billing:prune-events
  --days 90` from the scheduler.

## Gotchas

- Not in `full`: `navree::billing` doesn't exist until you add the
  `billing` feature.
- The fake's webhooks are queued like real ones: run the queue
  (`run_pending`) before asserting on subscriptions. Changes made through
  `Billing` (swap, cancel, ...) need no queue: the local copy is written at
  once and the fake sends no webhook for them.
- Missing `#[guard(InWorkspace::guard())]` on a workspace billing route
  checks `billing.manage` globally (403 for workspace owners).
- `success_url` is not proof of payment: wait for `SubscriptionCreated`.
- A checkout for a billable with a valid subscription is a 409
  `already_subscribed`: `swap`.
- No `billing.return_url` and no `app.url`: checkouts without explicit
  URLs fail (`BillingError::Config`).
- Paddle: the checkout opens the default payment link (set one in Paddle);
  trials come from the price's trial period (`trial_days` is not sent);
  no trial-ending webhook; invoices have no document links.
- Stripe: requests are pinned to `2025-03-31.basil` (`STRIPE_API_VERSION`);
  metered prices must have a Billing Meter (usage is sent as meter events).
- Usage for a billable whose plan lacks the meter is `UnknownMeter` (400);
  without a valid subscription `NoSubscription` (404).
- Provider failures are 502 with the provider's message (no keys or URLs).

Docs: `/docs/packages/billing/`.
