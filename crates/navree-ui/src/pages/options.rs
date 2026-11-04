//! [`PagesOptions`]: where the error pages send people, what they show, and whether
//! a development build shows more.

use navree_config::{Config, Environment};

use super::labels::StateLabels;
use crate::html::Href;

/// The settings of the error pages [`UiModule`](crate::UiModule) draws.
///
/// Set them in code with [`UiModule::with_pages`](crate::UiModule::with_pages), or in
/// configuration (the `ui` section), which overrides the code:
///
/// ```toml
/// [ui]
/// sign_in = "/sign-in"        # where "Sign in" leads; the page the user came from is added as `next`
/// home = "/"                  # where "Go to home" leads
/// sign_in_redirect = false    # send a guest straight to sign in, instead of showing a page
/// debug = false               # add details to error pages; on by default in a debug build only
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct PagesOptions {
    /// Where "Sign in" leads (default `/sign-in`). A path on this site, or the address of
    /// a sign-in provider.
    pub sign_in: String,
    /// Where "Go to home" leads (default `/`).
    pub home: String,
    /// Sends a guest who asks for a page to the sign-in page at once, with a `303`,
    /// instead of showing the sign-in state (default off). Only for a `GET` that
    /// asks for a page; an action or a region always gets its own answer.
    pub redirect_guests: bool,
    /// Adds the status, code, message and cause to an error page, marked "Development
    /// only". On by default when the build has debug assertions, off in a release build.
    /// In `production` (the app's `env` or `app.env`) it is always off, whatever the build,
    /// the configuration or the code say. Never turn it on where users can see it.
    pub debug: bool,
    /// The words of every state.
    pub labels: StateLabels,
}

impl Default for PagesOptions {
    fn default() -> Self {
        Self {
            sign_in: "/sign-in".to_owned(),
            home: "/".to_owned(),
            redirect_guests: false,
            debug: cfg!(debug_assertions),
            labels: StateLabels::default(),
        }
    }
}

impl PagesOptions {
    /// Sets where "Sign in" leads.
    pub fn sign_in(mut self, sign_in: impl Into<String>) -> Self {
        self.sign_in = sign_in.into();
        self
    }

    /// Sets where "Go to home" leads.
    pub fn home(mut self, home: impl Into<String>) -> Self {
        self.home = home.into();
        self
    }

    /// Sends guests straight to sign in.
    pub fn redirect_guests(mut self, redirect: bool) -> Self {
        self.redirect_guests = redirect;
        self
    }

    /// Turns the development details on or off.
    pub fn debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: StateLabels) -> Self {
        self.labels = labels;
        self
    }

    /// These options with the `ui` section of `config` applied. A value that is not a
    /// usable address is ignored.
    pub fn configured(mut self, config: &Config) -> Self {
        if let Some(value) = config.get_string("ui.sign_in").filter(|v| usable(v)) {
            self.sign_in = value;
        }
        if let Some(value) = config.get_string("ui.home").filter(|v| usable(v)) {
            self.home = value;
        }
        if let Some(value) = config.get_bool("ui.sign_in_redirect") {
            self.redirect_guests = value;
        }
        if let Some(value) = config.get_bool("ui.debug") {
            self.debug = value;
        }
        // Production is final: whatever asked for the details, they stay off.
        if is_production(environment(config).as_deref()) {
            self.debug = false;
        }
        self
    }

    /// The options with any address that is not usable put back to its default, so a
    /// mistake in configuration shows as the default page and not as a broken link.
    pub(crate) fn checked(mut self) -> Self {
        let defaults = Self::default();
        if !usable(&self.sign_in) {
            self.sign_in = defaults.sign_in;
        }
        if !usable(&self.home) {
            self.home = defaults.home;
        }
        self
    }
}

/// The app's environment (`APP_ENV` or `app.env`); `None` when it names none.
fn environment(config: &Config) -> Option<String> {
    config.environment().map(|env| env.to_string())
}

/// Whether `env` takes the production behaviour (`production`, in any case).
pub(crate) fn is_production(env: Option<&str>) -> bool {
    env.is_some_and(|env| Environment::parse(env).behaves_like_production())
}

/// Whether `address` can be a link target.
fn usable(address: &str) -> bool {
    !address.is_empty() && address.len() <= 2048 && Href::parse(address).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mistake_in_an_address_puts_the_default_back() {
        let options = PagesOptions::default()
            .sign_in("javascript:alert(1)")
            .home("")
            .checked();
        assert_eq!(options.sign_in, "/sign-in");
        assert_eq!(options.home, "/");
        let options = PagesOptions::default()
            .sign_in("https://sso.example/login")
            .home("/dashboard")
            .checked();
        assert_eq!(options.sign_in, "https://sso.example/login");
        assert_eq!(options.home, "/dashboard");
        let long = format!("/{}", "a".repeat(3000));
        assert_eq!(PagesOptions::default().home(long).checked().home, "/");
    }

    #[test]
    fn production_is_production_in_any_case() {
        assert!(is_production(Some("production")));
        assert!(is_production(Some(" Production ")));
        assert!(is_production(Some("PRODUCTION")));
        assert!(!is_production(Some("staging")));
        assert!(!is_production(Some("")));
        assert!(!is_production(None));
    }

    #[test]
    fn debug_follows_the_build_unless_told() {
        assert_eq!(PagesOptions::default().debug, cfg!(debug_assertions));
        assert!(!PagesOptions::default().debug(false).debug);
        assert!(PagesOptions::default().debug(true).debug);
    }
}
