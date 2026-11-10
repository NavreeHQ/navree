//! The text-message providers' own test modes: one real request to each.
//! Opt in: a test runs only when the keys for its provider are set, and
//! prints `SKIPPED` and passes otherwise, so a fork or a machine without the
//! accounts is not told it failed. They are `#[ignore]`d too; run them with
//! `-- --ignored sandbox_` (the `sandbox` job of
//! `.github/workflows/integrations.yml` does, with the keys from the
//! repository's secrets).
//!
//! | Provider | Mode | Variables |
//! |---|---|---|
//! | Twilio SMS | the account's test credentials and the magic sender `+15005550006`: Twilio checks the request and sends nothing | `NAVREE_SANDBOX_TWILIO_SID`, `NAVREE_SANDBOX_TWILIO_TOKEN` |
//! | WhatsApp Cloud API | the test number of a Meta app and its `hello_world` template, to a recipient the app lists | `NAVREE_SANDBOX_META_TOKEN`, `_PHONE_NUMBER_ID`, `_TO` |
//! | Vonage SMS | no test mode: a trial account sends one real message to a number it has verified | `NAVREE_SANDBOX_VONAGE_KEY`, `_SECRET`, `_FROM`, `_TO` |
//!
//! Amazon SNS has no sandbox that sends to an arbitrary number (its SMS
//! sandbox needs each recipient verified in the account), so it is not here.
//! What a sandbox cannot show: delivery receipts need a public address for the
//! provider to call. The Meta and Vonage tests send one message each, which
//! counts against the account's free allowance.

use navree_contracts::Secret;
use navree_notify::{
    MetaOptions, MetaWhatsApp, SmsDriver, SmsMessage, SmsOptions, TwilioOptions, TwilioSms,
    VonageOptions, VonageSms, WhatsAppDriver, WhatsAppMessage, WhatsAppTemplate,
};

/// The value of `variable` when it is set and not empty.
fn sandbox(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        _ => {
            println!("SKIPPED {test}: set {variable}");
            None
        }
    }
}

#[tokio::test]
#[ignore = "calls the providers' sandboxes"]
async fn sandbox_twilio_accepts_a_message_with_its_test_credentials() {
    let test = "sandbox_twilio_accepts_a_message_with_its_test_credentials";
    let (Some(sid), Some(token)) = (
        sandbox("NAVREE_SANDBOX_TWILIO_SID", test),
        sandbox("NAVREE_SANDBOX_TWILIO_TOKEN", test),
    ) else {
        return;
    };
    let twilio = TwilioOptions {
        account_sid: sid,
        auth_token: Secret::new(token),
        ..TwilioOptions::default()
    };
    // +15005550006 is the sender Twilio's test credentials accept.
    let sms = SmsOptions {
        from: Some("+15005550006".to_owned()),
        ..SmsOptions::default()
    };

    let sent = TwilioSms::new(&twilio, &sms)
        .expect("a driver")
        .send(
            "+919876543210",
            &SmsMessage::new("Your OTP is 482915. It is valid for 10 minutes."),
        )
        .await
        .expect("Twilio accepts the request");

    assert_eq!(sent.provider, "twilio");
    assert!(sent.message_id.is_some(), "{sent:?}");
}

#[tokio::test]
#[ignore = "calls the providers' sandboxes"]
async fn sandbox_whatsapp_cloud_api_sends_its_hello_world_template_to_a_listed_number() {
    let test = "sandbox_whatsapp_cloud_api_sends_its_hello_world_template_to_a_listed_number";
    let (Some(token), Some(phone_number_id), Some(to)) = (
        sandbox("NAVREE_SANDBOX_META_TOKEN", test),
        sandbox("NAVREE_SANDBOX_META_PHONE_NUMBER_ID", test),
        sandbox("NAVREE_SANDBOX_META_TO", test),
    ) else {
        return;
    };
    let meta = MetaOptions {
        phone_number_id,
        access_token: Secret::new(token),
        ..MetaOptions::default()
    };

    let sent = MetaWhatsApp::new(&meta)
        .expect("a driver")
        .send(
            &to,
            &WhatsAppMessage::template(WhatsAppTemplate::new("hello_world", "en_US")),
        )
        .await
        .expect("the Cloud API accepts the message");

    assert_eq!(sent.provider, "meta");
    assert!(sent.message_id.is_some(), "{sent:?}");
}

#[tokio::test]
#[ignore = "calls the providers' sandboxes"]
async fn sandbox_vonage_sends_one_message_to_a_verified_number() {
    let test = "sandbox_vonage_sends_one_message_to_a_verified_number";
    let (Some(key), Some(secret), Some(from), Some(to)) = (
        sandbox("NAVREE_SANDBOX_VONAGE_KEY", test),
        sandbox("NAVREE_SANDBOX_VONAGE_SECRET", test),
        sandbox("NAVREE_SANDBOX_VONAGE_FROM", test),
        sandbox("NAVREE_SANDBOX_VONAGE_TO", test),
    ) else {
        return;
    };
    let vonage = VonageOptions {
        api_key: key,
        api_secret: Secret::new(secret),
        ..VonageOptions::default()
    };
    let sms = SmsOptions {
        from: Some(from),
        ..SmsOptions::default()
    };

    let sent = VonageSms::new(&vonage, &sms)
        .expect("a driver")
        .send(&to, &SmsMessage::new("Sandbox test from Navree."))
        .await
        .expect("Vonage accepts the message");

    assert_eq!(sent.provider, "vonage");
    assert!(sent.message_id.is_some(), "{sent:?}");
}
