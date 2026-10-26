//! [`NotifyOptions`]: configuration section `notify`.

use navis_config::ConfigSection;
use serde::{Deserialize, Serialize};

/// How [`NotifyModule`](crate::NotifyModule) sends notifications
/// (configuration section `notify`, e.g. `APP_NOTIFY__QUEUE=true`).
///
/// Build it with `..NotifyOptions::default()` so new fields keep compiling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifyOptions {
    /// Queue notifications: [`Notifier::send`](crate::Notifier::send)
    /// renders the notification, then dispatches one job `notify.send` per
    /// channel through the app's `dyn Dispatcher`, so a queue worker
    /// delivers (and retries) each channel on its own. Without a
    /// dispatcher in the app, notifications are delivered at once. Default:
    /// `false` (deliver at once).
    pub queue: bool,
    /// Serve the signed-in user's notifications under
    /// `{api_prefix}/notifications` (authenticated routes, so the app needs
    /// a `dyn Authenticator`). Default: `true`.
    pub routes: bool,
}

impl Default for NotifyOptions {
    fn default() -> Self {
        Self {
            queue: false,
            routes: true,
        }
    }
}

impl ConfigSection for NotifyOptions {
    const KEY: &'static str = "notify";
}

#[cfg(test)]
mod tests {
    use navis_config::{Config, ConfigSources};

    use super::*;

    #[test]
    fn a_missing_section_uses_the_defaults() {
        let config = Config::load(&ConfigSources::new()).unwrap();
        assert_eq!(
            config.typed::<NotifyOptions>().unwrap(),
            NotifyOptions::default()
        );
        assert!(NotifyOptions::default().routes);
    }

    #[test]
    fn environment_strings_set_the_flags() {
        let sources = ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_NOTIFY__QUEUE", "true"),
                ("APP_NOTIFY__ROUTES", "false"),
            ],
        );
        let options = Config::load(&sources)
            .unwrap()
            .typed::<NotifyOptions>()
            .unwrap();
        assert_eq!(
            options,
            NotifyOptions {
                queue: true,
                routes: false,
            }
        );
    }
}
