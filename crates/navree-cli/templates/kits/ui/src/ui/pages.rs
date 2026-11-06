//! The page that belongs to no feature: the profile. The sign-in, two-factor
//! and password pages are `AuthPages`.
//!
//! | Route | Answers |
//! |---|---|
//! | `GET /profile` | who is signed in, and the way to set up two-factor authentication |

use http::Method;
use navree::http::{Controller, RouteDef};
use navree::prelude::*;
use navree::ui::*;

use super::shell::Desk;

/// `GET /profile`.
async fn profile(desk: Desk) -> AppPage {
    let account = desk.account().clone();
    desk.page("Profile")
        .crumb("Home", "/")
        .actions(
            Button::outline("Two-factor authentication")
                .icon(icons::KEY)
                .href("/account/two-factor"),
        )
        .body(FactGrid::new().cols(2).cards([
            FactCard::new("Name", account.name).icon(icons::USER),
            FactCard::new("Email", account.email).icon(icons::MAIL),
        ]))
}

/// The routes above.
#[injectable]
pub struct AccountPages;

impl Controller for AccountPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/profile", profile)
                .name("profile")
                .authenticated()
                .page(),
        ]
    }
}
