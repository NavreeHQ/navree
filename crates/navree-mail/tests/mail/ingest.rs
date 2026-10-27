//! Provider webhook payloads parsed into `ProviderEvent`s.

use navree_mail::{MailEventKind, ProviderEvent};
use serde_json::json;

#[test]
fn ses_events() {
    let delivery = ProviderEvent::from_ses(&json!({
        "eventType": "Delivery",
        "mail": { "messageId": "m-1", "timestamp": "2026-10-27T10:00:00.000Z", "destination": ["a@x.test", "b@x.test"] },
        "delivery": { "recipients": ["a@x.test", "b@x.test"], "timestamp": "2026-10-27T10:00:02.000Z" },
    }));
    assert_eq!(delivery.len(), 2);
    assert_eq!(delivery[0].kind, MailEventKind::Delivered);
    assert_eq!(delivery[0].provider, "ses");
    assert_eq!(delivery[0].provider_id.as_deref(), Some("m-1"));
    assert_eq!(delivery[1].recipient.as_deref(), Some("b@x.test"));
    assert_eq!(
        delivery[0].occurred_at.unwrap().to_rfc3339(),
        "2026-10-27T10:00:02+00:00"
    );

    let transient = ProviderEvent::from_ses(&json!({
        "notificationType": "Bounce",
        "mail": { "messageId": "m-2" },
        "bounce": { "bounceType": "Transient", "bounceSubType": "MailboxFull", "bouncedRecipients": [{ "emailAddress": "a@x.test" }] },
    }));
    assert_eq!(transient[0].kind, MailEventKind::Bounced);
    assert!(!transient[0].permanent);
    assert_eq!(transient[0].detail.as_deref(), Some("MailboxFull"));

    let complaint = ProviderEvent::from_ses(&json!({
        "eventType": "Complaint",
        "mail": { "messageId": "m-3" },
        "complaint": { "complainedRecipients": [{ "emailAddress": "a@x.test" }] },
    }));
    assert_eq!(complaint[0].kind, MailEventKind::Complained);
    assert!(complaint[0].suppresses());

    let click = ProviderEvent::from_ses(&json!({
        "eventType": "Click",
        "mail": { "messageId": "m-4", "destination": ["a@x.test"] },
        "click": { "link": "https://acme.test/x", "timestamp": "2026-10-27T10:05:00.000Z" },
    }));
    assert_eq!(click[0].kind, MailEventKind::Clicked);
    assert_eq!(click[0].url.as_deref(), Some("https://acme.test/x"));

    let open = ProviderEvent::from_ses(
        &json!({ "eventType": "Open", "mail": { "messageId": "m-5" }, "open": {} }),
    );
    assert_eq!(open[0].kind, MailEventKind::Opened);
    let delayed = ProviderEvent::from_ses(&json!({
        "eventType": "DeliveryDelay",
        "mail": { "messageId": "m-6" },
        "deliveryDelay": { "delayedRecipients": [{ "emailAddress": "a@x.test" }] },
    }));
    assert_eq!(delayed[0].kind, MailEventKind::Deferred);

    assert!(ProviderEvent::from_ses(&json!({ "eventType": "Send", "mail": {} })).is_empty());
}

#[test]
fn mailgun_events() {
    let event =
        |data: serde_json::Value| ProviderEvent::from_mailgun(&json!({ "event-data": data }));

    let temporary = event(json!({
        "event": "failed", "severity": "temporary", "recipient": "a@x.test",
        "message": { "headers": { "message-id": "<20261027.1@mg.acme.test>" } },
        "delivery-status": { "description": "", "message": "Mailbox full" },
    }));
    assert_eq!(temporary[0].kind, MailEventKind::Bounced);
    assert!(!temporary[0].permanent);
    assert_eq!(
        temporary[0].provider_id.as_deref(),
        Some("20261027.1@mg.acme.test")
    );
    assert_eq!(temporary[0].detail.as_deref(), Some("Mailbox full"));

    let clicked = event(
        json!({ "event": "clicked", "url": "https://acme.test", "timestamp": 1_793_088_000.25 }),
    );
    assert_eq!(clicked[0].kind, MailEventKind::Clicked);
    assert_eq!(clicked[0].url.as_deref(), Some("https://acme.test"));
    assert_eq!(
        clicked[0].occurred_at.unwrap().timestamp_millis(),
        1_793_088_000_250
    );

    for (name, kind) in [
        ("delivered", MailEventKind::Delivered),
        ("complained", MailEventKind::Complained),
        ("opened", MailEventKind::Opened),
    ] {
        assert_eq!(event(json!({ "event": name }))[0].kind, kind);
    }
    assert!(event(json!({ "event": "accepted" })).is_empty());
}

#[test]
fn postmark_events() {
    let bounce = |kind: &str, inactive: bool| {
        ProviderEvent::from_postmark(&json!({
            "RecordType": "Bounce", "MessageID": "p-1", "Type": kind, "Email": "a@x.test", "Inactive": inactive,
        }))
        .remove(0)
    };
    assert!(bounce("HardBounce", false).permanent);
    assert!(bounce("BadEmailAddress", false).permanent);
    assert!(!bounce("SoftBounce", false).permanent);
    assert!(
        bounce("SoftBounce", true).permanent,
        "an inactive recipient"
    );

    let click = ProviderEvent::from_postmark(&json!({
        "RecordType": "Click", "MessageID": "p-2", "Recipient": "a@x.test",
        "OriginalLink": "https://acme.test/y", "ReceivedAt": "2026-10-27T10:00:00Z",
    }));
    assert_eq!(click[0].kind, MailEventKind::Clicked);
    assert_eq!(click[0].url.as_deref(), Some("https://acme.test/y"));
    assert_eq!(click[0].recipient.as_deref(), Some("a@x.test"));

    let complaint = ProviderEvent::from_postmark(
        &json!({ "RecordType": "SpamComplaint", "MessageID": "p-3", "Email": "a@x.test" }),
    );
    assert_eq!(complaint[0].kind, MailEventKind::Complained);
    assert!(
        ProviderEvent::from_postmark(&json!({ "RecordType": "SubscriptionChange" })).is_empty()
    );
}

#[test]
fn resend_events() {
    let bounced = ProviderEvent::from_resend(&json!({
        "type": "email.bounced",
        "created_at": "2026-10-27T10:00:00.000Z",
        "data": { "email_id": "r-1", "to": ["a@x.test", "b@x.test"], "bounce": { "type": "Permanent", "message": "No such user" } },
    }));
    assert_eq!(bounced.len(), 2);
    assert!(bounced[0].permanent);
    assert_eq!(bounced[0].detail.as_deref(), Some("No such user"));
    assert_eq!(bounced[1].recipient.as_deref(), Some("b@x.test"));

    let transient = ProviderEvent::from_resend(&json!({
        "type": "email.bounced", "data": { "email_id": "r-2", "to": "a@x.test", "bounce": { "type": "Transient" } },
    }));
    assert!(!transient[0].permanent);
    assert_eq!(transient[0].recipient.as_deref(), Some("a@x.test"));

    let clicked = ProviderEvent::from_resend(&json!({
        "type": "email.clicked", "data": { "email_id": "r-3", "to": ["a@x.test"], "click": { "link": "https://acme.test" } },
    }));
    assert_eq!(clicked[0].url.as_deref(), Some("https://acme.test"));
    assert!(ProviderEvent::from_resend(&json!({ "type": "email.sent", "data": {} })).is_empty());
}

#[test]
fn events_built_by_hand() {
    let event = ProviderEvent::new("custom", "id-1", MailEventKind::Bounced)
        .recipient("a@x.test")
        .permanent()
        .detail("550");
    assert!(event.suppresses());
    assert!(!ProviderEvent::new("custom", "id-1", MailEventKind::Bounced).suppresses());
    assert_eq!(MailEventKind::Complained.as_str(), "complained");
    assert_eq!(
        "clicked".parse::<MailEventKind>(),
        Ok(MailEventKind::Clicked)
    );
}
