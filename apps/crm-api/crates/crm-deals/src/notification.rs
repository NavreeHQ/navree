//! Telling owners about their deals: [`DealNotification`] (database,
//! broadcast and mail; data `{title, message, url, deal_id}`)
//! and the queued listener [`NotifyDealOwner`].

use std::marker::PhantomData;
use std::sync::Arc;

use crm_core::{NotifiableUser, NotificationData};
use navree::BoxFuture;
use navree::auth::AuthService;
use navree::config::Config;
use navree::events::Listener;
use navree::mail::TemplateMail;
use navree::notify::{Channel, Notifiable, Notification, Notifier};
use navree::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::events::{DealAssigned, DealWon};

/// The mail template (`.html` and `.txt`) of deal notifications, with
/// context `{name, title, message, url}`.
pub const MAIL_TEMPLATE: &str = "mail/deal-notification";

/// The templates [`DealsModule`](crate::DealsModule) embeds.
pub(crate) const TEMPLATES: &[(&str, &str)] = &[
    (
        "mail/deal-notification.html",
        include_str!("../templates/mail/deal-notification.html"),
    ),
    (
        "mail/deal-notification.txt",
        include_str!("../templates/mail/deal-notification.txt"),
    ),
];

/// What a [`DealNotification`] is about.
pub trait DealNotice: Send + Sync + 'static {
    /// The notification type stored and broadcast.
    const TYPE: &'static str;
}

/// A deal was assigned to the recipient (type `deal.assigned`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assigned {}

impl DealNotice for Assigned {
    const TYPE: &'static str = "deal.assigned";
}

/// The recipient's deal was won (type `deal.won`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Won {}

impl DealNotice for Won {
    const TYPE: &'static str = "deal.won";
}

/// A notification to a deal's owner, stored (`/api/notifications`),
/// broadcast on `private-users.{id}` (`notification.created`) and mailed.
/// Its `data` is [`NotificationData`] about the deal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct DealNotification<K> {
    /// `{title, message, url, deal_id}`.
    pub data: NotificationData,
    /// The recipient's name, for the mail's greeting.
    pub recipient: String,
    /// The deal's title, for the mail's subject.
    pub deal_title: String,
    /// The absolute link the mail points at (`app.url` + `data.url`).
    pub link: String,
    #[serde(skip)]
    kind: PhantomData<fn() -> K>,
}

impl<K: DealNotice> DealNotification<K> {
    /// A notification with `data` for `recipient` about the deal titled
    /// `deal_title`, linking to `link`.
    pub fn new(
        data: NotificationData,
        recipient: impl Into<String>,
        deal_title: impl Into<String>,
        link: impl Into<String>,
    ) -> Self {
        DealNotification {
            data,
            recipient: recipient.into(),
            deal_title: deal_title.into(),
            link: link.into(),
            kind: PhantomData,
        }
    }
}

impl<K: DealNotice> Notification for DealNotification<K> {
    const TYPE: &'static str = K::TYPE;

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Broadcast, Channel::Mail]
    }

    fn to_mail(&self, _notifiable: &dyn Notifiable) -> Option<TemplateMail> {
        Some(TemplateMail {
            to: Vec::new(),
            subject: format!("{}: {}", self.data.title, self.deal_title),
            template: MAIL_TEMPLATE.to_owned(),
            context: json!({
                "name": self.recipient,
                "title": self.data.title,
                "message": self.data.message,
                "url": self.link,
            }),
        })
    }

    fn to_database(&self, _notifiable: &dyn Notifiable) -> Option<Value> {
        serde_json::to_value(&self.data).ok()
    }
}

/// Notifies the owner of a deal that was assigned to them or won. Queued:
/// the request only enqueues it (with its transaction), a queue worker
/// sends it.
#[injectable]
pub struct NotifyDealOwner {
    notifier: Arc<Notifier>,
    auth: Arc<AuthService>,
    config: Arc<Config>,
}

impl NotifyDealOwner {
    /// Sends a `K` notification with `title` and `message` about deal
    /// `deal_id` to `owner_id` (nothing if the user is gone).
    async fn notify<K: DealNotice>(
        &self,
        owner_id: Uuid,
        deal_id: Uuid,
        deal_title: &str,
        title: &str,
        message: String,
    ) -> Result<(), BoxError> {
        let Some(owner) = self.auth.find_user(owner_id).await? else {
            return Ok(());
        };
        let data = NotificationData::new(title, message, format!("/deals/{deal_id}")).deal(deal_id);
        let link = self.link(&data.url);
        let notification = DealNotification::<K>::new(data, owner.name.clone(), deal_title, link);
        self.notifier
            .send(&NotifiableUser::from(&owner), notification)
            .await?;
        Ok(())
    }

    /// `path` on the web app (`app.url`), or `path` itself without one.
    fn link(&self, path: &str) -> String {
        match self.config.get_string("app.url") {
            Some(base) if !base.trim().is_empty() => {
                format!("{}{path}", base.trim().trim_end_matches('/'))
            }
            _ => path.to_owned(),
        }
    }
}

impl Listener<DealAssigned> for NotifyDealOwner {
    fn handle<'a>(
        &'a self,
        event: &'a DealAssigned,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let assigner = self.auth.find_user(event.assigned_by).await?;
            let message = match assigner {
                Some(user) => format!("{} assigned you “{}”", user.name, event.title),
                None => format!("“{}” is now yours", event.title),
            };
            self.notify::<Assigned>(
                event.owner_id,
                event.deal_id,
                &event.title,
                "Deal assigned to you",
                message,
            )
            .await
        })
    }

    fn queued(&self) -> bool {
        true
    }
}

impl Listener<DealWon> for NotifyDealOwner {
    fn handle<'a>(
        &'a self,
        event: &'a DealWon,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            self.notify::<Won>(
                event.owner_id,
                event.deal_id,
                &event.title,
                "Deal won",
                format!("“{}” was marked won", event.title),
            )
            .await
        })
    }

    fn queued(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Owner;

    impl Notifiable for Owner {
        fn notifiable_type(&self) -> &'static str {
            navree::notify::USER_NOTIFIABLE_TYPE
        }

        fn notifiable_id(&self) -> String {
            "7".to_owned()
        }

        fn route_mail(&self) -> Option<String> {
            Some("rep1@navree.test".to_owned())
        }
    }

    fn won() -> DealNotification<Won> {
        let deal = Uuid::from_u128(9);
        let data =
            NotificationData::new("Deal won", "“Agra” was marked won", "/deals/9").deal(deal);
        DealNotification::new(data, "Priya", "Agra", "http://localhost:3000/deals/9")
    }

    #[test]
    fn the_stored_data_is_the_contract_shape_and_the_type_names_the_event() {
        let notification = won();
        assert_eq!(
            notification.to_database(&Owner),
            Some(json!({
                "title": "Deal won",
                "message": "“Agra” was marked won",
                "url": "/deals/9",
                "deal_id": Uuid::from_u128(9),
            }))
        );
        assert_eq!(<DealNotification<Won> as Notification>::TYPE, "deal.won");
        assert_eq!(
            <DealNotification<Assigned> as Notification>::TYPE,
            "deal.assigned"
        );
        assert_eq!(
            notification.via(&Owner),
            [Channel::Database, Channel::Broadcast, Channel::Mail]
        );
    }

    #[test]
    fn the_mail_greets_the_owner_and_links_to_the_web_app() {
        let mail = won().to_mail(&Owner).expect("a mail");
        assert_eq!(mail.subject, "Deal won: Agra");
        assert_eq!(mail.template, MAIL_TEMPLATE);
        assert!(mail.to.is_empty(), "sent to the owner's address");
        assert_eq!(
            mail.context,
            json!({
                "name": "Priya",
                "title": "Deal won",
                "message": "“Agra” was marked won",
                "url": "http://localhost:3000/deals/9",
            })
        );
    }

    #[test]
    fn notifications_survive_the_queue() {
        let notification = won();
        let json = serde_json::to_value(&notification).unwrap();
        let back: DealNotification<Won> = serde_json::from_value(json).unwrap();
        assert_eq!(back, notification);
    }
}
