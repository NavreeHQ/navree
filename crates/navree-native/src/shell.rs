//! [`Shell`]: the app's chrome as data. A client fetches it once from
//! [`SHELL_PATH`](crate::SHELL_PATH), and again when the `build` of an answer
//! changes, and draws it as its system does: a tab bar on a phone, a sidebar
//! on a tablet or a desktop, a list on a watch.

use serde::{Deserialize, Serialize};

use crate::action::Present;
use crate::node::{Icon, Node};

fn is_true(value: &bool) -> bool {
    *value
}

fn yes() -> bool {
    true
}

/// One destination of the app.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavItem {
    /// Its name.
    pub label: String,
    /// Its icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Its address.
    pub href: String,
    /// A count beside it, in the page's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// The destinations under it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<NavItem>,
}

/// A group of destinations, with or without a label.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavGroup {
    /// The group's label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Its destinations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<NavItem>,
}

/// The app's destinations.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nav {
    /// The groups, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<NavGroup>,
}

/// The person signed in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// Their name.
    pub name: String,
    /// Their email address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// What their menu offers: links and buttons.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub menu: Vec<Node>,
}

/// How the screens under an address are presented.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// The address, where `*` stands for one segment: `/orders/*/edit`.
    #[serde(rename = "match")]
    pub matches: String,
    /// How a link to it is presented.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present: Option<Present>,
    /// Whether the app's navigation stays around it. A sign-in screen has none.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub chrome: bool,
}

/// The brand's part in the look: it tints the system's own theme and never
/// replaces it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    /// The theme's name in the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The accent, as `#rrggbb`, on a light ground.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    /// The accent on a dark ground.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_dark: Option<String>,
}

/// Where a client registers for push messages.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Push {
    /// The address a device's token is posted to.
    pub register: String,
}

/// The device features the app uses.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// The features, by name. They can only narrow what the client was built
    /// with, never widen it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caps: Vec<String>,
    /// Push messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push: Option<Push>,
    /// Whether the person may lock the app behind the device's own unlock,
    /// and after how long away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock: Option<crate::device::AppLock>,
}

/// The app's chrome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shell {
    /// The protocol's major version.
    pub navree: u32,
    /// The server's build.
    pub build: String,
    /// The app's name.
    pub name: String,
    /// A line under the name: the workspace, the environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// The address of the app's first screen.
    pub home: String,
    /// The brand's part in the look.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    /// The destinations.
    #[serde(default)]
    pub nav: Nav,
    /// The person signed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    /// How the screens under an address are presented, the first match first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<Route>,
    /// The device features the app uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<Device>,
}

impl Shell {
    /// The chrome of the app `name`, with nothing in it yet.
    pub fn new(build: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            navree: crate::MAJOR,
            build: build.into(),
            name: name.into(),
            context: None,
            home: "/".to_owned(),
            theme: None,
            nav: Nav::default(),
            user: None,
            routes: Vec::new(),
            device: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_keeps_the_chrome_unless_it_says_not() {
        let routes: Vec<Route> = serde_json::from_str(
            r#"[{"match":"/orders/new","present":"sheet"},{"match":"/sign-in","present":"full","chrome":false}]"#,
        )
        .unwrap();
        assert!(routes[0].chrome);
        assert_eq!(routes[0].present, Some(Present::Sheet));
        assert!(!routes[1].chrome);
        assert_eq!(
            serde_json::to_string(&routes).unwrap(),
            r#"[{"match":"/orders/new","present":"sheet"},{"match":"/sign-in","present":"full","chrome":false}]"#
        );
    }
}
