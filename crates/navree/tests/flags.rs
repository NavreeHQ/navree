//! The examples of the feature flags docs page
//! (`docs/packages/feature-flags.mdx`), through the
//! facade alone: `#[flag]` and `#[requires_flag]` expand to
//! `::navree::flags::*`, and the result boots, routes and resolves.

use std::collections::HashMap;
use std::sync::Mutex;

use navree::access::{AccessService, Subject};
use navree::db::Db;
use navree::flags::{FlagScope, Flags, FlagsFake, PlanResolver, Rollout, flag};
use navree::testing::{TestApp, TestAppBuilder, principal};
use navree::workspace::{NewWorkspace, Workspaces};
use navree::{BoxFuture, prelude::*};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

/// The redesigned deal pipeline board with drag-and-drop between stages.
#[flag]
pub struct NewPipeline;

/// AI summaries on the deal page.
#[flag(name = "ai-summaries", default = false, expose)]
pub struct AiSummaries;

/// The CSV export of deals; on unless someone turns it off.
#[flag(default = true)]
pub struct DealExport;

/// The `Deal` subject (`deals.*` permissions).
pub struct Deal;

impl Subject for Deal {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}

/// The board, in version 1 or 2.
#[derive(Serialize)]
pub struct DealBoard {
    version: u8,
}

/// A forecast.
#[derive(Serialize)]
pub struct Forecast {
    currency: &'static str,
}

/// A service that checks a flag for a scope it builds.
#[injectable]
pub struct DealSummaries {
    flags: Arc<Flags>,
}

impl DealSummaries {
    /// A summary when AI summaries are on for the owner in the workspace.
    pub async fn maybe_summarize(
        &self,
        owner_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<String>, BoxError> {
        let scope = FlagScope::user(owner_id).workspace(workspace_id);
        if !self.flags.active::<AiSummaries>(&scope).await? {
            return Ok(None);
        }
        Ok(Some("A summary.".to_owned()))
    }
}

/// The deals routes behind flags.
#[injectable]
pub struct DealsController;

#[controller("/deals", version = "1")]
impl DealsController {
    #[get("/board")]
    #[can(Read, Deal)]
    async fn board(&self, flags: CurrentFlags) -> Result<Json<DealBoard>> {
        let version = if flags.active::<NewPipeline>().await? {
            2
        } else {
            1
        };
        Ok(Json(DealBoard { version }))
    }

    #[get("/forecast")]
    #[can(Read, Deal)]
    #[requires_flag(NewPipeline)]
    async fn forecast(&self) -> Result<Json<Forecast>> {
        Ok(Json(Forecast { currency: "INR" }))
    }

    #[get("/summary")]
    #[authenticated]
    #[requires_flag(AiSummaries, status = 403)]
    async fn summary(&self) -> &'static str {
        "summary"
    }
}

#[module(
    imports = [
        FlagsModule::for_feature::<NewPipeline>(),
        FlagsModule::for_feature::<AiSummaries>(),
        FlagsModule::for_feature::<DealExport>(),
        AccessModule::for_subject::<Deal>(),
    ],
    providers = [DealSummaries],
    controllers = [DealsController],
)]
/// The deals module: its flags, service and routes.
pub struct DealsModule;

/// Plans of workspaces, set by the test (an app with billing reads them
/// from subscriptions: see `with_billing` below).
#[injectable]
pub struct Plans {
    #[inject(default)]
    plans: Mutex<HashMap<Uuid, String>>,
}

impl PlanResolver for Plans {
    fn plan(&self, workspace: Uuid) -> BoxFuture<'_, Result<Option<String>, BoxError>> {
        let plan = self.plans.lock().unwrap().get(&workspace).cloned();
        Box::pin(async move { Ok(plan) })
    }
}

/// Registers the plan resolver.
#[module(global, register = Self::wire)]
pub struct PlansModule;

impl PlansModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.plan_resolver::<Plans>().exports::<Plans>();
    }
}

fn deals_app() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("workspace.users_table", "")
        .module(DatabaseModule::from_config())
        .module(CacheModule::from_config())
        .module(EventsModule::for_root())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(PlansModule)
        .module(DealsModule)
        .module(FlagsModule::from_config())
}

async fn reader(app: &TestApp) -> Uuid {
    let user = Uuid::now_v7();
    let access = app.get::<AccessService>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    access
        .grant_to_user(&uow, user, "deals.read")
        .await
        .unwrap();
    uow.commit().await.unwrap();
    user
}

#[tokio::test]
async fn the_docs_examples_run_through_the_facade() {
    let app = deals_app().build().await;
    let flags = app.get::<Flags>().await;
    let rep = reader(&app).await;
    let (ada, grace) = (Uuid::now_v7(), Uuid::now_v7());
    let workspaces = app.get::<Workspaces>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let agra = workspaces
        .create(&uow, ada, NewWorkspace::named("Agra"))
        .await
        .unwrap();
    uow.commit().await.unwrap();

    // #[requires_flag]: a 404 while the flag is off.
    let client = app.acting_as(principal(rep));
    client
        .get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_error(404, None);
    client
        .get("/api/v1/deals/board")
        .send()
        .await
        .assert_json_path("version", json!(1));

    // Turning flags on and off.
    flags
        .activate::<NewPipeline>(FlagScope::workspace(agra.id))
        .await
        .unwrap();
    flags
        .activate::<NewPipeline>(FlagScope::user(ada))
        .await
        .unwrap();
    flags
        .deactivate::<NewPipeline>(FlagScope::user(grace))
        .await
        .unwrap();
    flags
        .forget::<NewPipeline>(FlagScope::user(grace))
        .await
        .unwrap();
    flags
        .activate::<NewPipeline>(FlagScope::everyone())
        .await
        .unwrap();
    client
        .get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("currency", json!("INR"));
    client
        .get("/api/v1/deals/board")
        .send()
        .await
        .assert_json_path("version", json!(2));
    assert!(flags.active::<NewPipeline>(&agra).await.unwrap());

    // Percentage rollouts.
    flags
        .rollout::<NewPipeline>(Rollout::percent(10).of_workspaces())
        .await
        .unwrap();
    flags
        .rollout::<NewPipeline>(Rollout::percent(50).of_workspaces())
        .await
        .unwrap();
    let inside = Rollout::percent(25)
        .of_workspaces()
        .includes::<NewPipeline>(agra.id);
    assert_eq!(
        inside,
        navree::flags::bucket("new-pipeline", agra.id) < 25,
        "the documented bucket"
    );

    // Rolling out by plan.
    app.get::<Plans>()
        .await
        .plans
        .lock()
        .unwrap()
        .insert(agra.id, "pro".to_owned());
    flags
        .rollout::<AiSummaries>(Rollout::plans(["pro", "business"]))
        .await
        .unwrap();
    let summaries = app.get::<DealSummaries>().await;
    assert_eq!(
        summaries.maybe_summarize(grace, agra.id).await.unwrap(),
        Some("A summary.".to_owned())
    );
    assert_eq!(
        summaries
            .maybe_summarize(grace, Uuid::now_v7())
            .await
            .unwrap(),
        None
    );
    flags
        .rollout::<AiSummaries>(
            Rollout::percent(50)
                .of_workspaces()
                .on_plans(["pro", "business"]),
        )
        .await
        .unwrap();

    // The kill switch.
    flags.kill::<NewPipeline>().await.unwrap();
    client
        .get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_error(404, None);
    flags.revive::<NewPipeline>().await.unwrap();
    client
        .get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_status(200);

    // The front end's flags: half of the Pro workspaces get AI summaries.
    let half = navree::flags::bucket("ai-summaries", agra.id) < 50;
    app.acting_as(principal(ada))
        .get("/api/flags")
        .header("X-Workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ai-summaries", json!(half));

    // The command line.
    app.run_command("flags:kill", &["new-pipeline"])
        .await
        .unwrap();
    assert!(flags.state::<NewPipeline>().await.unwrap().killed);
}

#[tokio::test]
async fn the_testing_examples_run_through_the_facade() {
    // The memory store, and the flags each test needs (as the docs page
    // writes it).
    let app = TestApp::builder()
        .temp_database()
        .config("flags.store", "memory")
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(DealsModule)
        .module(FlagsModule::from_config())
        .build()
        .await;

    // A rep who may read deals (the route's `#[can(Read, Deal)]`).
    let rep_id = Uuid::now_v7();
    let uow = app.get::<Db>().await.uow(None, None).await;
    app.get::<AccessService>()
        .await
        .grant_to_user(&uow, rep_id, "deals.read")
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let rep = app.acting_as(principal(rep_id));

    rep.get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_error(404, None);

    app.get::<Flags>()
        .await
        .activate::<NewPipeline>(FlagScope::user(rep_id))
        .await
        .unwrap();

    rep.get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("currency", json!("INR"));

    // A FlagsFake.
    let fake = FlagsFake::new().on::<NewPipeline>().off::<AiSummaries>();
    let app = deals_app()
        .override_value(Arc::new(fake.flags()))
        .build()
        .await;
    let rep = reader(&app).await;
    app.acting_as(principal(rep))
        .get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_status(200);
    fake.assert_checked::<NewPipeline>();
    fake.assert_not_checked::<DealExport>();
}

/// "Rolling out by plan" with billing: a `PlanResolver` that reads the
/// workspace's subscription from billing's local copy
/// (`cargo test -p navree --features flags,billing --test flags`).
#[cfg(feature = "billing")]
mod with_billing {
    use navree::BoxFuture;
    use navree::billing::{BillableRef, Billing, Subscription, WORKSPACE_BILLABLE};
    use navree::db::Db;
    use navree::flags::{FlagScope, Flags, PlanResolver, Rollout};
    use navree::prelude::*;
    use navree::queue::Queue;
    use navree::testing::TestApp;
    use navree::workspace::{NewWorkspace, Workspace, Workspaces};
    use uuid::Uuid;

    use super::AiSummaries;

    /// The plan of a workspace's valid subscription, from billing's local
    /// copy (no call to the payment provider).
    #[injectable]
    pub struct SubscriptionPlans {
        billing: Arc<Billing>,
    }

    impl PlanResolver for SubscriptionPlans {
        fn plan(&self, workspace: Uuid) -> BoxFuture<'_, Result<Option<String>, BoxError>> {
            Box::pin(async move {
                let billable = BillableRef::new(WORKSPACE_BILLABLE, workspace);
                let subscription = self.billing.subscription(&billable).await?;
                Ok(subscription
                    .filter(Subscription::valid)
                    .map(|subscription| subscription.plan))
            })
        }
    }

    /// The module that owns billing: workspaces are billed, and rollouts by
    /// plan read their subscriptions.
    #[module(
        imports = [
            BillingModule::for_feature::<Workspace>(),
            FlagsModule::for_feature::<AiSummaries>(),
        ],
        register = Self::wire,
    )]
    pub struct SubscriptionsModule;

    impl SubscriptionsModule {
        fn wire(&self, m: &mut ModuleBuilder) {
            m.plan_resolver::<SubscriptionPlans>();
        }
    }

    #[tokio::test]
    async fn plan_rollouts_read_billing_subscriptions() {
        let app = TestApp::builder()
            .temp_database()
            .fake_auth()
            .config("app.key", navree::crypto::generate_key())
            .config("billing.provider", "fake")
            .config("billing.plans.pro.prices.monthly", "price_pro")
            .config(
                "billing.return_url",
                "https://app.acme.test/settings/billing",
            )
            .config("queue.driver", "memory")
            .module(CryptoModule::from_config())
            .module(DatabaseModule::from_config())
            .module(EventsModule::for_root())
            .module(QueueModule::from_config())
            .module(AuthModule::from_config())
            .module(AccessModule::for_root(AccessOptions::default()))
            .module(WorkspaceModule::from_config())
            .module(BillingModule::from_config())
            .module(SubscriptionsModule)
            .module(FlagsModule::from_config())
            .build()
            .await;
        let owner = app
            .get::<AuthService>()
            .await
            .create_user("Anu Lakhotia", "anu@example.com", "password123")
            .await
            .unwrap();
        let workspaces = app.get::<Workspaces>().await;
        let uow = app.get::<Db>().await.uow(None, None).await;
        let agra = workspaces
            .create(&uow, owner.id, NewWorkspace::named("Agra"))
            .await
            .unwrap();
        let gulati = workspaces
            .create(&uow, owner.id, NewWorkspace::named("Gulati"))
            .await
            .unwrap();
        uow.commit().await.unwrap();

        // Agra subscribes to Pro; Gulati has no subscription.
        let billing = app.get::<Billing>().await;
        let checkout = billing.checkout(&agra, "pro").create().await.unwrap();
        let fake = billing.fake().expect("the fake provider");
        fake.complete_checkout(&checkout.id).await.unwrap();
        app.get::<Queue>().await.run_pending().await.unwrap();
        assert!(billing.subscribed(&agra, "pro").await.unwrap());

        let flags = app.get::<Flags>().await;
        flags
            .rollout::<AiSummaries>(Rollout::plans(["pro", "business"]))
            .await
            .unwrap();
        assert!(flags.active::<AiSummaries>(&agra).await.unwrap());
        assert!(!flags.active::<AiSummaries>(&gulati).await.unwrap());
        assert!(
            flags
                .active::<AiSummaries>(FlagScope::user(owner.id).workspace(agra.id))
                .await
                .unwrap()
        );
    }
}
