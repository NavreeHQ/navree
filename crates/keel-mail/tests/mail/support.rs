//! Shared helpers: embedded templates, a view module and a capturing job
//! dispatcher.

use std::sync::{Arc, Mutex};

use keel_contracts::{BoxError, BoxFuture, Dispatcher, JobEnvelope};
use keel_kernel::{Module, ModuleBuilder};
use keel_mail::{MailOptions, TransportConfig};
use keel_view::{ModuleBuilderViewExt, ViewModule, ViewOptions};

pub const FROM: &str = "App <app@example.com>";

/// Embeds the `mail/welcome` template pair.
pub struct MailTemplates;

impl Module for MailTemplates {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[
            ("mail/welcome.html", "<p>Welcome, {{ name }}!</p>"),
            ("mail/welcome.txt", "Welcome, {{ name }}!"),
        ]);
    }
}

/// `ViewModule` with no template directories (embedded templates only).
pub fn view_module() -> impl Module {
    ViewModule::for_root(ViewOptions {
        dirs: Vec::new(),
        reload: false,
    })
}

/// Options using the array transport and [`FROM`].
pub fn array_options() -> MailOptions {
    MailOptions {
        from: FROM.to_owned(),
        transport: TransportConfig::Array,
        queue: None,
    }
}

/// Records dispatched job envelopes instead of queueing them.
#[derive(Default)]
pub struct CapturingDispatcher {
    envelopes: Mutex<Vec<JobEnvelope>>,
}

impl CapturingDispatcher {
    pub fn envelopes(&self) -> Vec<JobEnvelope> {
        self.envelopes.lock().expect("lock").clone()
    }
}

impl Dispatcher for CapturingDispatcher {
    fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        let mut envelopes = self.envelopes.lock().expect("lock");
        envelopes.push(env);
        let id = format!("job-{}", envelopes.len());
        Box::pin(async move { Ok(id) })
    }
}

pub fn dispatcher() -> (Arc<CapturingDispatcher>, Arc<dyn Dispatcher>) {
    let capturing = Arc::new(CapturingDispatcher::default());
    let dispatcher: Arc<dyn Dispatcher> = capturing.clone();
    (capturing, dispatcher)
}
