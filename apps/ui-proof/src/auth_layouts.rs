//! The page of a sign-in form in its layouts: the panel first or after the form, flush or inset
//! from the window's edges in the theme's panel corner, on the theme's inverse surface; the
//! app's brand at the top of the form's column and a line at its foot; a form with no card
//! around it and its links drawn as buttons, as `AuthPages` draws them with its options.
//!
//! | Page | Shows |
//! |---|---|
//! | `/auth-layouts?case=plain` | the layout as it always was: the panel first and flush, the form in a card |
//! | `/auth-layouts?case=flush-end` | the panel after the form, flush, its line facing the form |
//! | `/auth-layouts?case=start` | the panel first, inset and inverse, with a brand and a footer |
//! | `/auth-layouts?case=end` | the panel after the form, inset and inverse, a brand, a footer, a form column 400 px wide with no card and links as ghost buttons |
//!
//! `?mode=dark` or `?mode=light` draws the page in that mode. The inset cases are in a theme with a
//! shape (a 12 px panel corner) and an inverse surface of its own.

use std::sync::Arc;

use http::Method;
use navree::http::extract::{Query, RawQuery};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::form::FormView;
use navree::ui::marketing::Brand;
use navree::ui::theme::{Appearance, Mode, Theme};
use navree::ui::*;
use serde::Deserialize;

use crate::themes::{DISPLAY, shaped_theme};

/// The sign-in form of the proof pages.
#[derive(Debug, Deserialize, Validate, Form)]
struct SignIn {
    /// Work email
    #[validate(email, length(max = 255))]
    #[form(widget = email, autofocus)]
    email: String,
    /// Password
    #[validate(length(min = 1, max = 1024))]
    #[form(widget = password)]
    password: String,
}

/// The panel beside the form: a line about the app, a list, a quiet note.
fn panel() -> Node {
    Node::list((
        Stack::gap(4).children((
            Text::muted("For families of every size"),
            Heading::h2("Your health, clearly understood."),
        )),
        Stack::gap(3).children((
            Text::new("Sleep, heart and steps, scored every day."),
            Text::new("Recovery from the watch you already have."),
            Row::gap(2).children(Badge::new("Private by default")),
        )),
        Text::muted("An example, not real data."),
    ))
}

/// The heading and the line under it.
fn heading(title: &str, lead: &str) -> Node {
    Stack::gap(2)
        .children((
            el("h1").slot("page-title").child(title.to_owned()),
            Text::muted(lead.to_owned()),
        ))
        .into()
}

/// The form in a card, as the pages draw it by default.
fn carded() -> Node {
    Stack::gap(4)
        .children((
            el("h1").slot("page-title").child("Sign in"),
            Card::new()
                .description("Use the email address and password of your account.")
                .body(
                    Stack::gap(4).children((
                        Node::from(
                            FormView::<SignIn>::new()
                                .post("/auth-layouts")
                                .one_column()
                                .submit("Sign in"),
                        ),
                        el("a")
                            .href("/auth-layouts?case=plain")
                            .child("Forgot your password?"),
                    )),
                ),
        ))
        .into()
}

/// The form with no card around it and its links drawn as ghost buttons.
fn plain() -> Node {
    Stack::gap(6)
        .children((
            heading(
                "Sign in to Kesar",
                "Sleep, heart and steps, scored every day.",
            ),
            Node::from(
                FormView::<SignIn>::new()
                    .post("/auth-layouts")
                    .one_column()
                    .submit("Sign in"),
            ),
            Stack::gap(2).children((
                Button::ghost("Forgot your password?")
                    .href("/auth-layouts?case=end")
                    .block(),
                Button::ghost("Email me a sign-in link")
                    .href("/auth-layouts?case=end")
                    .block(),
            )),
        ))
        .into()
}

fn brand() -> Brand {
    Brand::new("Kesar", "/auth-layouts?case=end").mark(Picture::decorative(
        &crate::app_blocks::MARK,
        24,
        24,
    ))
}

fn footer() -> Text {
    Text::new("Every sign-in is checked for a new device.")
}

/// The case `name`: the layout and the theme it is drawn in. An unknown name is `end`.
fn layout(name: &str) -> (AuthLayout, Theme) {
    match name {
        "plain" => (
            AuthLayout::new(carded())
                .aside(panel())
                .aside_label("About Kesar"),
            Theme::NAVREE,
        ),
        "flush-end" => (
            AuthLayout::new(carded())
                .aside(panel())
                .aside_label("About Kesar")
                .aside_end(),
            Theme::NAVREE,
        ),
        "start" => (
            AuthLayout::new(carded())
                .brand(brand())
                .footer(footer())
                .aside(panel())
                .aside_label("About Kesar")
                .aside_inset()
                .aside_inverse(),
            shaped_theme(),
        ),
        _ => (
            AuthLayout::new(plain())
                .brand(brand())
                .footer(footer())
                .aside(panel())
                .aside_label("About Kesar")
                .aside_end()
                .aside_inset()
                .aside_inverse()
                .body_width(400),
            shaped_theme(),
        ),
    }
}

#[derive(Default, Deserialize)]
struct Case {
    case: Option<String>,
}

fn page(headers: &http::HeaderMap, case: &Case, query: Option<&str>) -> View {
    let (layout, theme) = layout(case.case.as_deref().unwrap_or("end"));
    View::new("Sign in")
        .appearance(Appearance::new(theme, Mode::Light).for_reader(headers, query))
        .font(&DISPLAY)
        .body(layout)
}

pub struct AuthLayoutPages;

impl Injectable for AuthLayoutPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for AuthLayoutPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/auth-layouts",
                |headers: http::HeaderMap, Query(case): Query<Case>, RawQuery(query): RawQuery| async move {
                    page(&headers, &case, query.as_deref())
                },
            )
            .public(),
        ]
    }
}
