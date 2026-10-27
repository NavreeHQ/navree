//! Per-user daily token budgets, counted in the app's cache by UTC day.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use navree_ai::{Ai, AiError, AiModule, FakeReply};
use navree_cache::CacheModule;
use navree_contracts::{Clock, FixedClock};
use navree_testing::TestApp;
use uuid::Uuid;

#[tokio::test]
async fn a_user_who_spent_their_tokens_waits_until_midnight_utc() {
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 10, 26, 23, 0, 0).unwrap(),
    ));
    let app = TestApp::builder()
        .config("ai.fake", true)
        .config("ai.budget.daily_tokens_per_user", 100)
        .override_value::<dyn Clock>(clock.clone())
        .module(CacheModule::from_config())
        .module(AiModule::from_config())
        .build()
        .await;
    let ai = app.get::<Ai>().await;
    let fake = ai.fake().unwrap();
    fake.respond_when(|_| true, FakeReply::text("ok").with_usage(60, 10));
    let (dana, sam) = (Uuid::now_v7(), Uuid::now_v7());

    ai.text("first").user(dana).await.unwrap();
    assert_eq!(ai.tokens_used_today(dana).await, 70);
    // 70 < 100: the next request may start, and goes over.
    ai.text("second").user(dana).await.unwrap();
    assert_eq!(ai.tokens_used_today(dana).await, 140);

    let error = ai.text("third").user(dana).await.unwrap_err();
    assert_eq!(
        error,
        AiError::BudgetExceeded {
            retry_after: Duration::from_secs(3600)
        }
    );
    let response = error.to_app_error();
    assert_eq!(response.status.as_u16(), 429);
    assert_eq!(response.headers.get("retry-after").unwrap(), "3600");
    fake.assert_prompt_count(2);

    // Others and requests without a user are not affected.
    ai.text("sam").user(sam).await.unwrap();
    ai.text("nobody").await.unwrap();

    // A new UTC day, a new allowance.
    clock.advance(Duration::from_secs(2 * 3600));
    ai.text("tomorrow").user(dana).await.unwrap();
    assert_eq!(ai.tokens_used_today(dana).await, 70);
}

#[tokio::test]
async fn budgets_are_off_by_default() {
    let app = TestApp::builder()
        .config("ai.fake", true)
        .module(AiModule::from_config())
        .build()
        .await;
    let ai = app.get::<Ai>().await;
    ai.fake()
        .unwrap()
        .respond_when(|_| true, FakeReply::text("ok").with_usage(1_000_000, 1));
    let user = Uuid::now_v7();

    for _ in 0..3 {
        ai.text("hi").user(user).await.unwrap();
    }
    assert_eq!(ai.tokens_used_today(user).await, 0);
}

#[tokio::test]
async fn streams_count_against_the_budget_when_they_finish() {
    use futures_util::StreamExt;

    let app = TestApp::builder()
        .config("ai.fake", true)
        .config("ai.budget.daily_tokens_per_user", 10)
        .module(AiModule::from_config())
        .build()
        .await;
    let ai = app.get::<Ai>().await;
    ai.fake()
        .unwrap()
        .respond_when(|_| true, FakeReply::text("a b c").with_usage(8, 4));
    let user = Uuid::now_v7();

    let stream = ai.stream("hi").user(user).await.unwrap();
    let _: Vec<_> = stream.collect().await;

    assert_eq!(ai.tokens_used_today(user).await, 12);
    assert!(matches!(
        ai.stream("again").user(user).await.unwrap_err(),
        AiError::BudgetExceeded { .. }
    ));
}
