//! [`Config`]: what an app's build tells the core once, when it starts.

use navree_native::Client;
use serde::{Deserialize, Serialize};

fn yes() -> bool {
    true
}

fn pages() -> usize {
    24
}

/// Which addresses from outside the app (a deep link, a notification) open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepLinks {
    /// Only a destination of the shell, or an address one of its routes names.
    #[default]
    Routes,
    /// Any address on the app's origin.
    Origin,
}

/// How the core keeps the last tree of an address.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheConfig {
    /// The most pages kept. `0` keeps none.
    #[serde(default = "pages")]
    pub pages: usize,
    /// Whether pages and the shell are also handed to the client to keep
    /// between launches, with `cache_write` effects.
    #[serde(default)]
    pub persist: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            pages: pages(),
            persist: false,
        }
    }
}

/// The system's sign-in session: where it starts and how it comes back.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignInConfig {
    /// The addresses that start a sign-in with another site, where `*` stands
    /// for one segment: `/auth/*/redirect`. A link to one opens the system's
    /// sign-in session and is never fetched.
    #[serde(default)]
    pub paths: Vec<String>,
    /// The scheme the session comes back on, when it is not a link to the
    /// app's own origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback: Option<String>,
    /// The providers this build signs in with through the system's own
    /// sheet, by the protocol's names: `apple` on Apple's systems, `google`
    /// on Android. A sign-in with any other goes the web's way, in the
    /// system's sign-in session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native: Vec<String>,
}

/// What an app's build tells the core once.
///
/// ```
/// use navree_native_core::Config;
///
/// let config: Config = serde_json::from_str(
///     r#"{"origin":"https://shop.example",
///         "client":{"platform":"ios","form":"phone","vocab":0,"text_percent":100}}"#,
/// )
/// .unwrap();
/// assert!(config.back_to_first);
/// assert_eq!(config.cache.pages, 24);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// The app's origin: `https://shop.example`. Everything else opens in the
    /// system's browser.
    pub origin: String,
    /// Other origins the app treats as its own: `https://*.shop.example` for
    /// workspaces, or a whole origin.
    #[serde(default)]
    pub also: Vec<String>,
    /// Whether plain `http` is taken for an origin on the machine itself. A
    /// release build says no.
    #[serde(default)]
    pub loopback_http: bool,
    /// What the client says about itself on every request. Its `caps` are
    /// the device features this build has, by the protocol's names: a control
    /// that calls one it lacks is drawn as the call's stand-in says.
    pub client: Client,
    /// Whether the system's back, with nothing left to go back to, leads to the
    /// first destination before it leaves the app. Android's way; Apple's
    /// systems have no such back.
    #[serde(default = "yes")]
    pub back_to_first: bool,
    /// Which addresses from outside the app open.
    #[serde(default)]
    pub deep_links: DeepLinks,
    /// The last tree of each address.
    #[serde(default)]
    pub cache: CacheConfig,
    /// The kinds this build of the client can't draw yet, as a node's `t`
    /// (`chart`) or a control's (`input:file`). A page that holds one is shown
    /// in the web view.
    #[serde(default)]
    pub undrawable: Vec<String>,
    /// The names of the plugin views this build has. A `native` node of
    /// another name is drawn from its `alt`.
    #[serde(default)]
    pub views: Vec<String>,
    /// The system's sign-in session.
    #[serde(default)]
    pub sign_in: SignInConfig,
}

impl Config {
    /// A configuration for `origin` and `client`, with everything else at its default.
    pub fn new(origin: impl Into<String>, client: Client) -> Self {
        Self {
            origin: origin.into(),
            also: Vec::new(),
            loopback_http: false,
            client,
            back_to_first: true,
            deep_links: DeepLinks::default(),
            cache: CacheConfig::default(),
            undrawable: Vec::new(),
            views: Vec::new(),
            sign_in: SignInConfig::default(),
        }
    }
}
