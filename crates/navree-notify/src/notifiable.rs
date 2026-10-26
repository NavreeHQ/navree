//! [`Notifiable`]: who receives notifications.

/// Something that receives notifications, usually a
/// user: it names the stored notifications' owner and routes the mail and
/// broadcast channels.
///
/// ```
/// use navree_notify::Notifiable;
///
/// struct User {
///     id: uuid::Uuid,
///     email: String,
/// }
///
/// impl Notifiable for User {
///     fn notifiable_type(&self) -> &'static str {
///         navree_notify::USER_NOTIFIABLE_TYPE
///     }
///
///     fn notifiable_id(&self) -> String {
///         self.id.to_string()
///     }
///
///     fn route_mail(&self) -> Option<String> {
///         Some(self.email.clone())
///     }
/// }
/// ```
pub trait Notifiable: Send + Sync {
    /// The owner type stored with database notifications (`user`). The
    /// `/api/notifications` endpoints serve the notifications of type
    /// [`USER_NOTIFIABLE_TYPE`](crate::USER_NOTIFIABLE_TYPE) for the
    /// signed-in user.
    fn notifiable_type(&self) -> &'static str;

    /// The owner id stored with database notifications (for users, the
    /// UUID as a string).
    fn notifiable_id(&self) -> String;

    /// The address the mail channel sends to when the notification's mail
    /// names no recipient; `None` skips the mail channel.
    fn route_mail(&self) -> Option<String>;

    /// The real-time channel the broadcast channel publishes on; `None`
    /// skips the broadcast channel. Default: `private-users.{id}`.
    fn broadcast_channel(&self) -> Option<String> {
        Some(format!("private-users.{}", self.notifiable_id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct User {
        id: u32,
    }

    impl Notifiable for User {
        fn notifiable_type(&self) -> &'static str {
            "user"
        }

        fn notifiable_id(&self) -> String {
            self.id.to_string()
        }

        fn route_mail(&self) -> Option<String> {
            None
        }
    }

    #[test]
    fn broadcast_channel_defaults_to_the_private_user_channel() {
        assert_eq!(
            User { id: 7 }.broadcast_channel().as_deref(),
            Some("private-users.7")
        );
    }
}
