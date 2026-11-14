//! What other modules add to the panel: links in its menu and cards on its
//! dashboard, checked at boot and drawn only for who may see them.

use std::sync::atomic::{AtomicUsize, Ordering};

use navree_helm::{HelmLink, HelmWidget, ModuleBuilderHelmPanelExt};
use navree_http::AppError;
use navree_kernel::{Module, ModuleBuilder};
use navree_testing::{TestAppBuilder, TestClient, principal};
use navree_view::{ModuleBuilderViewExt, ViewModule, ViewOptions};
use serde_json::json;
use uuid::Uuid;

use crate::support::*;

/// The boot error of an app that must not boot.
async fn boot_error(builder: TestAppBuilder) -> String {
    match builder.try_build().await {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    }
}

/// A module that adds a link labelled `Out` to `.0`.
struct OutboundLink(&'static str);

impl Module for OutboundLink {
    fn register(&self, m: &mut ModuleBuilder) {
        m.helm_nav(HelmLink::new("Out", self.0));
    }
}

/// A module that adds what `.0` adds.
struct Adds(fn(&mut ModuleBuilder));

impl Module for Adds {
    fn register(&self, m: &mut ModuleBuilder) {
        (self.0)(m);
    }
}

/// The open pipeline's card, keyed `pipeline`.
fn pipeline() -> HelmWidget {
    HelmWidget::new("pipeline", "Open pipeline").template("admin/widgets/pipeline")
}

/// A card whose key has a capital letter.
fn capital_key(m: &mut ModuleBuilder) {
    m.helm_widget(
        HelmWidget::new("Pipeline", "Open pipeline").template("admin/widgets/pipeline"),
    );
}

/// A card with no template.
fn no_template(m: &mut ModuleBuilder) {
    m.helm_widget(HelmWidget::new("pipeline", "Open pipeline"));
}

/// Two cards with one key.
fn taken_key(m: &mut ModuleBuilder) {
    let stalled = HelmWidget::new("pipeline", "Stalled deals").template("admin/widgets/stalled");
    m.helm_widget(pipeline()).helm_widget(stalled);
}

/// One card that is right.
fn one_card(m: &mut ModuleBuilder) {
    m.helm_widget(pipeline());
}

/// A link and a card that are right.
fn link_and_card(m: &mut ModuleBuilder) {
    m.helm_nav(HelmLink::new("Status", "/status?tab=today"))
        .helm_widget(pipeline());
}

#[tokio::test]
async fn helm_refuses_an_off_site_link_and_a_card_without_a_template_or_with_a_bad_or_taken_key() {
    for href in [
        "https://evil.example",
        "//evil.example",
        "/\\evil.example",
        "javascript:alert(1)",
        "deals",
        "",
    ] {
        let error = boot_error(
            builder()
                .module(ViewModule::for_root(ViewOptions::default()))
                .module(OutboundLink(href)),
        )
        .await;
        assert!(
            error.contains(
                "m.helm_nav(\"Out\") in `OutboundLink`: the link must be a path on this app, such as /announcements"
            ),
            "{href:?}: {error}"
        );
    }

    let cases: [(fn(&mut ModuleBuilder), &str); 3] = [
        (
            capital_key,
            "Helm widget key `Pipeline` in `Adds` must be lower-case letters, digits and -",
        ),
        (
            no_template,
            "Helm widget `pipeline` has no template: add .template(\"admin/widgets/pipeline\")",
        ),
        (taken_key, "two Helm widgets use the key `pipeline`"),
    ];
    for (adds, expected) in cases {
        let error = boot_error(
            builder()
                .module(ViewModule::for_root(ViewOptions::default()))
                .module(Adds(adds)),
        )
        .await;
        assert!(error.contains(expected), "expected `{expected}` in:\n{error}");
    }

    // A card renders an app template, so the app needs ViewModule.
    let error = boot_error(builder().module(Adds(one_card))).await;
    assert!(
        error.contains("Helm widget `pipeline` renders an app template, so the app needs ViewModule"),
        "{error}"
    );
    // A link and a card that are right boot.
    builder()
        .module(ViewModule::for_root(ViewOptions::default()))
        .module(Adds(link_and_card))
        .build()
        .await;
}

/// How many times the open pipeline's card read its data.
static CALLS: AtomicUsize = AtomicUsize::new(0);

/// A module that adds to the panel: a link with markup in its label that only users who
/// read deals see, a link for every panel user, the open pipeline's card for users who
/// read deals (its data counts in [`CALLS`] and holds a script), and a card whose data
/// fails.
struct PanelAdditions;

impl Module for PanelAdditions {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[(
            "admin/widgets/pipeline.html",
            "<p class=\"widget\">{{ data.open }} open: {{ data.note }}</p>",
        )])
        .helm_nav(
            HelmLink::new("<b>Notices</b>", "/announcements")
                .group("Workspace")
                .icon("megaphone")
                .can("read", "Deal"),
        )
        .helm_nav(HelmLink::new("Status", "/status"))
        .helm_widget(
            HelmWidget::new("pipeline", "Open pipeline")
                .template("admin/widgets/pipeline")
                .can("read", "Deal")
                .data(|_ctx| async {
                    CALLS.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({ "open": 3, "note": "<script>alert(1)</script>" }))
                }),
        )
        .helm_widget(
            HelmWidget::new("stalled", "Stalled deals")
                .template("admin/widgets/pipeline")
                .data(|_ctx| async { Err(AppError::internal("down")) }),
        );
    }
}

/// The dashboard as `client` gets it; it answers 200.
async fn dashboard(client: &TestClient) -> String {
    client
        .get("/admin")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text()
}

#[tokio::test]
async fn helm_shows_contributed_links_and_widgets_to_who_may_see_them() {
    let app = builder()
        .module(ViewModule::for_root(ViewOptions::default()))
        .module(PanelAdditions)
        .build()
        .await;
    let (_, rep) = user(&app, "rep").await;
    let before = CALLS.load(Ordering::SeqCst);
    let html = dashboard(&rep).await;
    assert!(html.contains(r#"href="/announcements""#), "{html}");
    assert!(html.contains("&lt;b&gt;Notices&lt;/b&gt;"), "{html}");
    assert!(!html.contains("<b>Notices"), "{html}");
    assert!(html.contains(r#"href="/status""#), "{html}");
    assert!(html.contains(r#"data-helm-widget="pipeline""#), "{html}");
    assert!(html.contains("3 open"), "{html}");
    // The card's data is escaped by its template.
    assert!(html.contains("&lt;script&gt;"), "{html}");
    assert!(!html.contains("<script>alert(1)"), "{html}");
    // The card whose data fails is left out, and the dashboard is drawn without it.
    assert!(!html.contains(r#"data-helm-widget="stalled""#), "{html}");
    assert_eq!(CALLS.load(Ordering::SeqCst), before + 1);

    // A user who may only enter the panel: the link without a check, and no card.
    let nobody = Uuid::now_v7();
    give_role(&app, nobody, "nothing", &["helm.view"]).await;
    let outsider = app.acting_as(principal(nobody));
    let before = CALLS.load(Ordering::SeqCst);
    let html = dashboard(&outsider).await;
    assert!(html.contains(r#"href="/status""#), "{html}");
    assert!(!html.contains(r#"href="/announcements""#), "{html}");
    assert!(!html.contains("Notices"), "{html}");
    assert!(!html.contains("data-helm-widget"), "{html}");
    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        before,
        "the card's data was read for a user who may not see it"
    );

    // With the dashboard off, no card is drawn; the links stay.
    let app = builder()
        .config("helm.dashboard", false)
        .module(ViewModule::for_root(ViewOptions::default()))
        .module(PanelAdditions)
        .build()
        .await;
    let (_, rep) = user(&app, "rep").await;
    let before = CALLS.load(Ordering::SeqCst);
    let html = dashboard(&rep).await;
    assert!(!html.contains("data-helm-widget"), "{html}");
    assert!(html.contains(r#"href="/announcements""#), "{html}");
    assert_eq!(CALLS.load(Ordering::SeqCst), before);
}

/// A module whose link and card ask to delete the panel, which no user of these tests may.
struct ForNobody;

impl Module for ForNobody {
    fn register(&self, m: &mut ModuleBuilder) {
        m.helm_nav(HelmLink::new("Danger zone", "/danger-zone").can("delete", "Helm"))
            .helm_widget(
                HelmWidget::new("danger-zone", "Danger zone")
                    .template("admin/widgets/danger-zone")
                    .can("delete", "Helm"),
            );
    }
}

#[tokio::test]
async fn helm_draws_the_same_menu_and_dashboard_when_no_link_or_card_is_for_the_reader() {
    let plain = builder()
        .module(ViewModule::for_root(ViewOptions::default()))
        .build()
        .await;
    let added = builder()
        .module(ViewModule::for_root(ViewOptions::default()))
        .module(ForNobody)
        .build()
        .await;
    let (_, rep) = user(&plain, "rep").await;
    let (_, same_rep) = user(&added, "rep").await;
    let html = dashboard(&rep).await;
    assert_eq!(html, dashboard(&same_rep).await);
    assert!(!html.contains("Danger zone"), "{html}");
}
