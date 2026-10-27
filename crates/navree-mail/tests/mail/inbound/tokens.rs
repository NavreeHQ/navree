//! `ReplyTokens`: signed reply addresses.

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use navree_contracts::Clock;
use navree_mail::{InboundError, ReplyTokens};
use navree_testing::TestApp;
use uuid::Uuid;

use super::inbound_app;

/// A clock the test moves.
struct At(std::sync::Mutex<chrono::DateTime<Utc>>);

impl Clock for At {
    fn now(&self) -> chrono::DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

#[test]
fn text_and_uuid_tokens_round_trip() {
    let tokens = ReplyTokens::new(b"app key");

    let token = tokens.sign("ticket:42").unwrap();
    assert!(
        token
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
        "{token}"
    );
    assert_eq!(tokens.verify(&token).unwrap(), "ticket:42");
    assert_eq!(
        tokens.verify(&token.to_ascii_uppercase()).unwrap(),
        "ticket:42",
        "mail servers may change the case of an address"
    );

    let id = Uuid::now_v7();
    let token = tokens.sign_uuid(id).unwrap();
    assert_eq!(tokens.verify_uuid(&token).unwrap(), id);
    assert_eq!(tokens.verify(&token).unwrap(), id.to_string());

    let text_uuid = tokens.sign(&id.to_string()).unwrap();
    assert_eq!(tokens.verify_uuid(&text_uuid).unwrap(), id);
    assert!(matches!(
        tokens.verify_uuid(&tokens.sign("ticket:42").unwrap()),
        Err(InboundError::InvalidToken)
    ));
}

#[test]
fn forged_and_damaged_tokens_are_refused() {
    let tokens = ReplyTokens::new(b"app key");
    let other = ReplyTokens::new(b"another key");
    let token = tokens.sign("ticket:42").unwrap();

    assert!(matches!(
        other.verify(&token),
        Err(InboundError::InvalidToken)
    ));
    let mut damaged = token.clone().into_bytes();
    damaged[4] = if damaged[4] == b'a' { b'b' } else { b'a' };
    assert!(matches!(
        tokens.verify(std::str::from_utf8(&damaged).unwrap()),
        Err(InboundError::InvalidToken)
    ));
    for junk in ["", "abc", "not-base32!", &token[..token.len() - 2]] {
        assert!(tokens.verify(junk).is_err(), "{junk}");
    }
}

#[test]
fn scoped_tokens_only_verify_in_their_scope() {
    let tokens = ReplyTokens::new(b"app key");
    let tickets = tokens.scoped("tickets");
    let announcements = tokens.scoped("announcements");
    let token = tickets.sign("42").unwrap();

    assert_eq!(tickets.verify(&token).unwrap(), "42");
    assert!(announcements.verify(&token).is_err());
    assert!(tokens.verify(&token).is_err());
}

#[test]
fn tokens_expire_after_the_ttl() {
    let start = Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
    let clock = Arc::new(At(std::sync::Mutex::new(start)));
    let tokens = ReplyTokens::new(b"app key")
        .with_ttl_days(30)
        .with_clock(clock.clone());
    let token = tokens.sign("ticket:42").unwrap();

    *clock.0.lock().unwrap() = start + Duration::days(30);
    assert_eq!(tokens.verify(&token).unwrap(), "ticket:42");

    *clock.0.lock().unwrap() = start + Duration::days(31);
    assert!(matches!(
        tokens.verify(&token),
        Err(InboundError::ExpiredToken)
    ));

    let forever = ReplyTokens::new(b"app key").with_clock(clock.clone());
    assert_eq!(forever.verify(&token).unwrap(), "ticket:42");
}

#[test]
fn addresses_fill_the_token_into_the_pattern() {
    let tokens = ReplyTokens::new(b"app key");
    let id = Uuid::now_v7();

    let address = tokens
        .address_for("reply+{token}@in.acme.test", id)
        .unwrap();
    let local = address.split_once('@').unwrap().0;
    assert!(local.len() <= 64, "{address}");
    let token = local.strip_prefix("reply+").unwrap();
    assert_eq!(tokens.verify_uuid(token).unwrap(), id);

    let address = tokens
        .address("t-{token}@in.acme.test", "ticket:42")
        .unwrap();
    assert!(address.starts_with("t-"));

    assert!(matches!(
        tokens.address("reply@in.acme.test", "x"),
        Err(InboundError::InvalidPattern { .. })
    ));
    assert!(matches!(
        tokens.address("reply+{token}@in.acme.test", &"x".repeat(40)),
        Err(InboundError::AddressTooLong { .. })
    ));
}

#[tokio::test]
async fn the_module_derives_the_key_from_the_app_key() {
    let mail = inbound_app(&[], |b| b).await;
    let tokens = mail.app.get::<ReplyTokens>().await;
    assert!(tokens.has_key());
    let token = tokens.sign("ticket:42").unwrap();
    assert_eq!(tokens.verify(&token).unwrap(), "ticket:42");

    let other = inbound_app(&[], |b| b).await;
    assert!(
        other
            .app
            .get::<ReplyTokens>()
            .await
            .verify(&token)
            .is_err(),
        "another app key gives other tokens"
    );
}

#[tokio::test]
async fn without_an_app_key_tokens_fail_clearly() {
    let app = TestApp::builder()
        .config("mail.from", "app@example.com")
        .module(crate::support::view_module())
        .module(navree_mail::MailModule::from_config())
        .build()
        .await;
    let tokens = app.get::<ReplyTokens>().await;

    assert!(!tokens.has_key());
    assert!(matches!(
        tokens.sign("x"),
        Err(InboundError::MissingKey)
    ));
}
