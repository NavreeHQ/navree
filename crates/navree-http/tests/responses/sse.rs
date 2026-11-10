//! Server-sent events: the bytes of every event, the panics that keep them
//! well formed, and when keep-alives go out.

use std::convert::Infallible;
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use bytes::Bytes;
use futures_util::{Stream, StreamExt as _, stream};
use http_body_util::BodyExt;
use navree_http::body::Body;
use navree_http::response::IntoResponse as _;
use navree_http::response::sse::{Event, KeepAlive, Sse};
use proptest::prelude::*;
use tokio::time::{Instant, advance};

use crate::support::ours;

/// One thing done to an event.
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

fn apply(event: Event, field: &Field) -> Event {
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

/// The bytes of an event, or the panic's message.
fn bytes_of(fields: &[Field]) -> Result<Bytes, String> {
    attempt(|| {
        let event = fields.iter().fold(Event::default(), apply);
        let response = Sse::new(stream::iter([Ok::<_, Infallible>(event)])).into_response();
        futures_util::FutureExt::now_or_never(response.into_body().collect())
            .unwrap()
            .unwrap()
            .to_bytes()
    })
}

/// The bytes of an event, or the message of the panic, as the format
/// (<https://html.spec.whatwg.org/multipage/server-sent-events.html>) and the
/// rules in `Event`'s documentation give them: `data` (and a JSON value, and
/// whatever a writer is given) is one `data:` line per line of text, and no
/// line when the text is empty; a name, an id and a comment are one line each
/// and may not hold a line break; an id may not hold a NUL; data, a name, a
/// retry and an id are set once; a retry is in whole milliseconds; the event
/// ends with a blank line.
fn model(fields: &[Field]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let (mut has_data, mut has_event, mut has_retry, mut has_id) = (false, false, false, false);
    fn line(
        out: &mut Vec<u8>,
        name: &str,
        value: &str,
        breaks_allowed: bool,
    ) -> Result<(), String> {
        if !breaks_allowed && value.contains(['\r', '\n']) {
            return Err("SSE field value cannot contain newlines or carriage returns".to_owned());
        }
        out.extend_from_slice(format!("{name}: {value}\n").as_bytes());
        Ok(())
    }
    fn data(out: &mut Vec<u8>, has_data: &mut bool, text: &str) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        if std::mem::replace(has_data, true) {
            return Err("Called `Event::data*` multiple times".to_owned());
        }
        out.extend_from_slice(b"data: ");
        let mut rest = text;
        while let Some(at) = rest.find(['\r', '\n']) {
            out.extend_from_slice(&rest.as_bytes()[..=at]);
            out.extend_from_slice(b"data: ");
            rest = &rest[at + 1..];
        }
        out.extend_from_slice(rest.as_bytes());
        out.push(b'\n');
        Ok(())
    }
    for field in fields {
        match field {
            Field::Data(text) => data(&mut out, &mut has_data, text)?,
            Field::Write(pieces) => data(&mut out, &mut has_data, &pieces.concat())?,
            Field::Json(value) => data(&mut out, &mut has_data, &value.to_string())?,
            Field::Comment(text) => line(&mut out, "", text, false)?,
            Field::Event(name) => {
                if std::mem::replace(&mut has_event, true) {
                    return Err("Called `Event::event` multiple times".to_owned());
                }
                line(&mut out, "event", name, false)?;
            }
            Field::Id(id) => {
                if std::mem::replace(&mut has_id, true) {
                    return Err("Called `Event::id` multiple times".to_owned());
                }
                if id.contains('\0') {
                    return Err("Event ID cannot contain null characters".to_owned());
                }
                line(&mut out, "id", id, false)?;
            }
            Field::Retry(duration) => {
                if std::mem::replace(&mut has_retry, true) {
                    return Err("Called `Event::retry` multiple times".to_owned());
                }
                line(&mut out, "retry", &duration.as_millis().to_string(), false)?;
            }
        }
    }
    out.push(b'\n');
    Ok(out)
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

    /// Every sequence of fields gives the bytes the format asks for, or the
    /// panic that refuses it.
    #[test]
    fn events_follow_the_format(fields in prop::collection::vec(field(), 0..6)) {
        prop_assert_eq!(
            bytes_of(&fields),
            model(&fields).map(Bytes::from),
            "{:?}",
            fields
        );
    }
}

#[test]
fn retries_at_every_scale() {
    for (duration, millis) in [
        (Duration::ZERO, "0"),
        (Duration::from_nanos(1), "0"),
        (Duration::from_micros(999), "0"),
        (Duration::from_millis(1), "1"),
        (Duration::from_millis(9), "9"),
        (Duration::from_millis(10), "10"),
        (Duration::from_millis(99), "99"),
        (Duration::from_millis(100), "100"),
        (Duration::from_millis(999), "999"),
        (Duration::from_secs(1), "1000"),
        (Duration::from_millis(1_001), "1001"),
        (Duration::from_millis(1_010), "1010"),
        (Duration::from_millis(1_100), "1100"),
        (Duration::from_secs(30), "30000"),
        (Duration::from_secs(u64::MAX), "18446744073709551615000"),
    ] {
        assert_eq!(
            bytes_of(&[Field::Retry(duration)]),
            Ok(Bytes::from(format!("retry: {millis}\n\n"))),
            "{duration:?}"
        );
    }
}

#[test]
fn an_event_without_fields_is_a_blank_line() {
    assert_eq!(bytes_of(&[]), Ok(Bytes::from_static(b"\n")));
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

async fn timeline_of(script: &[Step], interval: Duration) -> Vec<(u64, String)> {
    let response = Sse::new(stream_of(script.to_vec(), |data| {
        Event::default().data(data)
    }))
    .keep_alive(KeepAlive::new().interval(interval).text("alive"))
    .into_response();
    timeline(response.into_body()).await
}

#[tokio::test(start_paused = true)]
async fn keep_alives_go_out_when_the_stream_is_quiet() {
    let second = Duration::from_secs(1);
    let line = timeline_of(
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
async fn a_failing_stream_ends_with_its_error() {
    let second = Duration::from_secs(1);
    let line = timeline_of(
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
async fn headers_and_frames() {
    let response = Sse::new(
        stream::iter(["1", "2"])
            .map(|data| Ok::<_, Infallible>(Event::default().id(data).data(data))),
    )
    .into_response();
    let seen = ours(response).await;
    assert_eq!(
        seen.line(),
        r#"200 | content-type: text/event-stream / cache-control: no-cache | ["id: 1\ndata: 1\n\n", "id: 2\ndata: 2\n\n"] | 0.."#
    );
    assert_eq!(seen.frames.len(), 2, "one frame per event");
}

#[tokio::test(start_paused = true)]
async fn the_default_keep_alive_is_an_empty_comment() {
    let events = stream::pending::<Result<Event, Infallible>>();
    let mut body = Sse::new(events)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(1)))
        .into_response()
        .into_body();
    let start = Instant::now();
    let frame = body.frame().await.unwrap().unwrap();
    assert_eq!(frame.into_data().unwrap(), ":\n\n");
    assert_eq!(start.elapsed(), Duration::from_secs(1));
}

#[test]
fn a_keep_alive_is_checked_when_it_is_made() {
    assert!(catch_unwind(|| KeepAlive::new().text("a\nb")).is_err());
}
