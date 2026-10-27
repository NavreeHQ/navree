//! `notify:vapid` prints a new VAPID key pair for the web push channel;
//! `notify:release` sends the held deliveries that are due.

use navree_crypto::webpush::VapidKey;
use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::{Notifier, Released};

/// The command name.
pub(crate) const VAPID_COMMAND: &str = "notify:vapid";

/// The release command's name.
pub(crate) const RELEASE_COMMAND: &str = "notify:release";

/// `notify:release`.
pub(crate) fn release() -> CommandSpec {
    CommandSpec::new(
        RELEASE_COMMAND,
        "Send the held notifications that are due (digests, quiet hours, send windows)",
        run_release,
    )
}

async fn run_release(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let released = ctx.get::<Notifier>().await?.release_due().await?;
    println!("{}", release_summary(&released));
    Ok(())
}

/// One line on what a release did.
pub(crate) fn release_summary(released: &Released) -> String {
    format!(
        "released {} held notification{}, {} digest{} ({} mail{} batched); {} dropped, {} to retry",
        released.sent,
        plural(released.sent),
        released.digests,
        plural(released.digests),
        released.batched,
        plural(released.batched),
        released.dropped,
        released.failed,
    )
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// `notify:vapid`.
pub(crate) fn vapid() -> CommandSpec {
    CommandSpec::new(
        VAPID_COMMAND,
        "Print a new VAPID key pair for notify.web_push (browser push notifications)",
        |_, _| async {
            println!("{}", vapid_settings(&VapidKey::generate()));
            Ok(())
        },
    )
}

/// The settings of `key`, as TOML and as environment variables.
pub(crate) fn vapid_settings(key: &VapidKey) -> String {
    let public = key.application_server_key();
    let private = key.private_key();
    format!(
        "[notify.web_push]\n\
         public_key = \"{public}\"\n\
         private_key = \"{private}\"\n\
         subject = \"mailto:you@example.com\"\n\
         \n\
         # or as environment variables:\n\
         # APP_NOTIFY__WEB_PUSH__PUBLIC_KEY={public}\n\
         # APP_NOTIFY__WEB_PUSH__PRIVATE_KEY={private}\n\
         \n\
         # Keep the private key secret. Browsers subscribe with the public key;\n\
         # a new pair invalidates every existing subscription."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_summary_counts_each_outcome() {
        let released = Released {
            sent: 1,
            digests: 2,
            batched: 5,
            dropped: 0,
            failed: 1,
        };
        assert_eq!(
            release_summary(&released),
            "released 1 held notification, 2 digests (5 mails batched); 0 dropped, 1 to retry"
        );
    }

    #[test]
    fn the_settings_hold_both_halves_of_one_key() {
        let key = VapidKey::generate();
        let text = vapid_settings(&key);
        assert!(text.contains(&format!(
            "public_key = \"{}\"",
            key.application_server_key()
        )));
        assert!(text.contains(&format!("private_key = \"{}\"", key.private_key())));
        assert!(text.contains("APP_NOTIFY__WEB_PUSH__PRIVATE_KEY="));
        let loaded = VapidKey::from_base64url(&key.application_server_key(), &key.private_key());
        assert!(loaded.is_ok());
    }
}
