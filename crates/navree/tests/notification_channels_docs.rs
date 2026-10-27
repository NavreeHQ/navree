//! The examples of `docs/digging-deeper/notification-channels`, through the
//! facade with the `notify` feature (in `full`): the notifiable and the
//! notification, the opt-out calls, the delivery log query, a callback
//! route of your own (with `webhooks`) and the testing example as written.
#![cfg(feature = "notify")]
// The page's examples are written as an app writes them, without doc comments.
#![allow(missing_docs)]

use navree::db::Db;
use navree::notify::{
    Channel, NotificationDelivery, Notifiable, Notification, Notifier, OptOutSource, OptOuts,
    SlackMessage, SlackRoute, SmsDriver, SmsFake, SmsMessage, TeamsDriver, TeamsFake,
    TeamsMessage, TeamsRoute, USER_NOTIFIABLE_TYPE, Urgency, WebPushDriver, WebPushFake,
    WebPushMessage, WhatsAppMessage, WhatsAppTemplate,
};
use navree::orm::Model as _;
use navree::prelude::*;
use navree::testing::TestApp;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// "Routing a notifiable".

/// An employee as the recipient of notifications.
#[derive(Clone, Debug)]
pub struct Employee {
    pub id: Uuid,
    pub email: String,
    /// Any format; read as E.164 in `notify.sms.default_region`.
    pub mobile: Option<String>,
    /// Saved when the Teams bot was installed for them.
    pub teams: Option<TeamsConversation>,
}

#[derive(Clone, Debug)]
pub struct TeamsConversation {
    pub service_url: String,
    pub conversation_id: String,
}

impl Notifiable for Employee {
    fn notifiable_type(&self) -> &'static str {
        USER_NOTIFIABLE_TYPE
    }

    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        Some(self.email.clone())
    }

    fn route_sms(&self) -> Option<String> {
        self.mobile.clone()
    }

    fn route_whatsapp(&self) -> Option<String> {
        self.mobile.clone()
    }

    fn route_teams(&self) -> Option<TeamsRoute> {
        let teams = self.teams.as_ref()?;
        Some(TeamsRoute::conversation(
            teams.service_url.clone(),
            teams.conversation_id.clone(),
        ))
    }
}

// ---------------------------------------------------------------------------
// "Rendering for each channel".

/// A training module is due today.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrainingDue {
    pub module: String,
    pub url: String,
}

impl Notification for TrainingDue {
    const TYPE: &'static str = "training.due";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![
            Channel::Database,
            Channel::MicrosoftTeams,
            Channel::Sms,
            Channel::WebPush,
        ]
    }

    fn to_database(&self, _notifiable: &dyn Notifiable) -> Option<Value> {
        Some(json!({ "title": "Training due", "message": self.module, "url": self.url }))
    }

    fn to_teams(&self, _notifiable: &dyn Notifiable) -> Option<TeamsMessage> {
        Some(
            TeamsMessage::new(format!("**{}** is due today.", self.module))
                .summary("Training due")
                .card(json!({
                    "type": "AdaptiveCard",
                    "version": "1.4",
                    "body": [{ "type": "TextBlock", "text": self.module, "weight": "Bolder", "wrap": true }],
                    "actions": [{ "type": "Action.OpenUrl", "title": "Start", "url": self.url }]
                })),
        )
    }

    fn to_sms(&self, _notifiable: &dyn Notifiable) -> Option<SmsMessage> {
        Some(SmsMessage::new(format!(
            "{} is due today: {}. Reply STOP to opt out.",
            self.module, self.url
        )))
    }

    fn to_web_push(&self, _notifiable: &dyn Notifiable) -> Option<WebPushMessage> {
        Some(
            WebPushMessage::new("Training due")
                .body(&self.module)
                .url(&self.url)
                .tag("training")
                .urgency(Urgency::High),
        )
    }
}

/// A deploy finished: posted to the team channel, whoever the notifiable is.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeployFinished {
    pub version: String,
}

impl Notification for DeployFinished {
    const TYPE: &'static str = "deploy.finished";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Slack]
    }

    fn to_slack(&self, _notifiable: &dyn Notifiable) -> Option<SlackMessage> {
        Some(
            SlackMessage::new(format!("Deployed {}", self.version))
                .blocks(json!([{
                    "type": "section",
                    "text": { "type": "mrkdwn", "text": format!("*Deployed* `{}`", self.version) }
                }]))
                .to(SlackRoute::channel("C0123456789")),
        )
    }
}

/// An order update over WhatsApp, as an approved template.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderShipped {
    pub order: String,
}

impl Notification for OrderShipped {
    const TYPE: &'static str = "order.shipped";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::WhatsApp]
    }

    fn to_whatsapp(&self, _notifiable: &dyn Notifiable) -> Option<WhatsAppMessage> {
        Some(WhatsAppMessage::template(
            WhatsAppTemplate::new("order_shipped", "en_US").parameter(&self.order),
        ))
    }
}

fn employee() -> Employee {
    Employee {
        id: Uuid::now_v7(),
        email: "priya@acme.test".into(),
        mobile: Some("(415) 555-0100".into()),
        teams: None,
    }
}

// ---------------------------------------------------------------------------
// "Testing": the page's test, as written.

#[tokio::test]
async fn due_training_is_texted_and_stop_opts_out() {
    let sms = SmsFake::new();
    let app = TestApp::builder()
        .temp_database()
        .config("notify.routes", false)
        .config("notify.sms.default_region", "US")
        .module(DatabaseModule::from_config())
        .module(NotifyModule::from_config())
        .override_value::<dyn SmsDriver>(sms.driver())
        .override_value::<dyn TeamsDriver>(TeamsFake::new().driver())
        .override_value::<dyn WebPushDriver>(WebPushFake::new().driver())
        .build()
        .await;
    let notifier = app.get::<Notifier>().await;
    let priya = employee();
    let due = TrainingDue {
        module: "Phishing basics".into(),
        url: "https://learn.acme.test/m/42".into(),
    };

    notifier.send(&priya, due.clone()).await.unwrap();
    let texts = sms.sent_to("+14155550100");
    assert!(texts[0].body.starts_with("Phishing basics is due today"));

    // The provider reports it delivered; Priya replies STOP.
    sms.deliver(app.ctx(), "+14155550100").await.unwrap();
    sms.reply(app.ctx(), "+14155550100", "STOP").await.unwrap();

    notifier.send(&priya, due).await.unwrap();
    assert_eq!(sms.sent().len(), 1, "no more texts after STOP");
    let opt_outs = app.get::<OptOuts>().await;
    assert!(
        opt_outs
            .is_opted_out(Channel::Sms, "+14155550100")
            .await
            .unwrap()
    );
}

// ---------------------------------------------------------------------------
// "Opt-outs" and "The delivery log".

#[tokio::test]
async fn opt_outs_and_the_delivery_log_work_as_shown() {
    let sms = SmsFake::new();
    let app = TestApp::builder()
        .temp_database()
        .config("notify.routes", false)
        .config("notify.sms.default_region", "US")
        .module(DatabaseModule::from_config())
        .module(NotifyModule::from_config())
        .override_value::<dyn SmsDriver>(sms.driver())
        .build()
        .await;
    let opt_outs = app.get::<OptOuts>().await;

    // A user turned texts off in their settings.
    opt_outs
        .opt_out(Channel::Sms, "(415) 555-0100", OptOutSource::Manual)
        .await
        .unwrap();
    assert!(
        opt_outs
            .is_opted_out(Channel::Sms, "+14155550100")
            .await
            .unwrap()
    );
    // ...and back on.
    opt_outs.opt_in(Channel::Sms, "+14155550100").await.unwrap();

    sms.refuse("Invalid 'To' Phone Number");
    let notifier = app.get::<Notifier>().await;
    let mut tom = employee();
    tom.mobile = Some("+1 212 555 0199".into());
    let _ = notifier
        .send(
            &tom,
            TrainingDue {
                module: "Data handling".into(),
                url: "https://learn.acme.test/m/7".into(),
            },
        )
        .await;

    let db = app.get::<Db>().await;
    let failed = NotificationDelivery::query()
        .where_(NotificationDelivery::CHANNEL.eq("sms"))
        .where_(NotificationDelivery::STATUS.eq("failed"))
        .latest_by(NotificationDelivery::CREATED_AT)
        .get(&*db)
        .await
        .unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].recipient.as_deref(), Some("+12125550199"));
}

// ---------------------------------------------------------------------------
// The other notifications of the page render as shown.

#[test]
fn the_slack_and_whatsapp_examples_render() {
    let priya = employee();
    let deploy = DeployFinished {
        version: "v2.4.1".into(),
    };
    let slack = deploy.to_slack(&priya).unwrap();
    assert_eq!(slack.to, Some(SlackRoute::channel("C0123456789")));
    let shipped = OrderShipped {
        order: "A-1042".into(),
    };
    let whatsapp = shipped.to_whatsapp(&priya).unwrap();
    assert_eq!(whatsapp.template.unwrap().parameters, ["A-1042"]);
}

// ---------------------------------------------------------------------------
// "Provider callbacks": a route of your own.

#[cfg(feature = "webhooks")]
mod own_route {
    use navree::notify::{ChannelCallback, DeliveryLog};
    use navree::prelude::*;
    use navree::webhooks::{Twilio, VerifiedWebhook};

    #[injectable]
    pub struct SmsHooks {
        log: Arc<DeliveryLog>,
    }

    #[controller("/hooks/sms")]
    impl SmsHooks {
        /// `POST /api/hooks/sms`: Twilio statuses and replies.
        #[post("/")]
        #[public]
        async fn twilio(&self, hook: VerifiedWebhook<Twilio>) -> Result<NoContent> {
            self.log
                .ingest(ChannelCallback::from_twilio(hook.body()))
                .await?;
            Ok(NoContent)
        }
    }

    #[test]
    fn the_route_compiles() {
        let _ = std::any::type_name::<SmsHooks>();
    }
}
