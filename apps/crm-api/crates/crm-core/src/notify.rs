//! Notifying CRM users: [`NotifiableUser`] (database, mail and the
//! `private-users.{id}` broadcast channel) and the `data` every CRM
//! notification stores and broadcasts, [`NotificationData`].

use navree::auth::User;
use navree::notify::{Notifiable, USER_NOTIFIABLE_TYPE};
use navree::openapi::ToSchema;
use navree::orm::Record;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models;

/// A user as the recipient of a notification: stored for the signed-in
/// user's `/api/notifications`, mailed to their address and broadcast on
/// `private-users.{id}` (event `notification.created`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifiableUser {
    /// The user's id.
    pub id: Uuid,
    /// The user's name (for greetings in mails).
    pub name: String,
    /// The user's email address.
    pub email: String,
}

impl From<&User> for NotifiableUser {
    fn from(user: &User) -> Self {
        NotifiableUser {
            id: user.id,
            name: user.name.clone(),
            email: user.email.clone(),
        }
    }
}

impl From<&Record<models::User>> for NotifiableUser {
    fn from(user: &Record<models::User>) -> Self {
        NotifiableUser {
            id: user.id,
            name: user.name.clone(),
            email: user.email.clone(),
        }
    }
}

impl Notifiable for NotifiableUser {
    fn notifiable_type(&self) -> &'static str {
        USER_NOTIFIABLE_TYPE
    }

    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        Some(self.email.clone())
    }
}

/// The `data` of every CRM notification, also the `notification.created`
/// broadcast's payload `data`: `{title, message, url, deal_id?}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationData {
    /// A short headline ("Deal won").
    pub title: String,
    /// One sentence.
    pub message: String,
    /// Where the web app should take the user (`/deals/{id}`).
    pub url: String,
    /// The deal it is about, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deal_id: Option<Uuid>,
}

impl NotificationData {
    /// Data without a deal.
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        NotificationData {
            title: title.into(),
            message: message.into(),
            url: url.into(),
            deal_id: None,
        }
    }

    /// The same data about `deal`.
    pub fn deal(mut self, deal: Uuid) -> Self {
        self.deal_id = Some(deal);
        self
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use navree::notify::Notifiable;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn user() -> navree::auth::User {
        navree::auth::User {
            id: Uuid::from_u128(7),
            name: "Priya Verma".into(),
            email: "rep1@navree.test".into(),
            password_hash: String::new(),
            email_verified_at: None,
            two_factor_secret: None,
            two_factor_recovery_codes: None,
            two_factor_confirmed_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn users_are_notified_by_mail_and_on_their_private_channel() {
        let user = NotifiableUser::from(&user());
        assert_eq!(user.notifiable_type(), navree::notify::USER_NOTIFIABLE_TYPE);
        assert_eq!(user.notifiable_id(), Uuid::from_u128(7).to_string());
        assert_eq!(user.route_mail().as_deref(), Some("rep1@navree.test"));
        assert_eq!(
            user.broadcast_channel(),
            Some(format!("private-users.{}", Uuid::from_u128(7)))
        );
        assert_eq!(user.name, "Priya Verma");
    }

    #[test]
    fn notification_data_has_the_contract_shape() {
        let deal = Uuid::from_u128(9);
        let data = NotificationData::new("Deal won", "Renewal was won", "/deals/9").deal(deal);
        assert_eq!(
            serde_json::to_value(&data).unwrap(),
            json!({ "title": "Deal won", "message": "Renewal was won", "url": "/deals/9", "deal_id": deal })
        );
        let plain = NotificationData::new("Tasks due today", "2 tasks", "/tasks");
        assert_eq!(
            serde_json::to_value(&plain).unwrap(),
            json!({ "title": "Tasks due today", "message": "2 tasks", "url": "/tasks" })
        );
    }
}
