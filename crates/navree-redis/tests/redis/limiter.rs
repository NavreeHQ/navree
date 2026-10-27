//! `RateLimiter` on a real Redis server (`NAVREE_TEST_REDIS_URL`).

use std::time::Duration;

use navree_redis::{RateLimit, RateLimiter};

use crate::support::TestRedis;

#[tokio::test]
async fn a_burst_then_one_request_per_interval() {
    let Some(t) = TestRedis::connect("a_burst_then_one_request_per_interval").await else {
        return;
    };
    let limiter = RateLimiter::new(t.redis.connection().clone(), &t.prefix);
    // Two processes share the count.
    let other = RateLimiter::new(t.redis.connection().clone(), &t.prefix);
    let minute = Duration::from_secs(60);

    assert_eq!(
        limiter.hit("exports:ada", 2, minute).await.unwrap(),
        RateLimit::Allowed { remaining: 1 }
    );
    assert_eq!(
        other.hit("exports:ada", 2, minute).await.unwrap(),
        RateLimit::Allowed { remaining: 0 }
    );
    let RateLimit::Limited { retry_after } = limiter.hit("exports:ada", 2, minute).await.unwrap()
    else {
        panic!("the third request is over the limit");
    };
    assert!(
        retry_after > Duration::from_secs(29) && retry_after <= Duration::from_secs(30),
        "{retry_after:?}"
    );
    assert!(
        !limiter
            .hit("exports:ada", 2, minute)
            .await
            .unwrap()
            .is_allowed()
    );
    assert!(
        limiter
            .hit("exports:bob", 2, minute)
            .await
            .unwrap()
            .is_allowed(),
        "keys are independent"
    );

    limiter.reset("exports:ada").await.unwrap();
    assert!(
        limiter
            .hit("exports:ada", 2, minute)
            .await
            .unwrap()
            .is_allowed()
    );

    // The window refills one request per interval.
    let fast = Duration::from_millis(200);
    assert!(limiter.hit("fast", 1, fast).await.unwrap().is_allowed());
    assert!(!limiter.hit("fast", 1, fast).await.unwrap().is_allowed());
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(limiter.hit("fast", 1, fast).await.unwrap().is_allowed());
    t.cleanup().await;
}
