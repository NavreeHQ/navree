//! Server-sent events: the bytes of every event, the panics that keep them
//! well formed, and when keep-alives go out.

use std::convert::Infallible;
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use axum::response::IntoResponse as _;
use axum::response::sse as previous;
use bytes::Bytes;
use futures_util::{Stream, StreamExt as _, stream};
use http_body_util::BodyExt;
use navree_http::body::Body;
use navree_http::response::IntoResponse as _;
use navree_http::response::sse::{Event, KeepAlive, Sse};
use proptest::prelude::*;
use tokio::time::{Instant, advance};

use crate::support::{ours, theirs};

/// One thing done to an event, on both sides.
#[derive(Clone, Debug)]
enum Field {
    Data(String),
    Json(serde_json::Value),
    Comment(String),
    Event(String),
    Id(String),
    Retry(Duration),
    Write(Vec<String>),
}

fn apply_ours(event: Event, field: &Field) -> Event {
    match field {
        Field::Data(data) => event.data(data),
        Field::Json(value) => event.json_data(value).unwrap(),
        Field::Comment(text) => event.comment(text),
        Field::Event(name) => event.event(name),
        Field::Id(id) => event.id(id),
        Field::Retry(duration) => event.retry(*duration),
        Field::Write(pieces) => {
            let mut writer = event.into_data_writer();
            for piece in pieces {
                writer.write_str(piece).unwrap();
            }
            writer.into_event()
        }
    }
}

fn apply_theirs(event: previous::Event, field: &Field) -> previous::Event {
    match field {
        Field::Data(data) => event.data(data),
        Field::Json(value) => event.json_data(value).unwrap(),
        Field::Comment(text) => event.comment(text),
        Field::Event(name) => event.event(name),
        Field::Id(id) => event.id(id),
        Field::Retry(duration) => event.retry(*duration),
        Field::Write(pieces) => {
            let mut writer = event.into_data_writer();
            for piece in pieces {
                writer.write_str(piece).unwrap();
            }
            writer.into_event()
        }
    }
}

/// The bytes of an event on each side, or the panic's message.
fn bytes_of(fields: &[Field]) -> (Result<Bytes, String>, Result<Bytes, String>) {
    let ours = attempt(|| {
        let event = fields.iter().fold(Event::default(), apply_ours);
        let response = Sse::new(stream::iter([Ok::<_, Infallible>(event)])).into_response();
        futures_util::FutureExt::now_or_never(response.into_body().collect())
            .unwrap()
            .unwrap()
            .to_bytes()
    });
    let theirs = attempt(|| {
        let event = fields.iter().fold(previous::Event::default(), apply_theirs);
        let response =
            previous::Sse::new(stream::iter([Ok::<_, Infallible>(event)])).into_response();
        futures_util::FutureExt::now_or_never(response.into_body().collect())
            .unwrap()
            .unwrap()
            .to_bytes()
    });
    (ours, theirs)
}

fn attempt(run: impl FnOnce() -> Bytes) -> Result<Bytes, String> {
    catch_unwind(AssertUnwindSafe(run)).map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_default()
            // The message of a failed assertion is the same up to the shape
            // of the assertion that made it.
            .replace("assertion `left == right` failed: ", "")
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned()
    })
}

fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "a", "é", " ", ":", "\n", "\r", "\r\n", "\n\n", "data: x", "\t", "🦀", "\0",
        ]),
        0..8,
    )
    .prop_map(|parts| parts.concat())
}

fn field() -> impl Strategy<Value = Field> {
    prop_oneof![
        text().prop_map(Field::Data),
        text().prop_map(Field::Comment),
        text().prop_map(Field::Event),
        text().prop_map(Field::Id),
        (0_u64..100, 0_u32..2_000_000_000)
            .prop_map(|(secs, nanos)| Field::Retry(Duration::new(secs, nanos))),
        prop::collection::vec(text(), 0..4).prop_map(Field::Write),
        text().prop_map(|value| Field::Json(serde_json::json!({ "text": value, "n": 1 }))),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    /// Every sequence of fields gives the same bytes, or the same panic.
    #[test]
    fn events_read_the_same(fields in prop::collection::vec(field(), 0..6)) {
        let (a, b) = bytes_of(&fields);
        prop_assert_eq!(a, b, "{:?}", fields);
    }
}

#[test]
fn retries_at_every_scale() {
    for duration in [
        Duration::ZERO,
        Duration::from_nanos(1),
        Duration::from_micros(999),
        Duration::from_millis(1),
        Duration::from_millis(9),
        Duration::from_millis(10),
        Duration::from_millis(99),
        Duration::from_millis(100),
        Duration::from_millis(999),
        Duration::from_secs(1),
        Duration::from_millis(1_001),
        Duration::from_millis(1_010),
        Duration::from_millis(1_100),
        Duration::from_secs(30),
        Duration::from_secs(u64::MAX),
    ] {
        let (a, b) = bytes_of(&[Field::Retry(duration)]);
        assert_eq!(a, b, "{duration:?}");
    }
}

#[test]
fn the_default_keep_alive_is_an_empty_comment() {
    let (a, b) = bytes_of(&[]);
    assert_eq!(a, b);
}

/// A script: at each time, an event or the stream's end.
#[derive(Clone, Debug)]
enum Step {
    Wait(Duration),
    Send(&'static str),
    Fail,
}

fn stream_of<E: Send + 'static>(
    script: Vec<Step>,
    event: impl Fn(&'static str) -> E + Send + 'static,
) -> impl Stream<Item = Result<E, std::io::Error>> + Send + 'static {
    stream::unfold(
        (script.into_iter(), event),
        |(mut steps, event)| async move {
            loop {
                match steps.next()? {
                    Step::Wait(time) => tokio::time::sleep(time).await,
                    Step::Send(data) => {
                        let item = Ok(event(data));
                        return Some((item, (steps, event)));
                    }
                    Step::Fail => {
                        return Some((Err(std::io::Error::other("gone")), (steps, event)));
                    }
                }
            }
        },
    )
}

/// Each frame with when it came, until the body ends.
async fn timeline(body: Body) -> Vec<(u64, String)> {
    let start = Instant::now();
    let mut body = body;
    let mut lines = Vec::new();
    while let Some(frame) = body.frame().await {
        let at = start.elapsed().as_millis() as u64;
        match frame {
            Ok(frame) => {
                let data = frame.into_data().unwrap();
                lines.push((at, String::from_utf8_lossy(&data).into_owned()));
            }
            Err(error) => {
                lines.push((at, format!("error: {error}")));
                break;
            }
        }
    }
    lines
}

async fn both_timelines(script: &[Step], interval: Duration) -> Vec<(u64, String)> {
    let ours = Sse::new(stream_of(script.to_vec(), |data| {
        Event::default().data(data)
    }))
    .keep_alive(KeepAlive::new().interval(interval).text("alive"))
    .into_response();
    let theirs = previous::Sse::new(stream_of(script.to_vec(), |data| {
        previous::Event::default().data(data)
    }))
    .keep_alive(previous::KeepAlive::new().interval(interval).text("alive"))
    .into_response();
    let (a, b) = tokio::join!(
        timeline(ours.into_body()),
        timeline(Body::new(theirs.into_body()))
    );
    assert_eq!(a, b, "{script:?}");
    a
}

#[tokio::test(start_paused = true)]
async fn keep_alives_go_out_at_the_same_times() {
    let second = Duration::from_secs(1);
    let line = both_timelines(
        &[
            Step::Wait(second * 25),
            Step::Send("one"),
            Step::Wait(second * 3),
            Step::Send("two"),
            Step::Wait(second * 11),
        ],
        second * 10,
    )
    .await;
    assert_eq!(
        line,
        [
            (10_000, ": alive\n\n".to_owned()),
            (20_000, ": alive\n\n".to_owned()),
            (25_000, "data: one\n\n".to_owned()),
            (28_000, "data: two\n\n".to_owned()),
            (38_000, ": alive\n\n".to_owned()),
        ],
        "a keep-alive only fills the quiet, and the end of the stream ends them"
    );
}

#[tokio::test(start_paused = true)]
async fn a_failing_stream_ends_with_its_error_on_both_sides() {
    let second = Duration::from_secs(1);
    let line = both_timelines(
        &[Step::Send("one"), Step::Wait(second * 3), Step::Fail],
        second * 2,
    )
    .await;
    assert_eq!(
        line.last(),
        Some(&(3_000, "error: gone".to_owned())),
        "{line:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn nothing_is_held_back_between_events() {
    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(1);
    let events = stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut body = Sse::new(events).into_response().into_body();
    sender.send(Ok(Event::default().data("now"))).await.unwrap();
    let frame = body.frame().await.unwrap().unwrap();
    assert_eq!(frame.into_data().unwrap(), "data: now\n\n");
    advance(Duration::from_secs(3600)).await;
    let pending = futures_util::FutureExt::now_or_never(body.frame());
    assert!(pending.is_none(), "no keep-alive was asked for");
}

#[tokio::test]
async fn headers_and_frames_are_the_same() {
    let a = Sse::new(
        stream::iter(["1", "2"])
            .map(|data| Ok::<_, Infallible>(Event::default().id(data).data(data))),
    )
    .into_response();
    let b = previous::Sse::new(
        stream::iter(["1", "2"])
            .map(|data| Ok::<_, Infallible>(previous::Event::default().id(data).data(data))),
    )
    .into_response();
    let a = ours(a).await;
    assert_eq!(a, theirs(b).await);
    assert_eq!(a.frames.len(), 2, "one frame per event");
}

#[test]
fn a_keep_alive_is_checked_when_it_is_made() {
    let a = catch_unwind(|| KeepAlive::new().text("a\nb"));
    let b = catch_unwind(|| previous::KeepAlive::new().text("a\nb"));
    assert_eq!(a.is_err(), b.is_err());
    assert!(a.is_err());
}
