//! Inbound mail (feature `inbound`): parsing, composing, reply tokens,
//! mailboxes, storage and the provider sources.

mod compose;
mod mailboxes;
mod parse;
mod poll;
mod sources;
mod tokens;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use navree_crypto::CryptoModule;
use navree_events::{EventBus, EventsModule};
use navree_kernel::{Module, ModuleBuilder};
use navree_mail::{
    Inbound, InboundMail, MailFake, MailModule, MailTransport, MailboxContext, ModuleBuilderMailExt,
};
use navree_storage::StorageModule;
use navree_testing::{TestApp, TestAppBuilder};
use tokio::sync::broadcast;

use crate::support::view_module;

/// The bytes of fixture `name` (`tests/fixtures/inbound/{name}`).
pub fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/inbound")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// What the test mailboxes saw: the pattern, the address, the mail.
pub type Seen = Arc<Mutex<Vec<(String, String, InboundMail)>>>;

/// Registers a recording mailbox per pattern.
pub struct Mailboxes {
    pub patterns: Vec<&'static str>,
    pub seen: Seen,
}

impl Module for Mailboxes {
    fn register(&self, m: &mut ModuleBuilder) {
        for pattern in &self.patterns {
            let seen = self.seen.clone();
            m.mailbox(pattern, move |mail: InboundMail, ctx: MailboxContext| {
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push((
                        ctx.pattern().to_owned(),
                        ctx.address().to_owned(),
                        mail,
                    ));
                    Ok(())
                }
            });
        }
    }
}

/// The app and what its mailboxes saw.
pub struct InboundApp {
    pub app: TestApp,
    pub seen: Seen,
    pub fake: MailFake,
}

impl InboundApp {
    pub async fn inbound(&self) -> Arc<Inbound> {
        self.app.get::<Inbound>().await
    }

    pub fn seen(&self) -> Vec<(String, String, InboundMail)> {
        self.seen.lock().unwrap().clone()
    }

    /// The JSON events named `name` emitted from now on.
    pub async fn events(&self, name: &str) -> broadcast::Receiver<serde_json::Value> {
        self.app.get::<EventBus>().await.subscribe_json(name)
    }
}

/// An app with an app key, events, a memory disk `mail`, the mail module
/// from config, and recording mailboxes for `patterns`.
pub fn builder(patterns: &[&'static str], seen: &Seen, fake: &MailFake) -> TestAppBuilder {
    TestApp::builder()
        .config("app.key", navree_crypto::generate_key())
        .config("mail.from", "App <app@example.com>")
        .config("storage.default", "mail")
        .config("storage.disks.mail.driver", "memory")
        .config("storage.disks.ses.driver", "memory")
        .module(CryptoModule::from_config())
        .module(EventsModule::for_root())
        .module(StorageModule::from_config())
        .module(view_module())
        .module(MailModule::from_config())
        .module(Mailboxes {
            patterns: patterns.to_vec(),
            seen: seen.clone(),
        })
        .override_value::<dyn MailTransport>(fake.transport())
}

/// Boots the inbound app, with `configure` applied to the builder.
pub async fn inbound_app(
    patterns: &[&'static str],
    configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder,
) -> InboundApp {
    let seen = Seen::default();
    let fake = MailFake::new();
    let app = configure(builder(patterns, &seen, &fake)).build().await;
    InboundApp { app, seen, fake }
}
