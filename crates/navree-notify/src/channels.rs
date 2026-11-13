//! The drivers configured in section `notify`, and how the notifier picks
//! a channel's driver: the app's own (`dyn SmsDriver`, a fake) first, then
//! the configured one.

use std::sync::{Arc, OnceLock};

use navree_contracts::BoxFuture;
use navree_kernel::{AppContext, WeakAppContext};

use crate::drivers::apns::ApnsPush;
use crate::drivers::fcm::FcmPush;
use crate::drivers::log::{LogSms, LogWhatsApp};
use crate::drivers::meta::MetaWhatsApp;
use crate::drivers::slack::SlackApi;
use crate::drivers::sns::SnsSms;
use crate::drivers::teams::TeamsApi;
use crate::drivers::twilio::{TwilioSms, TwilioWhatsApp};
use crate::drivers::vonage::VonageSms;
use crate::drivers::web_push::VapidWebPush;
use crate::{
    Channel, ChannelError, DeviceTarget, NotifyError, NotifyOptions, PushDriver, PushMessage,
    PushOptions, PushPlatform, SentMessage, SlackDriver, SlackOptions, SmsDriver, SmsDriverKind,
    TeamsDriver, TeamsOptions, WebPushDriver, WhatsAppDriver, WhatsAppDriverKind,
};

/// The drivers built from the options. The SMS, WhatsApp and web push
/// drivers are built at boot when configured; the Slack and Teams drivers,
/// which need no configuration, on first use, so an app that never posts
/// there builds no HTTP client for them.
pub(crate) struct ChannelDrivers {
    app: WeakAppContext,
    slack_options: SlackOptions,
    teams_options: TeamsOptions,
    slack: OnceLock<Result<Arc<dyn SlackDriver>, String>>,
    teams: OnceLock<Result<Arc<dyn TeamsDriver>, String>>,
    sms: Option<Arc<dyn SmsDriver>>,
    whatsapp: Option<Arc<dyn WhatsAppDriver>>,
    web_push: Option<Arc<VapidWebPush>>,
    push: Option<Arc<PlatformPush>>,
}

impl ChannelDrivers {
    /// The drivers `options` configure; connection tokens come from `app`.
    pub(crate) fn from_options(
        options: &NotifyOptions,
        push: &PushOptions,
        app: &WeakAppContext,
    ) -> Result<Self, String> {
        let sms: Option<Arc<dyn SmsDriver>> = match options.sms.driver {
            None => None,
            Some(SmsDriverKind::Twilio) => {
                Some(Arc::new(TwilioSms::new(&options.twilio, &options.sms)?))
            }
            Some(SmsDriverKind::Vonage) => {
                Some(Arc::new(VonageSms::new(&options.vonage, &options.sms)?))
            }
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
        let apns = if push.apns.is_configured() {
            Some(ApnsPush::new(push)?)
        } else {
            None
        };
        let fcm = if push.fcm.is_configured() {
            Some(FcmPush::new(push)?)
        } else {
            None
        };
        let push = (apns.is_some() || fcm.is_some()).then(|| Arc::new(PlatformPush { apns, fcm }));
        Ok(Self {
            app: app.clone(),
            slack_options: options.slack.clone(),
            teams_options: options.teams.clone(),
            slack: OnceLock::new(),
            teams: OnceLock::new(),
            sms,
            whatsapp,
            web_push,
            push,
        })
    }

    pub(crate) async fn slack(
        &self,
        ctx: &AppContext,
    ) -> Result<Arc<dyn SlackDriver>, NotifyError> {
        if let Some(driver) = ctx.try_get::<dyn SlackDriver>().await {
            return Ok(driver);
        }
        let built = self.slack.get_or_init(|| {
            SlackApi::new(&self.slack_options)
                .map(|driver| Arc::new(driver.with_app(self.app.clone())) as Arc<dyn SlackDriver>)
        });
        built
            .clone()
            .map_err(|reason| unavailable(Channel::Slack, reason))
    }

    pub(crate) async fn teams(
        &self,
        ctx: &AppContext,
    ) -> Result<Arc<dyn TeamsDriver>, NotifyError> {
        if let Some(driver) = ctx.try_get::<dyn TeamsDriver>().await {
            return Ok(driver);
        }
        let built = self.teams.get_or_init(|| {
            TeamsApi::new(&self.teams_options)
                .map(|driver| Arc::new(driver.with_app(self.app.clone())) as Arc<dyn TeamsDriver>)
        });
        built
            .clone()
            .map_err(|reason| unavailable(Channel::MicrosoftTeams, reason))
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

    pub(crate) async fn push(&self, ctx: &AppContext) -> Option<Arc<dyn PushDriver>> {
        match ctx.try_get::<dyn PushDriver>().await {
            Some(driver) => Some(driver),
            None => self
                .push
                .clone()
                .map(|driver| driver as Arc<dyn PushDriver>),
        }
    }

    /// The application server key of the configured VAPID key pair.
    pub(crate) fn application_server_key(&self) -> Option<String> {
        self.web_push
            .as_ref()
            .map(|driver| driver.application_server_key())
    }
}

/// The configured push services: Apple's for iOS devices, Firebase's for
/// Android ones.
struct PlatformPush {
    apns: Option<ApnsPush>,
    fcm: Option<FcmPush>,
}

impl PushDriver for PlatformPush {
    fn send<'a>(
        &'a self,
        device: &'a DeviceTarget,
        message: &'a PushMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        match (device.platform, &self.apns, &self.fcm) {
            (PushPlatform::Ios, Some(apns), _) => apns.send(device, message),
            (PushPlatform::Android, _, Some(fcm)) => fcm.send(device, message),
            (platform, _, _) => Box::pin(async move {
                Err(ChannelError::Config(format!(
                    "no push service is configured for {platform} devices"
                )))
            }),
        }
    }

    fn serves(&self, platform: PushPlatform) -> bool {
        match platform {
            PushPlatform::Ios => self.apns.is_some(),
            PushPlatform::Android => self.fcm.is_some(),
        }
    }
}

/// [`NotifyError::Unavailable`] for a driver that could not be built.
fn unavailable(channel: Channel, reason: String) -> NotifyError {
    NotifyError::Unavailable {
        channel: Some(channel),
        reason,
    }
}
