//! What other modules add to the panel: links in its menu and cards on its
//! dashboard, checked at boot and drawn only for who may see them.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use navree_helm::{
    HelmLink, HelmPage, HelmWidget, ModuleBuilderHelmExt, ModuleBuilderHelmPanelExt,
};
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
    m.helm_widget(HelmWidget::new("Pipeline", "Open pipeline").template("admin/widgets/pipeline"));
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

    type Adds = fn(&mut ModuleBuilder);
    let cases: [(Adds, &str); 3] = [
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
        assert!(
            error.contains(expected),
            "expected `{expected}` in:\n{error}"
        );
    }

    // A card renders an app template, so the app needs ViewModule.
    let error = boot_error(builder().module(Adds(one_card))).await;
    assert!(
        error.contains(
            "Helm widget `pipeline` renders an app template, so the app needs ViewModule"
        ),
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

/// What the latest announcement's cards are given: a title someone typed, holding a script.
fn latest() -> serde_json::Value {
    json!({ "title": "<script>alert(1)</script>" })
}

/// A card that names a `.txt` template.
fn text_template(m: &mut ModuleBuilder) {
    m.helm_widget(
        HelmWidget::new("latest", "Latest announcement")
            .template("admin/widgets/latest.txt")
            .data(|_ctx| async { Ok(latest()) }),
    );
}

/// Two cards whose data holds a script: `latest` names its template without an extension,
/// and only a `.txt` file of that name exists; `latest-html` names an `.html` template.
struct LatestCards;

impl Module for LatestCards {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[
            ("admin/widgets/latest.txt", "Latest: {{ data.title }}"),
            (
                "admin/widgets/latest-html.html",
                "<p class=\"latest\">Latest: {{ data.title }}</p>",
            ),
        ])
        .helm_widget(
            HelmWidget::new("latest", "Latest announcement")
                .template("admin/widgets/latest")
                .data(|_ctx| async { Ok(latest()) }),
        )
        .helm_widget(
            HelmWidget::new("latest-html", "Latest announcement")
                .template("admin/widgets/latest-html.html")
                .data(|_ctx| async { Ok(latest()) }),
        );
    }
}

#[tokio::test]
async fn a_card_writes_only_through_an_html_template_whatever_its_data_holds() {
    // A template that is not HTML stops the boot.
    let error = boot_error(
        builder()
            .module(ViewModule::for_root(ViewOptions::default()))
            .module(Adds(text_template)),
    )
    .await;
    assert!(
        error.contains(
            "Helm widget `latest` names the template `admin/widgets/latest.txt`, which is not HTML: name an .html template, such as .template(\"admin/widgets/latest\")"
        ),
        "{error}"
    );

    // A name without an extension is its `.html` template and never a `.txt` file of that
    // name: with no `.html` one, the card fails, is left out, and the dashboard is drawn.
    let app = builder()
        .module(ViewModule::for_root(ViewOptions::default()))
        .module(LatestCards)
        .build()
        .await;
    let (_, rep) = user(&app, "rep").await;
    let html = dashboard(&rep).await;
    assert!(!html.contains(r#"data-helm-widget="latest""#), "{html}");
    assert!(!html.contains("Latest: <"), "{html}");
    // The `.html` template escapes the same data.
    assert!(html.contains(r#"data-helm-widget="latest-html""#), "{html}");
    assert!(
        html.contains("<p class=\"latest\">Latest: &lt;script&gt;"),
        "{html}"
    );
    assert!(!html.contains("<script>alert(1)"), "{html}");
}

/// The pretty plugin, a community plugin. A test binary is one crate, so the plugin's own
/// crate is this module: its `crate_name` is the module's path, and the app's subjects
/// (`Deal` and `Company` of `support`) belong to another crate, as they do in an app.
mod navree_plugin_pretty {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    /// The plugin, whose crate is this module.
    const PRETTY: PluginInfo =
        PluginInfo::new("navree-plugin-pretty", module_path!(), "pretty", "1.0.0");

    /// A module of the plugin that registers what `.0` registers.
    pub struct PrettyStaff(pub fn(&mut ModuleBuilder));

    impl Module for PrettyStaff {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PRETTY);
            (self.0)(m);
        }
    }

    /// A module of the plugin that adds to the panel what `.0` adds.
    pub struct PrettyPanel(pub fn(&mut ModuleBuilder));

    impl Module for PrettyPanel {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PRETTY);
            (self.0)(m);
        }
    }
}

use navree_plugin_pretty::{PrettyPanel, PrettyStaff};

/// Lets every user work in every workspace.
fn every_workspace(m: &mut ModuleBuilder) {
    m.helm_staff_workspaces(|_| async { Ok(true) });
}

/// Lets every user work with the settings of every organization.
fn every_organization(m: &mut ModuleBuilder) {
    m.helm_staff_organizations(|_| async { Ok(true) });
}

#[tokio::test]
async fn a_plugin_may_not_decide_who_works_in_other_workspaces() {
    type Registers = fn(&mut ModuleBuilder);
    let cases: [(Registers, &str); 2] = [
        (
            every_workspace,
            "plugin `navree-plugin-pretty` decides who works in workspaces they are not members of (m.helm_staff_workspaces in `PrettyStaff`); only the app decides that: call it from a module of the app",
        ),
        (
            every_organization,
            "plugin `navree-plugin-pretty` decides who works with the settings of organizations they are not members of (m.helm_staff_organizations in `PrettyStaff`); only the app decides that: call it from a module of the app",
        ),
    ];
    for (registers, expected) in cases {
        let alone = boot_error(builder().module(PrettyStaff(registers))).await;
        // Whatever the order, beside the app's own policy.
        let after_the_apps = boot_error(
            builder()
                .module(Adds(registers))
                .module(PrettyStaff(registers)),
        )
        .await;
        let before_the_apps = boot_error(
            builder()
                .module(PrettyStaff(registers))
                .module(Adds(registers)),
        )
        .await;
        for error in [alone, after_the_apps, before_the_apps] {
            assert!(
                error.contains(expected),
                "expected `{expected}` in:\n{error}"
            );
        }
        // The same call from a module of the app boots.
        builder().module(Adds(registers)).build().await;
    }
}

/// A page with no template.
fn page_without_template(m: &mut ModuleBuilder) {
    m.helm_page(HelmPage::new("report", "Report"));
}

/// A card that names a `.txt` template under the pretty plugin's namespace.
fn text_card(m: &mut ModuleBuilder) {
    m.helm_widget(
        HelmWidget::new("pipeline", "Open pipeline").template("pretty/admin/widgets/pipeline.txt"),
    );
}

#[tokio::test]
async fn helm_hints_name_a_template_under_a_plugins_namespace() {
    type Registers = fn(&mut ModuleBuilder);
    let cases: [(Registers, &str); 3] = [
        (
            page_without_template,
            "Helm page `report` has no template: add `.template(\"pretty/admin/report\")`",
        ),
        (
            no_template,
            "Helm widget `pipeline` has no template: add .template(\"pretty/admin/widgets/pipeline\")",
        ),
        (
            text_card,
            "Helm widget `pipeline` names the template `pretty/admin/widgets/pipeline.txt`, which is not HTML: name an .html template, such as .template(\"pretty/admin/widgets/pipeline\")",
        ),
    ];
    for (registers, expected) in cases {
        let error = boot_error(
            builder()
                .module(ViewModule::for_root(ViewOptions::default()))
                .module(PrettyPanel(registers)),
        )
        .await;
        assert!(
            error.contains(expected),
            "expected `{expected}` in:\n{error}"
        );
    }

    // A module of the app keeps the hint it had.
    let error = boot_error(
        builder()
            .module(ViewModule::for_root(ViewOptions::default()))
            .module(Adds(page_without_template)),
    )
    .await;
    assert!(
        error.contains("Helm page `report` has no template: add `.template(\"admin/report\")`"),
        "{error}"
    );
}

/// The warnings logged on this thread while the guard lives (the tests run on a
/// current-thread runtime, so the boot logs on this thread).
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

impl Warnings {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(self.clone())
    }

    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct Message(String);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl tracing::Subscriber for Warnings {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() <= tracing::Level::WARN
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::WARN {
            let mut message = Message(String::new());
            event.record(&mut message);
            self.0.lock().unwrap().push(message.0);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// What the report pages are given: a note someone typed, holding markup.
fn note() -> serde_json::Value {
    json!({ "note": "<b>Amritsar</b>" })
}

/// The custom page `slug` as `client` gets it: its status and its body.
async fn panel_page(client: &TestClient, slug: &str) -> (u16, String) {
    let response = client
        .get(&format!("/admin/pages/{slug}"))
        .header("accept", "text/html")
        .send()
        .await;
    (response.status().as_u16(), response.text())
}

/// A page of the pretty plugin that names a `.txt` template.
fn plugin_text_page(m: &mut ModuleBuilder) {
    m.helm_page(HelmPage::new("report", "Report").template("pretty/admin/report.txt"));
}

/// Two pages of the pretty plugin whose data holds markup: `report` names its template
/// without an extension, and only a `.txt` file of that name exists; `summary` has an
/// `.html` one.
fn plugin_pages(m: &mut ModuleBuilder) {
    m.templates(&[
        ("pretty/admin/report.txt", "Report: {{ data.note }}"),
        (
            "pretty/admin/summary.html",
            "<p class=\"summary\">{{ data.note }}</p>",
        ),
    ])
    .helm_page(
        HelmPage::new("report", "Report")
            .template("pretty/admin/report")
            .data(|_ctx| async { Ok(note()) }),
    )
    .helm_page(
        HelmPage::new("summary", "Summary")
            .template("pretty/admin/summary")
            .data(|_ctx| async { Ok(note()) }),
    );
}

#[tokio::test]
async fn a_plugins_page_renders_only_an_html_template() {
    // A template that is not HTML stops the boot.
    let error = boot_error(
        builder()
            .module(ViewModule::for_root(ViewOptions::default()))
            .module(PrettyPanel(plugin_text_page)),
    )
    .await;
    assert!(
        error.contains(
            "Helm page `report` names the template `pretty/admin/report.txt`, which is not HTML: name an .html template, such as .template(\"pretty/admin/report\")"
        ),
        "{error}"
    );

    // A name without an extension is its `.html` template and never a `.txt` file of that
    // name: with no `.html` one, the page fails, and its data never reaches the panel raw.
    let app = builder()
        .module(ViewModule::for_root(ViewOptions::default()))
        .module(PrettyPanel(plugin_pages))
        .build()
        .await;
    let (_, rep) = user(&app, "rep").await;
    let (status, html) = panel_page(&rep, "report").await;
    assert_eq!(status, 500, "{html}");
    assert!(!html.contains("<b>Amritsar"), "{html}");
    // The `.html` template escapes the same data.
    let (status, html) = panel_page(&rep, "summary").await;
    assert_eq!(status, 200, "{html}");
    assert!(
        html.contains("<p class=\"summary\">&lt;b&gt;Amritsar"),
        "{html}"
    );
    assert!(!html.contains("<b>Amritsar"), "{html}");
}

/// A page of the app that names a `.txt` template.
fn app_text_page(m: &mut ModuleBuilder) {
    m.templates(&[("admin/report.txt", "Report: {{ data.note }}")])
        .helm_page(
            HelmPage::new("report", "Report")
                .template("admin/report.txt")
                .data(|_ctx| async { Ok(note()) }),
        );
}

#[tokio::test]
async fn an_apps_page_whose_template_is_not_html_is_warned_about_and_renders_as_before() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let app = builder()
        .module(ViewModule::for_root(ViewOptions::default()))
        .module(Adds(app_text_page))
        .build()
        .await;
    let warning = "Helm page `report` names the template `admin/report.txt`, which is not HTML, so what it writes is not escaped: name an .html template, such as .template(\"admin/report\")";
    let logged = warnings.all();
    assert_eq!(
        logged.iter().filter(|line| *line == warning).count(),
        1,
        "{logged:?}"
    );

    let (_, rep) = user(&app, "rep").await;
    let (status, html) = panel_page(&rep, "report").await;
    assert_eq!(status, 200, "{html}");
    // As before: a `.txt` template writes values as they are, which is what the warning says.
    assert!(html.contains("Report: <b>Amritsar</b>"), "{html}");
}

/// Two pages of the app on HTML templates: `summary` names its template without the
/// extension, `overview` with it.
fn app_html_pages(m: &mut ModuleBuilder) {
    m.templates(&[
        ("admin/summary.html", "<p class=\"summary\">{{ data.note }}</p>"),
        ("admin/overview.html", "<p class=\"overview\">{{ data.note }}</p>"),
    ])
    .helm_page(
        HelmPage::new("summary", "Summary")
            .template("admin/summary")
            .data(|_ctx| async { Ok(note()) }),
    )
    .helm_page(
        HelmPage::new("overview", "Overview")
            .template("admin/overview.html")
            .data(|_ctx| async { Ok(note()) }),
    );
}

#[tokio::test]
async fn an_apps_html_page_renders_as_before_without_a_warning() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let app = builder()
        .module(ViewModule::for_root(ViewOptions::default()))
        .module(Adds(app_html_pages))
        .build()
        .await;
    let logged = warnings.all();
    assert!(
        logged.iter().all(|line| !line.starts_with("Helm page")),
        "{logged:?}"
    );

    let (_, rep) = user(&app, "rep").await;
    for slug in ["summary", "overview"] {
        let (status, html) = panel_page(&rep, slug).await;
        assert_eq!(status, 200, "{html}");
        assert!(
            html.contains(&format!("<p class=\"{slug}\">&lt;b&gt;Amritsar")),
            "{html}"
        );
    }
}
