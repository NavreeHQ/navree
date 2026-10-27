//! The delivery pipeline: log, suppressions, outbox, tenant transports and
//! tracking (`mail.delivery.enabled`).

use std::sync::Arc;

use uuid::Uuid;

use crate::mailer::SendOptions;
use crate::{MailError, MailTransport, Message, SendReport, Sent, TransportTier};

/// What the [`Mailer`](crate::Mailer) asks of the delivery services.
pub(crate) struct Pipeline;

impl Pipeline {
    pub(crate) async fn hold(
        &self,
        _message: &Message,
        _options: &SendOptions,
    ) -> Result<SendReport, MailError> {
        Err(MailError::Missing("not built yet"))
    }

    pub(crate) async fn suppress(
        &self,
        _message: &mut Message,
        _options: &SendOptions,
    ) -> Result<Vec<String>, MailError> {
        Ok(Vec::new())
    }

    pub(crate) async fn log_suppressed(
        &self,
        _message: &Message,
        _options: &SendOptions,
        _suppressed: Vec<String>,
    ) -> Result<SendReport, MailError> {
        Err(MailError::Missing("not built yet"))
    }

    pub(crate) async fn resolve(
        &self,
        _message: &Message,
        _options: &SendOptions,
    ) -> Result<Option<(Arc<dyn MailTransport>, TransportTier)>, MailError> {
        Ok(None)
    }

    pub(crate) async fn start(
        &self,
        _message: &Message,
        _options: &SendOptions,
        _transport: &'static str,
        _tier: TransportTier,
    ) -> Result<Uuid, MailError> {
        Err(MailError::Missing("not built yet"))
    }

    pub(crate) fn track(&self, _message: &mut Message, _delivery_id: Uuid) -> Result<(), MailError> {
        Ok(())
    }

    pub(crate) async fn finish(
        &self,
        _delivery_id: Uuid,
        _outcome: &Result<Sent, MailError>,
    ) -> Result<(), MailError> {
        Ok(())
    }
}
