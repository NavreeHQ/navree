//! Mailboxes: routing by address, handlers, rejections, storage, the
//! size limit and the event.

use std::sync::{Arc, Mutex};

use navree_kernel::{
    BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_mail::inbound::{InboundStatus, StoredMail};
use navree_mail::{
    Inbound, InboundError, InboundMail, Incoming, MailboxContext, MailboxHandler,
    ModuleBuilderMailExt, ReplyTokens,
};
use navree_storage::Storage;
use navree_testing::TestApp;
use uuid::Uuid;

use super::{Seen, fixture, inbound_app};

fn reply(to: &str) -> Incoming {
    Incoming::compose()
        .from("Ada <ada@example.com>")
        .to(to)
        .subject("Re: [#42] Printer on fire")
        .text("Thanks!")
        .build()
}

#[tokio::test]
async fn mail_goes_to_the_mailbox_of_its_recipient() {
    let mail = inbound_app(&["support@acme.test", "billing@acme.test"], |b| b).await;

    let received = mail
        .inbound()
        .await
        .receive(reply("Billing@Acme.test"))
        .await
        .unwrap();

    assert_eq!(received.status, InboundStatus::Handled);
    assert_eq!(received.mailbox.as_deref(), Some("billing@acme.test"));
    assert_eq!(received.address.as_deref(), Some("billing@acme.test"));
    let seen = mail.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "billing@acme.test");
    assert_eq!(seen[0].2.id, received.id);
    assert_eq!(
        seen[0].2.subject.as_deref(),
        Some("Re: [#42] Printer on fire")
    );
}

#[tokio::test]
async fn exact_addresses_win_over_patterns_and_wildcards() {
    let mail = inbound_app(
        &["*@in.acme.test", "{user}@in.acme.test", "vip@in.acme.test"],
        |b| b,
    )
    .await;
    let inbound = mail.inbound().await;
    assert_eq!(
        inbound.mailboxes(),
        vec!["vip@in.acme.test", "{user}@in.acme.test", "*@in.acme.test"]
    );

    inbound.receive(reply("vip@in.acme.test")).await.unwrap();
    inbound.receive(reply("ada@in.acme.test")).await.unwrap();

    let seen = mail.seen();
    assert_eq!(seen[0].0, "vip@in.acme.test");
    assert_eq!(seen[1].0, "{user}@in.acme.test");
}

#[tokio::test]
async fn the_envelope_finds_mailboxes_the_headers_hide() {
    let mail = inbound_app(&["archive@in.acme.test"], |b| b).await;

    let incoming = Incoming::compose()
        .from("ada@example.com")
        .to("team@example.com")
        .envelope_to("archive@in.acme.test")
        .text("bcc'd")
        .build();
    let received = mail.inbound().await.receive(incoming).await.unwrap();

    assert_eq!(received.status, InboundStatus::Handled);
    assert_eq!(received.address.as_deref(), Some("archive@in.acme.test"));
}

#[tokio::test]
async fn mail_without_a_mailbox_is_unrouted_and_still_announced() {
    let mail = inbound_app(&["support@acme.test"], |b| b).await;
    let mut events = mail.events("mail.inbound.received").await;

    let received = mail
        .inbound()
        .await
        .receive(reply("nobody@acme.test"))
        .await
        .unwrap();

    assert_eq!(received.status, InboundStatus::Unrouted);
    assert_eq!(received.mailbox, None);
    assert!(mail.seen().is_empty());
    let event = events.try_recv().expect("event");
    assert_eq!(event["status"], "unrouted");
    assert_eq!(event["recipients"][0], "nobody@acme.test");
    assert_eq!(event["subject"], "Re: [#42] Printer on fire");
    assert_eq!(event["source"], "test");
}

#[tokio::test]
async fn the_event_describes_a_handled_message() {
    let mail = inbound_app(&["reply+{token}@in.acme.test"], |b| {
        b.config("mail.inbound.disk", "mail")
    })
    .await;
    let mut events = mail.events("mail.inbound.received").await;

    let incoming = Incoming::new("raw", fixture("report.eml")).recipient("reply+x@in.acme.test");
    let received = mail.inbound().await.receive(incoming).await.unwrap();

    let event = events.try_recv().expect("event");
    assert_eq!(event["id"], received.id.to_string());
    assert_eq!(event["status"], "handled");
    assert_eq!(event["mailbox"], "reply+{token}@in.acme.test");
    assert_eq!(event["address"], "reply+x@in.acme.test");
    assert_eq!(event["message_id"], "multi-0002@example.com");
    assert_eq!(event["from"], "grace@example.com");
    assert_eq!(event["attachments"], 2);
    assert_eq!(event["stored"]["disk"], "mail");
    assert!(event.get("text").is_none(), "no bodies in the event");
}

/// The captured token and the ticket a reply mailbox verified.
type Verified = Arc<Mutex<Vec<(Option<String>, Uuid)>>>;

/// The boot error of `builder`'s app.
async fn boot_error(builder: navree_testing::TestAppBuilder) -> String {
    match builder.try_build().await {
        Ok(_) => panic!("the boot should fail"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test]
async fn params_and_verified_reply_tokens_reach_the_handler() {
    let seen: Verified = Arc::default();
    struct Replies(Verified);
    impl Module for Replies {
        fn register(&self, m: &mut ModuleBuilder) {
            let seen = self.0.clone();
            m.mailbox(
                "reply+{token}@in.acme.test",
                move |_mail: InboundMail, ctx: MailboxContext| {
                    let seen = seen.clone();
                    async move {
                        let ticket = ctx.verified_uuid("token")?;
                        seen.lock()
                            .unwrap()
                            .push((ctx.param("token").map(str::to_owned), ticket));
                        Ok(())
                    }
                },
            );
        }
    }
    let mail = inbound_app(&[], |b| b.module(Replies(seen.clone()))).await;
    let tokens = mail.app.get::<ReplyTokens>().await;
    let ticket = Uuid::now_v7();
    let address = tokens
        .address_for("reply+{token}@in.acme.test", ticket)
        .unwrap();

    let received = mail.inbound().await.receive(reply(&address)).await.unwrap();

    assert_eq!(received.status, InboundStatus::Handled);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen[0].1, ticket);
    assert!(address.contains(seen[0].0.as_deref().unwrap()));
}

#[tokio::test]
async fn a_forged_reply_token_rejects_the_message() {
    struct Strict;
    impl Module for Strict {
        fn register(&self, m: &mut ModuleBuilder) {
            m.mailbox(
                "reply+{token}@in.acme.test",
                |_mail: InboundMail, ctx: MailboxContext| async move {
                    ctx.verified("token")?;
                    Ok(())
                },
            );
        }
    }
    let mail = inbound_app(&[], |b| b.module(Strict)).await;
    let mut events = mail.events("mail.inbound.received").await;

    let received = mail
        .inbound()
        .await
        .receive(reply("reply+forged@in.acme.test"))
        .await
        .unwrap();

    assert_eq!(received.status, InboundStatus::Rejected);
    assert_eq!(
        received.reason.as_deref(),
        Some("the reply token is invalid")
    );
    assert_eq!(events.try_recv().expect("event")["status"], "rejected");
}

#[tokio::test]
async fn a_mailbox_can_reject_on_purpose() {
    struct Closed;
    impl Module for Closed {
        fn register(&self, m: &mut ModuleBuilder) {
            m.mailbox(
                "old@acme.test",
                |_mail: InboundMail, ctx: MailboxContext| async move {
                    Err(ctx.reject("this mailbox is closed"))
                },
            );
        }
    }
    let mail = inbound_app(&[], |b| b.module(Closed)).await;

    let received = mail
        .inbound()
        .await
        .receive(reply("old@acme.test"))
        .await
        .unwrap();

    assert_eq!(received.status, InboundStatus::Rejected);
    assert_eq!(
        received.reason.as_deref(),
        Some("the message was rejected: this mailbox is closed")
    );
}

#[tokio::test]
async fn a_failing_mailbox_is_an_error_so_the_provider_retries() {
    struct Failing;
    impl Module for Failing {
        fn register(&self, m: &mut ModuleBuilder) {
            m.mailbox(
                "support@acme.test",
                |_mail: InboundMail, _ctx: MailboxContext| async move {
                    Err::<(), BoxError>("database unavailable".into())
                },
            );
        }
    }
    let mail = inbound_app(&[], |b| b.module(Failing)).await;
    let mut events = mail.events("mail.inbound.received").await;

    let error = mail
        .inbound()
        .await
        .receive(reply("support@acme.test"))
        .await
        .unwrap_err();

    assert!(
        matches!(&error, InboundError::Handler { mailbox, .. } if mailbox == "support@acme.test"),
        "{error}"
    );
    assert!(error.to_string().contains("database unavailable"));
    assert!(events.try_recv().is_err(), "no event for a failure");
}

/// A mailbox with a dependency, registered with `mailbox_with`.
struct TicketReplies {
    log: Arc<ReplyLog>,
}

#[derive(Default)]
struct ReplyLog(Mutex<Vec<String>>);

impl Injectable for TicketReplies {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<ReplyLog>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(TicketReplies {
                log: r.get::<ReplyLog>().await?,
            })
        })
    }
}

impl MailboxHandler for TicketReplies {
    fn handle(
        &self,
        mail: InboundMail,
        ctx: MailboxContext,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.log.0.lock().unwrap().push(format!(
                "{} -> {}",
                mail.from.map(|a| a.email).unwrap_or_default(),
                ctx.address()
            ));
            Ok(())
        })
    }
}

struct TicketsModule;

impl Module for TicketsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(ReplyLog::default())
            .mailbox_with::<TicketReplies>("tickets@acme.test");
    }
}

#[tokio::test]
async fn injectable_mailboxes_get_their_dependencies() {
    let mail = inbound_app(&[], |b| b.module(TicketsModule)).await;

    mail.inbound()
        .await
        .receive(reply("tickets@acme.test"))
        .await
        .unwrap();

    let log = mail
        .app
        .ctx()
        .resolver_for(navree_kernel::ModuleId::of::<TicketsModule>());
    let entries = log.get::<ReplyLog>().await.unwrap();
    assert_eq!(
        entries.0.lock().unwrap().clone(),
        vec!["ada@example.com -> tickets@acme.test"]
    );
}

#[tokio::test]
async fn raw_messages_are_stored_once_on_the_disk() {
    let mail = inbound_app(&["support@acme.test"], |b| {
        b.config("mail.inbound.disk", "mail")
            .config("mail.inbound.path", "inbound/mail")
    })
    .await;
    let inbound = mail.inbound().await;
    let raw = fixture("report.eml");

    let first = inbound
        .receive(Incoming::new("raw", raw.clone()))
        .await
        .unwrap();
    let again = inbound
        .receive(Incoming::new("raw", raw.clone()))
        .await
        .unwrap();

    let stored = first.stored.clone().unwrap();
    assert_eq!(stored.disk, "mail");
    assert!(stored.path.starts_with("inbound/mail/"), "{}", stored.path);
    assert!(stored.path.ends_with(".eml"));
    assert_eq!(again.stored, first.stored, "a retry lands on the same file");
    let disk = mail.app.get::<Storage>().await.disk("mail").unwrap();
    assert_eq!(
        disk.get(&stored.path).await.unwrap().as_ref(),
        raw.as_slice()
    );
    assert_eq!(disk.list("inbound").await.unwrap().len(), 1);
    assert_eq!(mail.seen()[0].2.stored.as_ref(), Some(&stored));

    let read = inbound.read(&stored).await.unwrap();
    assert_eq!(read.subject.as_deref(), Some("Quarterly report"));
    assert_eq!(read.stored, Some(stored));
}

#[tokio::test]
async fn messages_over_the_limit_are_refused() {
    let mail = inbound_app(&["support@acme.test"], |b| {
        b.config("mail.inbound.max_size", 512)
    })
    .await;
    let big = Incoming::compose()
        .from("ada@example.com")
        .to("support@acme.test")
        .text("x".repeat(600))
        .build();

    let error = mail.inbound().await.receive(big).await.unwrap_err();

    assert!(
        matches!(error, InboundError::TooLarge { max: 512, .. }),
        "{error}"
    );
    assert!(mail.seen().is_empty());
}

#[tokio::test]
async fn messages_over_the_reading_limits_are_unparseable() {
    let mail = inbound_app(&["support@acme.test"], |b| {
        b.config("mail.inbound.max_parts", 3)
            .config("mail.inbound.max_headers", 20)
            .config("mail.inbound.max_addresses", 2)
    })
    .await;
    let inbound = mail.inbound().await;

    // The message, its multipart and two attachments: four parts.
    let parts = Incoming::compose()
        .from("ada@example.com")
        .to("support@acme.test")
        .text("see attached")
        .attach("a.txt", "text/plain", b"a".to_vec())
        .attach("b.txt", "text/plain", b"b".to_vec())
        .build();
    let error = inbound.receive(parts).await.unwrap_err();
    assert!(
        matches!(&error, InboundError::Unparseable(why) if why.contains("mail.inbound.max_parts")),
        "{error}"
    );

    let mut compose = Incoming::compose()
        .from("ada@example.com")
        .to("support@acme.test");
    for index in 0..20 {
        compose = compose.header(format!("X-Trace-{index}"), "hop");
    }
    let error = inbound.receive(compose.build()).await.unwrap_err();
    assert!(
        matches!(&error, InboundError::Unparseable(why) if why.contains("mail.inbound.max_headers")),
        "{error}"
    );

    let crowd = Incoming::compose()
        .from("ada@example.com")
        .to("support@acme.test")
        .cc("a@example.com")
        .cc("b@example.com")
        .cc("c@example.com")
        .build();
    let error = inbound.receive(crowd).await.unwrap_err();
    assert!(
        matches!(&error, InboundError::Unparseable(why) if why.contains("mail.inbound.max_addresses")),
        "{error}"
    );
    assert_eq!(
        error.to_app_error().unwrap().status,
        http::StatusCode::BAD_REQUEST
    );
    assert!(mail.seen().is_empty());

    // Within the limits the same pipeline takes the message.
    let fine = Incoming::compose()
        .from("ada@example.com")
        .to("support@acme.test")
        .text("hello")
        .build();
    assert_eq!(
        inbound.receive(fine).await.unwrap().status,
        InboundStatus::Handled
    );
}

#[tokio::test]
async fn the_mail_fake_plays_the_provider() {
    let mail = inbound_app(&["support@acme.test"], |b| b).await;

    let received = mail
        .fake
        .receive(mail.app.ctx(), reply("support@acme.test"))
        .await
        .unwrap();

    assert_eq!(received.status, InboundStatus::Handled);
    assert_eq!(mail.fake.received(), vec![received]);
    let (pattern, address) = {
        let inbound = mail.inbound().await;
        let parsed = reply("support@acme.test").parse().unwrap();
        inbound
            .mailbox_for(&parsed)
            .map(|(p, a)| (p.to_owned(), a))
            .unwrap()
    };
    assert_eq!(pattern, "support@acme.test");
    assert_eq!(address, "support@acme.test");
}

#[tokio::test]
async fn an_invalid_pattern_fails_the_boot() {
    let message = boot_error(
        TestApp::builder()
            .config("mail.from", "app@example.com")
            .module(crate::support::view_module())
            .module(navree_mail::MailModule::from_config())
            .module(super::Mailboxes {
                patterns: vec!["reply+{token@in.acme.test"],
                seen: Seen::default(),
            }),
    )
    .await;

    assert!(message.contains("reply+{token@in.acme.test"), "{message}");
}

#[tokio::test]
async fn a_disk_without_storage_fails_the_boot() {
    let error = boot_error(
        TestApp::builder()
            .config("mail.from", "app@example.com")
            .config("mail.inbound.disk", "mail")
            .module(crate::support::view_module())
            .module(navree_mail::MailModule::from_config()),
    )
    .await;

    assert!(error.contains("StorageModule"), "{error}");
}

#[tokio::test]
async fn an_unknown_disk_fails_the_boot() {
    let error = boot_error(
        TestApp::builder()
            .config("mail.from", "app@example.com")
            .config("mail.inbound.disk", "missing")
            .config("storage.disks.mail.driver", "memory")
            .config("storage.default", "mail")
            .module(navree_storage::StorageModule::from_config())
            .module(crate::support::view_module())
            .module(navree_mail::MailModule::from_config()),
    )
    .await;

    assert!(error.contains("`missing`"), "{error}");
}

#[test]
fn stored_mail_serializes() {
    let stored = StoredMail {
        disk: "mail".into(),
        path: "a.eml".into(),
    };
    assert_eq!(
        serde_json::to_value(&stored).unwrap(),
        serde_json::json!({"disk": "mail", "path": "a.eml"})
    );
    let _: fn(&Inbound) = |_| {};
}

/// Records whether a task spawned before the message arrived has run by
/// the time the mailbox gets the message.
struct Watch {
    other_ran: Arc<std::sync::atomic::AtomicBool>,
    saw: Arc<std::sync::atomic::AtomicBool>,
}

impl Module for Watch {
    fn register(&self, m: &mut ModuleBuilder) {
        let (other_ran, saw) = (self.other_ran.clone(), self.saw.clone());
        m.mailbox(
            "watch@acme.test",
            move |_mail: InboundMail, _ctx: MailboxContext| {
                let other_ran = other_ran.load(std::sync::atomic::Ordering::SeqCst);
                saw.store(other_ran, std::sync::atomic::Ordering::SeqCst);
                async move { Ok(()) }
            },
        );
    }
}

/// On a runtime with one thread, a task spawned before the message can
/// only run if the parsing gave the thread up.
#[tokio::test(flavor = "current_thread")]
async fn a_received_message_is_parsed_off_the_async_thread() {
    let other_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mail = inbound_app(&[], |b| {
        b.module(Watch {
            other_ran: other_ran.clone(),
            saw: saw.clone(),
        })
    })
    .await;
    let inbound = mail.inbound().await;

    let flag = other_ran.clone();
    let other = tokio::spawn(async move { flag.store(true, std::sync::atomic::Ordering::SeqCst) });
    let received = inbound.receive(reply("watch@acme.test")).await.unwrap();
    other.await.unwrap();

    assert_eq!(received.status, InboundStatus::Handled);
    assert!(
        saw.load(std::sync::atomic::Ordering::SeqCst),
        "the message was parsed on the async thread"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_stored_message_is_read_back_off_the_async_thread() {
    let mail = inbound_app(&[], |b| b).await;
    let inbound = mail.inbound().await;
    let disk = mail.app.get::<Storage>().await.disk("mail").unwrap();
    disk.put("one.eml", fixture("reply.eml")).await.unwrap();
    let stored = StoredMail {
        disk: "mail".into(),
        path: "one.eml".into(),
    };

    let other_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = other_ran.clone();
    let other = tokio::spawn(async move { flag.store(true, std::sync::atomic::Ordering::SeqCst) });
    let read = inbound.read(&stored).await.unwrap();
    let ran_during_the_read = other_ran.load(std::sync::atomic::Ordering::SeqCst);
    other.await.unwrap();

    assert_eq!(read.stored, Some(stored));
    assert!(
        ran_during_the_read,
        "the message was parsed on the async thread"
    );
}
