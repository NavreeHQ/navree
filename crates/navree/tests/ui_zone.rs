//! Navree UI's date components read the reader's zone from navree-time: today's day for
//! the pickers and the zone the times of a timeline and a feed are shown in, from
//! `CurrentZone`, on the app's clock.
//!
//! `cargo test -p navree --features ui,time,testing --test ui_zone`

use std::sync::Arc;

use http::Method;
use navree::contracts::{Clock, FixedClock, ZoneLevel, ZoneOwner};
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use navree::time::ZonesFake;
use navree::ui::dates::{DatePicker, DateRangePicker};
use navree::ui::{ActivityFeed, Timeline, TimelineEvent, UiModule, View};
use uuid::Uuid;

/// A page that draws its date components with the request's zone.
async fn page(zone: CurrentZone) -> Response {
    View::new("Zone")
        .body((
            DateRangePicker::new("from", "to").today_in(&zone),
            DatePicker::new("due").today_in(&zone),
            Timeline::new("steps", "Steps")
                .zone(&zone)
                .event(TimelineEvent::new("Sent").at(1_793_592_667)),
            ActivityFeed::new("feed", "Feed").zone(&zone),
        ))
        .into_response()
}

struct Pages;

impl Injectable for Pages {
    fn dependencies() -> Vec<navree::kernel::ProviderKey> {
        Vec::new()
    }

    fn construct(
        _: Resolver,
    ) -> navree::contracts::BoxFuture<'static, Result<Self, navree::kernel::KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![RouteDef::on(Method::GET, "!/zone-page", page).authenticated()]
    }
}

struct PagesModule;

impl Module for PagesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
    }
}

// Twenty to two in the morning of 6 November in Kolkata is still the evening of the 5th in
// UTC. The picker marks the person's today, not the server's, and the timeline says which
// zone its times are in.
#[tokio::test]
async fn the_pickers_and_the_timeline_use_the_readers_zone() {
    let kolkata = Uuid::now_v7();
    let users = ZonesFake::new(ZoneLevel::User).owner(ZoneOwner::new(kolkata).zone("Asia/Kolkata"));
    let clock = Arc::new(FixedClock::new("2026-11-05T20:00:00Z".parse().unwrap()));
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(TimeModule::from_config())
        .module(users.module())
        .module(UiModule)
        .module(PagesModule)
        .override_value::<dyn Clock>(clock)
        .build()
        .await;

    let html = app
        .acting_as(principal(kolkata))
        .get("/zone-page")
        .send()
        .await
        .text();
    assert_eq!(
        html.matches(r#"data-today="2026-11-06""#).count(),
        2,
        "{html}"
    );
    assert!(
        html.contains("Times in Asia/Kolkata (GMT+05:30)"),
        "the timeline names its zone: {html}"
    );

    // A person with no zone of their own is on UTC: the 5th.
    let stranger = Uuid::now_v7();
    let html = app
        .acting_as(principal(stranger))
        .get("/zone-page")
        .send()
        .await
        .text();
    assert_eq!(
        html.matches(r#"data-today="2026-11-05""#).count(),
        2,
        "{html}"
    );
    assert!(html.contains("Times in UTC"), "{html}");
}
