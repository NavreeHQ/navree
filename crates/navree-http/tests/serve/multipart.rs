//! A form from a client that takes its time: the server's pace on a body
//! holds for `Multipart`, which reads as it goes.

use std::time::{Duration, Instant};

use http::Method;
use navree_http::ServerOptions;
use navree_http::extract::Multipart;
use navree_http::routing::Router;
use tokio::io::AsyncWriteExt;

use crate::support::{Dribble, connect, dribble, no_limits, read_until_closed, start};

const LIMIT_MS: u64 = 300;
const LIMIT: Duration = Duration::from_millis(LIMIT_MS);

fn app() -> Router {
    Router::new().route(Method::POST, "/form", |mut form: Multipart| async move {
        while let Some(mut field) = form.next_field().await? {
            while field.chunk().await?.is_some() {}
        }
        Ok::<_, navree_http::extract::MultipartError>("read")
    })
}

fn pace(rate: Option<u64>) -> ServerOptions {
    ServerOptions {
        body_read_timeout_ms: Some(LIMIT_MS),
        body_min_bytes_per_sec: rate,
        ..no_limits()
    }
}

const HEAD: &str = "Content-Type: multipart/form-data; boundary=b";

/// Stopped in the preamble, in a part's head, and in a value.
#[tokio::test]
async fn a_form_that_stops_arriving_is_answered_408_and_the_connection_closes() {
    let server = start(app(), pace(None)).await;
    for body in [
        "-",
        "--b\r\nContent-Disp",
        "--b\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nhalf a val",
    ] {
        let started = Instant::now();
        let mut client = connect(server.addr).await;
        let request = format!(
            "POST /form HTTP/1.1\r\nHost: test\r\n{HEAD}\r\nContent-Length: 1000\r\n\r\n{body}"
        );
        client.write_all(request.as_bytes()).await.unwrap();
        let response = read_until_closed(&mut client).await;
        assert!(response.starts_with("HTTP/1.1 408"), "{body}: {response}");
        assert!(started.elapsed() >= LIMIT, "{:?}", started.elapsed());
    }
}

#[tokio::test]
async fn a_form_dripping_below_the_minimum_rate_is_cut_off_though_it_never_stalls() {
    let server = start(app(), pace(Some(1_000_000))).await;
    let mut client = connect(server.addr).await;
    let request =
        format!("POST /form HTTP/1.1\r\nHost: test\r\n{HEAD}\r\nContent-Length: 200\r\n\r\n");
    client.write_all(request.as_bytes()).await.unwrap();
    let ended = dribble(&mut client, &[b'x'; 200], Duration::from_millis(50)).await;
    let Dribble::Closed(seen) = ended else {
        panic!("the server let a dripping form finish");
    };
    assert!(seen.starts_with("HTTP/1.1 408"), "{seen}");
}

#[tokio::test]
async fn a_form_at_a_decent_pace_is_read() {
    let server = start(app(), pace(None)).await;
    let mut client = connect(server.addr).await;
    let body = "--b\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n--b--\r\n";
    let request = format!(
        "POST /form HTTP/1.1\r\nHost: test\r\nConnection: close\r\n{HEAD}\r\n\
         Content-Length: {}\r\n\r\n",
        body.len()
    );
    client.write_all(request.as_bytes()).await.unwrap();
    for piece in body.as_bytes().chunks(8) {
        tokio::time::sleep(Duration::from_millis(20)).await;
        client.write_all(piece).await.unwrap();
    }
    let response = read_until_closed(&mut client).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("read"), "{response}");
}
