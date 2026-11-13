//! The native app push drivers against a local stand-in for each service:
//! Apple's push service ([`ApnsPush`]) and Firebase Cloud Messaging
//! ([`FcmPush`]). The answers are the ones the services document.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeDelta, TimeZone, Utc};
use navree_contracts::{Clock, FixedClock, Secret};
use navree_crypto::jose::{Jws, SigningKey};
use navree_notify::{
    ApnsPush, ChannelError, DeviceTarget, FcmPush, PushDriver, PushEnvironment, PushMessage,
    PushOptions, PushPlatform, Urgency,
};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::drivers::local_client;

/// A device token as Apple gives it.
const APPLE_TOKEN: &str = "740f4707bebcf74f9b7c25d48e3358945f6aa01da5ddb387462c7eaf61bb78ad";

/// A registration token as Firebase gives it.
const FIREBASE_TOKEN: &str = "dQw4w9WgXcQ:APA91bHun4MxP5egoKMwt2KZFBaFUH-1RYqx-w0rk3l2";

/// The RSA key of the test service account.
const SERVICE_ACCOUNT_KEY: &str = include_str!("../fixtures/rsa-2048.pem");

fn clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 11, 12, 9, 30, 0).unwrap(),
    ))
}

fn header_of<'a>(request: &'a Request, name: &str) -> &'a str {
    request
        .headers
        .get(name)
        .unwrap_or_else(|| panic!("header {name}"))
        .to_str()
        .unwrap()
}

fn bearer(request: &Request) -> String {
    header_of(request, "authorization")
        .strip_prefix("Bearer ")
        .or_else(|| header_of(request, "authorization").strip_prefix("bearer "))
        .expect("a bearer token")
        .to_owned()
}

// ---------------------------------------------------------------------
// Apple
// ---------------------------------------------------------------------

/// Apple's side: the team key, and a driver pointed at `server` for both
/// environments.
struct Apple {
    key: SigningKey,
    driver: ApnsPush,
}

fn apple(server: &MockServer, clock: Arc<FixedClock>) -> Apple {
    let key = SigningKey::generate_es256();
    let mut options = PushOptions::default();
    options.apns.team_id = "DEF123GHIJ".to_owned();
    options.apns.key_id = "ABC123DEFG".to_owned();
    options.apns.key = Secret::new(key.to_pkcs8_pem().unwrap());
    options.apns.bundle_ids = vec!["com.acme.crm".to_owned()];
    options.apns.endpoint = Some(server.uri());
    options.apns.sandbox_endpoint = Some(format!("{}/sandbox", server.uri()));
    let driver = ApnsPush::new(&options)
        .unwrap()
        .with_client(local_client())
        .with_clock(clock);
    Apple { key, driver }
}

fn iphone() -> DeviceTarget {
    DeviceTarget::new(PushPlatform::Ios, APPLE_TOKEN, "com.acme.crm")
}

fn accepted() -> ResponseTemplate {
    ResponseTemplate::new(200).insert_header("apns-id", "eabeae54-14a8-11e5-b60b-1697f925ec7b")
}

fn apple_error(status: u16, reason: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "reason": reason }))
}

#[tokio::test]
async fn apns_sends_with_the_topic_type_priority_and_expiry() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/3/device/{APPLE_TOKEN}")))
        .and(header("apns-topic", "com.acme.crm"))
        .and(header("apns-push-type", "alert"))
        .and(header("apns-priority", "10"))
        .and(header("apns-collapse-id", "deal-42"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    let clock = clock();
    let apple = apple(&server, clock.clone());

    let message = PushMessage::new("Deal won")
        .body("Agra renewal was won by Gurpreet Kaur")
        .href("/deals/42")
        .badge(2)
        .collapse("deal-42")
        .ttl_secs(3600);
    let sent = apple.driver.send(&iphone(), &message).await.unwrap();

    assert_eq!(sent.provider, "apns");
    assert_eq!(
        sent.message_id.as_deref(),
        Some("eabeae54-14a8-11e5-b60b-1697f925ec7b")
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    let expiry = (clock.now() + TimeDelta::hours(1)).timestamp();
    assert_eq!(header_of(request, "apns-expiration"), expiry.to_string());
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(
        body,
        json!({
            "aps": {
                "alert": { "title": "Deal won", "body": "Agra renewal was won by Gurpreet Kaur" },
                "badge": 2,
            },
            "href": "/deals/42",
        })
    );
}

#[tokio::test]
async fn apns_provider_token_is_es256_with_kid_and_iss() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    let clock = clock();
    let apple = apple(&server, clock.clone());

    apple
        .driver
        .send(&iphone(), &PushMessage::new("Hi"))
        .await
        .unwrap();

    let request = &server.received_requests().await.unwrap()[0];
    let jws = Jws::parse(&bearer(request)).unwrap();
    jws.verify(&apple.key.verifying_key()).unwrap();
    assert_eq!(jws.header().kid.as_deref(), Some("ABC123DEFG"));
    let claims: Value = jws.claims().unwrap();
    assert_eq!(
        claims,
        json!({ "iss": "DEF123GHIJ", "iat": clock.now().timestamp() })
    );
}

#[tokio::test]
async fn apns_reuses_its_token_and_renews_it_after_45_minutes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    let clock = clock();
    let apple = apple(&server, clock.clone());
    let message = PushMessage::new("Hi");

    apple.driver.send(&iphone(), &message).await.unwrap();
    clock.advance(Duration::from_secs(30 * 60));
    apple.driver.send(&iphone(), &message).await.unwrap();
    clock.advance(Duration::from_secs(16 * 60));
    apple.driver.send(&iphone(), &message).await.unwrap();

    let tokens: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(bearer)
        .collect();
    assert_eq!(
        tokens[0], tokens[1],
        "a token younger than 45 minutes is reused"
    );
    assert_ne!(tokens[1], tokens[2], "an older one is signed again");
}

#[tokio::test]
async fn apns_renews_an_expired_provider_token_and_retries_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(apple_error(403, "ExpiredProviderToken"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    let apple = apple(&server, clock());

    let sent = apple
        .driver
        .send(&iphone(), &PushMessage::new("Hi"))
        .await
        .unwrap();

    assert_eq!(sent.provider, "apns");
    let tokens: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(bearer)
        .collect();
    assert_eq!(tokens.len(), 2);
    assert_ne!(tokens[0], tokens[1], "the stale token was replaced");

    // A second refusal of the new token is not retried again.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(apple_error(403, "ExpiredProviderToken"))
        .mount(&server)
        .await;
    let apple = self::apple(&server, clock());
    let error = apple
        .driver
        .send(&iphone(), &PushMessage::new("Hi"))
        .await
        .unwrap_err();
    assert!(
        matches!(&error, ChannelError::Refused { status: 403, code: Some(code), .. } if code == "ExpiredProviderToken"),
        "{error:?}"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn apns_unregistered_and_bad_tokens_are_gone() {
    for answer in [
        ResponseTemplate::new(410)
            .set_body_json(json!({ "reason": "Unregistered", "timestamp": 1_762_934_400_000_u64 })),
        apple_error(410, "ExpiredToken"),
        apple_error(400, "BadDeviceToken"),
        apple_error(400, "DeviceTokenNotForTopic"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(answer)
            .mount(&server)
            .await;
        let error = apple(&server, clock())
            .driver
            .send(&iphone(), &PushMessage::new("Hi"))
            .await
            .unwrap_err();
        assert!(matches!(error, ChannelError::Gone), "{error:?}");
    }
}

#[tokio::test]
async fn apns_throttling_and_server_errors_are_refusals() {
    for (status, reason) in [
        (429, "TooManyRequests"),
        (500, "InternalServerError"),
        (503, "ServiceUnavailable"),
        (400, "BadTopic"),
        (403, "InvalidProviderToken"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(apple_error(status, reason))
            .mount(&server)
            .await;
        let error = apple(&server, clock())
            .driver
            .send(&iphone(), &PushMessage::new("Hi"))
            .await
            .unwrap_err();
        match error {
            ChannelError::Refused {
                provider,
                status: got,
                code,
                ..
            } => {
                assert_eq!(provider, "apns");
                assert_eq!(got, status);
                assert_eq!(code.as_deref(), Some(reason));
            }
            other => panic!("{reason}: {other:?}"),
        }
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "{reason} is not retried by the driver"
        );
    }
}

#[tokio::test]
async fn apns_refuses_a_payload_over_4096_bytes_before_sending() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    let apple = apple(&server, clock());

    let message = PushMessage::new("Order update").body("ਪ".repeat(1500));
    let error = apple.driver.send(&iphone(), &message).await.unwrap_err();

    assert!(
        matches!(&error, ChannelError::InvalidMessage(m) if m.contains("4096")),
        "{error:?}"
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn apns_refuses_a_link_outside_the_app_before_sending() {
    let server = MockServer::start().await;
    let apple = apple(&server, clock());
    for href in [
        "https://evil.example/",
        "//evil.example",
        "/%2F%2Fevil.example",
    ] {
        let error = apple
            .driver
            .send(&iphone(), &PushMessage::new("Hi").href(href))
            .await
            .unwrap_err();
        assert!(matches!(error, ChannelError::InvalidMessage(_)), "{href}");
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn apns_sends_a_silent_push_in_the_background() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("apns-push-type", "background"))
        .and(header("apns-priority", "5"))
        .respond_with(accepted())
        .mount(&server)
        .await;
    let apple = apple(&server, clock());

    apple
        .driver
        .send(
            &iphone(),
            &PushMessage::silent()
                .data("refresh", "inbox")
                .urgency(Urgency::High)
                .ttl_secs(0),
        )
        .await
        .unwrap();

    let request = &server.received_requests().await.unwrap()[0];
    assert_eq!(header_of(request, "apns-expiration"), "0");
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(
        body,
        json!({ "aps": { "content-available": 1 }, "refresh": "inbox" })
    );
}

#[tokio::test]
async fn apns_sandbox_devices_go_to_the_sandbox() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/sandbox/3/device/{APPLE_TOKEN}")))
        .respond_with(accepted())
        .mount(&server)
        .await;
    let apple = apple(&server, clock());

    apple
        .driver
        .send(
            &iphone().environment(PushEnvironment::Sandbox),
            &PushMessage::new("Hi"),
        )
        .await
        .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn apns_sends_only_to_ios_devices() {
    let server = MockServer::start().await;
    let apple = apple(&server, clock());
    assert!(apple.driver.serves(PushPlatform::Ios));
    assert!(!apple.driver.serves(PushPlatform::Android));
    let android = DeviceTarget::new(PushPlatform::Android, FIREBASE_TOKEN, "com.acme.crm");
    assert!(matches!(
        apple.driver.send(&android, &PushMessage::new("Hi")).await,
        Err(ChannelError::InvalidRecipient(_))
    ));
}

// ---------------------------------------------------------------------
// Firebase
// ---------------------------------------------------------------------

/// The service account's key file, its token endpoint at `server`.
fn service_account() -> String {
    json!({
        "type": "service_account",
        "project_id": "acme-crm",
        "private_key_id": "a1b2c3d4e5",
        "private_key": SERVICE_ACCOUNT_KEY,
        "client_email": "push@acme-crm.iam.gserviceaccount.com",
        "client_id": "105589345678901234567",
        "token_uri": "https://oauth2.googleapis.com/token",
    })
    .to_string()
}

fn firebase(server: &MockServer, clock: Arc<FixedClock>) -> FcmPush {
    let mut options = PushOptions::default();
    options.fcm.service_account = Secret::new(service_account());
    options.fcm.packages = vec!["com.acme.crm".to_owned()];
    options.fcm.endpoint = Some(server.uri());
    options.fcm.token_url = Some(format!("{}/token", server.uri()));
    FcmPush::new(&options)
        .unwrap()
        .with_client(local_client())
        .with_clock(clock)
}

fn pixel() -> DeviceTarget {
    DeviceTarget::new(PushPlatform::Android, FIREBASE_TOKEN, "com.acme.crm")
}

/// Google's token endpoint, handing out `token`.
async fn token_endpoint(server: &MockServer, token: &str, times: Option<u64>) {
    let mock = Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": token,
            "expires_in": 3599,
            "token_type": "Bearer",
        })));
    match times {
        Some(n) => mock.up_to_n_times(n).mount(server).await,
        None => mock.mount(server).await,
    }
}

fn sent_message() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(
        json!({ "name": "projects/acme-crm/messages/0:1500415314455276%31bd1c9631bd1c96" }),
    )
}

fn firebase_error(status: u16, grpc: &str, code: Option<&str>, message: &str) -> ResponseTemplate {
    let mut error = json!({ "code": status, "message": message, "status": grpc });
    if let Some(code) = code {
        error["details"] = json!([{
            "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError",
            "errorCode": code,
        }]);
    }
    ResponseTemplate::new(status).set_body_json(json!({ "error": error }))
}

const SEND_PATH: &str = "/v1/projects/acme-crm/messages:send";

fn sends(server_requests: &[Request]) -> Vec<&Request> {
    server_requests
        .iter()
        .filter(|r| r.url.path() == SEND_PATH)
        .collect()
}

fn token_requests(server_requests: &[Request]) -> Vec<&Request> {
    server_requests
        .iter()
        .filter(|r| r.url.path() == "/token")
        .collect()
}

#[tokio::test]
async fn fcm_exchanges_a_service_account_assertion_for_a_token() {
    let server = MockServer::start().await;
    token_endpoint(&server, "ya29.first", None).await;
    Mock::given(method("POST"))
        .and(path(SEND_PATH))
        .and(header("authorization", "Bearer ya29.first"))
        .respond_with(sent_message())
        .mount(&server)
        .await;
    let clock = clock();
    let driver = firebase(&server, clock.clone());

    let message = PushMessage::new("Deal won")
        .body("Agra renewal was won by Gurpreet Kaur")
        .href("/deals/42")
        .badge(2)
        .sound("default")
        .collapse("deal-42")
        .urgency(Urgency::High);
    let sent = driver.send(&pixel(), &message).await.unwrap();

    assert_eq!(sent.provider, "fcm");
    assert_eq!(
        sent.message_id.as_deref(),
        Some("projects/acme-crm/messages/0:1500415314455276%31bd1c9631bd1c96")
    );
    let requests = server.received_requests().await.unwrap();
    let exchange = token_requests(&requests)[0];
    let form: Vec<(String, String)> = form_urlencoded::parse(&exchange.body)
        .into_owned()
        .collect();
    assert_eq!(
        form[0],
        (
            "grant_type".to_owned(),
            "urn:ietf:params:oauth:grant-type:jwt-bearer".to_owned()
        )
    );
    let assertion = Jws::parse(&form[1].1).unwrap();
    let key = SigningKey::from_pem(SERVICE_ACCOUNT_KEY).unwrap();
    assertion.verify(&key.verifying_key()).unwrap();
    assert_eq!(assertion.header().kid.as_deref(), Some("a1b2c3d4e5"));
    let claims: Value = assertion.claims().unwrap();
    let now = clock.now().timestamp();
    assert_eq!(
        claims,
        json!({
            "iss": "push@acme-crm.iam.gserviceaccount.com",
            "scope": "https://www.googleapis.com/auth/firebase.messaging",
            "aud": format!("{}/token", server.uri()),
            "iat": now,
            "exp": now + 3600,
        })
    );
    let body: Value = serde_json::from_slice(&sends(&requests)[0].body).unwrap();
    assert_eq!(
        body,
        json!({
            "message": {
                "token": FIREBASE_TOKEN,
                "notification": { "title": "Deal won", "body": "Agra renewal was won by Gurpreet Kaur" },
                "data": { "href": "/deals/42" },
                "android": {
                    "ttl": "86400s",
                    "restricted_package_name": "com.acme.crm",
                    "priority": "HIGH",
                    "collapse_key": "deal-42",
                    "notification": { "sound": "default", "tag": "deal-42", "notification_count": 2 },
                },
            },
        })
    );
}

#[tokio::test]
async fn fcm_caches_the_access_token_until_it_expires() {
    let server = MockServer::start().await;
    token_endpoint(&server, "ya29.first", Some(1)).await;
    token_endpoint(&server, "ya29.second", None).await;
    Mock::given(method("POST"))
        .and(path(SEND_PATH))
        .respond_with(sent_message())
        .mount(&server)
        .await;
    let clock = clock();
    let driver = firebase(&server, clock.clone());
    let message = PushMessage::new("Hi");

    driver.send(&pixel(), &message).await.unwrap();
    clock.advance(Duration::from_secs(50 * 60));
    driver.send(&pixel(), &message).await.unwrap();
    clock.advance(Duration::from_secs(10 * 60));
    driver.send(&pixel(), &message).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        token_requests(&requests).len(),
        2,
        "fetched again only once expired"
    );
    let used: Vec<String> = sends(&requests).iter().map(|r| bearer(r)).collect();
    assert_eq!(used, ["ya29.first", "ya29.first", "ya29.second"]);
}

#[tokio::test]
async fn fcm_renews_on_unauthenticated_and_retries_once() {
    let server = MockServer::start().await;
    token_endpoint(&server, "ya29.revoked", Some(1)).await;
    token_endpoint(&server, "ya29.fresh", None).await;
    Mock::given(method("POST"))
        .and(path(SEND_PATH))
        .and(header("authorization", "Bearer ya29.revoked"))
        .respond_with(firebase_error(
            401,
            "UNAUTHENTICATED",
            None,
            "Request had invalid authentication credentials. Expected OAuth 2 access token, login cookie or other valid authentication credential.",
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEND_PATH))
        .and(header("authorization", "Bearer ya29.fresh"))
        .respond_with(sent_message())
        .mount(&server)
        .await;
    let driver = firebase(&server, clock());

    driver
        .send(&pixel(), &PushMessage::new("Hi"))
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(token_requests(&requests).len(), 2);
    assert_eq!(sends(&requests).len(), 2);
}

#[tokio::test]
async fn fcm_unregistered_and_invalid_tokens_are_gone() {
    for answer in [
        firebase_error(
            404,
            "NOT_FOUND",
            Some("UNREGISTERED"),
            "Requested entity was not found.",
        ),
        firebase_error(
            400,
            "INVALID_ARGUMENT",
            Some("INVALID_ARGUMENT"),
            "The registration token is not a valid FCM registration token",
        ),
        firebase_error(
            403,
            "PERMISSION_DENIED",
            Some("SENDER_ID_MISMATCH"),
            "SenderId mismatch",
        ),
    ] {
        let server = MockServer::start().await;
        token_endpoint(&server, "ya29.first", None).await;
        Mock::given(method("POST"))
            .and(path(SEND_PATH))
            .respond_with(answer)
            .mount(&server)
            .await;
        let error = firebase(&server, clock())
            .send(&pixel(), &PushMessage::new("Hi"))
            .await
            .unwrap_err();
        assert!(matches!(error, ChannelError::Gone), "{error:?}");
    }
}

#[tokio::test]
async fn fcm_quota_and_server_errors_are_refusals() {
    for (status, grpc, code) in [
        (429, "RESOURCE_EXHAUSTED", "QUOTA_EXCEEDED"),
        (503, "UNAVAILABLE", "UNAVAILABLE"),
        (500, "INTERNAL", "INTERNAL"),
        (400, "INVALID_ARGUMENT", "INVALID_ARGUMENT"),
    ] {
        let server = MockServer::start().await;
        token_endpoint(&server, "ya29.first", None).await;
        Mock::given(method("POST"))
            .and(path(SEND_PATH))
            .respond_with(firebase_error(
                status,
                grpc,
                Some(code),
                "Invalid value at 'message.android.ttl'",
            ))
            .mount(&server)
            .await;
        let error = firebase(&server, clock())
            .send(&pixel(), &PushMessage::new("Hi"))
            .await
            .unwrap_err();
        match error {
            ChannelError::Refused {
                provider,
                status: got,
                code: got_code,
                ..
            } => {
                assert_eq!(provider, "fcm");
                assert_eq!(got, status);
                assert_eq!(got_code.as_deref(), Some(code));
            }
            other => panic!("{code}: {other:?}"),
        }
    }
}

#[tokio::test]
async fn fcm_refuses_a_payload_over_4096_bytes_before_sending() {
    let server = MockServer::start().await;
    token_endpoint(&server, "ya29.first", None).await;
    let driver = firebase(&server, clock());

    let message = PushMessage::silent().data("note", "ज".repeat(1500));
    let error = driver.send(&pixel(), &message).await.unwrap_err();

    assert!(
        matches!(&error, ChannelError::InvalidMessage(m) if m.contains("4096")),
        "{error:?}"
    );
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "not even a token"
    );
}

#[tokio::test]
async fn secrets_never_appear_in_errors_or_debug() {
    let mut options = PushOptions::default();
    options.fcm.service_account =
        Secret::new("{\"private_key\": \"secret-key-material\"".to_owned());
    options.fcm.packages = vec!["com.acme.crm".to_owned()];
    let error = FcmPush::new(&options).unwrap_err();
    assert!(!error.contains("secret-key-material"), "{error}");

    options.fcm.service_account = Secret::new(
        json!({
            "private_key": "-----BEGIN PRIVATE KEY-----\nc2VjcmV0LWtleS1tYXRlcmlhbA==\n-----END PRIVATE KEY-----",
            "client_email": "push@acme-crm.iam.gserviceaccount.com",
            "project_id": "acme-crm",
        })
        .to_string(),
    );
    let error = FcmPush::new(&options).unwrap_err();
    assert!(!error.contains("c2VjcmV0"), "{error}");

    let mut options = PushOptions::default();
    options.apns.team_id = "DEF123GHIJ".to_owned();
    options.apns.key_id = "ABC123DEFG".to_owned();
    options.apns.key = Secret::new("not-a-pem-secret-value".to_owned());
    options.apns.bundle_ids = vec!["com.acme.crm".to_owned()];
    let error = ApnsPush::new(&options).unwrap_err();
    assert!(!error.contains("not-a-pem-secret-value"), "{error}");

    let server = MockServer::start().await;
    let apple = apple(&server, clock());
    let printed = format!("{:?}", apple.driver);
    assert!(!printed.contains("PRIVATE"), "{printed}");
    let driver = firebase(&server, clock());
    let printed = format!("{driver:?}");
    assert!(
        !printed.contains("PRIVATE") && !printed.contains("a1b2c3d4e5"),
        "{printed}"
    );
}

// ---------------------------------------------------------------------
// Configured in `notify.push`
// ---------------------------------------------------------------------

#[tokio::test]
async fn the_configured_services_send_by_platform() {
    use navree_notify::{Channel, Notifiable, Notification, Notifier, NotifyOptions, PushDevices};

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Ping;

    impl Notification for Ping {
        const TYPE: &'static str = "ping";

        fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
            vec![Channel::Push]
        }

        fn to_push(&self, _n: &dyn Notifiable) -> Option<PushMessage> {
            Some(PushMessage::new("Ping").href("/inbox"))
        }
    }

    let apple_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/3/device/{APPLE_TOKEN}")))
        .respond_with(accepted())
        .mount(&apple_server)
        .await;
    let google = MockServer::start().await;
    token_endpoint(&google, "ya29.first", None).await;
    Mock::given(method("POST"))
        .and(path(SEND_PATH))
        .respond_with(sent_message())
        .mount(&google)
        .await;
    let key = SigningKey::generate_es256();
    let app = crate::support::builder(NotifyOptions::default())
        .config("notify.push.apns.team_id", "DEF123GHIJ")
        .config("notify.push.apns.key_id", "ABC123DEFG")
        .config("notify.push.apns.key", key.to_pkcs8_pem().unwrap())
        .config("notify.push.apns.bundle_ids", json!(["com.acme.crm"]))
        .config("notify.push.apns.endpoint", apple_server.uri())
        .config("notify.push.fcm.service_account", service_account())
        .config("notify.push.fcm.packages", json!(["com.acme.crm"]))
        .config("notify.push.fcm.endpoint", google.uri())
        .config(
            "notify.push.fcm.token_url",
            format!("{}/token", google.uri()),
        )
        .build()
        .await;
    let user = crate::support::TestUser::new();
    let devices = app.get::<PushDevices>().await;
    for (session, platform, token) in [
        ("session-1", PushPlatform::Ios, APPLE_TOKEN),
        ("session-2", PushPlatform::Android, FIREBASE_TOKEN),
    ] {
        devices
            .register(
                &user,
                session,
                navree_notify::NewPushDevice {
                    platform,
                    token: token.to_owned(),
                    app_id: "com.acme.crm".to_owned(),
                    environment: PushEnvironment::Production,
                },
            )
            .await
            .unwrap();
    }

    app.get::<Notifier>().await.send(&user, Ping).await.unwrap();

    assert_eq!(apple_server.received_requests().await.unwrap().len(), 1);
    let google_requests = google.received_requests().await.unwrap();
    assert_eq!(sends(&google_requests).len(), 1);
    let providers: Vec<Option<String>> = crate::support::deliveries(&app)
        .await
        .into_iter()
        .map(|d| d.provider)
        .collect();
    assert!(
        providers.contains(&Some("apns".to_owned())),
        "{providers:?}"
    );
    assert!(providers.contains(&Some("fcm".to_owned())), "{providers:?}");
}

#[tokio::test]
async fn a_platform_without_a_service_is_skipped() {
    use navree_notify::{Channel, Notifiable, Notification, Notifier, NotifyOptions, PushDevices};

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Ping;

    impl Notification for Ping {
        const TYPE: &'static str = "ping";

        fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
            vec![Channel::Push]
        }

        fn to_push(&self, _n: &dyn Notifiable) -> Option<PushMessage> {
            Some(PushMessage::new("Ping"))
        }
    }

    let apple_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(accepted())
        .mount(&apple_server)
        .await;
    let key = SigningKey::generate_es256();
    let app = crate::support::builder(NotifyOptions::default())
        .config("notify.push.apns.team_id", "DEF123GHIJ")
        .config("notify.push.apns.key_id", "ABC123DEFG")
        .config("notify.push.apns.key", key.to_pkcs8_pem().unwrap())
        .config("notify.push.apns.bundle_ids", json!(["com.acme.crm"]))
        .config("notify.push.apns.endpoint", apple_server.uri())
        .build()
        .await;
    let user = crate::support::TestUser::new();
    app.get::<PushDevices>()
        .await
        .register(
            &user,
            "session-2",
            navree_notify::NewPushDevice {
                platform: PushPlatform::Android,
                token: FIREBASE_TOKEN.to_owned(),
                app_id: "com.acme.crm".to_owned(),
                environment: PushEnvironment::Production,
            },
        )
        .await
        .unwrap();

    app.get::<Notifier>().await.send(&user, Ping).await.unwrap();

    assert!(apple_server.received_requests().await.unwrap().is_empty());
    assert!(crate::support::deliveries(&app).await.is_empty());
}
