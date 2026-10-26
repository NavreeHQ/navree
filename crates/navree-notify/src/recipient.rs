//! [`RecipientScope`]: notification work in the recipient's context.

use navree_contracts::BoxFuture;
use navree_kernel::AppContext;

/// Runs notification work in the context of its recipient.
///
/// With one in the app (registered as `Arc<dyn RecipientScope>`; the
/// `I18nModule` of navree-i18n registers one that sets the recipient's
/// saved locale), the [`Notifier`](crate::Notifier) renders every
/// notification (`via`, `to_mail`, `to_database`, `to_broadcast`) inside
/// [`scope`](RecipientScope::scope), and renders its mail there when it
/// is delivered, also from a queued `notify.send` job.
///
/// ```
/// use navree_contracts::BoxFuture;
/// use navree_notify::RecipientScope;
///
/// tokio::task_local! {
///     static RECIPIENT: String;
/// }
///
/// struct TagRecipient;
///
/// impl RecipientScope for TagRecipient {
///     fn scope<'a>(
///         &'a self,
///         notifiable_type: &'a str,
///         notifiable_id: &'a str,
///         work: BoxFuture<'a, ()>,
///     ) -> BoxFuture<'a, ()> {
///         Box::pin(RECIPIENT.scope(format!("{notifiable_type}:{notifiable_id}"), work))
///     }
/// }
/// ```
pub trait RecipientScope: Send + Sync + 'static {
    /// Runs `work` for the notifiable `notifiable_type` / `notifiable_id`
    /// (as [`Notifiable`](crate::Notifiable) names it: `user` and the
    /// user's UUID).
    fn scope<'a>(
        &'a self,
        notifiable_type: &'a str,
        notifiable_id: &'a str,
        work: BoxFuture<'a, ()>,
    ) -> BoxFuture<'a, ()>;
}

/// Runs `work` inside the app's [`RecipientScope`] for the notifiable,
/// or as it is when the app has none.
pub(crate) async fn within<T: Send>(
    ctx: &AppContext,
    notifiable_type: &str,
    notifiable_id: &str,
    work: impl Future<Output = T> + Send,
) -> T {
    match ctx.try_get::<dyn RecipientScope>().await {
        Some(scope) => {
            let mut output = None;
            scope
                .scope(
                    notifiable_type,
                    notifiable_id,
                    Box::pin(async {
                        output = Some(work.await);
                    }),
                )
                .await;
            output.expect("a RecipientScope must run the work it is given")
        }
        None => work.await,
    }
}
