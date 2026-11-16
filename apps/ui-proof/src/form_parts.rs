//! `/forms/parts`: a form's own parts, in a theme whose large field step is 48 px and whose
//! buttons are pills, following the device's light or dark. A sign-in start whose submit
//! button is an outline button of its own, the width of the form; and a checkout whose tax
//! number is in the mono face and whose fields, the plan's select among them, are all a size
//! larger.

use std::sync::Arc;

use http::Method;
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::form::{FormView, Rejected};
use navree::ui::theme::{
    Appearance, ControlSizes, Corner, Corners, FieldSizes, Mode, Shape, Sizes, Theme, ThemeId,
};
use navree::ui::{
    AppPage, AppShell, Button, Card, Form, Options, Reply, Size, Stack, Submit, ToastTone,
    Validate, icons,
};
use serde::Deserialize;

/// Pill buttons, 10 px fields, fields 40 px high and 48 px at the large step.
pub fn parts_theme() -> Theme {
    let shape = Shape::new()
        .corners(Corners::new().button(Corner::Pill).field(Corner::Px(10)))
        .sizes(
            Sizes::new()
                .controls(ControlSizes::new().lg(48))
                .fields(FieldSizes::new().md(40).lg(48)),
        );
    Theme::custom(
        ThemeId::parse("proof-form-parts").expect("a theme id"),
        "Form parts",
        *Theme::NEUTRAL.spec(),
    )
    .and_then(|theme| theme.with_shape(shape))
    .expect("the theme's values are in range")
}

/// The first step of signing in.
#[derive(Debug, Deserialize, Validate, Form)]
#[form(size = lg)]
pub struct Start {
    /// Work email
    #[validate(email)]
    #[form(placeholder = "you@company.com")]
    pub email: String,
}

/// A plan.
#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    Starter,
    Growth,
    Scale,
}

/// A checkout, every field a size larger.
#[derive(Debug, Deserialize, Validate, Form)]
#[form(size = lg)]
pub struct Checkout {
    /// Plan
    #[validate(skip)]
    #[form(wide, help = "You can change it at any time.")]
    pub plan: Plan,
    /// Company
    #[validate(length(min = 1, max = 120))]
    pub company: String,
    /// GSTIN
    #[validate(custom(gstin_shape))]
    #[form(
        mono,
        placeholder = "22AAAAA0000A1Z5",
        error = "A GSTIN is 15 letters and digits."
    )]
    pub gstin: String,
}

/// A GSTIN's shape, checked on the server only: 15 letters and digits.
fn gstin_shape(text: &str) -> Result<(), String> {
    if text.len() == 15 && text.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err("A GSTIN is 15 letters and digits.".to_owned())
    }
}

fn start_form() -> FormView<Start> {
    FormView::<Start>::new()
        .post("/forms/parts/start")
        .submit_button(
            Button::outline("Continue with email")
                .icon(icons::MAIL)
                .size(Size::Lg)
                .block(),
        )
}

fn page_with(shell: &AppShell, checkout: FormView<Checkout>) -> AppPage {
    shell
        .page("Form parts")
        .appearance(Appearance::new(parts_theme(), Mode::System))
        .body(
            Stack::gap(6).children((
                Card::titled("Sign in").body(start_form()),
                Card::titled("Checkout").body(
                    checkout
                        .post("/forms/parts/checkout")
                        .submit_button(Button::primary("Pay and start").icon(icons::LOCK).block()),
                ),
            )),
        )
}

pub struct FormPartsPages;

impl Injectable for FormPartsPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for FormPartsPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/forms/parts", |shell: AppShell| async move {
                page_with(&shell, FormView::new())
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/forms/parts/checkout",
                |shell: AppShell, sent: Result<Submit<Checkout>, Rejected<Checkout>>| async move {
                    checkout(&shell, sent)
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/forms/parts/start",
                |sent: Result<Submit<Start>, Rejected<Start>>| async move {
                    match sent {
                        Ok(Submit(start)) => Reply::redirect("/forms/parts")
                            .toast(
                                ToastTone::Success,
                                format!("A link went to {}.", start.email),
                            )
                            .into_response(),
                        Err(rejected) => rejected.into_response(),
                    }
                },
            )
            .public(),
        ]
    }
}

/// `POST /forms/parts/checkout`.
fn checkout(shell: &AppShell, sent: Result<Submit<Checkout>, Rejected<Checkout>>) -> Response {
    match sent {
        Ok(Submit(order)) => Reply::redirect("/forms/parts")
            .toast(
                ToastTone::Success,
                format!("{} is on {:?}.", order.company, order.plan),
            )
            .into_response(),
        Err(rejected) => rejected.respond(|form| page_with(shell, form)),
    }
}
