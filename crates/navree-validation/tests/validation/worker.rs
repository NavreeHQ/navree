//! A request body is read on the async worker only when it is short: from
//! 16 KiB the extractors deserialize it, sanitize its `SafeHtml` fields and
//! check it on Tokio's blocking pool, whatever fields it holds.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use http::{Request, StatusCode};
use navree_http::Json;
use navree_http::body::Body;
use navree_http::extract::FromRequest;
use navree_validation::{HtmlPolicy, SafeHtml, Valid, Validate};
use serde::{Deserialize, Deserializer};

/// The cheapest way to make the sanitizer work for its size: a `<pre>` drops
/// a newline each time it is read, and the text is full of entities.
fn field(bytes: usize) -> String {
    let mut field = String::from("<pre>");
    field.push_str(&"\n".repeat(8));
    field.push_str(&"&".repeat(bytes.saturating_sub(5 + 8 + 6)));
    field.push_str("</pre>");
    field
}

/// Just under the length where the body leaves the worker.
const SMALL: usize = 16 * 1024 - 1;

#[derive(Deserialize, Validate)]
struct Blocks {
    #[validate(skip)]
    blocks: Vec<SafeHtml>,
}

fn json_request(body: String) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

/// How long `fields` take to sanitize, on this thread.
fn inline_cost(fields: &[String]) -> Duration {
    let json = serde_json::to_string(fields).unwrap();
    let started = Instant::now();
    let _: Vec<SafeHtml> = serde_json::from_str(&json).unwrap();
    started.elapsed()
}

/// The longest gap of a 2 ms ticker on a one-worker runtime while the
/// extractor reads `body`, and what the extractor answered.
fn longest_gap(body: String) -> (Duration, Result<usize, StatusCode>) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async move {
        let done = Arc::new(AtomicBool::new(false));
        let stop = done.clone();
        let ticker = tokio::spawn(async move {
            let mut longest = Duration::ZERO;
            while !stop.load(Ordering::Relaxed) {
                let started = Instant::now();
                tokio::time::sleep(Duration::from_millis(2)).await;
                longest = longest.max(started.elapsed());
            }
            longest
        });
        tokio::task::yield_now().await;
        let read = tokio::spawn(async move {
            let read = Valid::<Json<Blocks>>::from_request(json_request(body)).await;
            read.map(|valid| valid.0.0.blocks.len())
                .map_err(|error| error.status)
        });
        let read = read.await.unwrap();
        done.store(true, Ordering::Relaxed);
        (ticker.await.unwrap(), read)
    })
}

/// Many fields under the length where one leaves the worker are as much work
/// as one long one: the body decides, not the field.
#[test]
fn a_body_of_small_fields_does_not_hold_the_worker() {
    // As many fields as it takes for them to cost a third of a second, so the
    // gap says whether the worker was held on a machine of any speed.
    let mut count = 2;
    let (fields, cost) = loop {
        let fields = vec![field(SMALL); count];
        let cost = inline_cost(&fields);
        if cost >= Duration::from_millis(300) || count >= 120 {
            break (fields, cost);
        }
        count *= 2;
    };
    let body = serde_json::json!({ "blocks": fields }).to_string();
    let length = body.len();
    let (gap, read) = longest_gap(body);
    println!(
        "{count} fields, body of {length} bytes: {cost:?} to sanitize, longest ticker gap {gap:?}"
    );
    assert_eq!(read, Ok(count));
    assert!(
        gap < cost / 3,
        "a body of {count} small fields held the worker for {gap:?} of the {cost:?} it takes"
    );
}

/// The same when a field is refused, so that the body is read again to report
/// it.
#[test]
fn a_body_that_is_read_twice_does_not_hold_the_worker_either() {
    let mut count = 2;
    let (fields, cost) = loop {
        let fields = vec![field(SMALL); count];
        let cost = inline_cost(&fields);
        if cost >= Duration::from_millis(300) || count >= 100 {
            break (fields, cost);
        }
        count *= 2;
    };
    let mut fields = fields;
    fields.push("<ul>".repeat(4000));
    let body = serde_json::json!({ "blocks": fields }).to_string();
    let (gap, read) = longest_gap(body);
    println!(
        "{count} fields and a refused one: {cost:?} to sanitize once, longest ticker gap {gap:?}"
    );
    assert_eq!(read, Err(StatusCode::BAD_REQUEST));
    assert!(
        gap < cost / 3,
        "a body read twice held the worker for {gap:?} of {cost:?}"
    );
}

static SEEN: Mutex<Vec<ThreadId>> = Mutex::new(Vec::new());

/// A value that notes the thread it is deserialized on.
#[derive(Debug)]
struct Noted;

impl<'de> Deserialize<'de> for Noted {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?;
        SEEN.lock().unwrap().push(std::thread::current().id());
        Ok(Noted)
    }
}

#[derive(Debug, Deserialize, Validate)]
struct Note {
    #[validate(skip)]
    #[allow(dead_code)]
    note: Noted,
}

/// The thread that polls a request for a body of `bytes`, and the thread that
/// deserializes the body.
fn threads(bytes: usize, mut builder: tokio::runtime::Builder) -> (ThreadId, ThreadId) {
    let runtime = builder.enable_all().build().unwrap();
    runtime.block_on(async move {
        let body = serde_json::json!({ "note": "x".repeat(bytes) }).to_string();
        let task = tokio::spawn(async move {
            SEEN.lock().unwrap().clear();
            let polled_on = std::thread::current().id();
            Valid::<Json<Note>>::from_request(json_request(body))
                .await
                .map_err(|error| error.status)
                .unwrap();
            (polled_on, SEEN.lock().unwrap().pop().unwrap())
        });
        task.await.unwrap()
    })
}

/// A short body is read where the request is: a hop to another thread costs
/// more than reading it.
#[test]
fn a_short_body_is_read_on_the_worker_and_a_long_one_is_not() {
    // One test at a time notes threads.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _guard = ONE_AT_A_TIME.lock().unwrap();
    let one_worker = || {
        let mut builder = tokio::runtime::Builder::new_multi_thread();
        builder.worker_threads(1);
        builder
    };
    for bytes in [10, 1024, 16 * 1024 - 100] {
        let (worker, read_on) = threads(bytes, one_worker());
        assert_eq!(read_on, worker, "{bytes} bytes");
    }
    for bytes in [16 * 1024, 100 * 1024, 1024 * 1024] {
        let (worker, read_on) = threads(bytes, one_worker());
        assert_ne!(read_on, worker, "{bytes} bytes");
    }
    // A current-thread runtime has no other worker, but the blocking pool is
    // another thread.
    let (worker, read_on) = threads(100 * 1024, tokio::runtime::Builder::new_current_thread());
    assert_ne!(read_on, worker, "current-thread runtime");
}

/// `clean` is a plain function: it works on the thread that calls it, whatever
/// runtime that thread belongs to or is inside of.
#[test]
fn clean_never_panics_in_a_runtime_that_cannot_block_in_place() {
    let long = "x".repeat(20_000);
    let policy = HtmlPolicy::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .build()
        .unwrap();

    // A local task on a `LocalSet` that drives a multi-threaded runtime.
    let (text, cleaner) = (long.clone(), policy.clone());
    let local = tokio::task::LocalSet::new();
    let cleaned = local.block_on(&runtime, async move {
        tokio::task::spawn_local(async move { cleaner.clean(&text).len() }).await
    });
    assert_eq!(cleaned.expect("clean panicked in a LocalSet"), 20_000);

    // A multi-threaded runtime's handle entered on a current-thread runtime's
    // thread.
    let (text, cleaner) = (long.clone(), policy.clone());
    let handle = runtime.handle().clone();
    let current = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let cleaned = current.block_on(async move {
        let _guard = handle.enter();
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cleaner.clean(&text).len()))
    });
    assert_eq!(
        cleaned.expect("clean panicked with a handle entered"),
        20_000
    );

    // A plain thread with the handle entered, and the blocking pool.
    let (text, cleaner) = (long.clone(), policy.clone());
    let handle = runtime.handle().clone();
    let cleaned = std::thread::spawn(move || {
        let _guard = handle.enter();
        cleaner.try_clean(&text).map(|out| out.len())
    })
    .join()
    .expect("try_clean panicked");
    assert_eq!(cleaned.unwrap(), 20_000);
    let cleaned = runtime.block_on(async move {
        tokio::task::spawn_blocking(move || policy.clean(&long).len())
            .await
            .unwrap()
    });
    assert_eq!(cleaned, 20_000);
}

/// Long bodies read the same on every kind of runtime, and with none.
#[test]
fn a_long_body_is_read_on_any_runtime_and_without_one() {
    let body = || serde_json::json!({ "blocks": vec![field(SMALL); 3] }).to_string();
    let read = |body: String| async move {
        Valid::<Json<Blocks>>::from_request(json_request(body))
            .await
            .map(|valid| valid.0.0.blocks.len())
            .map_err(|error| error.status)
    };
    let current = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert_eq!(current.block_on(read(body())), Ok(3));

    let multi = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    let text = body();
    let counted = local.block_on(&multi, async move {
        tokio::task::spawn_local(read(text)).await.unwrap()
    });
    assert_eq!(counted, Ok(3));
}

/// No runtime at all: the extractor is a future like any other.
#[test]
fn a_long_body_is_read_without_a_runtime() {
    let body = serde_json::json!({ "blocks": vec![field(SMALL); 3] }).to_string();
    let read = Valid::<Json<Blocks>>::from_request(json_request(body));
    let read = futures_util::future::FutureExt::now_or_never(read);
    let read = read.expect("the extractor needed more than one poll");
    assert_eq!(read.map(|valid| valid.0.0.blocks.len()).ok(), Some(3));
}
