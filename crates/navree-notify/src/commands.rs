//! `notify:vapid`: prints a new VAPID key pair for the web push channel.

use navree_crypto::webpush::VapidKey;
use navree_kernel::CommandSpec;

/// The command name.
pub(crate) const VAPID_COMMAND: &str = "notify:vapid";

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
    fn the_settings_hold_both_halves_of_one_key() {
        let key = VapidKey::generate();
        let text = vapid_settings(&key);
        assert!(text.contains(&format!("public_key = \"{}\"", key.application_server_key())));
        assert!(text.contains(&format!("private_key = \"{}\"", key.private_key())));
        assert!(text.contains("APP_NOTIFY__WEB_PUSH__PRIVATE_KEY="));
        let loaded = VapidKey::from_base64url(&key.application_server_key(), &key.private_key());
        assert!(loaded.is_ok());
    }
}
