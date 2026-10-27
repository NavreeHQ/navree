//! The SES, Mailgun, Postmark and Resend transports against a local mock
//! server, and their configuration.

use navree_contracts::Secret;
use navree_mail::{
    MailError, MailTransport, MailgunConfig, MailgunRegion, MailgunTransport, Message,
    PostmarkConfig, PostmarkTransport, ResendConfig, ResendTransport, SesConfig, SesTransport,
    TransportConfig,
};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn message() -> Message {
    Message::new()
        .from("Acme Billing <billing@acme.test>")
        .to("Ada <ada@example.com>")
        .cc("bob@example.com")
        .bcc("audit@acme.test")
        .reply_to("help@acme.test")
        .subject("Your invoice")
        .text("Thanks")
        .html("<p>Thanks</p>")
        .header("X-Campaign", "spring")
        .metadata("invoice id", "42")
}

async fn only_request(server: &MockServer) -> Request {
    let mut requests = server.received_requests().await.expect("recording");
    assert_eq!(requests.len(), 1, "one request");
    requests.remove(0)
}

fn json_body(request: &Request) -> Value {
    serde_json::from_slice(&request.body).expect("a JSON body")
}

fn header_value<'a>(request: &'a Request, name: &str) -> &'a str {
    request
        .headers
        .get(name)
        .unwrap_or_else(|| panic!("header {name}"))
        .to_str()
        .expect("text")
}

// ---------------------------------------------------------------------
// SES
// ---------------------------------------------------------------------

fn ses(server: &MockServer) -> SesTransport {
    SesTransport::new(SesConfig {
        region: "eu-west-1".to_owned(),
        access_key_id: "AKIDEXAMPLE".to_owned(),
        secret_access_key: Secret::new("wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned()),
        configuration_set: Some("events".to_owned()),
        endpoint: Some(server.uri()),
        ..SesConfig::default()
    })
    .expect("ses")
}

#[tokio::test]
async fn ses_sends_simple_content_signed_with_sigv4() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/email/outbound-emails"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "MessageId": "0100018c-abc" })))
        .mount(&server)
        .await;

    let sent = ses(&server).deliver(&message()).await.expect("deliver");

    assert_eq!(sent.transport, "ses");
    assert_eq!(sent.provider_id.as_deref(), Some("0100018c-abc"));
    let request = only_request(&server).await;
    let authorization = header_value(&request, "authorization");
    assert!(
        authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"),
        "{authorization}"
    );
    assert!(authorization.contains("/eu-west-1/ses/aws4_request, SignedHeaders=content-type;host;x-amz-date, Signature="));
    assert_eq!(header_value(&request, "x-amz-date").len(), 16);
    assert_eq!(
        json_body(&request),
        json!({
            "FromEmailAddress": "Acme Billing <billing@acme.test>",
            "Destination": {
                "ToAddresses": ["Ada <ada@example.com>"],
                "CcAddresses": ["bob@example.com"],
                "BccAddresses": ["audit@acme.test"],
            },
            "ReplyToAddresses": ["help@acme.test"],
            "Content": { "Simple": {
                "Subject": { "Data": "Your invoice", "Charset": "UTF-8" },
                "Body": {
                    "Text": { "Data": "Thanks", "Charset": "UTF-8" },
                    "Html": { "Data": "<p>Thanks</p>", "Charset": "UTF-8" },
                },
                "Headers": [{ "Name": "X-Campaign", "Value": "spring" }],
            } },
            "EmailTags": [{ "Name": "invoice_id", "Value": "42" }],
            "ConfigurationSetName": "events",
        })
    );
}

#[tokio::test]
async fn ses_reports_the_providers_refusal() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(
            json!({ "message": "Email address is not verified: billing@acme.test" }),
        ))
        .mount(&server)
        .await;

    let error = ses(&server).send(&message()).await.unwrap_err();

    assert!(
        matches!(&error, MailError::Provider { transport: "ses", status: 400, message } if message.contains("not verified")),
        "{error}"
    );
    assert!(!error.to_string().contains("wJalr"));
}

#[tokio::test]
async fn ses_check_reads_the_account() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/email/account"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "SendingEnabled": true })))
        .mount(&server)
        .await;
    ses(&server).check().await.expect("usable");

    let refused = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&refused)
        .await;
    let error = ses(&refused).check().await.unwrap_err();
    assert!(
        matches!(&error, MailError::Unusable(reason) if reason.contains("refused the credentials")),
        "{error}"
    );
}

#[test]
fn ses_needs_a_region_and_credentials() {
    let error = SesTransport::new(SesConfig::default()).unwrap_err();
    assert!(error.to_string().contains("region"), "{error}");
}

// ---------------------------------------------------------------------
// Mailgun
// ---------------------------------------------------------------------

fn mailgun(server: &MockServer) -> MailgunTransport {
    MailgunTransport::new(MailgunConfig {
        domain: "mg.acme.test".to_owned(),
        api_key: Secret::new("key-123".to_owned()),
        region: MailgunRegion::Eu,
        endpoint: Some(server.uri()),
    })
    .expect("mailgun")
}

#[tokio::test]
async fn mailgun_posts_a_form_with_basic_auth() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/mg.acme.test/messages"))
        .and(header("authorization", "Basic YXBpOmtleS0xMjM="))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({ "id": "<20261027.1@mg.acme.test>", "message": "Queued. Thank you." }),
        ))
        .mount(&server)
        .await;

    let sent = mailgun(&server).deliver(&message()).await.expect("deliver");

    assert_eq!(sent.provider_id.as_deref(), Some("20261027.1@mg.acme.test"));
    let request = only_request(&server).await;
    let form: Vec<(String, String)> = url::form_urlencoded::parse(&request.body)
        .into_owned()
        .collect();
    let field = |name: &str| {
        form.iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(field("from"), ["Acme Billing <billing@acme.test>"]);
    assert_eq!(field("to"), ["Ada <ada@example.com>"]);
    assert_eq!(field("cc"), ["bob@example.com"]);
    assert_eq!(field("bcc"), ["audit@acme.test"]);
    assert_eq!(field("subject"), ["Your invoice"]);
    assert_eq!(field("text"), ["Thanks"]);
    assert_eq!(field("html"), ["<p>Thanks</p>"]);
    assert_eq!(field("h:Reply-To"), ["help@acme.test"]);
    assert_eq!(field("h:X-Campaign"), ["spring"]);
    assert_eq!(field("v:invoice id"), ["42"]);
}

#[tokio::test]
async fn mailgun_check_reads_the_domain() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v3/domains/mg.acme.test"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let error = mailgun(&server).check().await.unwrap_err();

    assert!(
        matches!(&error, MailError::Unusable(reason) if reason.contains("does not know this domain")),
        "{error}"
    );
}

// ---------------------------------------------------------------------
// Postmark
// ---------------------------------------------------------------------

fn postmark(server: &MockServer) -> PostmarkTransport {
    PostmarkTransport::new(PostmarkConfig {
        server_token: Secret::new("server-token".to_owned()),
        message_stream: Some("outbound".to_owned()),
        endpoint: Some(server.uri()),
    })
    .expect("postmark")
}

#[tokio::test]
async fn postmark_posts_json_with_the_server_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/email"))
        .and(header("x-postmark-server-token", "server-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({ "ErrorCode": 0, "Message": "OK", "MessageID": "b7bc2f4a-e38e" }),
        ))
        .mount(&server)
        .await;

    let sent = postmark(&server).deliver(&message()).await.expect("deliver");

    assert_eq!(sent.provider_id.as_deref(), Some("b7bc2f4a-e38e"));
    assert_eq!(
        json_body(&only_request(&server).await),
        json!({
            "From": "Acme Billing <billing@acme.test>",
            "To": "Ada <ada@example.com>",
            "Cc": "bob@example.com",
            "Bcc": "audit@acme.test",
            "Subject": "Your invoice",
            "TextBody": "Thanks",
            "HtmlBody": "<p>Thanks</p>",
            "ReplyTo": "help@acme.test",
            "Headers": [{ "Name": "X-Campaign", "Value": "spring" }],
            "Metadata": { "invoice id": "42" },
            "MessageStream": "outbound",
        })
    );
}

#[tokio::test]
async fn postmark_reports_its_error_code_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(422).set_body_json(
            json!({ "ErrorCode": 406, "Message": "You tried to send to inactive recipient(s)." }),
        ))
        .mount(&server)
        .await;

    let error = postmark(&server).send(&message()).await.unwrap_err();

    assert!(
        matches!(&error, MailError::Provider { transport: "postmark", status: 422, message } if message.contains("inactive")),
        "{error}"
    );
}

// ---------------------------------------------------------------------
// Resend
// ---------------------------------------------------------------------

fn resend(server: &MockServer) -> ResendTransport {
    ResendTransport::new(ResendConfig {
        api_key: Secret::new("re_123".to_owned()),
        endpoint: Some(server.uri()),
    })
    .expect("resend")
}

#[tokio::test]
async fn resend_posts_json_with_a_bearer_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/emails"))
        .and(header("authorization", "Bearer re_123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "49a3999c-0ce1" })))
        .mount(&server)
        .await;

    let sent = resend(&server).deliver(&message()).await.expect("deliver");

    assert_eq!(sent.provider_id.as_deref(), Some("49a3999c-0ce1"));
    assert_eq!(
        json_body(&only_request(&server).await),
        json!({
            "from": "Acme Billing <billing@acme.test>",
            "to": ["Ada <ada@example.com>"],
            "cc": ["bob@example.com"],
            "bcc": ["audit@acme.test"],
            "subject": "Your invoice",
            "text": "Thanks",
            "html": "<p>Thanks</p>",
            "reply_to": "help@acme.test",
            "headers": { "X-Campaign": "spring" },
            "tags": [{ "name": "invoice id", "value": "42" }],
        })
    );
}

#[tokio::test]
async fn resend_check_lists_domains() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/domains"))
        .and(header("authorization", "Bearer re_123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
        .mount(&server)
        .await;

    resend(&server).check().await.expect("usable");
}

// ---------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------

fn parse(value: Value) -> TransportConfig {
    serde_json::from_value(value).expect("transport config")
}

#[test]
fn api_transports_read_their_shorthands() {
    assert_eq!(
        parse(json!(
            "ses://AKID:se%2Fcret@eu-west-1?configuration_set=events&endpoint=http://127.0.0.1:9"
        )),
        TransportConfig::Ses(SesConfig {
            region: "eu-west-1".to_owned(),
            access_key_id: "AKID".to_owned(),
            secret_access_key: Secret::new("se/cret".to_owned()),
            session_token: None,
            configuration_set: Some("events".to_owned()),
            endpoint: Some("http://127.0.0.1:9".to_owned()),
        })
    );
    assert_eq!(
        parse(json!("mailgun://key-1@mg.acme.test?region=eu")),
        TransportConfig::Mailgun(MailgunConfig {
            domain: "mg.acme.test".to_owned(),
            api_key: Secret::new("key-1".to_owned()),
            region: MailgunRegion::Eu,
            endpoint: None,
        })
    );
    assert_eq!(
        parse(json!("postmark://token-1@api.postmarkapp.com?stream=broadcast")),
        TransportConfig::Postmark(PostmarkConfig {
            server_token: Secret::new("token-1".to_owned()),
            message_stream: Some("broadcast".to_owned()),
            endpoint: None,
        })
    );
    assert_eq!(
        parse(json!("resend://re_1@api.resend.com")),
        TransportConfig::Resend(ResendConfig {
            api_key: Secret::new("re_1".to_owned()),
            endpoint: None,
        })
    );
    let error = serde_json::from_value::<TransportConfig>(json!("mailgun://k@mg.acme.test?region=asia"))
        .unwrap_err();
    assert!(error.to_string().contains("`us` or `eu`"), "{error}");
}

#[test]
fn api_transports_read_their_tables() {
    let config = parse(json!({
        "type": "failover",
        "transports": [
            { "type": "ses", "region": "us-east-1", "access_key_id": "AKID", "secret_access_key": "s" },
            { "type": "postmark", "server_token": "t" },
        ]
    }));
    let TransportConfig::Failover(list) = &config else {
        panic!("a failover");
    };
    assert_eq!(list[0].kind(), "ses");
    assert_eq!(list[1].kind(), "postmark");
    let built = config.build().expect("build");
    assert_eq!(built.name(), "failover");
}

#[test]
fn the_json_form_round_trips_with_its_credentials() {
    for config in [
        parse(json!("ses://AKID:secret@eu-west-1?configuration_set=events")),
        parse(json!("mailgun://key-1@mg.acme.test")),
        parse(json!("postmark://token-1@api.postmarkapp.com")),
        parse(json!("resend://re_1@api.resend.com")),
        TransportConfig::smtp("smtps://u:p@smtp.acme.test"),
        TransportConfig::Failover(vec![TransportConfig::Log, TransportConfig::Array]),
    ] {
        let json = config.to_json();
        assert_eq!(json["type"], config.kind());
        assert_eq!(parse(json), config);
    }
}

#[test]
fn debug_output_never_shows_credentials() {
    let config = parse(json!("ses://AKID:topsecret@eu-west-1"));
    assert!(!format!("{config:?}").contains("topsecret"));
    let transport = config.build().expect("build");
    assert!(!format!("{transport:?}").contains("topsecret"));
    let mailgun = parse(json!("mailgun://key-topsecret@mg.acme.test"));
    assert!(!format!("{mailgun:?}").contains("topsecret"));
}
