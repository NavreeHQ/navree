//! Links modules add to the sidebar: checked at boot, drawn for who may open
//! them, and answered as JSON. A test binary of its own, because it reads the
//! warnings the boot logs.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use http::request::Parts;
use http::{Method, StatusCode};
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{
    BoxError, ModuleBuilderPermissionsExt, Permission, PermissionCheck, Principal,
};
use navree_http::{
    Controller, HttpModule, ModuleBuilderHttpExt, RequestWorkspace, RouteDef, RouteKind,
};
use navree_kernel::{
    BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, Navree, Resolver,
};
use navree_plugin_announcements::Announcements;
use navree_plugin_pretty::PrettyShell;
use navree_testing::{TestApp, TestAppBuilder, principal};
use navree_ui::{
    AppShell, ErrorShell, ModuleBuilderShellExt, NavGroup, NavItem, Shell, ShellNavItem, UiModule,
    icons,
};
use serde_json::{Value, json};
use uuid::Uuid;

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

/// The app's module: it registers the app's one `Shell` and answers `GET /`
/// with the home page, drawn from the `AppShell` extractor.
struct Desk(Shell);

impl Module for Desk {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(self.0.clone()).exports::<Shell>();
        m.controller::<Home>();
    }
}

/// `GET /`: the home page.
struct Home;

impl Injectable for Home {
    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Home) })
    }
}

impl Controller for Home {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/", |shell: AppShell| async move {
                shell.page("Home")
            })
            .page()
            .public(),
        ]
    }
}

/// A test app with the toolkit and the app's `shell`, before a test adds its
/// modules.
fn builder(shell: Shell) -> TestAppBuilder {
    TestApp::builder().module(UiModule).module(Desk(shell))
}

/// The boot error of an app that must not boot.
async fn boot_error(builder: TestAppBuilder) -> String {
    match builder.try_build().await {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    }
}

/// A module that adds one link, labelled `Out`, to `.0`.
struct Outbound(&'static str);

impl Module for Outbound {
    fn register(&self, m: &mut ModuleBuilder) {
        m.nav_item(ShellNavItem::new("Out", self.0));
    }
}

/// The announcements plugin, a community plugin whose crate is this module: the app's modules
/// at the top of the file (`Desk`, `Grants`, ...) are in no crate it names, so none of them acts
/// for it.
mod navree_plugin_announcements {
    use navree_contracts::{ModuleBuilderPermissionsExt, Permission};
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};
    use navree_ui::{ModuleBuilderShellExt, ShellNavItem, icons};

    /// The plugin's info.
    const NOTICES: PluginInfo = PluginInfo::new(
        "navree-plugin-announcements",
        module_path!(),
        "announcements",
        "1.0.0",
    )
    .title("Announcements");

    /// The announcements plugin's module: it describes its two permissions and adds three
    /// links, one that needs `announcements.read`, one that needs `announcements.publish`
    /// and one that needs nothing.
    pub(super) struct Announcements;

    impl Module for Announcements {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(NOTICES)
                .permissions(&[
                    Permission::new("announcements.read", "Read the workspace's announcements"),
                    Permission::new(
                        "announcements.publish",
                        "Publish an announcement to the workspace",
                    ),
                ])
                .nav_item(
                    ShellNavItem::new("Announcements", "/announcements")
                        .icon(icons::MEGAPHONE)
                        .permission("announcements.read"),
                )
                .nav_item(
                    ShellNavItem::new("New announcement", "/announcements/new")
                        .permission("announcements.publish")
                        .order(-1),
                )
                .nav_item(ShellNavItem::new("Notice board", "/notice-board"));
        }
    }
}

/// Who holds what in the app, as a `dyn PermissionCheck`: `Some(true)` exactly for a
/// listed `(user, permission, workspace)`, the workspace `None` for app-wide, else
/// `Some(false)`. A permission in `failing` is answered with an error.
#[derive(Clone, Default)]
struct Grants {
    held: Arc<Mutex<Vec<Held>>>,
    failing: Arc<Mutex<Vec<&'static str>>>,
}

/// One grant: who holds which permission, in which workspace (`None` for app-wide).
type Held = (Uuid, &'static str, Option<Uuid>);

impl Grants {
    fn grant(&self, user: Uuid, permission: &'static str, workspace: Option<Uuid>) {
        self.held
            .lock()
            .unwrap()
            .push((user, permission, workspace));
    }

    fn revoke(&self, user: Uuid, permission: &'static str) {
        self.held
            .lock()
            .unwrap()
            .retain(|(holder, name, _)| !(*holder == user && *name == permission));
    }

    fn fail(&self, permission: &'static str) {
        self.failing.lock().unwrap().push(permission);
    }
}

impl PermissionCheck for Grants {
    fn check<'a>(
        &'a self,
        principal: &'a Principal,
        permission: &'a str,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<Option<bool>, BoxError>> {
        let failing = self.failing.lock().unwrap().contains(&permission);
        let answer = if failing {
            Err(BoxError::from("the role table is unavailable"))
        } else {
            let held = self.held.lock().unwrap().iter().any(|(user, name, scope)| {
                *user == principal.user_id
                    && *name == permission
                    && scope.is_none_or(|scope| Some(scope) == workspace)
            });
            Ok(Some(held))
        };
        Box::pin(async move { answer })
    }
}

impl Module for Grants {
    fn register(&self, m: &mut ModuleBuilder) {
        let grants = self.clone();
        m.multi(Factory::<dyn PermissionCheck>::new(move |_| {
            let grants = grants.clone();
            async move { Ok(Arc::new(grants) as Arc<dyn PermissionCheck>) }
        }));
    }
}

/// A `dyn RequestWorkspace` that takes the request's `x-workspace` header as its
/// workspace. A test double: it checks nothing.
struct HeaderWorkspace;

impl RequestWorkspace for HeaderWorkspace {
    fn resolve<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Option<Uuid>> {
        let named = parts
            .headers
            .get("x-workspace")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| Uuid::parse_str(value).ok());
        Box::pin(async move { named })
    }
}

impl Module for HeaderWorkspace {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(Factory::<dyn RequestWorkspace>::new(|_| async {
            Ok(Arc::new(HeaderWorkspace) as Arc<dyn RequestWorkspace>)
        }));
    }
}

/// The home page as `reader` gets it (a guest with `None`), naming `workspace` in
/// `x-workspace` when there is one. It answers 200.
async fn home(app: &TestApp, reader: Option<Uuid>, workspace: Option<Uuid>) -> String {
    let client = match reader {
        Some(user) => app.acting_as(principal(user)),
        None => app.client(),
    };
    let mut request = client.get("/").header("accept", "text/html");
    if let Some(workspace) = workspace {
        request = request.header("x-workspace", workspace.to_string());
    }
    let response = request.send().await;
    let page = response.text();
    assert_eq!(response.status().as_u16(), 200, "{page}");
    page
}

/// The link that needs `announcements.read`.
const READ_LINK: &str = r#"href="/announcements""#;

#[tokio::test(flavor = "current_thread")]
async fn a_contributed_link_that_leaves_the_site_fails_the_boot() {
    for href in [
        "https://evil.example/x",
        "//evil.example/x",
        "/\\evil.example",
        "\\\\evil.example",
        "javascript:alert(1)",
        "deals",
        "",
        "?page=2",
        "#top",
        "\u{3000}/deals",
        " /deals",
        "/a\nb",
    ] {
        let error =
            boot_error(builder(Shell::new("Kesar").contributed()).module(Outbound(href))).await;
        assert!(
            error.contains(
                "m.nav_item(\"Out\") in `Outbound`: the link must be a path on this app, such as /announcements"
            ),
            "{href:?}: {error}"
        );
    }
    for href in ["/", "/announcements?tab=new", "/ਖ਼ਬਰਾਂ"] {
        if let Err(error) = builder(Shell::new("Kesar").contributed())
            .module(Outbound(href))
            .try_build()
            .await
        {
            panic!("{href}: {error}");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_link_asking_for_an_undeclared_permission_warns_at_boot() {
    /// A module of the app with a link to its reports, which asks for
    /// `reports.view`; `.0` says whether the module describes it.
    struct Extras(bool);

    impl Module for Extras {
        fn register(&self, m: &mut ModuleBuilder) {
            m.nav_item(ShellNavItem::new("Reports", "/reports").permission("reports.view"));
            if self.0 {
                m.permissions(&[Permission::new(
                    "reports.view",
                    "View the monthly reports of the Amritsar branch",
                )]);
            }
        }
    }

    let warnings = Warnings::default();
    let _guard = warnings.install();
    let warning = "m.nav_item(\"Reports\") in `Extras` asks for the permission `reports.view`, which no module declares with m.permissions";
    builder(Shell::new("Kesar").contributed())
        .module(Extras(false))
        .build()
        .await;
    let first = warnings.all();
    assert_eq!(
        first.iter().filter(|line| *line == warning).count(),
        1,
        "{first:?}"
    );
    builder(Shell::new("Kesar").contributed())
        .module(Extras(true))
        .build()
        .await;
    let described = warnings.all();
    assert!(
        described[first.len()..]
            .iter()
            .all(|line| !line.contains("reports.view")),
        "{described:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn contributed_links_show_only_to_readers_with_their_permission() {
    let grants = Grants::default();
    let (harpreet, kuldeep) = (Uuid::now_v7(), Uuid::now_v7());
    grants.grant(harpreet, "announcements.read", None);
    let app = builder(Shell::new("Kesar").contributed())
        .module(Announcements)
        .module(grants.clone())
        .build()
        .await;

    let page = home(&app, Some(harpreet), None).await;
    assert!(page.contains(READ_LINK), "{page}");
    assert!(page.contains(r#"href="/notice-board""#), "{page}");
    assert!(
        page.contains(r#"<p data-slot="sidebar-group-label">Announcements</p>"#),
        "{page}"
    );
    assert!(!page.contains(r#"href="/announcements/new""#), "{page}");
    // Kuldeep holds nothing; the guest has no principal.
    for page in [
        home(&app, Some(kuldeep), None).await,
        home(&app, None, None).await,
    ] {
        assert!(page.contains(r#"href="/notice-board""#), "{page}");
        assert!(!page.contains(READ_LINK), "{page}");
        assert!(!page.contains(r#"href="/announcements/new""#), "{page}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_contributed_link_follows_a_grant_from_one_request_to_the_next() {
    let grants = Grants::default();
    let kuldeep = Uuid::now_v7();
    let app = builder(Shell::new("Kesar").contributed())
        .module(Announcements)
        .module(grants.clone())
        .build()
        .await;

    let page = home(&app, Some(kuldeep), None).await;
    assert!(!page.contains(READ_LINK), "{page}");
    grants.grant(kuldeep, "announcements.read", None);
    let page = home(&app, Some(kuldeep), None).await;
    assert!(page.contains(READ_LINK), "the grant is read again: {page}");
    grants.revoke(kuldeep, "announcements.read");
    let page = home(&app, Some(kuldeep), None).await;
    assert!(!page.contains(READ_LINK), "the grant taken away: {page}");
}

#[tokio::test(flavor = "current_thread")]
async fn without_a_request_workspace_provider_the_check_is_app_wide() {
    let grants = Grants::default();
    let (meera, office) = (Uuid::now_v7(), Uuid::now_v7());
    // Meera holds the permission only in the workspace `office`.
    grants.grant(meera, "announcements.read", Some(office));
    let app = builder(Shell::new("Kesar").contributed())
        .module(Announcements)
        .module(grants.clone())
        .build()
        .await;
    let page = home(&app, Some(meera), Some(office)).await;
    assert!(!page.contains(READ_LINK), "{page}");

    let app = builder(Shell::new("Kesar").contributed())
        .module(Announcements)
        .module(grants.clone())
        .module(HeaderWorkspace)
        .build()
        .await;
    let page = home(&app, Some(meera), Some(office)).await;
    assert!(page.contains(READ_LINK), "{page}");
    let page = home(&app, Some(meera), None).await;
    assert!(!page.contains(READ_LINK), "{page}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_failing_permission_check_hides_the_link() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let grants = Grants::default();
    let harpreet = Uuid::now_v7();
    grants.grant(harpreet, "announcements.read", None);
    grants.fail("announcements.read");
    let app = builder(Shell::new("Kesar").contributed())
        .module(Announcements)
        .module(grants.clone())
        .build()
        .await;

    let page = home(&app, Some(harpreet), None).await;
    assert!(!page.contains(READ_LINK), "{page}");
    assert!(page.contains(r#"href="/notice-board""#), "{page}");
    let warning = "a sidebar link's permission could not be checked; the link is left out";
    let logged = warnings.all();
    assert!(logged.iter().any(|line| line == warning), "{logged:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn contributed_links_are_grouped_after_the_shells_own_and_ordered() {
    /// A module of the app (no plugin) with two links that need nothing.
    struct Extras;

    impl Module for Extras {
        fn register(&self, m: &mut ModuleBuilder) {
            m.nav_item(ShellNavItem::new("Reports", "/reports"))
                .nav_item(ShellNavItem::new("Members", "/members").group("Workspace"));
        }
    }

    let grants = Grants::default();
    let harpreet = Uuid::now_v7();
    grants.grant(harpreet, "announcements.read", None);
    grants.grant(harpreet, "announcements.publish", None);
    let shell = Shell::new("Kesar")
        .group(NavGroup::new("Sales").item(NavItem::new("Deals", "/deals")))
        .contributed();
    let app = builder(shell)
        .module(Announcements)
        .module(Extras)
        .module(grants.clone())
        .build()
        .await;

    let page = home(&app, Some(harpreet), None).await;
    let at = |needle: &str| {
        page.find(needle)
            .unwrap_or_else(|| panic!("no {needle} in {page}"))
    };
    let positions: Vec<usize> = [
        r#"href="/deals""#,
        r#"<p data-slot="sidebar-group-label">Announcements</p>"#,
        r#"href="/announcements/new""#,
        READ_LINK,
        r#"href="/notice-board""#,
        r#"<p data-slot="sidebar-group-label">More</p>"#,
        r#"href="/reports""#,
        r#"<p data-slot="sidebar-group-label">Workspace</p>"#,
        r#"href="/members""#,
    ]
    .into_iter()
    .map(at)
    .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{positions:?}: {page}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn contributed_labels_and_groups_are_escaped() {
    /// A module whose link's label and group hold markup.
    struct Marked;

    impl Module for Marked {
        fn register(&self, m: &mut ModuleBuilder) {
            m.nav_item(
                ShellNavItem::new("<b>Notices</b> & \"more\"", "/notices").group("<i>Board</i>"),
            );
        }
    }

    let app = builder(Shell::new("Kesar").contributed())
        .module(Marked)
        .build()
        .await;
    let page = home(&app, None, None).await;
    assert!(
        page.contains("&lt;b&gt;Notices&lt;/b&gt; &amp; &quot;more&quot;"),
        "{page}"
    );
    assert!(page.contains("&lt;i&gt;Board&lt;/i&gt;"), "{page}");
    assert!(!page.contains("<b>Notices"), "{page}");
    assert!(!page.contains("<i>Board"), "{page}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_shell_without_contributed_draws_the_same_page_whatever_modules_add() {
    let grants = Grants::default();
    let harpreet = Uuid::now_v7();
    grants.grant(harpreet, "announcements.read", None);
    grants.grant(harpreet, "announcements.publish", None);
    let bare = builder(Shell::new("Kesar")).build().await;
    let added = builder(Shell::new("Kesar"))
        .module(Announcements)
        .module(grants.clone())
        .build()
        .await;

    assert_eq!(
        home(&bare, None, None).await,
        home(&added, None, None).await
    );
    assert_eq!(
        home(&bare, Some(harpreet), None).await,
        home(&added, Some(harpreet), None).await
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_reader_who_may_open_no_contributed_link_gets_the_page_drawn_without_them() {
    /// A module of the app whose only link needs a permission.
    struct Guarded;

    impl Module for Guarded {
        fn register(&self, m: &mut ModuleBuilder) {
            m.permissions(&[Permission::new(
                "reports.view",
                "View the monthly reports of the Amritsar branch",
            )])
            .nav_item(ShellNavItem::new("Reports", "/reports").permission("reports.view"));
        }
    }

    let kuldeep = Uuid::now_v7();
    let contributed = builder(Shell::new("Kesar").contributed())
        .module(Guarded)
        .module(Grants::default())
        .build()
        .await;
    let plain = builder(Shell::new("Kesar"))
        .module(Guarded)
        .module(Grants::default())
        .build()
        .await;

    let page = home(&contributed, Some(kuldeep), None).await;
    assert_eq!(page, home(&plain, Some(kuldeep), None).await);
    assert!(!page.contains("sidebar-group-label"), "{page}");
}

/// Where the links a caller may open are answered as JSON.
const NAVIGATION: &str = "/_navree/ui/navigation";

/// What a guest gets there in an app with the announcements plugin: the one link that
/// needs no permission.
fn guest_items() -> Value {
    json!({
        "items": [
            {
                "label": "Notice board",
                "href": "/notice-board",
                "group": "Announcements",
                "order": 0,
                "icon": null,
            },
        ],
    })
}

#[tokio::test(flavor = "current_thread")]
async fn the_navigation_endpoint_answers_a_guest_the_links_that_need_no_permission() {
    let grants = Grants::default();
    let (harpreet, kuldeep) = (Uuid::now_v7(), Uuid::now_v7());
    grants.grant(harpreet, "announcements.read", None);
    let app = builder(Shell::new("Kesar").contributed())
        .module(Announcements)
        .module(grants.clone())
        .build()
        .await;

    let guest = app.client().get(NAVIGATION).send().await;
    assert_eq!(guest.status(), StatusCode::OK, "{}", guest.text());
    assert!(
        guest
            .header("content-type")
            .is_some_and(|kind| kind.starts_with("application/json")),
        "{:?}",
        guest.headers()
    );
    assert_eq!(guest.header("cache-control"), Some("private, no-store"));
    let vary: Vec<String> = guest
        .headers()
        .get_all("vary")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .collect();
    assert!(
        vary.iter().any(|name| name == "cookie") && vary.iter().any(|name| name == "authorization"),
        "{vary:?}"
    );
    let body = guest.json_value();
    assert_eq!(body, guest_items());
    assert_eq!(body.as_object().map(|object| object.len()), Some(1));

    let theirs = app
        .acting_as(principal(harpreet))
        .get(NAVIGATION)
        .send()
        .await;
    assert_eq!(
        theirs.json_value(),
        json!({
            "items": [
                {
                    "label": "Announcements",
                    "href": "/announcements",
                    "group": "Announcements",
                    "order": 0,
                    "icon": { "view_box": "0 0 24 24", "shapes": icons::MEGAPHONE.shapes() },
                },
                {
                    "label": "Notice board",
                    "href": "/notice-board",
                    "group": "Announcements",
                    "order": 0,
                    "icon": null,
                },
            ],
        })
    );
    // Kuldeep may open none of the links with a permission: the guest's bytes, no more.
    let none = app
        .acting_as(principal(kuldeep))
        .get(NAVIGATION)
        .send()
        .await;
    assert_eq!(none.status(), StatusCode::OK);
    assert_eq!(none.bytes(), guest.bytes());

    let routes = navree_http::route_table(app.ctx()).await.unwrap();
    let route = routes
        .iter()
        .find(|route| route.name.as_deref() == Some("ui.navigation"))
        .expect("the navigation route");
    assert_eq!(route.method, "GET");
    assert_eq!(route.full_path, NAVIGATION);
    assert!(route.access.public);
    assert_eq!(route.kind, RouteKind::Json);
    let api = route.api.as_ref().expect("documented");
    assert!(api.summary.is_some());
    assert_eq!(api.tags, ["UI"]);
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_without_authentication_boots_with_contributed_links() {
    use tower::ServiceExt;

    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::default()))
        .module(HttpModule)
        .module(UiModule)
        .module(Announcements)
        .module(Desk(Shell::new("Kesar").contributed()))
        .boot()
        .await
        .expect("an app with no authenticator boots");
    let router = navree_http::build_router(app.ctx())
        .await
        .expect("its router builds");
    let mut request = http::Request::builder()
        .method(Method::GET)
        .uri(NAVIGATION)
        .body(navree_http::body::Body::empty())
        .unwrap();
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49152);
    request
        .extensions_mut()
        .insert(navree_http::serve::ConnectInfo(peer));
    let response = router
        .oneshot(request)
        .await
        .unwrap_or_else(|never| match never {});
    assert_eq!(response.status(), StatusCode::OK);
    let body = navree_http::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body, guest_items());
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_with_no_contributed_links_has_no_navigation_route() {
    let app = builder(Shell::new("Kesar").contributed()).build().await;

    let routes = navree_http::route_table(app.ctx()).await.unwrap();
    assert!(
        !routes
            .iter()
            .any(|route| route.name.as_deref() == Some("ui.navigation"))
    );
    let toolkit = routes
        .iter()
        .filter(|route| {
            route
                .name
                .as_deref()
                .is_some_and(|name| name.starts_with("ui."))
        })
        .count();
    assert_eq!(toolkit, 5);
    let response = app.client().get(NAVIGATION).send().await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// The pretty plugin, a community plugin whose crate is this module: the app's `OwnShell`, at
/// the top of the file, is in no crate it names.
mod navree_plugin_pretty {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};
    use navree_ui::ErrorShell;

    /// The plugin's info.
    const PRETTY: PluginInfo =
        PluginInfo::new("navree-plugin-pretty", module_path!(), "pretty", "1.0.0");

    /// A module of the pretty plugin that builds the shell of the app's error pages.
    pub(super) struct PrettyShell;

    impl Module for PrettyShell {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PRETTY)
                .contribute(ErrorShell::new(|_| async { None }));
        }
    }
}

/// A module of the app, naming no plugin, that contributes the same shell.
struct OwnShell;

impl Module for OwnShell {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(ErrorShell::new(|_| async { None }));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugins_error_shell_fails_the_boot() {
    let refusal = "plugin `navree-plugin-pretty` builds the shell of the app's error pages (an ErrorShell in `PrettyShell`); only the app chooses that: remove the plugin or update it to a version that leaves the shell to the app, and to use the plugin's shell, contribute it from a module of the app";
    let alone = boot_error(builder(Shell::new("Kesar")).module(PrettyShell)).await;
    // Whatever the order, beside the app's own shell.
    let after_the_apps = boot_error(
        builder(Shell::new("Kesar"))
            .module(OwnShell)
            .module(PrettyShell),
    )
    .await;
    let before_the_apps = boot_error(
        builder(Shell::new("Kesar"))
            .module(PrettyShell)
            .module(OwnShell),
    )
    .await;
    for error in [alone, after_the_apps, before_the_apps] {
        assert!(error.contains(refusal), "{error}");
    }

    // The same contribution from a module that names no plugin boots.
    if let Err(error) = builder(Shell::new("Kesar"))
        .module(OwnShell)
        .try_build()
        .await
    {
        panic!("{error}");
    }
}
