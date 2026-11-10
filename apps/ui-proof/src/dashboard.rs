//! A dashboard: stats, a region fetched after paint and one that polls.

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree::http::response::IntoResponse;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::{
    AppPage, AppShell, Bars, Card, Grid, Node, Region, Reply, Stack, Stat, Text, View,
};

/// `GET /`.
pub fn page(shell: &AppShell) -> AppPage {
    shell.page("Overview").body(
        Stack::gap(4).children((
            Grid::cols(4).gap(4).children((
                Stat::new("Open deals", "312").trend("+12 this week", true),
                Stat::new("Pipeline value", "₹4.2 Cr").trend("+3.1%", true),
                Stat::new("Won this month", "₹64 L").trend("-2.4%", false),
                Stat::new("Tasks due today", "18"),
            )),
            Grid::cols(3).gap(4).children((
                Card::titled("Revenue by month")
                    .span(2)
                    .body(Region::new("revenue").src("/fragments/revenue").lazy()),
                Card::titled("Activity").body(
                    Region::new("activity")
                        .src("/fragments/activity")
                        .poll(Duration::from_secs(5))
                        .children(activity()),
                ),
            )),
        )),
    )
}

fn revenue() -> Node {
    Bars::new(
        "Revenue by month, rising from 12 to 61",
        &[12, 18, 15, 24, 31, 28, 36, 41, 39, 48, 55, 61],
    )
    .into()
}

fn activity() -> Node {
    Stack::gap(1)
        .children((
            Text::muted("Anu moved Agra renewal to Proposal"),
            Text::muted("Arun created Gulati renewal"),
        ))
        .into()
}

/// A static page: nothing on it needs a script.
pub fn plain() -> View {
    View::new("Plain page").body(navree::ui::el("main").slot("page").children((
        navree::ui::Heading::h1("Nothing interactive here"),
        Text::new("This page ships no JavaScript. Its links are prefetched by the browser, from a header."),
        navree::ui::Button::primary("Back to the app").href("/").prefetch(),
    )))
}

pub struct DashboardPages;

impl Injectable for DashboardPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for DashboardPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/",
                |shell: AppShell| async move { page(&shell) },
            )
            .public(),
            RouteDef::on(Method::GET, "!/plain", || async { plain() }).public(),
            RouteDef::on(Method::POST, "!/sign-out", || async {
                // The sign-in demo's cookie goes too.
                let mut reply = Reply::redirect("/plain").into_response();
                reply.headers_mut().append(
                    http::header::SET_COOKIE,
                    http::HeaderValue::from_static(
                        "proof_user=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax",
                    ),
                );
                reply
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/revenue", || async {
                View::new("Revenue").body(revenue())
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/activity", || async {
                View::new("Activity").body(activity())
            })
            .public(),
        ]
    }
}
