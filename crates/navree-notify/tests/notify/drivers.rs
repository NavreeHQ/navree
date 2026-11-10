//! The HTTP drivers against a local mock server: Slack, Teams, Twilio,
//! Vonage, Amazon SNS, the WhatsApp Cloud API and web push.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use http::Method;
use navree_contracts::{BoxFuture, Secret};
use navree_crypto::hmac_sha256;
use navree_crypto::jose::{Algorithm, Jws, VerifyingKey};
use navree_crypto::webpush::VapidKey;
use navree_http_client::{FakeResponse, HttpClient, HttpClientOptions};
use navree_kernel::BoxError;
use navree_notify::{
    ChannelError, ConnectionTokens, MetaOptions, MetaWhatsApp, PushTarget, SlackApi, SlackDriver,
    SlackMessage, SlackOptions, SlackRoute, SmsDriver, SmsMessage, SmsOptions, SnsOptions, SnsSms,
    TeamsApi, TeamsDriver, TeamsMessage, TeamsOptions, TeamsRoute, TwilioOptions, TwilioSms,
    TwilioWhatsApp, Urgency, VapidWebPush, VonageOptions, VonageSms, WebPushDriver, WebPushMessage,
    WebPushOptions, WhatsAppDriver, WhatsAppMessage, WhatsAppOptions, WhatsAppTemplate,
};
use ring::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::agreement::{self, ECDH_P256, EphemeralPrivateKey, UnparsedPublicKey};
use ring::rand::SystemRandom;
use serde_json::{Value, json};
use uuid::Uuid;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// A client that may call the local mock server.
pub fn local_client() -> HttpClient {
    let mut options = HttpClientOptions {
        retries: 0,
        ..HttpClientOptions::default()
    };
    options.ssrf.allow_hosts.push("127.0.0.1".to_owned());
    HttpClient::new(options).expect("client")
}

async fn only_request(server: &MockServer) -> Request {
    let mut requests = server.received_requests().await.expect("recording");
    assert_eq!(requests.len(), 1, "one request");
    requests.remove(0)
}

fn form(request: &Request) -> Vec<(String, String)> {
    form_urlencoded::parse(&request.body).into_owned().collect()
}

fn form_value<'a>(form: &'a [(String, String)], name: &str) -> Option<&'a str> {
    form.iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

fn header_value<'a>(request: &'a Request, name: &str) -> &'a str {
    request
        .headers
        .get(name)
        .unwrap_or_else(|| panic!("header {name}"))
        .to_str()
        .unwrap()
}

fn basic(user: &str, password: &str) -> String {
    format!("Basic {}", STANDARD.encode(format!("{user}:{password}")))
}

/// Tokens by connection, for bot routes that name one.
struct StaticTokens(String);

impl ConnectionTokens for StaticTokens {
    fn token(&self, _connection: Uuid) -> BoxFuture<'_, Result<Secret<String>, BoxError>> {
        let token = Secret::new(self.0.clone());
        Box::pin(async move { Ok(token) })
    }
}

// ---------------------------------------------------------------------
// Slack
// ---------------------------------------------------------------------

fn slack(server: &MockServer, bot_token: &str) -> SlackApi {
    SlackApi::new(&SlackOptions {
        bot_token: Secret::new(bot_token.to_owned()),
        endpoint: Some(format!("{}/api", server.uri())),
    })
    .unwrap()
    .with_client(local_client())
}

#[tokio::test]
async fn slack_webhooks_get_the_message_json() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/services/T0/B0/X"))
        .and(body_json(
            json!({ "text": "Deal won", "blocks": [{ "type": "divider" }] }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;

    let route = SlackRoute::webhook(format!("{}/services/T0/B0/X", server.uri()));
    let message = SlackMessage::new("Deal won").blocks(json!([{ "type": "divider" }]));
    let sent = slack(&server, "").send(&route, &message).await.unwrap();

    assert_eq!(sent.provider, "slack");
    assert_eq!(sent.message_id, None);
}

#[tokio::test]
async fn a_refused_slack_webhook_reports_slacks_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(404).set_body_string("no_service"))
        .mount(&server)
        .await;

    let route = SlackRoute::webhook(format!("{}/services/gone", server.uri()));
    let error = slack(&server, "")
        .send(&route, &SlackMessage::new("x"))
        .await
        .unwrap_err();

    match error {
        ChannelError::Refused { status, code, .. } => {
            assert_eq!(status, 404);
            assert_eq!(code.as_deref(), Some("no_service"));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn slack_channels_are_posted_with_the_bot_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat.postMessage"))
        .and(header("authorization", "Bearer xoxb-config"))
        .and(body_json(json!({
            "channel": "C0123",
            "text": "Reply",
            "thread_ts": "1700000000.000100",
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({ "ok": true, "channel": "C0123", "ts": "1700000001.000200" }),
            ),
        )
        .mount(&server)
        .await;

    let sent = slack(&server, "xoxb-config")
        .send(
            &SlackRoute::channel("C0123"),
            &SlackMessage::new("Reply").thread("1700000000.000100"),
        )
        .await
        .unwrap();

    assert_eq!(sent.message_id.as_deref(), Some("1700000001.000200"));
}

#[tokio::test]
async fn slack_errors_in_an_ok_response_are_refusals() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "ok": false, "error": "channel_not_found" })),
        )
        .mount(&server)
        .await;

    let error = slack(&server, "xoxb-config")
        .send(&SlackRoute::channel("C404"), &SlackMessage::new("x"))
        .await
        .unwrap_err();

    assert!(
        matches!(&error, ChannelError::Refused { code: Some(code), .. } if code == "channel_not_found"),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_slack_connection_route_uses_the_connections_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer xoxb-install"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true, "ts": "1.2" })))
        .mount(&server)
        .await;

    let driver = slack(&server, "xoxb-config")
        .with_tokens(Arc::new(StaticTokens("xoxb-install".to_owned())));
    let route = SlackRoute::channel("C1").connection(Uuid::now_v7());
    let sent = driver.send(&route, &SlackMessage::new("x")).await.unwrap();

    assert_eq!(sent.message_id.as_deref(), Some("1.2"));
}

#[tokio::test]
async fn a_slack_channel_route_needs_a_token() {
    let server = MockServer::start().await;
    let driver = slack(&server, "");
    let error = driver
        .send(&SlackRoute::channel("C1"), &SlackMessage::new("x"))
        .await
        .unwrap_err();
    assert!(matches!(error, ChannelError::Config(_)), "{error:?}");
    let error = driver
        .send(
            &SlackRoute::channel("C1").connection(Uuid::now_v7()),
            &SlackMessage::new("x"),
        )
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("ConnectionTokens"),
        "names the missing seam: {error}"
    );
}

// ---------------------------------------------------------------------
// Microsoft Teams
// ---------------------------------------------------------------------

fn teams(server: &MockServer, app_id: Option<&str>) -> TeamsApi {
    TeamsApi::new(&TeamsOptions {
        app_id: app_id.map(str::to_owned),
        app_password: Secret::new("bot-secret".to_owned()),
        token_url: Some(format!(
            "{}/botframework.com/oauth2/v2.0/token",
            server.uri()
        )),
        ..TeamsOptions::default()
    })
    .unwrap()
    .with_client(local_client())
}

#[tokio::test]
async fn teams_webhooks_get_an_adaptive_card() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/workflows/abc/invoke"))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;

    let route = TeamsRoute::webhook(format!("{}/workflows/abc/invoke", server.uri()));
    teams(&server, None)
        .send(&route, &TeamsMessage::new("Training due today"))
        .await
        .unwrap();

    let request = only_request(&server).await;
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["type"], "message");
    let attachment = &body["attachments"][0];
    assert_eq!(
        attachment["contentType"],
        "application/vnd.microsoft.card.adaptive"
    );
    assert_eq!(
        attachment["content"]["body"][0]["text"],
        "Training due today"
    );
}

#[tokio::test]
async fn teams_proactive_messages_use_a_cached_bot_framework_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/botframework.com/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token_type": "Bearer",
            "expires_in": 3599,
            "access_token": "bot-token",
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/emea/v3/conversations/a:1Xyz/activities"))
        .and(header("authorization", "Bearer bot-token"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "1|0001" })))
        .expect(2)
        .mount(&server)
        .await;

    let driver = teams(&server, Some("bot-app-id"));
    let route = TeamsRoute::conversation(format!("{}/emea/", server.uri()), "a:1Xyz");
    let message = TeamsMessage::new("Due today").summary("Training");
    let sent = driver.send(&route, &message).await.unwrap();
    driver.send(&route, &message).await.unwrap();

    assert_eq!(sent.provider, "teams");
    assert_eq!(sent.message_id.as_deref(), Some("1|0001"));
    let requests = server.received_requests().await.unwrap();
    let token_request = requests
        .iter()
        .find(|r| r.url.path().ends_with("/token"))
        .unwrap();
    let form = form(token_request);
    assert_eq!(form_value(&form, "grant_type"), Some("client_credentials"));
    assert_eq!(form_value(&form, "client_id"), Some("bot-app-id"));
    assert_eq!(form_value(&form, "client_secret"), Some("bot-secret"));
    assert_eq!(
        form_value(&form, "scope"),
        Some("https://api.botframework.com/.default")
    );
    let activity: Value = serde_json::from_slice(
        &requests
            .iter()
            .find(|r| r.url.path().ends_with("/activities"))
            .unwrap()
            .body,
    )
    .unwrap();
    assert_eq!(
        activity,
        json!({ "type": "message", "text": "Due today", "summary": "Training" })
    );
}

#[tokio::test]
async fn a_teams_connection_route_uses_the_connections_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/amer/v3/conversations/19:abc@thread.skype/activities",
        ))
        .and(header("authorization", "Bearer tenant-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "a1" })))
        .mount(&server)
        .await;

    let driver =
        teams(&server, None).with_tokens(Arc::new(StaticTokens("tenant-token".to_owned())));
    let route = TeamsRoute::conversation(format!("{}/amer", server.uri()), "19:abc@thread.skype")
        .connection(Uuid::now_v7());
    let sent = driver.send(&route, &TeamsMessage::new("x")).await.unwrap();

    assert_eq!(sent.message_id.as_deref(), Some("a1"));
}

#[tokio::test]
async fn teams_refusals_carry_the_bot_frameworks_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/botframework.com/oauth2/v2.0/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "access_token": "t", "expires_in": 3600 })),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/emea/v3/conversations/a:1/activities"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": { "code": "BotNotInConversationRoster", "message": "The bot is not part of the conversation roster." }
        })))
        .mount(&server)
        .await;

    let route = TeamsRoute::conversation(format!("{}/emea/", server.uri()), "a:1");
    let error = teams(&server, Some("bot"))
        .send(&route, &TeamsMessage::new("x"))
        .await
        .unwrap_err();

    assert!(
        matches!(&error, ChannelError::Refused { status: 403, code: Some(code), .. } if code == "BotNotInConversationRoster"),
        "{error:?}"
    );
    let error = teams(&server, None)
        .send(&route, &TeamsMessage::new("x"))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ChannelError::Config(_)),
        "no bot, no connection"
    );
}

#[tokio::test]
async fn a_refused_bot_framework_token_is_a_refusal() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_client",
            "error_description": "AADSTS7000215: Invalid client secret provided.\r\nTrace ID: x",
        })))
        .mount(&server)
        .await;

    let route = TeamsRoute::conversation(format!("{}/emea/", server.uri()), "a:1");
    let error = teams(&server, Some("bot"))
        .send(&route, &TeamsMessage::new("x"))
        .await
        .unwrap_err();

    match error {
        ChannelError::Refused { code, message, .. } => {
            assert_eq!(code.as_deref(), Some("invalid_client"));
            assert_eq!(message, "AADSTS7000215: Invalid client secret provided.");
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------
// Twilio
// ---------------------------------------------------------------------

pub fn twilio_options(server: &MockServer) -> TwilioOptions {
    TwilioOptions {
        account_sid: "AC123".to_owned(),
        auth_token: Secret::new("tw-token".to_owned()),
        endpoint: Some(server.uri()),
        ..TwilioOptions::default()
    }
}

pub fn sms_options(from: &str) -> SmsOptions {
    SmsOptions {
        from: Some(from.to_owned()),
        status_callback: Some("https://api.acme.test/api/webhooks/notify/twilio".to_owned()),
        ..SmsOptions::default()
    }
}

#[tokio::test]
async fn twilio_sms_posts_the_messages_api_form() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/2010-04-01/Accounts/AC123/Messages.json"))
        .and(header("authorization", basic("AC123", "tw-token").as_str()))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(json!({ "sid": "SM0001", "status": "queued" })),
        )
        .mount(&server)
        .await;

    let sent = TwilioSms::new(&twilio_options(&server), &sms_options("+15550001111"))
        .unwrap()
        .with_client(local_client())
        .send("+14155550100", &SmsMessage::new("Your code is 481 516"))
        .await
        .unwrap();

    assert_eq!(sent.provider, "twilio");
    assert_eq!(sent.message_id.as_deref(), Some("SM0001"));
    let form = form(&only_request(&server).await);
    assert_eq!(form_value(&form, "To"), Some("+14155550100"));
    assert_eq!(form_value(&form, "From"), Some("+15550001111"));
    assert_eq!(form_value(&form, "Body"), Some("Your code is 481 516"));
    assert_eq!(
        form_value(&form, "StatusCallback"),
        Some("https://api.acme.test/api/webhooks/notify/twilio")
    );
}

#[tokio::test]
async fn twilio_messaging_services_and_api_keys() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("authorization", basic("SK999", "tw-token").as_str()))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sid": "SM2" })))
        .mount(&server)
        .await;

    let options = TwilioOptions {
        api_key: Some("SK999".to_owned()),
        ..twilio_options(&server)
    };
    TwilioSms::new(&options, &SmsOptions::default())
        .unwrap()
        .with_client(local_client())
        .send("+14155550100", &SmsMessage::new("hi").from("MG42"))
        .await
        .unwrap();

    let form = form(&only_request(&server).await);
    assert_eq!(form_value(&form, "MessagingServiceSid"), Some("MG42"));
    assert_eq!(form_value(&form, "From"), None);
}

#[tokio::test]
async fn twilio_refusals_keep_the_error_code_and_21610_is_an_opt_out() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_string_contains(
            "To=%2B15550000000",
        ))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "code": 21211, "message": "Invalid 'To' Phone Number", "status": 400
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_string_contains(
            "To=%2B14155550100",
        ))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "code": 21610, "message": "Attempt to send to unsubscribed recipient", "status": 400
        })))
        .mount(&server)
        .await;
    let driver = TwilioSms::new(&twilio_options(&server), &sms_options("+15550001111"))
        .unwrap()
        .with_client(local_client());

    let error = driver
        .send("+15550000000", &SmsMessage::new("x"))
        .await
        .unwrap_err();
    assert!(
        matches!(&error, ChannelError::Refused { status: 400, code: Some(code), message, .. }
            if code == "21211" && message == "Invalid 'To' Phone Number"),
        "{error:?}"
    );
    let error = driver
        .send("+14155550100", &SmsMessage::new("x"))
        .await
        .unwrap_err();
    assert!(matches!(error, ChannelError::OptedOut { .. }), "{error:?}");
}

#[tokio::test]
async fn twilio_needs_a_sender() {
    let server = MockServer::start().await;
    let error = TwilioSms::new(&twilio_options(&server), &SmsOptions::default())
        .unwrap()
        .send("+14155550100", &SmsMessage::new("x"))
        .await
        .unwrap_err();
    assert!(matches!(error, ChannelError::Config(_)));
    assert!(TwilioSms::new(&TwilioOptions::default(), &SmsOptions::default()).is_err());
}

#[tokio::test]
async fn twilio_whatsapp_sends_templates_as_content() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sid": "SMwa" })))
        .mount(&server)
        .await;

    let whatsapp = WhatsAppOptions {
        from: Some("+15550001111".to_owned()),
        ..WhatsAppOptions::default()
    };
    let message = WhatsAppMessage::template(
        WhatsAppTemplate::new("HXb5b62575e6e4ff6129ad7c8efe1f983e", "")
            .parameter("Anu")
            .parameter("A-1042"),
    );
    let sent = TwilioWhatsApp::new(&twilio_options(&server), &whatsapp)
        .unwrap()
        .with_client(local_client())
        .send("+447400123456", &message)
        .await
        .unwrap();

    assert_eq!(sent.message_id.as_deref(), Some("SMwa"));
    let form = form(&only_request(&server).await);
    assert_eq!(form_value(&form, "To"), Some("whatsapp:+447400123456"));
    assert_eq!(form_value(&form, "From"), Some("whatsapp:+15550001111"));
    assert_eq!(
        form_value(&form, "ContentSid"),
        Some("HXb5b62575e6e4ff6129ad7c8efe1f983e")
    );
    let variables: Value =
        serde_json::from_str(form_value(&form, "ContentVariables").unwrap()).unwrap();
    assert_eq!(variables, json!({ "1": "Anu", "2": "A-1042" }));
    assert_eq!(form_value(&form, "Body"), None);
}

// ---------------------------------------------------------------------
// Vonage
// ---------------------------------------------------------------------

pub fn vonage_options(server: &MockServer) -> VonageOptions {
    VonageOptions {
        api_key: "abcd1234".to_owned(),
        api_secret: Secret::new("vonage-secret".to_owned()),
        endpoint: Some(server.uri()),
    }
}

#[tokio::test]
async fn vonage_sends_through_the_messages_api() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header(
            "authorization",
            basic("abcd1234", "vonage-secret").as_str(),
        ))
        .and(body_json(json!({
            "message_type": "text",
            "channel": "sms",
            "to": "447700900000",
            "from": "Agra",
            "text": "Your code is 481 516",
            "webhook_url": "https://api.acme.test/api/webhooks/notify/twilio",
        })))
        .respond_with(
            ResponseTemplate::new(202)
                .set_body_json(json!({ "message_uuid": "aaaaaaaa-bbbb-cccc-dddd-0123456789ab" })),
        )
        .mount(&server)
        .await;

    let sent = VonageSms::new(&vonage_options(&server), &sms_options("Agra"))
        .unwrap()
        .with_client(local_client())
        .send("+447700900000", &SmsMessage::new("Your code is 481 516"))
        .await
        .unwrap();

    assert_eq!(sent.provider, "vonage");
    assert_eq!(
        sent.message_id.as_deref(),
        Some("aaaaaaaa-bbbb-cccc-dddd-0123456789ab")
    );
}

#[tokio::test]
async fn vonage_problems_name_their_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "type": "https://developer.nexmo.com/api-errors/messages-olympus#1150",
            "title": "Invalid params",
            "detail": "The value of one or more parameters is invalid.",
        })))
        .mount(&server)
        .await;

    let error = VonageSms::new(&vonage_options(&server), &sms_options("Agra"))
        .unwrap()
        .with_client(local_client())
        .send("+447700900000", &SmsMessage::new("x"))
        .await
        .unwrap_err();

    match error {
        ChannelError::Refused {
            status,
            code,
            message,
            ..
        } => {
            assert_eq!(status, 422);
            assert_eq!(code.as_deref(), Some("1150"));
            assert_eq!(
                message,
                "Invalid params: The value of one or more parameters is invalid."
            );
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------
// Amazon SNS
// ---------------------------------------------------------------------

pub fn sns_options(server: &MockServer) -> SnsOptions {
    SnsOptions {
        region: "us-east-1".to_owned(),
        access_key_id: "AKIDEXAMPLE".to_owned(),
        secret_access_key: Secret::new("wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned()),
        endpoint: Some(server.uri()),
        ..SnsOptions::default()
    }
}

#[tokio::test]
async fn sns_publishes_to_the_number_signed_with_sigv4() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<PublishResponse xmlns=\"http://sns.amazonaws.com/doc/2010-03-31/\"><PublishResult><MessageId>94f20ce6-13c5-43a0-9a9e-ca52d816e90b</MessageId></PublishResult><ResponseMetadata><RequestId>f187a3c1</RequestId></ResponseMetadata></PublishResponse>",
        ))
        .mount(&server)
        .await;

    let sent = SnsSms::new(&sns_options(&server), &sms_options("Agra"))
        .unwrap()
        .with_client(local_client())
        .send("+14155550100", &SmsMessage::new("Your code is 481 516"))
        .await
        .unwrap();

    assert_eq!(sent.provider, "sns");
    assert_eq!(
        sent.message_id.as_deref(),
        Some("94f20ce6-13c5-43a0-9a9e-ca52d816e90b")
    );
    let request = only_request(&server).await;
    let authorization = header_value(&request, "authorization");
    assert!(
        authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"),
        "{authorization}"
    );
    assert!(authorization.contains(
        "/us-east-1/sns/aws4_request, SignedHeaders=content-type;host;x-amz-date, Signature="
    ));
    assert_eq!(header_value(&request, "x-amz-date").len(), 16);
    let form = form(&request);
    assert_eq!(form_value(&form, "Action"), Some("Publish"));
    assert_eq!(form_value(&form, "PhoneNumber"), Some("+14155550100"));
    assert_eq!(form_value(&form, "Message"), Some("Your code is 481 516"));
    assert_eq!(
        form_value(&form, "MessageAttributes.entry.1.Name"),
        Some("AWS.SNS.SMS.SMSType")
    );
    assert_eq!(
        form_value(&form, "MessageAttributes.entry.1.Value.StringValue"),
        Some("Transactional")
    );
    assert_eq!(
        form_value(&form, "MessageAttributes.entry.2.Name"),
        Some("AWS.SNS.SMS.SenderID")
    );
    assert_eq!(
        form_value(&form, "MessageAttributes.entry.2.Value.StringValue"),
        Some("Agra")
    );
}

#[tokio::test]
async fn sns_errors_are_read_from_the_xml() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_string(
            "<ErrorResponse><Error><Type>Sender</Type><Code>InvalidParameter</Code><Message>Invalid parameter: PhoneNumber Reason: +1 is not valid to publish to</Message></Error><RequestId>x</RequestId></ErrorResponse>",
        ))
        .mount(&server)
        .await;

    let error = SnsSms::new(&sns_options(&server), &SmsOptions::default())
        .unwrap()
        .with_client(local_client())
        .send("+1", &SmsMessage::new("x").from("+15550001111"))
        .await
        .unwrap_err();

    assert!(
        matches!(&error, ChannelError::Refused { code: Some(code), message, .. }
            if code == "InvalidParameter" && message.starts_with("Invalid parameter")),
        "{error:?}"
    );
    let form = form(&only_request(&server).await);
    assert_eq!(
        form_value(&form, "MessageAttributes.entry.2.Name"),
        Some("AWS.MM.SMS.OriginationNumber"),
        "a number is the origination number"
    );
}

// ---------------------------------------------------------------------
// WhatsApp Cloud API
// ---------------------------------------------------------------------

pub fn meta_options(server: &MockServer) -> MetaOptions {
    MetaOptions {
        phone_number_id: "1055".to_owned(),
        access_token: Secret::new("EAAG-token".to_owned()),
        endpoint: Some(server.uri()),
        ..MetaOptions::default()
    }
}

#[tokio::test]
async fn the_cloud_api_gets_text_messages() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v21.0/1055/messages"))
        .and(header("authorization", "Bearer EAAG-token"))
        .and(body_json(json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": "447400123456",
            "type": "text",
            "text": { "preview_url": false, "body": "Your order has shipped" },
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messaging_product": "whatsapp",
            "contacts": [{ "input": "447400123456", "wa_id": "447400123456" }],
            "messages": [{ "id": "wamid.HBgLNDQ3NDAwMTIzNDU2" }],
        })))
        .mount(&server)
        .await;

    let sent = MetaWhatsApp::new(&meta_options(&server))
        .unwrap()
        .with_client(local_client())
        .send(
            "+447400123456",
            &WhatsAppMessage::text("Your order has shipped"),
        )
        .await
        .unwrap();

    assert_eq!(sent.provider, "meta");
    assert_eq!(
        sent.message_id.as_deref(),
        Some("wamid.HBgLNDQ3NDAwMTIzNDU2")
    );
}

#[tokio::test]
async fn cloud_api_errors_keep_their_code_and_131050_is_an_opt_out() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_string_contains("447400000001"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "(#131030) Recipient phone number not in allowed list", "type": "OAuthException", "code": 131030, "fbtrace_id": "A" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_string_contains("447400000002"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "User has stopped marketing messages", "code": 131050 }
        })))
        .mount(&server)
        .await;
    let driver = MetaWhatsApp::new(&meta_options(&server))
        .unwrap()
        .with_client(local_client());

    let error = driver
        .send("+447400000001", &WhatsAppMessage::text("x"))
        .await
        .unwrap_err();
    assert!(
        matches!(&error, ChannelError::Refused { code: Some(code), .. } if code == "131030"),
        "{error:?}"
    );
    let error = driver
        .send("+447400000002", &WhatsAppMessage::text("x"))
        .await
        .unwrap_err();
    assert!(matches!(error, ChannelError::OptedOut { .. }), "{error:?}");
}

// ---------------------------------------------------------------------
// Web push
// ---------------------------------------------------------------------

/// A browser's subscription keys, and how it reads a push.
pub struct Browser {
    private: EphemeralPrivateKey,
    pub p256dh: Vec<u8>,
    pub auth: [u8; 16],
}

impl Browser {
    pub fn new() -> Self {
        let private = EphemeralPrivateKey::generate(&ECDH_P256, &SystemRandom::new()).unwrap();
        let p256dh = private.compute_public_key().unwrap().as_ref().to_vec();
        Self {
            private,
            p256dh,
            auth: *b"0123456789abcdef",
        }
    }

    pub fn target(&self, endpoint: &str) -> PushTarget {
        PushTarget {
            endpoint: endpoint.to_owned(),
            p256dh: URL_SAFE_NO_PAD.encode(&self.p256dh),
            auth: URL_SAFE_NO_PAD.encode(self.auth),
        }
    }

    /// Decrypts an `aes128gcm` body (RFC 8291).
    pub fn decrypt(self, body: &[u8]) -> Vec<u8> {
        let salt = &body[..16];
        let as_public = &body[21..86];
        let mut record = body[86..].to_vec();
        let ecdh = agreement::agree_ephemeral(
            self.private,
            &UnparsedPublicKey::new(&ECDH_P256, as_public),
            |secret| secret.to_vec(),
        )
        .unwrap();
        let prk_key = hmac_sha256(&self.auth, &ecdh);
        let mut info = b"WebPush: info\0".to_vec();
        info.extend_from_slice(&self.p256dh);
        info.extend_from_slice(as_public);
        info.push(1);
        let ikm = hmac_sha256(&prk_key, &info);
        let prk = hmac_sha256(salt, &ikm);
        let cek = &hmac_sha256(&prk, b"Content-Encoding: aes128gcm\0\x01")[..16];
        let nonce: [u8; 12] = hmac_sha256(&prk, b"Content-Encoding: nonce\0\x01")[..12]
            .try_into()
            .unwrap();
        let key = LessSafeKey::new(UnboundKey::new(&AES_128_GCM, cek).unwrap());
        let plain = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::empty(),
                &mut record,
            )
            .unwrap();
        plain[..plain.len() - 1].to_vec()
    }
}

pub fn web_push_options(key: &VapidKey) -> WebPushOptions {
    WebPushOptions {
        public_key: Some(key.application_server_key()),
        private_key: Secret::new(key.private_key()),
        subject: "mailto:ops@acme.test".to_owned(),
        ..WebPushOptions::default()
    }
}

#[tokio::test]
async fn pushes_are_encrypted_for_the_browser_and_signed_with_vapid() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/push/v2/abc"))
        .and(header("content-encoding", "aes128gcm"))
        .and(header("content-type", "application/octet-stream"))
        .and(header("ttl", "60"))
        .and(header("urgency", "high"))
        .and(header("topic", "deals"))
        .respond_with(
            ResponseTemplate::new(201).insert_header("location", "https://push.test/m/msg-1"),
        )
        .mount(&server)
        .await;
    let key = VapidKey::generate();
    let browser = Browser::new();
    let target = browser.target(&format!("{}/push/v2/abc", server.uri()));
    let message = WebPushMessage::new("Deal won")
        .body("Agra renewal")
        .url("/deals/42")
        .ttl_secs(60)
        .urgency(Urgency::High)
        .topic("deals");

    let sent = VapidWebPush::new(&web_push_options(&key))
        .unwrap()
        .with_client(local_client())
        .send(&target, &message)
        .await
        .unwrap();

    assert_eq!(sent.provider, "web_push");
    assert_eq!(
        sent.message_id.as_deref(),
        Some("https://push.test/m/msg-1")
    );
    let request = only_request(&server).await;
    let authorization = header_value(&request, "authorization").to_owned();
    let (token, k) = authorization
        .strip_prefix("vapid t=")
        .and_then(|rest| rest.split_once(", k="))
        .expect("vapid t=..., k=...");
    assert_eq!(k, key.application_server_key());
    let point = key.public_key();
    let jws = Jws::parse(token).unwrap();
    jws.verify(&VerifyingKey::ec(Algorithm::ES256, &point[1..33], &point[33..65]).unwrap())
        .unwrap();
    let claims: Value = jws.claims().unwrap();
    assert_eq!(claims["aud"], server.uri());
    assert_eq!(claims["sub"], "mailto:ops@acme.test");
    let payload: Value = serde_json::from_slice(&browser.decrypt(&request.body)).unwrap();
    assert_eq!(
        payload,
        json!({ "title": "Deal won", "body": "Agra renewal", "url": "/deals/42" })
    );
}

#[tokio::test]
async fn the_configured_ttl_applies_without_one_on_the_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("ttl", "86400"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;
    let key = VapidKey::generate();
    let target = Browser::new().target(&format!("{}/push/1", server.uri()));

    VapidWebPush::new(&web_push_options(&key))
        .unwrap()
        .with_client(local_client())
        .send(&target, &WebPushMessage::new("hi"))
        .await
        .unwrap();
}

#[tokio::test]
async fn gone_subscriptions_and_refusals() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/push/gone"))
        .respond_with(ResponseTemplate::new(410))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/push/bad"))
        .respond_with(ResponseTemplate::new(403).set_body_string("invalid JWT provided"))
        .mount(&server)
        .await;
    let key = VapidKey::generate();
    let driver = VapidWebPush::new(&web_push_options(&key))
        .unwrap()
        .with_client(local_client());
    let browser = Browser::new();

    let gone = driver
        .send(
            &browser.target(&format!("{}/push/gone", server.uri())),
            &WebPushMessage::new("x"),
        )
        .await
        .unwrap_err();
    assert!(matches!(gone, ChannelError::Gone));
    let refused = driver
        .send(
            &browser.target(&format!("{}/push/bad", server.uri())),
            &WebPushMessage::new("x"),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(&refused, ChannelError::Refused { status: 403, message, .. } if message == "invalid JWT provided"),
        "{refused:?}"
    );
}

#[tokio::test]
async fn endpoints_at_internal_addresses_are_refused() {
    let server = MockServer::start().await;
    let key = VapidKey::generate();
    // The driver's own client keeps the default SSRF policy.
    let error = VapidWebPush::new(&web_push_options(&key))
        .unwrap()
        .send(
            &Browser::new().target(&format!("{}/push/1", server.uri())),
            &WebPushMessage::new("x"),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(error, ChannelError::Unreachable { .. }),
        "{error:?}"
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn malformed_subscription_keys_are_invalid_recipients() {
    let key = VapidKey::generate();
    let driver = VapidWebPush::new(&web_push_options(&key)).unwrap();
    let target = PushTarget {
        endpoint: "https://fcm.googleapis.com/fcm/send/x".to_owned(),
        p256dh: "%%%".to_owned(),
        auth: "AAAA".to_owned(),
    };
    let error = driver
        .send(&target, &WebPushMessage::new("x"))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ChannelError::InvalidRecipient(_)),
        "{error:?}"
    );
    assert!(
        VapidWebPush::new(&WebPushOptions::default()).is_err(),
        "a key pair is required"
    );
}

/// A push endpoint's URL is the subscription's secret: its path stays out of the
/// error a failed send gives.
#[tokio::test]
async fn a_failed_push_does_not_name_the_path_of_its_endpoint() {
    let key = VapidKey::generate();
    let (client, server) = HttpClient::fake();
    server.on(
        Method::POST,
        "https://push.example/*",
        FakeResponse::connection_failed(),
    );
    let driver = VapidWebPush::new(&web_push_options(&key))
        .unwrap()
        .with_client(client);
    let device = Browser::new().target("https://push.example/SECRETPATH");
    let error = driver
        .send(&device, &WebPushMessage::new("hello"))
        .await
        .unwrap_err();
    let text = format!("{error} {error:?}");
    assert!(!text.contains("SECRETPATH"), "{text}");
}
