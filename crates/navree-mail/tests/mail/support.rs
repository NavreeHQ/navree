//! Shared helpers: embedded templates, a view module and a capturing job
//! dispatcher.

use std::sync::{Arc, Mutex};

use navree_contracts::{BoxError, BoxFuture, Dispatcher, JobEnvelope};
use navree_kernel::{Module, ModuleBuilder};
use navree_mail::{MailOptions, TransportConfig};
use navree_view::{ModuleBuilderViewExt, ViewModule, ViewOptions};

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
        ..MailOptions::default()
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

/// The bytes of the attachment named `filename` in a raw MIME message, decoded
/// by the transfer encoding its part declares (`base64`, `7bit` or `8bit`), so a
/// test checks what arrives whichever encoding the MIME writer chose for it.
pub fn attachment_bytes(raw: &str, filename: &str) -> Vec<u8> {
    use base64::Engine as _;
    let raw = raw.replace("\r\n", "\n");
    let name = format!("filename=\"{filename}\"");
    let at = raw
        .find(&name)
        .unwrap_or_else(|| panic!("no attachment {filename} in:\n{raw}"));
    // The part's headers start after the boundary line before the name.
    let start = raw[..at].rfind("\n--").map_or(0, |n| n + 1);
    let (head, rest) = raw[start..]
        .split_once("\n\n")
        .unwrap_or_else(|| panic!("the part of {filename} has no body:\n{raw}"));
    let body = &rest[..rest.find("\n--").unwrap_or(rest.len())];
    let encoding = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-transfer-encoding:")
                .map(|value| value.trim().to_owned())
        })
        .unwrap_or_else(|| "7bit".to_owned());
    match encoding.as_str() {
        "base64" => base64::engine::general_purpose::STANDARD
            .decode(body.split_whitespace().collect::<String>())
            .unwrap_or_else(|e| panic!("{filename}: not base64: {e}")),
        "7bit" | "8bit" => body.as_bytes().to_vec(),
        other => panic!("{filename}: a transfer encoding this helper does not read: {other}"),
    }
}
