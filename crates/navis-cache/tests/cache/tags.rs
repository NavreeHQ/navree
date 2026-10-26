//! `TaggedCache`: entries grouped by tags and flushed together.

use std::time::Duration;

use navis_cache::Cache;

const MINUTE: Duration = Duration::from_secs(60);

#[tokio::test]
async fn tags_flush_only_tagged() {
    let cache = Cache::memory();
    let users = cache.tags(&["users"]);
    let posts = cache.tags(&["posts"]);
    users.put("list", &vec![1, 2], MINUTE).await.unwrap();
    posts.put("list", &vec![9], MINUTE).await.unwrap();
    cache.put("list", &vec![0], MINUTE).await.unwrap();

    users.flush().await.unwrap();

    assert_eq!(users.get::<Vec<u32>>("list").await.unwrap(), None);
    assert_eq!(posts.get::<Vec<u32>>("list").await.unwrap(), Some(vec![9]));
    assert_eq!(cache.get::<Vec<u32>>("list").await.unwrap(), Some(vec![0]));

    users.put("list", &vec![3], MINUTE).await.unwrap();
    assert_eq!(users.get::<Vec<u32>>("list").await.unwrap(), Some(vec![3]));
}

#[tokio::test]
async fn flushing_any_tag_of_an_entry_invalidates_it() {
    let cache = Cache::memory();
    cache
        .tags(&["users", "teams"])
        .put("member:1", &true, MINUTE)
        .await
        .unwrap();

    cache.tags(&["teams"]).flush().await.unwrap();

    assert!(
        !cache
            .tags(&["users", "teams"])
            .has("member:1")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn tagged_entries_are_addressed_by_their_tag_set_in_any_order() {
    let cache = Cache::memory();
    cache.tags(&["a", "b"]).put("k", &1, MINUTE).await.unwrap();

    assert_eq!(
        cache.tags(&["b", "a"]).get::<i32>("k").await.unwrap(),
        Some(1)
    );
    assert_eq!(cache.tags(&["a"]).get::<i32>("k").await.unwrap(), None);
    assert_eq!(cache.get::<i32>("k").await.unwrap(), None);
}

#[tokio::test]
async fn tagged_remember_add_increment_and_forget() {
    let cache = Cache::memory();
    let tagged = cache.tags(&["stats"]);

    let n: u32 = tagged
        .remember("n", MINUTE, || async { Ok(5) })
        .await
        .unwrap();
    let again: u32 = tagged
        .remember("n", MINUTE, || async { Ok(6) })
        .await
        .unwrap();
    assert_eq!((n, again), (5, 5));

    assert!(tagged.add("once", &1, MINUTE).await.unwrap());
    assert!(!tagged.add("once", &2, MINUTE).await.unwrap());

    assert_eq!(tagged.increment("hits", 2).await.unwrap(), 2);
    assert_eq!(tagged.decrement("hits", 1).await.unwrap(), 1);

    assert!(tagged.forget("once").await.unwrap());
    assert!(!tagged.has("once").await.unwrap());

    tagged.flush().await.unwrap();
    assert_eq!(tagged.get::<i64>("hits").await.unwrap(), None);
    assert_eq!(tagged.tags(), ["stats".to_owned()]);
}
