//! Every async API returns `Send` futures, so it can be used from axum
//! handlers and spawned tasks.

use std::future::Future;
use std::time::Duration;

use navis_cache::{Cache, Locks, Ttl};

fn assert_send<F: Future + Send>(_: F) {}

#[test]
fn futures_are_send() {
    let cache = Cache::memory();
    let tagged = cache.tags(&["t"]);
    let locks = Locks::memory();
    let lock = locks.lock("l", Duration::from_secs(1));
    let ttl = Duration::from_secs(1);

    assert_send(cache.get::<u32>("k"));
    assert_send(cache.put("k", &1, ttl));
    assert_send(cache.add("k", &1, Ttl::Default));
    assert_send(cache.has("k"));
    assert_send(cache.forget("k"));
    assert_send(cache.pull::<u32>("k"));
    assert_send(cache.increment("k", 1));
    assert_send(cache.decrement("k", 1));
    assert_send(cache.remember("k", ttl, || async { Ok(1u32) }));
    assert_send(cache.flexible("k", ttl, ttl, || async { Ok(1u32) }));
    assert_send(cache.flush());
    assert_send(tagged.get::<u32>("k"));
    assert_send(tagged.put("k", &1, ttl));
    assert_send(tagged.remember("k", ttl, || async { Ok(1u32) }));
    assert_send(tagged.flexible("k", ttl, ttl, || async { Ok(1u32) }));
    assert_send(tagged.flush());
    assert_send(lock.acquire());
    assert_send(lock.block(ttl));
    assert_send(lock.release());
}
