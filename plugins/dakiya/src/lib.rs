//! `navree-plugin-dakiya`: a worked mail-transport plugin over [`HttpClient`].
//!
//! Registers the `dakiya` mail transport with `m.mail_transport("dakiya", ..)`, selected by
//! setting `mail.transport = "dakiya"` (or listing `"dakiya"` in a failover transport) and
//! configured under `[mail.dakiya]`.

pub mod settings;
pub mod transport;

use navree::http_client::HttpClient;
use navree::plugin::prelude::*;
use navree::prelude::*;

pub use settings::DakiyaSettings;
pub use transport::DakiyaTransport;

/// Registers the `dakiya` plugin and its `dakiya` mail transport.
#[derive(Clone, Copy, Debug, Default)]
pub struct DakiyaModule;

impl Module for DakiyaModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(navree::plugin_info!(title = "Dakiya mail"));
        m.mail_transport("dakiya", |ctx| async move {
            let settings: DakiyaSettings = ctx.settings()?;
            let client = ctx.app().get::<HttpClient>().await?;
            Ok(DakiyaTransport::new((*client).clone(), settings)?)
        });
    }
}
