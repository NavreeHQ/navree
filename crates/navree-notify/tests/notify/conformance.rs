//! The driver conformance suites: each channel's fake and every real driver
//! (against a local mock server) answer the same way to an accepted
//! message, a refused one and, where the provider has one, an opt-out or a
//! gone subscription.

use std::sync::Arc;

use navree_crypto::webpush::VapidKey;
use navree_notify::{
    ChannelError, LogSms, LogWhatsApp, MetaWhatsApp, SentMessage, SlackApi, SlackDriver, SlackFake,
    SlackMessage, SlackOptions, SlackRoute, SmsDriver, SmsFake, SmsMessage, SmsOptions, SnsSms,
    TeamsApi, TeamsDriver, TeamsFake, TeamsMessage, TeamsOptions, TeamsRoute, TwilioSms,
    TwilioWhatsApp, VapidWebPush, VonageSms, WebPushDriver, WebPushFake, WebPushMessage,
    WhatsAppDriver, WhatsAppFake, WhatsAppMessage, WhatsAppOptions,
};
use serde_json::json;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::drivers::{
    Browser, local_client, meta_options, sms_options, sns_options, twilio_options, vonage_options,
    web_push_options,
};

const NUMBER: &str = "+14155550100";

/// What the provider does with the next message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scenario {
    Accept,
    Refuse,
    OptOut,
    Gone,
}

/// A mock server answering every POST with `response`.
async fn answering(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

fn check_accepted(driver: &str, result: Result<SentMessage, ChannelError>, needs_id: bool) {
    let sent = result.unwrap_or_else(|error| panic!("{driver}: accepted message failed: {error}"));
    assert!(!sent.provider.is_empty(), "{driver}: names its provider");
    if needs_id {
        assert!(
            sent.message_id.as_deref().is_some_and(|id| !id.is_empty()),
            "{driver}: returns the provider's message id"
        );
    }
}

fn check_refused(driver: &str, result: Result<SentMessage, ChannelError>) {
    match result {
        Err(ChannelError::Refused {
            status, message, ..
        }) => {
            assert!(status >= 400, "{driver}: refusal status {status}");
            assert!(!message.is_empty(), "{driver}: refusal message");
        }
        other => panic!("{driver}: expected a refusal, got {other:?}"),
    }
}

fn check(
    driver: &str,
    scenario: Scenario,
    result: Result<SentMessage, ChannelError>,
    needs_id: bool,
) {
    match scenario {
        Scenario::Accept => check_accepted(driver, result, needs_id),
        Scenario::Refuse => check_refused(driver, result),
        Scenario::OptOut => assert!(
            matches!(result, Err(ChannelError::OptedOut { .. })),
            "{driver}: expected an opt-out, got {result:?}"
        ),
        Scenario::Gone => assert!(
            matches!(result, Err(ChannelError::Gone)),
            "{driver}: expected a gone subscription, got {result:?}"
        ),
    }
}

// ---------------------------------------------------------------------
// SMS
// ---------------------------------------------------------------------

/// An SMS driver set up for `scenario`, and the server it talks to.
async fn sms_driver(name: &str, scenario: Scenario) -> (Arc<dyn SmsDriver>, Option<MockServer>) {
    match name {
        "fake" => {
            let fake = SmsFake::new();
            match scenario {
                Scenario::Refuse => fake.refuse("refused by the fake"),
                Scenario::OptOut => fake.opt_out_at_provider(NUMBER),
                _ => {}
            }
            (fake.driver(), None)
        }
        "log" => (Arc::new(LogSms::new(Some("Agra".into()))), None),
        "twilio" => {
            let server = answering(match scenario {
                Scenario::Accept => {
                    ResponseTemplate::new(201).set_body_json(json!({ "sid": "SM1" }))
                }
                Scenario::Refuse => ResponseTemplate::new(400).set_body_json(
                    json!({ "code": 21211, "message": "Invalid 'To' Phone Number" }),
                ),
                _ => ResponseTemplate::new(400)
                    .set_body_json(json!({ "code": 21610, "message": "Unsubscribed recipient" })),
            })
            .await;
            let driver = TwilioSms::new(&twilio_options(&server), &sms_options("+15550001111"))
                .unwrap()
                .with_client(local_client());
            (Arc::new(driver), Some(server))
        }
        "vonage" => {
            let server = answering(match scenario {
                Scenario::Accept => ResponseTemplate::new(202).set_body_json(
                    json!({ "message_uuid": "aaaaaaaa-bbbb-cccc-dddd-0123456789ab" }),
                ),
                _ => ResponseTemplate::new(422).set_body_json(json!({
                    "type": "https://developer.nexmo.com/api-errors/messages-olympus#1150",
                    "title": "Invalid params",
                })),
            })
            .await;
            let driver = VonageSms::new(&vonage_options(&server), &sms_options("Agra"))
                .unwrap()
                .with_client(local_client());
            (Arc::new(driver), Some(server))
        }
        "sns" => {
            let server = answering(match scenario {
                Scenario::Accept => ResponseTemplate::new(200).set_body_string(
                    "<PublishResponse><PublishResult><MessageId>94f20ce6</MessageId></PublishResult></PublishResponse>",
                ),
                _ => ResponseTemplate::new(400).set_body_string(
                    "<ErrorResponse><Error><Code>InvalidParameter</Code><Message>Invalid parameter</Message></Error></ErrorResponse>",
                ),
            })
            .await;
            let driver = SnsSms::new(&sns_options(&server), &SmsOptions::default())
                .unwrap()
                .with_client(local_client());
            (Arc::new(driver), Some(server))
        }
        other => panic!("unknown driver {other}"),
    }
}

#[tokio::test]
async fn sms_drivers_conform() {
    let suites: [(&str, &[Scenario]); 5] = [
        (
            "fake",
            &[Scenario::Accept, Scenario::Refuse, Scenario::OptOut],
        ),
        ("log", &[Scenario::Accept]),
        (
            "twilio",
            &[Scenario::Accept, Scenario::Refuse, Scenario::OptOut],
        ),
        ("vonage", &[Scenario::Accept, Scenario::Refuse]),
        ("sns", &[Scenario::Accept, Scenario::Refuse]),
    ];
    for (name, scenarios) in suites {
        for &scenario in scenarios {
            let (driver, _server) = sms_driver(name, scenario).await;
            let result = driver
                .send(NUMBER, &SmsMessage::new("Your code is 481 516"))
                .await;
            check(name, scenario, result, true);
        }
    }
}

// ---------------------------------------------------------------------
// WhatsApp
// ---------------------------------------------------------------------

async fn whatsapp_driver(
    name: &str,
    scenario: Scenario,
) -> (Arc<dyn WhatsAppDriver>, Option<MockServer>) {
    match name {
        "fake" => {
            let fake = WhatsAppFake::new();
            match scenario {
                Scenario::Refuse => fake.refuse("refused by the fake"),
                Scenario::OptOut => fake.opt_out_at_provider(NUMBER),
                _ => {}
            }
            (fake.driver(), None)
        }
        "log" => (Arc::new(LogWhatsApp::new()), None),
        "twilio" => {
            let server = answering(match scenario {
                Scenario::Accept => {
                    ResponseTemplate::new(201).set_body_json(json!({ "sid": "SMwa" }))
                }
                Scenario::Refuse => ResponseTemplate::new(400).set_body_json(
                    json!({ "code": 63016, "message": "Outside the allowed window" }),
                ),
                _ => ResponseTemplate::new(400)
                    .set_body_json(json!({ "code": 21610, "message": "Unsubscribed recipient" })),
            })
            .await;
            let options = WhatsAppOptions {
                from: Some("+15550001111".into()),
                ..WhatsAppOptions::default()
            };
            let driver = TwilioWhatsApp::new(&twilio_options(&server), &options)
                .unwrap()
                .with_client(local_client());
            (Arc::new(driver), Some(server))
        }
        "meta" => {
            let server = answering(match scenario {
                Scenario::Accept => ResponseTemplate::new(200)
                    .set_body_json(json!({ "messages": [{ "id": "wamid.X" }] })),
                Scenario::Refuse => ResponseTemplate::new(400).set_body_json(
                    json!({ "error": { "code": 131030, "message": "Recipient not allowed" } }),
                ),
                _ => ResponseTemplate::new(400).set_body_json(
                    json!({ "error": { "code": 131050, "message": "User stopped marketing messages" } }),
                ),
            })
            .await;
            let driver = MetaWhatsApp::new(&meta_options(&server))
                .unwrap()
                .with_client(local_client());
            (Arc::new(driver), Some(server))
        }
        other => panic!("unknown driver {other}"),
    }
}

#[tokio::test]
async fn whatsapp_drivers_conform() {
    let all = [Scenario::Accept, Scenario::Refuse, Scenario::OptOut];
    let suites: [(&str, &[Scenario]); 4] = [
        ("fake", &all),
        ("log", &[Scenario::Accept]),
        ("twilio", &all),
        ("meta", &all),
    ];
    for (name, scenarios) in suites {
        for &scenario in scenarios {
            let (driver, _server) = whatsapp_driver(name, scenario).await;
            let result = driver.send(NUMBER, &WhatsAppMessage::text("hi")).await;
            check(name, scenario, result, true);
        }
    }
}

// ---------------------------------------------------------------------
// Slack and Teams (webhook routes)
// ---------------------------------------------------------------------

#[tokio::test]
async fn slack_drivers_conform() {
    for scenario in [Scenario::Accept, Scenario::Refuse] {
        let fake = SlackFake::new();
        if scenario == Scenario::Refuse {
            fake.refuse("invalid_payload");
        }
        let route = SlackRoute::webhook("https://hooks.slack.com/services/T/B/X");
        check(
            "fake",
            scenario,
            fake.send(&route, &SlackMessage::new("x")).await,
            false,
        );

        let server = answering(match scenario {
            Scenario::Accept => ResponseTemplate::new(200).set_body_string("ok"),
            _ => ResponseTemplate::new(400).set_body_string("invalid_payload"),
        })
        .await;
        let driver = SlackApi::new(&SlackOptions::default())
            .unwrap()
            .with_client(local_client());
        let route = SlackRoute::webhook(format!("{}/services/T/B/X", server.uri()));
        check(
            "slack",
            scenario,
            driver.send(&route, &SlackMessage::new("x")).await,
            false,
        );
    }
}

#[tokio::test]
async fn teams_drivers_conform() {
    for scenario in [Scenario::Accept, Scenario::Refuse] {
        let fake = TeamsFake::new();
        if scenario == Scenario::Refuse {
            fake.refuse("Bad payload");
        }
        let route = TeamsRoute::webhook("https://prod.logic.azure.com/workflows/x");
        check(
            "fake",
            scenario,
            fake.send(&route, &TeamsMessage::new("x")).await,
            false,
        );

        let server = answering(match scenario {
            Scenario::Accept => ResponseTemplate::new(202),
            _ => ResponseTemplate::new(400).set_body_json(
                json!({ "error": { "code": "InvalidRequestContent", "message": "Bad payload" } }),
            ),
        })
        .await;
        let driver = TeamsApi::new(&TeamsOptions::default())
            .unwrap()
            .with_client(local_client());
        let route = TeamsRoute::webhook(format!("{}/workflows/x", server.uri()));
        check(
            "teams",
            scenario,
            driver.send(&route, &TeamsMessage::new("x")).await,
            false,
        );
    }
}

// ---------------------------------------------------------------------
// Web push
// ---------------------------------------------------------------------

#[tokio::test]
async fn web_push_drivers_conform() {
    for scenario in [Scenario::Accept, Scenario::Refuse, Scenario::Gone] {
        let endpoint = "https://fcm.googleapis.com/fcm/send/abc";
        let fake = WebPushFake::new();
        match scenario {
            Scenario::Refuse => fake.refuse("bad request"),
            Scenario::Gone => fake.expire(endpoint),
            _ => {}
        }
        let target = Browser::new().target(endpoint);
        check(
            "fake",
            scenario,
            fake.send(&target, &WebPushMessage::new("x")).await,
            false,
        );

        let server = answering(match scenario {
            Scenario::Accept => ResponseTemplate::new(201),
            Scenario::Refuse => ResponseTemplate::new(400).set_body_string("bad request"),
            _ => ResponseTemplate::new(410),
        })
        .await;
        let driver = VapidWebPush::new(&web_push_options(&VapidKey::generate()))
            .unwrap()
            .with_client(local_client());
        let target = Browser::new().target(&format!("{}/fcm/send/abc", server.uri()));
        check(
            "web_push",
            scenario,
            driver.send(&target, &WebPushMessage::new("x")).await,
            false,
        );
    }
}
