//! Notifications in the recipient's locale: the `RecipientScope`
//! navree-i18n registers for navree-notify.

use std::sync::Arc;

use navree_contracts::BoxFuture;
use navree_notify::{RecipientScope, USER_NOTIFIABLE_TYPE};
use uuid::Uuid;

use crate::I18n;
use crate::locale::Locale;

/// Renders a notification, and its mail when it is delivered, in the
/// notifiable user's saved locale (the default locale when the user saved
/// none). Other notifiables keep the current locale.
pub(crate) struct UserLocaleScope {
    pub(crate) i18n: Arc<I18n>,
}

impl RecipientScope for UserLocaleScope {
    fn scope<'a>(
        &'a self,
        notifiable_type: &'a str,
        notifiable_id: &'a str,
        work: BoxFuture<'a, ()>,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let user = (notifiable_type == USER_NOTIFIABLE_TYPE)
                .then(|| Uuid::parse_str(notifiable_id).ok())
                .flatten();
            let Some(user) = user else {
                return work.await;
            };
            let locale = match self.i18n.user_locale(user).await {
                Ok(locale) => locale,
                Err(error) => {
                    tracing::warn!(
                        target: "navree::i18n",
                        %error,
                        "could not read the recipient's locale; using the default one"
                    );
                    self.i18n.locale(&self.i18n.default_locale())
                }
            };
            Locale::scope(locale, work).await;
        })
    }
}
