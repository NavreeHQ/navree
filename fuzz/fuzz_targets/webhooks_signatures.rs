//! `navree_webhooks`: the incoming signature headers and bodies of every
//! verifier (Standard Webhooks, Stripe, GitHub, Paddle, a configured HMAC
//! scheme, Slack, Meta, Mailgun, Twilio, Vonage, Postmark, Microsoft Graph,
//! Google Pub/Sub claims, AWS SNS messages).
//!
//! Part one gives every verifier the attacker's bytes in the headers and
//! the body. Part two signs a request the way its sender does and requires
//! the verifier to accept it, with the same timestamp and (where the scheme
//! has one) id, and to refuse it once the body changes.
#![no_main]

use arbitrary::Arbitrary;
use chrono::DateTime;
use http::{HeaderMap, HeaderName, HeaderValue};
use libfuzzer_sys::fuzz_target;
use navree_webhooks::{
    AwsSns, GitHub, GooglePubSub, Hmac, Mailgun, Meta, MicrosoftGraph, Paddle, Postmark,
    PubSubPush, Slack, SnsMessage, StandardWebhooks, Stripe, Tolerance, Twilio, Vonage,
};
use navree_webhooks::{IncomingRequest, SourceOptions};

const NAMES: [&str; 12] = [
    "webhook-id",
    "webhook-timestamp",
    "webhook-signature",
    "stripe-signature",
    "x-hub-signature-256",
    "paddle-signature",
    "x-slack-signature",
    "x-slack-request-timestamp",
    "x-twilio-signature",
    "authorization",
    "x-signature",
    "x-amz-sns-message-type",
];

#[derive(Debug, Arbitrary)]
struct Input {
    secret: String,
    headers: Vec<(u8, Vec<u8>)>,
    body: Vec<u8>,
    url: String,
    now: u32,
    tolerance: Option<u16>,
    // For the signed requests.
    id: String,
    other_body: Vec<u8>,
    format: String,
    signed: String,
}

fn attack(input: &Input) {
    let mut headers = HeaderMap::new();
    for (name, value) in &input.headers {
        if let Ok(value) = HeaderValue::from_bytes(value) {
            let name = NAMES[usize::from(*name) % NAMES.len()];
            headers.append(HeaderName::from_static(name), value);
        }
    }
    let now = DateTime::from_timestamp(i64::from(input.now), 0).expect("a time");
    let tolerance = input
        .tolerance
        .map(|secs| Tolerance::at(now, u64::from(secs)));
    let (secret, body) = (input.secret.as_str(), input.body.as_slice());

    let _ = StandardWebhooks::verify(secret, &headers, body, tolerance);
    let _ = Stripe::verify(secret, &headers, body, tolerance);
    let _ = GitHub::verify(secret, &headers, body);
    let _ = Paddle::verify(secret, &headers, body, tolerance);
    let _ = Slack::verify(secret, &headers, body, tolerance);
    let _ = Meta::verify(secret, &headers, body);
    let _ = Mailgun::verify(secret, body, tolerance);
    let _ = Twilio::verify(secret, &input.url, &headers, body);
    let _ = Vonage::verify(secret, &headers, body, tolerance);
    let _ = Postmark::verify(secret, &headers, body);
    let _ = MicrosoftGraph::verify(secret, body);

    // A configured HMAC scheme: the templates are strings from config.
    let mut source = SourceOptions::new(secret);
    source.header = Some("x-signature".to_owned());
    source.format = Some(input.format.clone());
    source.signed = Some(input.signed.clone());
    source.timestamp_header = Some("webhook-timestamp".to_owned());
    source.id_header = Some("webhook-id".to_owned());
    let _ = Hmac::verify(&source, &headers, body, tolerance);
    let request = IncomingRequest::new(&source, &headers, body).with_url(input.url.clone());
    let _ = request.query("hub.mode");
    let _ = request.query("validationToken");

    // Messages and claims that arrive as JSON.
    if let Ok(message) = serde_json::from_slice::<SnsMessage>(body) {
        let _ = message.is_notification();
        let _ = message.string_to_sign();
        let _ = AwsSns::is_sns_url(&message.signing_cert_url);
    }
    let _ = AwsSns::is_sns_url(&input.url);
    if let Ok(push) = serde_json::from_slice::<PubSubPush>(body) {
        let _ = push.message.data();
    }
    if let Ok(claims) = serde_json::from_slice::<serde_json::Value>(body) {
        let _ = GooglePubSub::check_claims(&claims, &input.url, secret, now);
    }
}

fn header_map(pairs: &[(&str, String)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.insert(
            HeaderName::from_bytes(name.as_bytes()).expect("a header name"),
            HeaderValue::from_str(value).expect("a signature is a header value"),
        );
    }
    headers
}

fn signed(input: &Input) {
    if input.secret.is_empty() {
        return;
    }
    let ts = i64::from(input.now);
    let now = DateTime::from_timestamp(ts, 0).expect("a time");
    let tolerance = Some(Tolerance::at(now, 300));
    let secret = input.secret.as_str();
    let body = input.body.as_slice();
    let other = input.other_body.as_slice();
    let id: String = input
        .id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();

    // Standard Webhooks (the id is a header value without spaces or dots).
    if !id.is_empty() {
        let signature = StandardWebhooks::sign(secret, &id, ts, body);
        let headers = header_map(&[
            ("webhook-id", id.clone()),
            ("webhook-timestamp", ts.to_string()),
            ("webhook-signature", signature),
        ]);
        let verified = StandardWebhooks::verify(secret, &headers, body, tolerance)
            .expect("a signed Standard Webhooks request verifies");
        assert_eq!(verified.timestamp, Some(now));
        assert_eq!(verified.id.as_deref(), Some(id.as_str()));
        if other != body {
            assert!(StandardWebhooks::verify(secret, &headers, other, tolerance).is_err());
        }
    }

    // Stripe.
    let headers = header_map(&[("stripe-signature", Stripe::sign(secret, ts, body))]);
    let verified =
        Stripe::verify(secret, &headers, body, tolerance).expect("a signed Stripe request");
    assert_eq!(verified.timestamp, Some(now));
    if other != body {
        assert!(Stripe::verify(secret, &headers, other, tolerance).is_err());
    }

    // GitHub and Meta share a scheme.
    let headers = header_map(&[("x-hub-signature-256", GitHub::sign(secret, body))]);
    GitHub::verify(secret, &headers, body).expect("a signed GitHub request");
    Meta::verify(secret, &headers, body).expect("a signed Meta request");
    if other != body {
        assert!(GitHub::verify(secret, &headers, other).is_err());
        assert!(Meta::verify(secret, &headers, other).is_err());
    }

    // Paddle.
    let headers = header_map(&[("paddle-signature", Paddle::sign(secret, ts, body))]);
    let verified =
        Paddle::verify(secret, &headers, body, tolerance).expect("a signed Paddle request");
    assert_eq!(verified.timestamp, Some(now));

    // Slack.
    let headers = header_map(&[
        ("x-slack-signature", Slack::sign(secret, ts, body)),
        ("x-slack-request-timestamp", ts.to_string()),
    ]);
    Slack::verify(secret, &headers, body, tolerance).expect("a signed Slack request");
    if other != body {
        assert!(Slack::verify(secret, &headers, other, tolerance).is_err());
    }

    // Vonage.
    let jti = if id.is_empty() {
        "jti".to_owned()
    } else {
        id.clone()
    };
    let headers = header_map(&[("authorization", Vonage::sign(secret, body, ts, &jti))]);
    let verified =
        Vonage::verify(secret, &headers, body, tolerance).expect("a signed Vonage request");
    assert_eq!(verified.id.as_deref(), Some(jti.as_str()));
    if other != body {
        assert!(Vonage::verify(secret, &headers, other, tolerance).is_err());
    }

    // Mailgun, JSON body.
    let token = if id.is_empty() {
        "token".to_owned()
    } else {
        id.clone()
    };
    let mailgun = serde_json::json!({
        "signature": {
            "timestamp": ts.to_string(),
            "token": token,
            "signature": Mailgun::sign(secret, ts, &token),
        },
        "event-data": { "id": "evt", "event": "delivered" },
    })
    .to_string();
    let verified =
        Mailgun::verify(secret, mailgun.as_bytes(), tolerance).expect("a signed Mailgun request");
    assert_eq!(verified.timestamp, Some(now));

    // Twilio: the URL as sent (its query string included).
    let url = "https://example.com/hooks/twilio?a=1";
    let params = [("From", "+15550100"), ("Body", "hi")];
    let headers = header_map(&[("x-twilio-signature", Twilio::sign(secret, url, &params))]);
    let form = format!("From={}&Body=hi", "%2B15550100");
    Twilio::verify(secret, url, &headers, form.as_bytes()).expect("a signed Twilio request");

    // Postmark: Basic credentials.
    if secret.contains(':') {
        let headers = header_map(&[("authorization", Postmark::authorization(secret))]);
        Postmark::verify(secret, &headers, body).expect("a Postmark request");
    }
}

fuzz_target!(|input: Input| {
    attack(&input);
    signed(&input);
});
