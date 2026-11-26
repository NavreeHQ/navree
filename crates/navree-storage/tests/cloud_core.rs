//! The core the cloud stores share (`navree_storage::cloud`): object keys
//! that keep their bytes and stay under their bucket, a credential that is
//! fetched again before it expires and once for many callers, requests
//! that are sent again when the store says so, a client that sends plain
//! HTTP only where an endpoint asks for it, and a file whose body stops
//! part-way and is read on from there.

#![cfg(feature = "__cloud")]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use futures_util::stream::BoxStream;
use futures_util::{StreamExt, TryStreamExt};
use http::HeaderMap;
use navree_kernel::BoxFuture;
use navree_storage::cloud::credentials::{Credential, Provider};
use navree_storage::cloud::http::{Client, ERROR_BODY_LIMIT, Request, Response};
use navree_storage::cloud::keys::{check_key, encode_component, encode_key, object_url};
use navree_storage::cloud::retry::{Replay, RetryPolicy, with_backoff};
use navree_storage::cloud::{Error, ErrorKind, xml};
use navree_storage::store::{Method, StoreError, Url};
use percent_encoding::percent_decode_str;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The address of a bucket, addressed in the path.
fn bucket() -> Url {
    Url::parse("https://s3.ap-south-1.amazonaws.com/ludhiana-invoices").unwrap()
}

#[test]
fn a_key_with_spaces_plus_percent_unicode_round_trips_its_encoding() {
    // What each character becomes.
    assert_eq!(
        encode_key("my file+v2 100%.txt"),
        "my%20file%2Bv2%20100%25.txt"
    );
    assert_eq!(encode_key("café/ñ/₹.txt"), "caf%C3%A9/%C3%B1/%E2%82%B9.txt");
    assert_eq!(encode_component("a/b c"), "a%2Fb%20c");

    for key in [
        "tenants/agra/चालान ₹1,00,000 + 18% GST/INV 1101 (final) é.pdf",
        "ਲੁਧਿਆਣਾ/ਰਸੀਦ 42.pdf",
        "a+b c%20d%2Fe",
        "already%2520encoded",
        "trailing slash/",
        "two//slashes",
        "query?and=1&more#fragment",
        "emoji 🧾.png",
    ] {
        let encoded = encode_key(key);
        assert!(
            encoded
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-._~/%".contains(&byte)),
            "{encoded}"
        );
        assert!(
            !encoded.contains('+') && !encoded.contains(' '),
            "{encoded}"
        );
        assert_eq!(percent_decode_str(&encoded).decode_utf8().unwrap(), key);

        // In an address, the path is the bucket's and then the encoded key,
        // and what a store decodes from it is the key, byte for byte.
        let url = object_url(&bucket(), key).unwrap();
        assert_eq!(url.host_str(), Some("s3.ap-south-1.amazonaws.com"));
        assert_eq!(url.path(), format!("/ludhiana-invoices/{encoded}"));
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
        let sent = url.path().strip_prefix("/ludhiana-invoices/").unwrap();
        assert_eq!(
            percent_decode_str(sent).decode_utf8().unwrap().as_bytes(),
            key.as_bytes()
        );

        // As one segment, for a store that takes the key as one.
        let component = encode_component(key);
        assert!(!component.contains('/'), "{component}");
        assert_eq!(percent_decode_str(&component).decode_utf8().unwrap(), key);
    }
}

#[test]
fn a_key_that_climbs_out_is_refused() {
    for key in [
        "..",
        "../other-bucket/secrets.txt",
        "tenants/agra/../../other-bucket/secrets.txt",
        "tenants/agra/..",
        "tenants/agra/./a.txt",
        ".",
        "",
        "a\0b",
        "a\r\nb",
    ] {
        let error = check_key(key).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Key, "{key:?}");
        assert_eq!(
            object_url(&bucket(), key).unwrap_err().kind(),
            ErrorKind::Key,
            "{key:?}"
        );
        // At a store's edge it is a failure, not a missing file, and no
        // request is worth sending again for it.
        assert!(matches!(StoreError::from(error), StoreError::Failed(_)));
        let error = check_key(key).unwrap_err();
        assert!(!error.retryable(Replay::Safe));
    }

    // What only looks like a way out is a key like any other: it stays
    // under the bucket, on the bucket's host, and comes back as it was.
    for key in [
        "//other.example/secrets.txt",
        "/leading/slash",
        "a\\..\\..\\b",
        "%2e%2e/%2e%2e/secrets.txt",
        "..%2f..%2fsecrets.txt",
        "...",
        "a/.../b",
        "..a/b..",
        "@other.example/a",
        "a:b@c/d",
    ] {
        check_key(key).unwrap_or_else(|error| panic!("{key:?}: {error}"));
        for base in [
            "https://s3.ap-south-1.amazonaws.com/ludhiana-invoices",
            "https://ludhiana-invoices.s3.ap-south-1.amazonaws.com",
            "http://127.0.0.1:10000/devstoreaccount1/archive",
        ] {
            let base = Url::parse(base).unwrap();
            let under = format!("{}/", base.path().trim_end_matches('/'));
            let url = object_url(&base, key).unwrap();
            assert_eq!(url.origin(), base.origin(), "{key:?}");
            assert_eq!(url.username(), "", "{key:?}");
            let sent = url
                .path()
                .strip_prefix(&under)
                .unwrap_or_else(|| panic!("{key:?} left {under}: {url}"));
            assert_eq!(percent_decode_str(sent).decode_utf8().unwrap(), key);
        }
    }
}

/// A clock a test moves: `now()` is a fixed start plus the seconds set.
#[derive(Clone)]
struct Hands(Arc<AtomicU64>);

impl Hands {
    fn new() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }

    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH
            + Duration::from_secs(1_795_000_000)
            + Duration::from_secs(self.0.load(Ordering::SeqCst))
    }

    fn set_minutes(&self, minutes: u64) {
        self.0.store(minutes * 60, Ordering::SeqCst);
    }
}

/// A provider of sessions `session-1`, `session-2`, ..., each good for
/// `minutes` from its fetch, which takes `fetch_takes`; and the number of
/// fetches.
fn sessions(
    hands: &Hands,
    minutes: u64,
    fetch_takes: Duration,
) -> (Arc<Provider<String>>, Arc<AtomicUsize>) {
    let fetches = Arc::new(AtomicUsize::new(0));
    let (counted, source_hands, clock_hands) = (fetches.clone(), hands.clone(), hands.clone());
    let provider = Provider::with_clock(
        move || {
            let n = counted.fetch_add(1, Ordering::SeqCst) + 1;
            let expires_at = source_hands.now() + Duration::from_secs(minutes * 60);
            async move {
                tokio::time::sleep(fetch_takes).await;
                Ok(Credential::until(format!("session-{n}"), expires_at))
            }
        },
        move || clock_hands.now(),
    );
    (Arc::new(provider), fetches)
}

/// The secrets `callers` tasks get when they all ask at once.
async fn ask_together(provider: &Arc<Provider<String>>, callers: usize) -> Vec<String> {
    let tasks: Vec<_> = (0..callers)
        .map(|_| {
            let provider = provider.clone();
            tokio::spawn(async move { provider.credential().await.map(|found| found.secret) })
        })
        .collect();
    let mut secrets = Vec::with_capacity(callers);
    for task in tasks {
        secrets.push(task.await.unwrap().unwrap());
    }
    secrets
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_credential_is_refreshed_before_it_expires_and_once_for_many_callers() {
    let hands = Hands::new();
    // A fetch slow enough for every caller to arrive while it is under way.
    let (provider, fetches) = sessions(&hands, 60, Duration::from_millis(50));

    // The first callers, all at once: one fetch, one session for all.
    let secrets = ask_together(&provider, 64).await;
    assert!(
        secrets.iter().all(|secret| secret == "session-1"),
        "{secrets:?}"
    );
    assert_eq!(fetches.load(Ordering::SeqCst), 1);

    // With more than five minutes left it is kept.
    hands.set_minutes(54);
    let secrets = ask_together(&provider, 8).await;
    assert!(
        secrets.iter().all(|secret| secret == "session-1"),
        "{secrets:?}"
    );
    assert_eq!(fetches.load(Ordering::SeqCst), 1);

    // Four minutes before it expires it still works, and is replaced all
    // the same: once, whoever asks.
    hands.set_minutes(56);
    let secrets = ask_together(&provider, 64).await;
    assert!(
        secrets.iter().all(|secret| secret == "session-2"),
        "{secrets:?}"
    );
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
    let renewed = provider.credential().await.unwrap();
    assert_eq!(
        renewed.expires_at,
        Some(hands.now() + Duration::from_secs(60 * 60))
    );
    assert_eq!(fetches.load(Ordering::SeqCst), 2);

    // Neither the provider nor the credential shows the secret.
    let shown = format!("{provider:?} {renewed:?}");
    assert!(!shown.contains("session-2"), "{shown}");
}

/// A store that answers each request with the next of `statuses` (and the
/// last one from then on), with an error body that names the request; and
/// the number of requests it got.
async fn store_answering(statuses: &[u16]) -> (MockServer, Arc<AtomicUsize>) {
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let (statuses, counted) = (statuses.to_vec(), calls.clone());
    Mock::given(any())
        .respond_with(move |_: &wiremock::Request| {
            let call = counted.fetch_add(1, Ordering::SeqCst);
            let status = statuses[call.min(statuses.len() - 1)];
            ResponseTemplate::new(status)
                .set_body_string(format!("<Error><Code>Answer{call}</Code></Error>"))
        })
        .mount(&server)
        .await;
    (server, calls)
}

/// A client for `server`, and the address of an object on it.
fn client_and_url(server: &MockServer) -> (Client, Url) {
    let client = Client::for_endpoint(Some(&server.uri())).unwrap();
    let url = Url::parse(&format!("{}/ludhiana-invoices/INV-1101.pdf", server.uri())).unwrap();
    (client, url)
}

/// A policy whose delays are too short to notice.
fn quick(max_retries: u32) -> RetryPolicy {
    RetryPolicy {
        max_retries,
        first_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(2),
        ..RetryPolicy::default()
    }
}

/// The status of a `GET` of `url`, sent again as `policy` and `replay` say.
async fn get(
    client: &Client,
    url: &Url,
    policy: &RetryPolicy,
    replay: Replay,
) -> navree_storage::cloud::Result<u16> {
    with_backoff(policy, replay, || async move {
        let request = Request::new(Method::GET, url.clone());
        let answer = client.send(request).await?.success().await?;
        Ok(answer.status().as_u16())
    })
    .await
}

#[tokio::test]
async fn throttling_and_5xx_are_retried_and_4xx_is_not() {
    // Throttling, a timeout the store reports and failures of its own:
    // asked again until it answers, even a request that must not arrive
    // twice, since the store says it did not carry it out.
    let (server, calls) = store_answering(&[503, 429, 500, 408, 502, 200]).await;
    let (client, url) = client_and_url(&server);
    let status = get(&client, &url, &quick(10), Replay::IfNotSent).await;
    assert_eq!(status.unwrap(), 200);
    assert_eq!(calls.load(Ordering::SeqCst), 6);

    // A refusal is the answer: asked once.
    for refusal in [400, 401, 403, 404, 409, 412, 416] {
        let (server, calls) = store_answering(&[refusal]).await;
        let (client, url) = client_and_url(&server);
        let error = get(&client, &url, &quick(10), Replay::Safe)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Status, "{refusal}");
        assert_eq!(error.status().map(|status| status.as_u16()), Some(refusal));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "{refusal}");
        // Only a 404 is a missing file.
        let missing = matches!(StoreError::from(error), StoreError::NotFound);
        assert_eq!(missing, refusal == 404, "{refusal}");
    }

    // A store that does not recover is asked once and then as often as
    // the policy allows; the failure is its last answer.
    let (server, calls) = store_answering(&[503]).await;
    let (client, url) = client_and_url(&server);
    let error = get(&client, &url, &quick(3), Replay::Safe)
        .await
        .unwrap_err();
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(error.status().map(|status| status.as_u16()), Some(503));
    assert_eq!(
        error.to_string(),
        "the store answered 503 Service Unavailable"
    );
    // The body is there for the driver to take the provider's code from,
    // and is not part of what the error shows.
    let body = error.body().unwrap();
    assert_eq!(
        xml::parse(body.as_bytes()).unwrap().value("Code"),
        Some("Answer3")
    );
    assert!(!format!("{error} {error:?}").contains("Answer3"));

    // With no retries allowed, the first answer is the last.
    let (server, calls) = store_answering(&[500, 200]).await;
    let (client, url) = client_and_url(&server);
    let error = get(&client, &url, &quick(0), Replay::Safe)
        .await
        .unwrap_err();
    assert_eq!(error.status().map(|status| status.as_u16()), Some(500));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_timeout_is_sent_again_only_when_a_second_arrival_does_no_harm() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3)))
        .mount(&server)
        .await;
    let (client, url) = client_and_url(&server);
    let policy = quick(2);
    for (replay, expected) in [(Replay::IfNotSent, 1), (Replay::Safe, 3)] {
        let attempts = AtomicUsize::new(0);
        let (attempts, client, url) = (&attempts, &client, &url);
        let error = with_backoff(&policy, replay, || async move {
            attempts.fetch_add(1, Ordering::SeqCst);
            let request = Request::new(Method::PUT, url.clone())
                .with_body("a part")
                .with_timeout(Duration::from_millis(100));
            client.send(request).await?.success().await
        })
        .await
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Timeout, "{error:?}");
        assert_eq!(attempts.load(Ordering::SeqCst), expected, "{replay:?}");
    }
}

#[tokio::test]
async fn a_request_sent_again_is_signed_with_a_credential_that_is_still_good() {
    let (server, calls) = store_answering(&[503, 200]).await;
    let (client, url) = client_and_url(&server);
    let hands = Hands::new();
    let (provider, fetches) = sessions(&hands, 10, Duration::ZERO);
    let (provider, client, url, hands) = (&provider, &client, &url, &hands);

    with_backoff(&quick(10), Replay::Safe, || async move {
        let session = provider.credential().await?;
        let request = Request::new(Method::PUT, url.clone())
            .secret("authorization", &format!("Bearer {}", session.secret))?
            .with_body("a part");
        let answer = client.send(request).await?;
        // The answer was long in coming: the session has run out by now.
        hands.set_minutes(11);
        answer.success().await
    })
    .await
    .unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
    let requests = server.received_requests().await.unwrap();
    let signed: Vec<&str> = requests
        .iter()
        .map(|request| request.headers["authorization"].to_str().unwrap())
        .collect();
    assert_eq!(signed, ["Bearer session-1", "Bearer session-2"]);
}

#[tokio::test]
async fn plain_http_is_sent_only_to_the_endpoint_configured_with_it() {
    let (server, calls) = store_answering(&[200]).await;
    let url = Url::parse(&format!("{}/a.txt", server.uri())).unwrap();

    // A client for the provider's own address, for an https endpoint and
    // for another http endpoint: nothing is sent.
    for client in [
        Client::for_endpoint(None).unwrap(),
        Client::for_endpoint(Some("https://127.0.0.1:1")).unwrap(),
        Client::for_endpoint(Some("http://127.0.0.1:9")).unwrap(),
    ] {
        let error = client
            .send(Request::new(Method::GET, url.clone()))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Refused, "{error:?}");
        assert!(!error.retryable(Replay::Safe));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // The client made for this endpoint sends it, and so does the client
    // for the credential services of a machine.
    for client in [
        Client::for_endpoint(Some(&server.uri())).unwrap(),
        Client::for_metadata().unwrap(),
    ] {
        let answer = client
            .send(Request::new(Method::GET, url.clone()))
            .await
            .unwrap();
        assert_eq!(answer.status(), 200);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn an_answer_that_points_elsewhere_is_not_followed() {
    let (elsewhere, elsewhere_calls) = store_answering(&[200]).await;
    let server = MockServer::start().await;
    let location = format!("{}/ludhiana-invoices/INV-1101.pdf", elsewhere.uri());
    Mock::given(any())
        .respond_with(ResponseTemplate::new(307).insert_header("location", location.as_str()))
        .mount(&server)
        .await;
    // A client that may send plain HTTP anywhere still follows nothing.
    let client = Client::for_metadata().unwrap();
    let url = Url::parse(&format!("{}/ludhiana-invoices/INV-1101.pdf", server.uri())).unwrap();
    let request = Request::new(Method::GET, url)
        .secret("authorization", "Bearer session-1")
        .unwrap();

    let answer = client.send(request).await.unwrap();
    assert_eq!(answer.status(), 307);
    assert_eq!(answer.header("location"), Some(location.as_str()));
    let error = answer.success().await.unwrap_err();
    assert_eq!(error.status().map(|status| status.as_u16()), Some(307));
    assert!(!error.retryable(Replay::Safe));
    assert_eq!(elsewhere_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_answer_is_read_whole_within_a_limit_or_in_chunks() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::path("/file"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 1000]))
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path("/failure"))
        .respond_with(ResponseTemplate::new(500).set_body_bytes(vec![b'e'; 100_000]))
        .mount(&server)
        .await;
    let client = Client::for_endpoint(Some(&server.uri())).unwrap();
    let get = |path: &str| {
        let url = Url::parse(&format!("{}{path}", server.uri())).unwrap();
        client.send(Request::new(Method::GET, url))
    };

    let answer = get("/file").await.unwrap();
    assert_eq!(answer.declared_length(), Some(1000));
    assert_eq!(answer.bytes().await.unwrap().len(), 1000);
    assert_eq!(
        get("/file").await.unwrap().bytes_up_to(1000).await.unwrap(),
        vec![b'x'; 1000]
    );
    let error = get("/file")
        .await
        .unwrap()
        .bytes_up_to(999)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Answer);
    assert_eq!(error.to_string(), "the answer is longer than 999 bytes");
    let chunks: Vec<Bytes> = get("/file")
        .await
        .unwrap()
        .stream()
        .try_collect()
        .await
        .unwrap();
    assert_eq!(chunks.concat(), vec![b'x'; 1000]);

    // Of an answer that is a failure, only the start is kept.
    let error = get("/failure").await.unwrap().success().await.unwrap_err();
    assert_eq!(error.status().map(|status| status.as_u16()), Some(500));
    assert_eq!(error.body().unwrap().len(), ERROR_BODY_LIMIT);
}

#[tokio::test]
async fn a_request_that_writes_says_its_length_and_one_that_asks_says_none() {
    let (server, _calls) = store_answering(&[200]).await;
    let (client, url) = client_and_url(&server);
    for (method, body) in [
        (Method::PUT, ""),
        (Method::PUT, "a part"),
        (Method::POST, ""),
        (Method::GET, ""),
        (Method::HEAD, ""),
        (Method::DELETE, ""),
    ] {
        let request = Request::new(method, url.clone()).with_body(body);
        client.send(request).await.unwrap();
    }
    // An empty file is written with a length of 0, which the providers
    // ask for.
    let requests = server.received_requests().await.unwrap();
    let lengths: Vec<Option<&str>> = requests
        .iter()
        .map(|request| {
            let length = request.headers.get("content-length")?;
            Some(length.to_str().unwrap())
        })
        .collect();
    assert_eq!(lengths, [Some("0"), Some("6"), Some("0"), None, None, None]);
}

/// A file of 1000 bytes whose every byte tells where it is, so that bytes
/// from another place show.
fn ledger() -> Vec<u8> {
    (0..1000u32).map(|at| (at % 251) as u8).collect()
}

/// The head of the answer to a `GET` of the whole ledger.
const WHOLE: &str = "HTTP/1.1 200 OK\r\ncontent-length: 1000\r\netag: \"v1\"\r\n\r\n";

/// The address of a store that answers its one request with `head` and
/// `body` and then hangs up, whatever length the head declares: a body
/// that stops part-way.
async fn hangs_up_after(head: &str, body: &[u8]) -> Url {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (head, body) = (head.to_owned(), body.to_vec());
    tokio::spawn(async move {
        let Ok((mut socket, _)) = listener.accept().await else {
            return;
        };
        // The whole request first: hanging up with something unread would
        // reset the connection, and a reset can take sent bytes with it.
        let mut request = Vec::new();
        let mut buffer = [0u8; 2048];
        while !request.windows(4).any(|end| end == b"\r\n\r\n") {
            match socket.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(read) => request.extend_from_slice(&buffer[..read]),
            }
        }
        let _ = socket.write_all(head.as_bytes()).await;
        let _ = socket.write_all(&body).await;
        let _ = socket.shutdown().await;
    });
    Url::parse(&format!("http://{address}/ludhiana-invoices/ledger.csv")).unwrap()
}

/// How a store answers a request for the rest of `file`: the bytes its
/// `Range` names, as a `206` that says which they are, with the entity tag
/// `etag`.
fn the_rest_of(
    file: Vec<u8>,
    etag: &'static str,
) -> impl Fn(&wiremock::Request) -> ResponseTemplate + Send + Sync + 'static {
    move |request: &wiremock::Request| {
        let range = request
            .headers
            .get("range")
            .and_then(|range| range.to_str().ok())
            .and_then(|range| range.strip_prefix("bytes="))
            .and_then(|range| range.split_once('-'));
        let Some((first, last)) = range else {
            return ResponseTemplate::new(400);
        };
        let first = first.parse::<usize>().ok();
        let last = match last {
            "" => Some(file.len() - 1),
            last => last.parse::<usize>().ok(),
        };
        match (first, last) {
            (Some(first), Some(last)) if first <= last && last < file.len() => {
                ResponseTemplate::new(206)
                    .insert_header(
                        "content-range",
                        format!("bytes {first}-{last}/{}", file.len()),
                    )
                    .insert_header("etag", etag)
                    .set_body_bytes(file[first..=last].to_vec())
            }
            _ => ResponseTemplate::new(416),
        }
    }
}

/// A store at which `answer` answers every request, and the address of the
/// ledger on it.
async fn store_with(answer: impl wiremock::Respond + 'static) -> (MockServer, Url) {
    let server = MockServer::start().await;
    Mock::given(any()).respond_with(answer).mount(&server).await;
    let url = Url::parse(&format!("{}/ludhiana-invoices/ledger.csv", server.uri())).unwrap();
    (server, url)
}

/// The answer to a request, once it comes.
type Answer = BoxFuture<'static, navree_storage::cloud::Result<Response>>;

/// How a driver asks for the rest of a file: one `GET` of `url` with the
/// headers it is handed, the answer given as it comes.
fn asking(client: &Client, url: &Url) -> impl FnMut(HeaderMap) -> Answer + Send + 'static {
    let (client, url) = (client.clone(), url.clone());
    move |rest: HeaderMap| {
        let (client, url) = (client.clone(), url.clone());
        Box::pin(async move {
            let mut request = Request::new(Method::GET, url);
            request.headers.extend(rest);
            client.send(request).await
        })
    }
}

/// The answer to a `GET` of `url`: a success whose body is not read yet.
async fn first_answer(client: &Client, url: Url) -> Response {
    let answer = client.send(Request::new(Method::GET, url)).await.unwrap();
    answer.success().await.unwrap()
}

/// What a stream gave before it ended, and the failure it ended with if it
/// did.
async fn drain(
    mut body: BoxStream<'static, navree_storage::cloud::Result<Bytes>>,
) -> (Vec<u8>, Option<Error>) {
    let mut read = Vec::new();
    loop {
        match body.next().await {
            Some(Ok(chunk)) => read.extend_from_slice(&chunk),
            Some(Err(failure)) => return (read, Some(failure)),
            None => return (read, None),
        }
    }
}

/// The `Range` of each request `server` got, in order.
async fn ranges_asked(server: &MockServer) -> Vec<String> {
    let requests = server.received_requests().await.unwrap();
    requests
        .iter()
        .map(|request| request.headers["range"].to_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_file_whose_body_stops_is_read_on_from_where_it_stopped() {
    let file = ledger();
    // Plain HTTP to more than one address: the two halves of a store.
    let client = Client::for_metadata().unwrap();
    let (rest, rest_url) = store_with(the_rest_of(file.clone(), "\"v1\"")).await;

    // The whole file, on a connection that drops after 400 of 1000 bytes.
    let first = first_answer(&client, hangs_up_after(WHOLE, &file[..400]).await).await;
    assert_eq!(first.declared_length(), Some(1000));
    let body = first.resuming_stream(&quick(3), asking(&client, &rest_url));
    let (read, failure) = drain(body).await;
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(read, file);

    // A range of it, bytes 100 to 899, that drops after 300 bytes.
    let head = "HTTP/1.1 206 Partial Content\r\ncontent-length: 800\r\n\
                content-range: bytes 100-899/1000\r\netag: \"v1\"\r\n\r\n";
    let first = first_answer(&client, hangs_up_after(head, &file[100..400]).await).await;
    let body = first.resuming_stream(&quick(3), asking(&client, &rest_url));
    let (read, failure) = drain(body).await;
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(read, &file[100..900]);

    // The rest was asked for once each time: from the byte after the last
    // that arrived, to the end of what was first asked, of the same file.
    assert_eq!(ranges_asked(&rest).await, ["bytes=400-", "bytes=400-899"]);
    let requests = rest.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|request| request.headers["if-match"] == "\"v1\"")
    );
}

#[tokio::test]
async fn a_file_replaced_while_it_is_read_ends_in_a_failure_and_is_never_mixed() {
    let file = ledger();
    let client = Client::for_metadata().unwrap();

    // The store says the file there is another one now.
    let (rest, rest_url) = store_with(ResponseTemplate::new(412)).await;
    let first = first_answer(&client, hangs_up_after(WHOLE, &file[..400]).await).await;
    let body = first.resuming_stream(&quick(3), asking(&client, &rest_url));
    let (read, failure) = drain(body).await;
    assert_eq!(read, &file[..400]);
    let failure = failure.unwrap();
    assert_eq!(failure.kind(), ErrorKind::Status);
    assert_eq!(failure.status().map(|status| status.as_u16()), Some(412));
    assert_eq!(ranges_asked(&rest).await, ["bytes=400-"], "asked once");

    // A store that takes no notice of `If-Match` and sends the rest of the
    // other file: its entity tag tells.
    let other: Vec<u8> = file.iter().rev().copied().collect();
    let (rest, rest_url) = store_with(the_rest_of(other, "\"v2\"")).await;
    let first = first_answer(&client, hangs_up_after(WHOLE, &file[..400]).await).await;
    let body = first.resuming_stream(&quick(3), asking(&client, &rest_url));
    let (read, failure) = drain(body).await;
    assert_eq!(read, &file[..400]);
    let failure = failure.unwrap();
    assert_eq!(failure.kind(), ErrorKind::Answer);
    assert_eq!(failure.to_string(), "the file changed while it was read");
    assert_eq!(ranges_asked(&rest).await, ["bytes=400-"], "asked once");
}

#[tokio::test]
async fn an_answer_that_is_not_the_rest_asked_for_ends_the_stream() {
    let file = ledger();
    let client = Client::for_metadata().unwrap();
    let whole_again = ResponseTemplate::new(200)
        .insert_header("etag", "\"v1\"")
        .set_body_bytes(file.clone());
    let from_the_start = ResponseTemplate::new(206)
        .insert_header("etag", "\"v1\"")
        .insert_header("content-range", "bytes 0-999/1000")
        .set_body_bytes(file.clone());
    let which_bytes_unsaid = ResponseTemplate::new(206)
        .insert_header("etag", "\"v1\"")
        .set_body_bytes(file[400..].to_vec());
    for (answer, says) in [
        (
            whole_again,
            "the store answered 200 OK when asked for the rest of a file",
        ),
        (
            from_the_start,
            "the rest of a file was answered with other bytes than were asked for",
        ),
        (
            which_bytes_unsaid,
            "the rest of a file was answered without saying which bytes it is",
        ),
    ] {
        let (rest, rest_url) = store_with(answer).await;
        let first = first_answer(&client, hangs_up_after(WHOLE, &file[..400]).await).await;
        let body = first.resuming_stream(&quick(3), asking(&client, &rest_url));
        let (read, failure) = drain(body).await;
        assert_eq!(read, &file[..400], "{says}");
        let failure = failure.unwrap();
        assert_eq!(failure.kind(), ErrorKind::Answer, "{says}");
        assert_eq!(failure.to_string(), says);
        assert_eq!(ranges_asked(&rest).await, ["bytes=400-"], "asked once");
    }
}

#[tokio::test]
async fn the_rest_is_asked_for_as_often_as_the_policy_allows_and_no_more() {
    let file = ledger();
    let client = Client::for_metadata().unwrap();

    // Busy twice, then the rest.
    let calls = Arc::new(AtomicUsize::new(0));
    let (counted, rest_of) = (calls.clone(), the_rest_of(file.clone(), "\"v1\""));
    let busy_twice = move |request: &wiremock::Request| {
        if counted.fetch_add(1, Ordering::SeqCst) < 2 {
            ResponseTemplate::new(503)
        } else {
            rest_of(request)
        }
    };
    let (rest, rest_url) = store_with(busy_twice).await;
    let first = first_answer(&client, hangs_up_after(WHOLE, &file[..400]).await).await;
    let body = first.resuming_stream(&quick(3), asking(&client, &rest_url));
    let (read, failure) = drain(body).await;
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(read, file);
    assert_eq!(ranges_asked(&rest).await, ["bytes=400-"; 3]);

    // Busy for good: asked as often as the policy allows, and the failure
    // is the store's last answer.
    let (rest, rest_url) = store_with(ResponseTemplate::new(503)).await;
    let first = first_answer(&client, hangs_up_after(WHOLE, &file[..400]).await).await;
    let body = first.resuming_stream(&quick(3), asking(&client, &rest_url));
    let (read, failure) = drain(body).await;
    assert_eq!(read, &file[..400]);
    let failure = failure.unwrap();
    assert_eq!(failure.status().map(|status| status.as_u16()), Some(503));
    assert_eq!(ranges_asked(&rest).await, ["bytes=400-"; 3]);

    // A policy with no retries asks for nothing: the failure is the body's.
    let (rest, rest_url) = store_with(the_rest_of(file.clone(), "\"v1\"")).await;
    let first = first_answer(&client, hangs_up_after(WHOLE, &file[..400]).await).await;
    let body = first.resuming_stream(&quick(0), asking(&client, &rest_url));
    let (read, failure) = drain(body).await;
    assert_eq!(read, &file[..400]);
    assert_eq!(failure.unwrap().status(), None);
    assert!(ranges_asked(&rest).await.is_empty());
}

#[tokio::test]
async fn a_file_without_a_strong_entity_tag_is_not_asked_for_again() {
    let file = ledger();
    let client = Client::for_metadata().unwrap();
    for head in [
        "HTTP/1.1 200 OK\r\ncontent-length: 1000\r\n\r\n",
        "HTTP/1.1 200 OK\r\ncontent-length: 1000\r\netag: W/\"v1\"\r\n\r\n",
    ] {
        let (rest, rest_url) = store_with(the_rest_of(file.clone(), "\"v1\"")).await;
        let first = first_answer(&client, hangs_up_after(head, &file[..400]).await).await;
        let body = first.resuming_stream(&quick(3), asking(&client, &rest_url));
        let (read, failure) = drain(body).await;
        // Nothing would say the rest is of the same file: the body's own
        // failure ends the stream.
        assert_eq!(read, &file[..400]);
        let failure = failure.unwrap();
        assert_ne!(failure.kind(), ErrorKind::Status, "{failure:?}");
        assert!(ranges_asked(&rest).await.is_empty(), "{head}");
    }
}

#[tokio::test]
async fn a_file_that_keeps_arriving_is_read_however_often_its_body_stops() {
    let file = ledger();
    let client = Client::for_metadata().unwrap();
    let first_url = hangs_up_after(WHOLE, &file[..400]).await;
    // The rest stops too, after 300 more bytes.
    let head = "HTTP/1.1 206 Partial Content\r\ncontent-length: 600\r\n\
                content-range: bytes 400-999/1000\r\netag: \"v1\"\r\n\r\n";
    let second_url = hangs_up_after(head, &file[400..700]).await;
    let (rest, rest_url) = store_with(the_rest_of(file.clone(), "\"v1\"")).await;

    let asked = Arc::new(AtomicUsize::new(0));
    let again = {
        let (client, asked) = (client.clone(), asked.clone());
        move |headers: HeaderMap| {
            let first_time = asked.fetch_add(1, Ordering::SeqCst) == 0;
            let url = if first_time { &second_url } else { &rest_url };
            let (client, url) = (client.clone(), url.clone());
            async move {
                let mut request = Request::new(Method::GET, url);
                request.headers.extend(headers);
                client.send(request).await
            }
        }
    };

    // One retry is all the policy allows, and each stop gets it afresh once
    // bytes have arrived in between.
    let first = first_answer(&client, first_url).await;
    let (read, failure) = drain(first.resuming_stream(&quick(1), again)).await;
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(read, file);
    assert_eq!(asked.load(Ordering::SeqCst), 2);
    assert_eq!(ranges_asked(&rest).await, ["bytes=700-"]);
}

#[test]
fn xml_parse_errors_have_no_source_and_hide_malformed_tags_and_entities() {
    for body in [
        "<Credentials><SecretAccessKey>harpreet-secret-99</SecretAccessKey></Wrong>",
        "<Credentials><harpreet-secret-99>1</other></Credentials>",
        "<Credentials>&#xharpreet-secret-99;</Credentials>",
        "<Credentials>&#harpreet-secret-99;</Credentials>",
    ] {
        let err = xml::parse(body.as_bytes()).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Answer);
        assert!(std::error::Error::source(&err).is_none(), "{err:?}");
        let rendered = format!("{err} {err:?}");
        assert!(!rendered.contains("harpreet-secret-99"), "{rendered}");
    }
}

#[tokio::test]
async fn credential_for_refreshes_early_when_link_lifetime_exceeds_remaining_ttl() {
    let hands = Hands::new();
    let (provider, fetches) = sessions(&hands, 60, Duration::ZERO);

    // Initial fetch: 60 minutes left, covers a 30-minute signed link.
    assert_eq!(
        provider
            .credential_for(Duration::from_secs(30 * 60))
            .await
            .unwrap()
            .secret,
        "session-1"
    );
    assert_eq!(fetches.load(Ordering::SeqCst), 1);

    // 45 minutes later: 15 minutes left (> 5m REFRESH_BEFORE), so normal
    // requests keep `session-1`, whereas a 30-minute signed link refreshes
    // early to `session-2`.
    hands.set_minutes(45);
    assert_eq!(provider.credential().await.unwrap().secret, "session-1");
    assert_eq!(fetches.load(Ordering::SeqCst), 1);

    assert_eq!(
        provider
            .credential_for(Duration::from_secs(30 * 60))
            .await
            .unwrap()
            .secret,
        "session-2"
    );
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn ranged_bytes_validates_content_range_and_bounds_200_ok_bodies() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::path("/ok-206"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-range", "bytes 2-5/10")
                .set_body_bytes(b"cdEF".to_vec()),
        )
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path("/bad-206"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-range", "bytes 0-3/10")
                .set_body_bytes(b"abcd".to_vec()),
        )
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path("/small-200"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"0123456789".to_vec()))
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path("/huge-200"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'Z'; 100_000]))
        .mount(&server)
        .await;

    let client = Client::for_endpoint(Some(&server.uri())).unwrap();
    let fetch = |path: &str| {
        let url = Url::parse(&format!("{}{path}", server.uri())).unwrap();
        client.send(Request::new(Method::GET, url))
    };

    assert_eq!(
        fetch("/ok-206")
            .await
            .unwrap()
            .ranged_bytes(2..6)
            .await
            .unwrap(),
        "cdEF"
    );
    assert_eq!(
        fetch("/bad-206")
            .await
            .unwrap()
            .ranged_bytes(2..6)
            .await
            .unwrap_err()
            .kind(),
        ErrorKind::Answer
    );
    assert_eq!(
        fetch("/small-200")
            .await
            .unwrap()
            .ranged_bytes(2..6)
            .await
            .unwrap(),
        "2345"
    );
    // An 8-byte sniff against a 100 KiB 200-OK body (> end + 64 KiB) fails
    // before reading the whole payload into memory.
    assert_eq!(
        fetch("/huge-200")
            .await
            .unwrap()
            .ranged_bytes(0..8)
            .await
            .unwrap_err()
            .kind(),
        ErrorKind::Answer
    );
}

#[tokio::test]
async fn prefixed_store_list_skips_exact_directory_marker_keys() {
    use chrono::Utc;
    use navree_storage::store::{Meta, PrefixedStore, Store, StoreResult, Upload};

    struct MarkerStore;

    impl std::fmt::Display for MarkerStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("MarkerStore")
        }
    }

    impl Store for MarkerStore {
        fn put<'a>(&'a self, _: &'a str, _: Bytes) -> BoxFuture<'a, StoreResult<()>> {
            unimplemented!()
        }
        fn begin_upload<'a>(&'a self, _: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>> {
            unimplemented!()
        }
        fn read<'a>(
            &'a self,
            _: &'a str,
            _: Option<std::ops::Range<u64>>,
        ) -> BoxFuture<'a, StoreResult<Bytes>> {
            unimplemented!()
        }
        fn open<'a>(
            &'a self,
            _: &'a str,
        ) -> BoxFuture<'a, StoreResult<navree_storage::store::Object>> {
            unimplemented!()
        }
        fn head<'a>(&'a self, _: &'a str) -> BoxFuture<'a, StoreResult<Meta>> {
            unimplemented!()
        }
        fn delete<'a>(&'a self, _: &'a str) -> BoxFuture<'a, StoreResult<()>> {
            unimplemented!()
        }
        fn list<'a>(&'a self, _: &'a str) -> BoxFuture<'a, StoreResult<Vec<Meta>>> {
            Box::pin(async {
                Ok(vec![
                    Meta {
                        path: "tenants/agra/".to_owned(),
                        size: 0,
                        last_modified: Utc::now(),
                    },
                    Meta {
                        path: "tenants/agra/challan.pdf".to_owned(),
                        size: 42,
                        last_modified: Utc::now(),
                    },
                ])
            })
        }
    }

    let prefixed = PrefixedStore::new(Arc::new(MarkerStore), "tenants/agra");
    let listed = prefixed.list("").await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].path, "challan.pdf");
}
