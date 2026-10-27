//! The drivers configured in section `notify`, and how the notifier picks
//! a channel's driver: the app's own (`dyn SmsDriver`, a fake) first, then
//! the configured one.

use std::sync::Arc;

use navree_kernel::{AppContext, WeakAppContext};

use crate::drivers::log::{LogSms, LogWhatsApp};
use crate::drivers::meta::MetaWhatsApp;
use crate::drivers::slack::SlackApi;
use crate::drivers::sns::SnsSms;
use crate::drivers::teams::TeamsApi;
use crate::drivers::twilio::{TwilioSms, TwilioWhatsApp};
use crate::drivers::vonage::VonageSms;
use crate::drivers::web_push::VapidWebPush;
use crate::{
    NotifyOptions, SlackDriver, SmsDriver, SmsDriverKind, TeamsDriver, WebPushDriver,
    WhatsAppDriver, WhatsAppDriverKind,
};

/// The drivers built from the options at boot.
pub(crate) struct ChannelDrivers {
    slack: Arc<dyn SlackDriver>,
    teams: Arc<dyn TeamsDriver>,
    sms: Option<Arc<dyn SmsDriver>>,
    whatsapp: Option<Arc<dyn WhatsAppDriver>>,
    web_push: Option<Arc<VapidWebPush>>,
}

impl ChannelDrivers {
    /// The drivers `options` configure; connection tokens come from `app`.
    pub(crate) fn from_options(options: &NotifyOptions, app: &WeakAppContext) -> Result<Self, String> {
        let slack = SlackApi::new(&options.slack)?.with_app(app.clone());
        let teams = TeamsApi::new(&options.teams)?.with_app(app.clone());
        let sms: Option<Arc<dyn SmsDriver>> = match options.sms.driver {
            None => None,
            Some(SmsDriverKind::Twilio) => Some(Arc::new(TwilioSms::new(&options.twilio, &options.sms)?)),
            Some(SmsDriverKind::Vonage) => Some(Arc::new(VonageSms::new(&options.vonage, &options.sms)?)),
            Some(SmsDriverKind::Sns) => Some(Arc::new(SnsSms::new(&options.sns, &options.sms)?)),
            Some(SmsDriverKind::Log) => Some(Arc::new(LogSms::new(options.sms.from.clone()))),
        };
        let whatsapp: Option<Arc<dyn WhatsAppDriver>> = match options.whatsapp.driver {
            None => None,
            Some(WhatsAppDriverKind::Twilio) => Some(Arc::new(TwilioWhatsApp::new(
                &options.twilio,
                &options.whatsapp,
            )?)),
            Some(WhatsAppDriverKind::Meta) => Some(Arc::new(MetaWhatsApp::new(&options.meta)?)),
            Some(WhatsAppDriverKind::Log) => Some(Arc::new(LogWhatsApp::new())),
        };
        let web_push = if options.web_push.is_configured() {
            Some(Arc::new(VapidWebPush::new(&options.web_push)?))
        } else {
            None
        };
        Ok(Self {
            slack: Arc::new(slack),
            teams: Arc::new(teams),
            sms,
            whatsapp,
            web_push,
        })
    }

    pub(crate) async fn slack(&self, ctx: &AppContext) -> Arc<dyn SlackDriver> {
        match ctx.try_get::<dyn SlackDriver>().await {
            Some(driver) => driver,
            None => self.slack.clone(),
        }
    }

    pub(crate) async fn teams(&self, ctx: &AppContext) -> Arc<dyn TeamsDriver> {
        match ctx.try_get::<dyn TeamsDriver>().await {
            Some(driver) => driver,
            None => self.teams.clone(),
        }
    }

    pub(crate) async fn sms(&self, ctx: &AppContext) -> Option<Arc<dyn SmsDriver>> {
        match ctx.try_get::<dyn SmsDriver>().await {
            Some(driver) => Some(driver),
            None => self.sms.clone(),
        }
    }

    pub(crate) async fn whatsapp(&self, ctx: &AppContext) -> Option<Arc<dyn WhatsAppDriver>> {
        match ctx.try_get::<dyn WhatsAppDriver>().await {
            Some(driver) => Some(driver),
            None => self.whatsapp.clone(),
        }
    }

    pub(crate) async fn web_push(&self, ctx: &AppContext) -> Option<Arc<dyn WebPushDriver>> {
        match ctx.try_get::<dyn WebPushDriver>().await {
            Some(driver) => Some(driver),
            None => self
                .web_push
                .clone()
                .map(|driver| driver as Arc<dyn WebPushDriver>),
        }
    }

    /// The application server key of the configured VAPID key pair.
    pub(crate) fn application_server_key(&self) -> Option<String> {
        self.web_push
            .as_ref()
            .map(|driver| driver.application_server_key())
    }
}
