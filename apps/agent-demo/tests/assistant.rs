//! The shop's assistant with the model scripted: the week's UPI payments as a donut drawn
//! from the reader's own data, a refund that waits for the manager's approval, an
//! order's card whose buttons only the manager gets and which refund once after asking,
//! and an order's page telling the assistant which order it is about.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_demo::assistant::ShopAssistant;
use agent_demo::shop::{self, MANAGER, Order, SAMPLE_ORDER, STAFF, rupees};
use agent_demo::{Settings, app};
use http::Uri;
use navree::access::{AbilityFactory, PrincipalAccess};
use navree::ai::ui::{Acted, Assistant};
use navree::ai::{Ai, AiFake, FakeReply};
use navree::auth::AuthService;
use navree::contracts::{AuthVia, Principal};
use navree::db::Db;
use navree::orm::Model as _;
use navree::testing::TestApp;
use navree::ui::LazyAsk;
use serde_json::json;
use uuid::Uuid;

/// The app on a database file of its own, removed when the test ends.
struct Shop {
    app: TestApp,
    path: PathBuf,
}

impl Drop for Shop {
    fn drop(&mut self) {
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{end}", self.path.display()));
        }
    }
}

async fn shop(name: &str) -> Shop {
    let path = std::env::temp_dir().join(format!("agent-demo-{name}-{}.db", std::process::id()));
    for end in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{end}", path.display()));
    }
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let app = TestApp::from_builder(app(0, url, Settings::default())).await;
    Shop { app, path }
}

/// The access of the person with `email`, as their request would have it.
async fn access(app: &TestApp, email: &str) -> Arc<PrincipalAccess> {
    let user = app
        .get::<AuthService>()
        .await
        .find_by_email(email)
        .await
        .unwrap()
        .expect("the seed made them");
    let principal = Principal {
        user_id: user.id,
        via: AuthVia::Token,
        token_abilities: None,
        session_id: None,
    };
    app.get::<AbilityFactory>()
        .await
        .access_for(Some(&principal))
        .await
        .unwrap()
}

async fn assistant(app: &TestApp) -> Arc<Assistant<ShopAssistant>> {
    app.get::<Assistant<ShopAssistant>>().await
}

async fn fake(app: &TestApp) -> AiFake {
    app.get::<Ai>().await.fake().expect("the demo runs AiFake")
}

/// The conversation once nothing in it is being written.
async fn settled(app: &TestApp, access: &Arc<PrincipalAccess>, id: Uuid) -> String {
    let assistant = assistant(app).await;
    for _ in 0..400 {
        let html = assistant
            .items(access.clone(), id, &LazyAsk::First, Some("tok"))
            .await
            .unwrap()
            .to_html();
        if !html.contains(r#"aria-busy="true""#) {
            tokio::time::sleep(Duration::from_millis(30)).await;
            return assistant
                .items(access.clone(), id, &LazyAsk::First, Some("tok"))
                .await
                .unwrap()
                .to_html();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the answer is still being written");
}

/// The signed URLs of the buttons in `html`.
fn buttons(html: &str) -> Vec<String> {
    html.match_indices(r#"action="/assistant/act?"#)
        .map(|(at, _)| {
            let rest = &html[at + r#"action=""#.len()..];
            rest[..rest.find('"').unwrap()].replace("&amp;", "&")
        })
        .collect()
}

async fn sample(app: &TestApp) -> Order {
    Order::query()
        .get(&*app.get::<Db>().await)
        .await
        .unwrap()
        .into_models()
        .into_iter()
        .find(|order| order.number == SAMPLE_ORDER)
        .unwrap()
}

#[tokio::test]
async fn upi_this_week_is_a_donut_of_the_readers_own_payments() {
    let shop = shop("donut").await;
    let app = &shop.app;
    let simran = access(app, MANAGER.1).await;
    let fake = fake(app).await;
    fake.respond_with([
        FakeReply::tool_call("show_payments_by_method", json!({ "period": "this_week" })),
        FakeReply::text("UPI took the most this week."),
    ]);
    let id = assistant(app)
        .await
        .start(simran.clone(), "How did UPI do this week?", None)
        .await
        .unwrap();
    let html = settled(app, &simran, id).await;

    assert!(html.contains(r#"data-genui="surface""#), "{html}");
    assert!(html.contains("Payments this week"), "{html}");
    assert!(html.contains("UPI"), "{html}");
    let since = shop::period_start("this_week", chrono::Utc::now());
    let orders: Vec<Order> = Order::query()
        .get(&*app.get::<Db>().await)
        .await
        .unwrap()
        .into_models()
        .into_iter()
        .filter(|order| order.placed_at >= since)
        .collect();
    assert!(html.contains(&format!("{} orders", orders.len())), "{html}");
    let total: i64 = orders.iter().map(Order::left).sum();
    assert!(html.contains(&rupees(total)), "{} in {html}", rupees(total));
    // The model chose the period; it never read the numbers.
    let result = fake.tool_results("show_payments_by_method")[0].to_string();
    assert!(!result.contains(&rupees(total)), "{result}");
}

#[tokio::test]
async fn a_refund_waits_for_the_managers_approval_in_the_page() {
    let shop = shop("approve").await;
    let app = &shop.app;
    let simran = access(app, MANAGER.1).await;
    let fake = fake(app).await;
    fake.respond(FakeReply::tool_call("refund_order", json!({ "order": SAMPLE_ORDER })));
    let assistant = assistant(app).await;
    let id = assistant
        .start(simran.clone(), "Refund order KK-1042", None)
        .await
        .unwrap();
    settled(app, &simran, id).await;
    let region = navree::ui::render(&assistant.approval(simran.clone(), id, Some("tok")).await.unwrap());

    assert!(region.contains("The assistant asks to: Refund an order"), "{region}");
    assert!(region.contains(SAMPLE_ORDER), "{region}");
    assert_eq!(sample(app).await.status, "paid", "nothing ran yet");

    let call = region
        .split(r#"name="call_id" value=""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_owned();
    fake.respond(FakeReply::text("Refunded ₹1,250 to Harpreet Kaur."));
    assistant.approve(simran.clone(), id, &call).await.unwrap();
    let html = settled(app, &simran, id).await;
    assert!(html.contains("Refunded ₹1,250 to Harpreet Kaur."), "{html}");
    let order = sample(app).await;
    assert_eq!((order.status.as_str(), order.refunded), ("refunded", 125_000));
}

#[tokio::test]
async fn staff_are_not_offered_refunds_and_see_an_order_without_buttons() {
    let shop = shop("staff").await;
    let app = &shop.app;
    let arjun = access(app, STAFF.1).await;
    let fake = fake(app).await;
    fake.respond_with([
        FakeReply::tool_call("show_order_card", json!({ "order": SAMPLE_ORDER })),
        FakeReply::text("Here is the order."),
    ]);
    let id = assistant(app)
        .await
        .start(arjun.clone(), "Show order KK-1042", None)
        .await
        .unwrap();
    let html = settled(app, &arjun, id).await;

    assert!(!fake.requests()[0].tools().iter().any(|t| t == "refund_order"));
    assert!(html.contains("Harpreet Kaur"), "{html}");
    assert!(html.contains("Only a manager can change an order."), "{html}");
    assert!(buttons(&html).is_empty(), "{html}");
}

#[tokio::test]
async fn the_cards_refund_asks_first_and_runs_once() {
    let shop = shop("card").await;
    let app = &shop.app;
    let simran = access(app, MANAGER.1).await;
    let fake = fake(app).await;
    fake.respond_with([
        FakeReply::tool_call("show_order_card", json!({ "order": SAMPLE_ORDER })),
        FakeReply::text("Here it is."),
    ]);
    let assistant = assistant(app).await;
    let id = assistant
        .start(simran.clone(), "Show order KK-1042", None)
        .await
        .unwrap();
    let html = settled(app, &simran, id).await;
    let urls = buttons(&html);
    assert_eq!(urls.len(), 2, "Mark delivered and Refund: {html}");
    assert!(html.contains("Refund order KK-1042?"), "{html}");
    let refund: Uri = urls[1].parse().unwrap();

    let (_, asked) = assistant.act(simran.clone(), &refund, false).await.unwrap();
    assert_eq!(asked, Acted::Ask { label: "Refund ₹1,250".into() });
    assert_eq!(sample(app).await.status, "paid");
    let (_, done) = assistant.act(simran.clone(), &refund, true).await.unwrap();
    assert_eq!(done, Acted::Done("Refunded ₹1,250 for order KK-1042".into()));
    assert_eq!(sample(app).await.status, "refunded");
    let again = assistant.act(simran.clone(), &refund, true).await.unwrap_err();
    assert_eq!(again.status.as_u16(), 409);
    // Arjun can't post Simran's button.
    let arjun = access(app, STAFF.1).await;
    let error = assistant.act(arjun, &refund, true).await.unwrap_err();
    assert_eq!(error.status.as_u16(), 403);
}

#[tokio::test]
async fn an_orders_page_tells_the_assistant_which_order_it_is_about() {
    let shop = shop("context").await;
    let app = &shop.app;
    let simran = access(app, MANAGER.1).await;
    let fake = fake(app).await;
    fake.respond(FakeReply::text("It was paid by UPI this morning."));
    let assistant = assistant(app).await;
    let order = sample(app).await;
    let context = assistant.context_ref(simran.user.unwrap(), "order", &order.id.to_string());
    let id = assistant
        .start(simran.clone(), "What about this order?", Some(&context))
        .await
        .unwrap();
    settled(app, &simran, id).await;
    let system = fake.requests().last().unwrap().system().unwrap().to_owned();
    assert!(
        system.contains("order KK-1042 by Harpreet Kaur of Ludhiana: ₹1,250 by UPI, paid"),
        "{system}"
    );
}
