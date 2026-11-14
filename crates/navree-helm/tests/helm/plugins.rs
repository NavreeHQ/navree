//! What other modules add to the panel: links in its menu and cards on its
//! dashboard, checked at boot.

use navree_helm::{HelmLink, HelmWidget, ModuleBuilderHelmPanelExt};
use navree_kernel::{Module, ModuleBuilder};
use navree_testing::TestAppBuilder;
use navree_view::{ViewModule, ViewOptions};

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
