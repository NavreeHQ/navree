//! A plugin's permissions, subjects and policies: what it may name and write,
//! what it may not, and that naming grants nothing. A test binary of its own,
//! because it reads the warnings the boot and the ability builds log.

#[path = "module/database.rs"]
mod database;
#[path = "module/support.rs"]
mod support;

use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use http::Method;
use navree_access::{
    AbilityCtx, AbilityFactory, AccessModule, AccessService, Policy, READ, Rule, Rules, Subject,
    UPDATE,
};
use navree_contracts::{ModuleBuilderPermissionsExt, Permission, permission_catalog, permitted};
use navree_http::{
    AppRouter, Controller, HttpOptions, ModuleBuilderHttpExt, ModuleBuilderMountExt, Mount,
    RouteDef,
};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ModuleId, PluginInfo, ProviderKey,
    Resolver,
};
use navree_testing::{TestApp, TestAppBuilder, TestResponse, principal};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{RecordingAudit, SUPER_ADMIN, give_role, insert_post};

/// The warnings logged on this thread while the guard lives, each with its target (the
/// tests run on a current-thread runtime, so the boot and the requests log on this thread).
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<(String, String)>>>);

impl Warnings {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(self.clone())
    }

    /// Every warning's message, in order.
    fn all(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(_, message)| message.clone())
            .collect()
    }

    /// The messages of the warnings at `target`, in order.
    fn at(&self, target: &str) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(at, _)| at == target)
            .map(|(_, message)| message.clone())
            .collect()
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
            let target = event.metadata().target().to_owned();
            self.0.lock().unwrap().push((target, message.0));
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// The crate of the community plugins built with [`plugin`]. A test binary is one crate,
/// so a plugin's own crate is this module: its `crate_name` is the module's path, and the
/// app's subjects (types of this binary outside the module, such as the posts of
/// `support`) belong to another crate, as they do in an app.
mod navree_plugin_announcements {
    use std::marker::PhantomData;

    use navree_access::{
        ALL, AbilityCtx, AccessModule, MANAGE, Policy, READ, Rules, Subject, UPDATE,
    };
    use navree_contracts::{ModuleBuilderPermissionsExt, Permission};
    use navree_kernel::{Module, ModuleBuilder, ModuleId, PluginInfo};

    /// A module of a community plugin that describes `permissions`: one per namespace.
    pub struct Plugin(pub PluginInfo, pub &'static [Permission]);

    impl Module for Plugin {
        fn id(&self) -> ModuleId {
            ModuleId::named::<Plugin>(self.0.namespace)
        }

        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(self.0.clone()).permissions(self.1);
        }
    }

    /// A module of the plugins' crate that names no plugin, and describes `.0`.
    pub struct Extras(pub &'static [Permission]);

    impl Module for Extras {
        fn register(&self, m: &mut ModuleBuilder) {
            m.permissions(self.0);
        }
    }

    /// A subject of the announcements plugin's crate, named under its namespace.
    pub struct Announcement;

    impl Subject for Announcement {
        const SUBJECT: &'static str = "announcements.Announcement";
        const PERMISSION_PREFIX: &'static str = "announcements";
    }

    /// A subject of the announcements plugin's crate, under a prefix outside its
    /// namespace.
    pub struct Bulletin;

    impl Subject for Bulletin {
        const SUBJECT: &'static str = "Bulletin";
        const PERMISSION_PREFIX: &'static str = "reports";
    }

    /// A subject of the announcements plugin's crate named outside its namespace.
    pub struct Unnamed;

    impl Subject for Unnamed {
        const SUBJECT: &'static str = "Announcement";
        const PERMISSION_PREFIX: &'static str = "announcements";
    }

    /// A subject of the announcements plugin's crate named like every subject.
    pub struct Everything;

    impl Subject for Everything {
        const SUBJECT: &'static str = "all";
        const PERMISSION_PREFIX: &'static str = "announcements.everything";
    }

    /// A subject of the announcements plugin's crate named like the app's `reports.*`
    /// permissions, which no subject of the app maps.
    pub struct Reports;

    impl Subject for Reports {
        const SUBJECT: &'static str = "reports";
        const PERMISSION_PREFIX: &'static str = "announcements.reports";
    }

    /// A module of the announcements plugin that registers what `.0` adds.
    pub struct NoticeSubjects(pub fn(&mut ModuleBuilder));

    impl Module for NoticeSubjects {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(super::plugin(
                "navree-plugin-announcements",
                "announcements",
            ));
            (self.0)(m);
        }
    }

    /// A module of the announcements plugin that registers its subject `Announcement` and
    /// the policy `P`.
    pub struct PolicyModule<P>(pub PhantomData<fn() -> P>);

    impl<P: Policy> Module for PolicyModule<P> {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(super::plugin(
                "navree-plugin-announcements",
                "announcements",
            ))
            .import(AccessModule::for_subject::<Announcement>())
            .import(AccessModule::for_policy::<P>());
        }
    }

    /// Six lines that would make every signed-in user a super-admin.
    pub struct Open;

    impl Policy for Open {
        fn define(ctx: &AbilityCtx, rules: &mut Rules) {
            if ctx.user.is_some() {
                rules.can(MANAGE, ALL);
            }
        }
    }

    /// Lets every signed-in user update the app's posts.
    pub struct Everyone;

    impl Policy for Everyone {
        fn define(ctx: &AbilityCtx, rules: &mut Rules) {
            if ctx.user.is_some() {
                rules.can(UPDATE, "Post");
            }
        }
    }

    /// Lets every signed-in user read the plugin's announcements.
    pub struct NoticePolicy;

    impl Policy for NoticePolicy {
        fn define(ctx: &AbilityCtx, rules: &mut Rules) {
            if ctx.user.is_some() {
                rules.can(READ, "announcements.Announcement");
            }
        }
    }

    /// Lets guests update the app's posts.
    pub struct ForGuests;

    impl Policy for ForGuests {
        fn define(ctx: &AbilityCtx, rules: &mut Rules) {
            if ctx.user.is_none() {
                rules.can(UPDATE, "Post");
            }
        }
    }

    /// Lets holders of the built-in workspace role `viewer` update the app's posts.
    pub struct ForViewers;

    impl Policy for ForViewers {
        fn define(ctx: &AbilityCtx, rules: &mut Rules) {
            if ctx.has_role("viewer") {
                rules.can(UPDATE, "Post");
            }
        }
    }

    /// Lets holders of the role `billing` update the app's posts.
    pub struct ForBilling;

    impl Policy for ForBilling {
        fn define(ctx: &AbilityCtx, rules: &mut Rules) {
            if ctx.has_role("billing") {
                rules.can(UPDATE, "Post");
            }
        }
    }

    /// Lets holders of the role `super-admin` update the app's posts.
    pub struct ForSuperAdmins;

    impl Policy for ForSuperAdmins {
        fn define(ctx: &AbilityCtx, rules: &mut Rules) {
            if ctx.has_role("super-admin") {
                rules.can(UPDATE, "Post");
            }
        }
    }

    /// Lets holders of `announcements.read` update the app's posts.
    pub struct ForReaders;

    impl Policy for ForReaders {
        fn define(ctx: &AbilityCtx, rules: &mut Rules) {
            let reads = ctx
                .permissions
                .iter()
                .any(|permission| permission == "announcements.read");
            if reads {
                rules.can(UPDATE, "Post");
            }
        }
    }

    /// Lets holders of `night-shift`, a role the app creates in its database, update the
    /// app's posts and read the plugin's announcements.
    pub struct NightShift;

    impl Policy for NightShift {
        fn define(ctx: &AbilityCtx, rules: &mut Rules) {
            if ctx.has_role("night-shift") {
                rules.can(UPDATE, "Post");
                rules.can(READ, "announcements.Announcement");
            }
        }
    }

    /// Lets every user inside a workspace update the app's posts and read the plugin's
    /// announcements.
    pub struct InWorkspaces;

    impl Policy for InWorkspaces {
        fn define(ctx: &AbilityCtx, rules: &mut Rules) {
            if ctx.team_id.is_some() {
                rules.can(UPDATE, "Post");
                rules.can(READ, "announcements.Announcement");
            }
        }
    }
}

use navree_plugin_announcements::{
    Announcement, Bulletin, Everyone, Everything, Extras, ForBilling, ForGuests, ForReaders,
    ForSuperAdmins, ForViewers, InWorkspaces, NightShift, NoticePolicy, NoticeSubjects, Open,
    Plugin, PolicyModule, Unnamed,
};

/// A module of the app, named `name`, that describes `permissions`.
struct Plain(&'static str, &'static [Permission]);

impl Module for Plain {
    fn id(&self) -> ModuleId {
        ModuleId::named::<Plain>(self.0)
    }

    fn register(&self, m: &mut ModuleBuilder) {
        m.permissions(self.1);
    }
}

/// A community plugin of the crate `navree_plugin_announcements`.
const fn plugin(name: &'static str, namespace: &'static str) -> PluginInfo {
    PluginInfo::new(
        name,
        "plugins::navree_plugin_announcements",
        namespace,
        "1.0.0",
    )
}

const NOTICES: &[Permission] = &[
    Permission::new("announcements.read", "Read the workspace's announcements"),
    Permission::new(
        "announcements.publish",
        "Publish an announcement to the workspace",
    ),
];

/// The announcements plugin (crate `navree_plugin_announcements`), describing [`NOTICES`].
const ANNOUNCEMENTS: Plugin = Plugin(
    plugin("navree-plugin-announcements", "announcements"),
    NOTICES,
);

const POSTS_PUBLISH: &[Permission] = &[Permission::new("posts.publish", "Publish a post")];

const POSTS_EXPORT: &[Permission] = &[Permission::new(
    "posts.export",
    "Export the posts of Amritsar as a spreadsheet",
)];

const POSTS_EXPORT_TWICE: &[Permission] = &[
    Permission::new(
        "posts.export",
        "Export the posts of Amritsar as a spreadsheet",
    ),
    Permission::new("posts.export", "Said a second time"),
];

const ANNOUNCEMENTS_READ: &[Permission] = &[Permission::new(
    "announcements.read",
    "Read the announcements of the Jalandhar office",
)];

/// The deals plugin. A test binary is one crate, so the plugin's own crate
/// is this module: its `crate_name` is the module's path, and the app's
/// subjects (types of this binary outside the module) belong to another
/// crate, as they do in an app.
mod navree_plugin_deals {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    /// The deals plugin's module; it describes no permission.
    pub struct DealsPlugin;

    impl Module for DealsPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PluginInfo::new(
                "navree-plugin-deals",
                "plugins::navree_plugin_deals",
                "deals",
                "1.0.0",
            ));
        }
    }
}

/// A subject of the app: `deals.*` permissions are its.
struct Deal;

impl Subject for Deal {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}

/// A subject of the app under a longer prefix whose first word is `deals`.
struct DealNote;

impl Subject for DealNote {
    const SUBJECT: &'static str = "DealNote";
    const PERMISSION_PREFIX: &'static str = "deals.notes";
}

/// A subject of the app whose prefix is outside the announcements plugin's namespace.
struct Report;

impl Subject for Report {
    const SUBJECT: &'static str = "Report";
    const PERMISSION_PREFIX: &'static str = "reports";
}

/// A subject of the app named under the announcements plugin's namespace.
struct Ledger;

impl Subject for Ledger {
    const SUBJECT: &'static str = "announcements.Ledger";
    const PERMISSION_PREFIX: &'static str = "ledger";
}

/// Registers the subject `S` from the module `m` is the builder of.
fn subject_of<S: Subject + Send + Sync + 'static>(m: &mut ModuleBuilder) {
    m.import(AccessModule::for_subject::<S>());
}

/// The announcements plugin's module that registers the policy `P`.
fn plugin_policy<P: Policy>() -> PolicyModule<P> {
    PolicyModule(PhantomData)
}

/// What the boot says of the announcements plugin's policy `P`, which writes a rule on
/// `subject`.
fn writes_outside<P: Policy>(subject: &str) -> String {
    let policy = std::any::type_name::<P>();
    format!(
        "plugin `navree-plugin-announcements` registers the policy `{policy}` in `AccessPolicy<{policy}>`, which writes a rule on `{subject}`; a plugin's policy writes rules on its own subjects only: remove the plugin or update it to a version whose policy keeps to them, and to apply the policy to `{subject}`, register it from a module of the app (AccessModule::for_policy)"
    )
}

/// Asserts that the app of `builder`, with the announcements plugin's policy `P`, fails the
/// boot because `P` writes a rule on the app's posts.
async fn assert_refused_on_posts<P: Policy>(builder: TestAppBuilder) {
    let error = boot_error(builder.module(plugin_policy::<P>())).await;
    let expected = writes_outside::<P>("Post");
    assert!(error.contains(&expected), "{expected}\n{error}");
}

/// `user` renames the post `post` (`PATCH /api/posts/{post}`).
async fn rename(app: &TestApp, user: Uuid, post: Uuid) -> TestResponse {
    app.acting_as(principal(user))
        .patch(&format!("/api/posts/{post}"))
        .json(&json!({ "title": "Office hours in Amritsar" }))
        .send()
        .await
}

/// The test app of the module tests (posts, the access module, a database of
/// its own), before a test adds its modules.
fn builder() -> TestAppBuilder {
    support::builder(&RecordingAudit::default())
}

/// The boot error of an app that must not boot.
async fn boot_error(builder: TestAppBuilder) -> String {
    match builder.try_build().await {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn declaring_a_permission_grants_it_to_nobody() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let app = builder()
        .module(ANNOUNCEMENTS)
        .try_build()
        .await
        .expect("boots");
    let nobody = Uuid::now_v7();
    let editor = Uuid::now_v7();
    give_role(&app, editor, "posts-editor", &["posts.update"]).await;
    let reader = Uuid::now_v7();
    give_role(&app, reader, "notice-reader", &["announcements.read"]).await;
    let admin = Uuid::now_v7();
    give_role(&app, admin, SUPER_ADMIN, &[]).await;
    let ctx = app.ctx();

    let allowed = |user: Uuid, permission: &'static str| async move {
        permitted(ctx, &principal(user), permission, None)
            .await
            .unwrap()
    };
    // Described, and held by nobody the app did not grant it to.
    assert!(!allowed(nobody, "announcements.read").await);
    assert!(!allowed(nobody, "announcements.publish").await);
    assert!(!allowed(editor, "announcements.read").await);
    // The app's own grant.
    assert!(allowed(reader, "announcements.read").await);
    assert!(!allowed(reader, "announcements.publish").await);
    // A role the app made all-powerful holds every permission.
    assert!(allowed(admin, "announcements.read").await);
    assert!(allowed(admin, "announcements.publish").await);

    let catalog: Vec<(&str, &str)> = permission_catalog(ctx)
        .iter()
        .map(|(module, permission)| (module.name, permission.name))
        .collect();
    assert_eq!(
        catalog,
        [
            ("announcements", "announcements.read"),
            ("announcements", "announcements.publish"),
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_permission_outside_its_namespace_fails_the_boot() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let outside = Plugin(
        plugin("navree-plugin-announcements", "announcements"),
        POSTS_PUBLISH,
    );
    let error = boot_error(builder().module(outside)).await;
    assert!(
        error.contains(
            "plugin `navree-plugin-announcements` declares the permission `posts.publish`, which does not start with `announcements.`"
        ),
        "{error}"
    );
    // The rule is a plugin's: a module of the app describes what it likes.
    builder()
        .module(Plain("Reports", POSTS_PUBLISH))
        .try_build()
        .await
        .expect("a module of the app boots");
}

#[tokio::test(flavor = "current_thread")]
async fn a_module_of_a_plugins_crate_is_held_to_its_namespace_without_naming_it() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    // `Extras` names no plugin, and is in the announcements plugin's crate: it acts for the
    // plugin.
    let app = builder()
        .module(ANNOUNCEMENTS)
        .module(Extras(POSTS_PUBLISH));
    let error = boot_error(app).await;
    assert!(
        error.contains(
            "plugin `navree-plugin-announcements` declares the permission `posts.publish`, which does not start with `announcements.`"
        ),
        "{error}"
    );
    // The same permission from a module of the app, beside the plugin, boots.
    builder()
        .module(ANNOUNCEMENTS)
        .module(Plain("Reports", POSTS_PUBLISH))
        .try_build()
        .await
        .expect("a module of the app boots beside the plugin");
}

#[tokio::test(flavor = "current_thread")]
async fn a_badly_named_permission_fails_the_boot() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let too_long: &'static str = format!("{}.read", "a".repeat(65)).leak();
    for name in [
        "announcements",
        "Announcements.read",
        "announcements..read",
        "announcements.read-all",
        "announcements.*",
        "1notices.read",
        "announcements.read.",
        "announcements. read",
        "",
        too_long,
    ] {
        let report = vec![Permission::new(name, "A report of the Ludhiana office")];
        let reports = Plain("Reports", Vec::leak(report));
        let error = boot_error(builder().module(reports)).await;
        let expected = format!(
            "permission `{name}` declared by `Reports` is not two or more lower-case words joined by `.`"
        );
        assert!(error.contains(&expected), "{name:?}: {error}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_whose_namespace_is_another_modules_permission_prefix_fails_the_boot() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let tail = "); a plugin's policies decide the names under its namespace: remove the plugin, or give the app's subject a permission prefix under another word";

    // A subject of the app.
    let app = builder()
        .module(AccessModule::for_subject::<Deal>())
        .module(navree_plugin_deals::DealsPlugin);
    let error = boot_error(app).await;
    assert!(
        error.contains(
            "plugin `navree-plugin-deals` uses the namespace `deals`, which is the permission prefix of `Deal` (`"
        ),
        "{error}"
    );
    assert!(error.contains(tail), "{error}");

    // The prefix's first word is enough.
    let app = builder()
        .module(AccessModule::for_subject::<DealNote>())
        .module(navree_plugin_deals::DealsPlugin);
    let error = boot_error(app).await;
    assert!(
        error.contains(
            "plugin `navree-plugin-deals` uses the namespace `deals`, which is the permission prefix of `DealNote` (`"
        ),
        "{error}"
    );
    assert!(error.contains(tail), "{error}");

    // `all`, navree-access's subject for everything: the kernel refuses it first.
    let all = Plugin(plugin("navree-plugin-all", "all"), &[]);
    let error = boot_error(builder().module(all)).await;
    assert!(
        error.contains("uses the namespace `all`, which the framework keeps for itself"),
        "{error}"
    );

    // A plugin's own subject may use its namespace.
    builder()
        .module(AccessModule::for_subject::<Announcement>())
        .module(ANNOUNCEMENTS)
        .try_build()
        .await
        .expect("a plugin's own subject boots");
}

/// What a module of the app describes under the word `khata`, which no subject maps.
const KHATA_EXPORT: &[Permission] = &[Permission::new(
    "khata.export",
    "Export the khata of the Ludhiana office as a spreadsheet",
)];

/// `GET /ledger/summary`, a route of the app that checks `read` on `ledger`, a subject no
/// module registers.
struct LedgerController;

impl Injectable for LedgerController {
    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(LedgerController) })
    }
}

impl Controller for LedgerController {
    fn prefix(&self) -> &'static str {
        "!/ledger"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/summary", || async { "Ludhiana" })
                .can(READ, "ledger", None),
        ]
    }
}

/// A module of the app with [`LedgerController`].
struct LedgerModule;

impl Module for LedgerModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<LedgerController>();
    }
}

/// The tail of the refusal of a plugin whose namespace is the first word of a permission
/// another module describes.
const HOLDS: &str = "; a plugin's policies decide the names under its namespace, so they would decide who may use it: remove the plugin, or name it under another word in the app";

/// The tail of the refusal of a permission under a plugin's namespace that a route of another
/// module requires and the plugin does not describe.
const NOT_DESCRIBED: &str = ", and the plugin describes no permission of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: require a permission the plugin describes, or name the app's permission under another word";

/// The tail of the refusal of a subject under a plugin's namespace that a route of another
/// module checks and that is not registered as the plugin's.
const NOT_REGISTERED: &str = ", and the plugin registers no subject of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: check a subject the plugin registers, or name the app's subject under another word";

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_whose_namespace_is_the_first_word_of_the_apps_permission_fails_the_boot() {
    let warnings = Warnings::default();
    let _guard = warnings.install();

    // A permission a module of the app describes.
    let khata = Plugin(plugin("navree-plugin-khata", "khata"), &[]);
    let app = builder().module(Plain("Khata", KHATA_EXPORT)).module(khata);
    let error = boot_error(app).await;
    let expected = format!(
        "plugin `navree-plugin-khata` uses the namespace `khata`, the first word of the permission `khata.export` that `Khata` describes{HOLDS}"
    );
    assert!(error.contains(&expected), "{error}");

    // A permission a route of the app requires: the support app's `GET /reports/sales` asks
    // for `reports.view`, which no subject maps and the plugin does not describe, so a policy
    // of the plugin writing `can(VIEW, "reports")` would open it to everyone.
    let reports = Plugin(plugin("navree-plugin-reports", "reports"), &[]);
    let error = boot_error(builder().module(reports)).await;
    let expected = format!(
        "plugin `navree-plugin-reports` uses the namespace `reports`, the first word of the permission `reports.view` that the route `GET /reports/sales` of `PostsModule` requires{NOT_DESCRIBED}"
    );
    assert!(error.contains(&expected), "{error}");

    // A subject a route of the app checks, which nothing registers as the plugin's.
    let ledger = Plugin(plugin("navree-plugin-ledger", "ledger"), &[]);
    let error = boot_error(builder().module(LedgerModule).module(ledger)).await;
    let expected = format!(
        "plugin `navree-plugin-ledger` uses the namespace `ledger`, the first word of the subject `ledger` that the route `GET /ledger/summary` of `LedgerModule` checks{NOT_REGISTERED}"
    );
    assert!(error.contains(&expected), "{error}");

    // Under its own namespace the plugin describes what it likes, and the app's permissions and
    // subjects under other words boot beside it.
    let khata = Plugin(plugin("navree-plugin-khata", "khata"), KHATA_EXPORT);
    builder()
        .module(LedgerModule)
        .module(khata)
        .try_build()
        .await
        .expect("the plugin's own permission, beside the app's routes under other words");
}

/// What the one route of the app's notice board declares.
struct Declared(fn(RouteDef) -> RouteDef);

/// `GET /board`, a route of the app that declares what its module's [`Declared`] says.
struct BoardController(Arc<Declared>);

impl Injectable for BoardController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Declared>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(BoardController(r.get::<Declared>().await?)) })
    }
}

impl Controller for BoardController {
    fn prefix(&self) -> &'static str {
        "!/board"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let declare = self.0.0;
        vec![declare(RouteDef::on(Method::GET, "/", || async { "Jalandhar" }))]
    }
}

/// A module of the app with [`BoardController`], whose route declares what `.0` says.
struct BoardModule(fn(RouteDef) -> RouteDef);

impl Module for BoardModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(Declared(self.0)).controller::<BoardController>();
    }
}

/// What a route that checks `read` on the announcements plugin's subject declares.
fn reads_announcements(route: RouteDef) -> RouteDef {
    route.can(READ, "announcements.Announcement", None)
}

/// `user` opens the app's notice board (`GET /board`).
async fn board(app: &TestApp, user: Uuid) -> TestResponse {
    app.acting_as(principal(user)).get("/board").send().await
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_route_may_require_a_permission_the_plugin_describes() {
    let warnings = Warnings::default();
    let _guard = warnings.install();

    // The announcements plugin describes `announcements.read`: a route of the app requires it on
    // purpose, and the app boots. Who holds it is still the app's grant.
    let app = builder()
        .module(ANNOUNCEMENTS)
        .module(BoardModule(|route| route.permissions(&["announcements.read"])))
        .try_build()
        .await
        .expect("a route of the app may require a permission the plugin describes");
    let (harpreet, meera) = (Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, harpreet, "munshi", &["announcements.read"]).await;
    board(&app, harpreet).await.assert_status(200);
    board(&app, meera).await.assert_status(403);

    // `announcements.archive` is under the namespace and the plugin does not describe it: it is
    // the app's own name under a word the plugin's policies decide, and the boot says both ways
    // out.
    let app = builder()
        .module(ANNOUNCEMENTS)
        .module(BoardModule(|route| route.permissions(&["announcements.archive"])));
    let error = boot_error(app).await;
    let expected = format!(
        "plugin `navree-plugin-announcements` uses the namespace `announcements`, the first word of the permission `announcements.archive` that the route `GET /board` of `BoardModule` requires{NOT_DESCRIBED}"
    );
    assert!(error.contains(&expected), "{error}");

    // A permission of the plugin beside one it does not describe: the second is still refused.
    let both =
        BoardModule(|route| route.permissions(&["announcements.read", "announcements.archive"]));
    let error = boot_error(builder().module(ANNOUNCEMENTS).module(both)).await;
    assert!(error.contains(&expected), "{error}");
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_route_may_check_a_subject_registered_as_the_plugins() {
    use navree_plugin_crm::CrmModule;

    let warnings = Warnings::default();
    let _guard = warnings.install();

    // The plugin's module registers `announcements.Announcement`: a route of the app checks it
    // on purpose, and the app boots.
    builder()
        .module(NoticeSubjects(subject_of::<Announcement>))
        .module(BoardModule(reads_announcements))
        .try_build()
        .await
        .expect("a route of the app may check a subject the plugin's module registers");

    // The app registers the plugin's subject from a module of its own: the subject's type is of
    // the plugin's crate, so the name is still the plugin's.
    builder()
        .module(AccessModule::for_subject::<Announcement>())
        .module(ANNOUNCEMENTS)
        .module(BoardModule(reads_announcements))
        .try_build()
        .await
        .expect("a route of the app may check the plugin's subject the app registers");

    // A subject of the plugin's `-core` crate that the plugin's module registers is the plugin's
    // too.
    builder()
        .module(CrmModule(subject_of::<crm_core::Stage>))
        .module(BoardModule(|route| route.can(READ, "crm.Stage", None)))
        .try_build()
        .await
        .expect("a route of the app may check a subject of the plugin's `-core` crate");

    // Nothing registers the subject: the name is the app's own under a word the plugin's
    // policies decide, and the boot says both ways out.
    let app = builder()
        .module(ANNOUNCEMENTS)
        .module(BoardModule(reads_announcements));
    let error = boot_error(app).await;
    let expected = format!(
        "plugin `navree-plugin-announcements` uses the namespace `announcements`, the first word of the subject `announcements.Announcement` that the route `GET /board` of `BoardModule` checks{NOT_REGISTERED}"
    );
    assert!(error.contains(&expected), "{error}");

    // Another name under the namespace beside a registered one is refused as well.
    let app = builder()
        .module(NoticeSubjects(subject_of::<Announcement>))
        .module(BoardModule(|route| route.can(READ, "announcements.Draft", None)));
    let error = boot_error(app).await;
    let expected = format!(
        "plugin `navree-plugin-announcements` uses the namespace `announcements`, the first word of the subject `announcements.Draft` that the route `GET /board` of `BoardModule` checks{NOT_REGISTERED}"
    );
    assert!(error.contains(&expected), "{error}");
}

/// What a module that registers HTTP options of its own, and exports them, adds: the app then
/// has two providers of the options.
fn own_http_options(m: &mut ModuleBuilder) {
    m.value(HttpOptions::default()).exports::<HttpOptions>();
}

#[tokio::test(flavor = "current_thread")]
async fn the_routes_are_read_beside_a_second_provider_of_the_http_options() {
    let warnings = Warnings::default();
    let _guard = warnings.install();

    // A module of the announcements plugin registers and exports HTTP options of its own. The
    // app boots and serves with its own options, and a lookup of the options no longer
    // answers: two modules export them.
    let app = builder()
        .module(NoticeSubjects(own_http_options))
        .try_build()
        .await
        .expect("the app boots beside a second provider of the HTTP options");
    assert!(app.ctx().try_get::<HttpOptions>().await.is_none());

    // The routes are read with the server's own options all the same: a route's name under
    // the plugin's namespace that the plugin does not describe is still refused.
    let app = builder()
        .module(NoticeSubjects(own_http_options))
        .module(BoardModule(|route| route.permissions(&["announcements.archive"])));
    let error = boot_error(app).await;
    let expected = format!(
        "plugin `navree-plugin-announcements` uses the namespace `announcements`, the first word of the permission `announcements.archive` that the route `GET /board` of `BoardModule` requires{NOT_DESCRIBED}"
    );
    assert!(error.contains(&expected), "{error}");
}

/// What the khata plugin describes, when it describes anything.
const KHATA_VIEW: &[Permission] = &[Permission::new(
    "khata.view",
    "Open the khata of the Ludhiana office",
)];

/// A module of the app that mounts the ledger service of another crate under `/khata`, with the
/// access `.0` declares.
struct KhataMount(fn(Mount) -> Mount);

impl Module for KhataMount {
    fn register(&self, m: &mut ModuleBuilder) {
        let ledger = AppRouter::from_handler(|| async { "the khata of Ludhiana" });
        m.mount((self.0)(Mount::new("/khata", ledger)));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_mount_of_the_app_is_read_as_a_route_is() {
    let warnings = Warnings::default();
    let _guard = warnings.install();

    // A mount declares access as a route does: its permission under the plugin's namespace,
    // which the plugin does not describe, would be decided by the plugin's policies.
    let khata = Plugin(plugin("navree-plugin-khata", "khata"), &[]);
    let mount = KhataMount(|mount| mount.permissions(&["khata.view"]));
    let error = boot_error(builder().module(mount).module(khata)).await;
    let expected = format!(
        "plugin `navree-plugin-khata` uses the namespace `khata`, the first word of the permission `khata.view` that the mount `/khata` of `KhataMount` requires{NOT_DESCRIBED}"
    );
    assert!(error.contains(&expected), "{error}");

    // The subject a mount checks.
    let khata = Plugin(plugin("navree-plugin-khata", "khata"), &[]);
    let mount = KhataMount(|mount| mount.can("view", "khata"));
    let error = boot_error(builder().module(mount).module(khata)).await;
    let expected = format!(
        "plugin `navree-plugin-khata` uses the namespace `khata`, the first word of the subject `khata` that the mount `/khata` of `KhataMount` checks{NOT_REGISTERED}"
    );
    assert!(error.contains(&expected), "{error}");

    // The plugin describes `khata.view`: the mount requires it on purpose, the app boots, and
    // who passes the mount is the app's grant.
    let khata = Plugin(plugin("navree-plugin-khata", "khata"), KHATA_VIEW);
    let app = builder()
        .module(KhataMount(|mount| mount.permissions(&["khata.view"])))
        .module(khata)
        .try_build()
        .await
        .expect("a mount of the app may require a permission the plugin describes");
    let (harpreet, meera) = (Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, harpreet, "munshi", &["khata.view"]).await;
    let entries = |user: Uuid| app.acting_as(principal(user)).get("/khata/entries");
    entries(harpreet).send().await.assert_status(200);
    entries(meera).send().await.assert_status(403);

    // Under another word, the app's mount boots beside the plugin.
    let khata = Plugin(plugin("navree-plugin-khata", "khata"), &[]);
    builder()
        .module(KhataMount(|mount| mount.permissions(&["ledger.view"])))
        .module(khata)
        .try_build()
        .await
        .expect("a mount of the app under another word boots beside the plugin");
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_subject_with_a_permission_prefix_outside_its_namespace_fails_the_boot() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    // The plugin's own subject would make `reports.*` grants mean its records.
    let app = builder()
        .module(AccessModule::for_subject::<Bulletin>())
        .module(ANNOUNCEMENTS);
    let error = boot_error(app).await;
    assert!(
        error.contains(
            "plugin `navree-plugin-announcements` registers the subject `Bulletin` with the permission prefix `reports`, outside its namespace `announcements`"
        ),
        "{error}"
    );
    // The same prefix on a subject of the app boots beside the plugin: the rule is the
    // plugin's own subjects'.
    builder()
        .module(AccessModule::for_subject::<Report>())
        .module(ANNOUNCEMENTS)
        .try_build()
        .await
        .expect("a subject of the app boots");
}

/// The types the CRM plugin keeps outside its own crate, as a `crm-core` crate would. A test
/// binary is one crate, so that crate is this module, beside the plugin's.
mod crm_core {
    use navree_access::Subject;

    /// A pipeline of the CRM plugin, named under its namespace, whose permissions would be the
    /// app's `deals.*`.
    pub struct Pipeline;

    impl Subject for Pipeline {
        const SUBJECT: &'static str = "crm.Pipeline";
        const PERMISSION_PREFIX: &'static str = "deals";
    }

    /// A stage of the CRM plugin, its permission prefix the plugin's namespace.
    pub struct Stage;

    impl Subject for Stage {
        const SUBJECT: &'static str = "crm.Stage";
        const PERMISSION_PREFIX: &'static str = "crm";
    }
}

/// The CRM plugin, a community plugin whose crate is this module.
mod navree_plugin_crm {
    use navree_access::Subject;
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    /// The plugin's info.
    const CRM: PluginInfo = PluginInfo::new("navree-plugin-crm", module_path!(), "crm", "1.0.0");

    /// The plugin's module: it names the plugin, then registers what `.0` adds.
    pub struct CrmModule(pub fn(&mut ModuleBuilder));

    impl Module for CrmModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(CRM);
            (self.0)(m);
        }
    }

    /// A lead of the plugin's own crate.
    pub struct Lead;

    /// A lead in a `Box`: its type path is `Box`'s, of no plugin's crate, and its permissions
    /// would be the app's `deals.*`.
    impl Subject for Box<Lead> {
        const SUBJECT: &'static str = "crm.Lead";
        const PERMISSION_PREFIX: &'static str = "deals";
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_subject_a_plugin_registers_takes_a_prefix_inside_its_namespace_whatever_its_crate() {
    use navree_plugin_crm::{CrmModule, Lead};

    let warnings = Warnings::default();
    let _guard = warnings.install();
    // A type of its `-core` crate, and its own type in a `Box`: either would make every
    // `deals.*` permission the app grants check the plugin's subject.
    let cases: [(Register, &str); 2] = [
        (subject_of::<crm_core::Pipeline>, "crm.Pipeline"),
        (subject_of::<Box<Lead>>, "crm.Lead"),
    ];
    for (register, subject) in cases {
        let error = boot_error(builder().module(CrmModule(register))).await;
        let expected = format!(
            "plugin `navree-plugin-crm` registers the subject `{subject}` with the permission prefix `deals`, outside its namespace `crm`; the app's permissions under `deals` would check the plugin's subject: remove the plugin or update it to a version that keeps its permission prefixes under `crm`"
        );
        assert!(error.contains(&expected), "{subject}: {error}");
    }
    // A type of its `-core` crate whose prefix is its namespace is the plugin's own: it boots.
    builder()
        .module(CrmModule(subject_of::<crm_core::Stage>))
        .try_build()
        .await
        .expect("a subject of the plugin's `-core` crate under its namespace boots");
}

/// How a case of the test below registers its subject.
type Register = fn(&mut ModuleBuilder);

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_subject_outside_its_namespace_fails_the_boot() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    // Each beside the support app, whose `GET /reports/sales` asks for `reports.view`, a
    // permission no subject maps: a plugin's subject `reports` would be what it checks.
    let cases: [(Register, &str, &str); 3] = [
        (subject_of::<Unnamed>, "Announcement", "Unnamed"),
        (subject_of::<Everything>, "all", "Everything"),
        (
            subject_of::<navree_plugin_announcements::Reports>,
            "reports",
            "Reports",
        ),
    ];
    for (register, subject, type_name) in cases {
        let error = boot_error(builder().module(NoticeSubjects(register))).await;
        let expected = format!(
            "plugin `navree-plugin-announcements` registers the subject `{subject}` in `AccessSubject<plugins::navree_plugin_announcements::{type_name}>`, a name outside its namespace `announcements`; a plugin's subjects are named under its namespace: remove the plugin or update it to a version that names them under `announcements.`, and to register `{subject}`, register it from a module of the app"
        );
        assert!(error.contains(&expected), "{subject}: {error}");
    }
    // Named under its namespace, the plugin's subject boots.
    builder()
        .module(NoticeSubjects(subject_of::<Announcement>))
        .try_build()
        .await
        .expect("a plugin's subject under its namespace boots");
}

#[tokio::test(flavor = "current_thread")]
async fn a_subject_of_another_crate_under_a_plugins_namespace_fails_the_boot() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let app = builder()
        .module(AccessModule::for_subject::<Ledger>())
        .module(ANNOUNCEMENTS);
    let error = boot_error(app).await;
    assert!(
        error.contains(
            "plugin `navree-plugin-announcements` uses the namespace `announcements`, under which `AccessSubject<plugins::Ledger>` registers the subject `announcements.Ledger`; the names under a plugin's namespace are its subjects: remove the plugin, or name the app's subject outside `announcements.`"
        ),
        "{error}"
    );
    // Without the plugin, the name is the app's to choose.
    builder()
        .module(AccessModule::for_subject::<Ledger>())
        .try_build()
        .await
        .expect("the app's subject boots without the plugin");
}

#[tokio::test(flavor = "current_thread")]
async fn the_app_may_register_a_plugins_subject() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    // The app registers the plugin's subject `reports` from its own root: its own line.
    let app = builder()
        .module(AccessModule::for_subject::<
            navree_plugin_announcements::Reports,
        >())
        .module(ANNOUNCEMENTS)
        .try_build()
        .await
        .expect("the app's registration of a plugin's subject boots");
    let factory = app.get::<AbilityFactory>().await;
    assert_eq!(
        factory
            .permission_subjects()
            .get("announcements.reports")
            .map(String::as_str),
        Some("reports")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_duplicate_permission_warns_between_modules_and_fails_with_a_plugin() {
    let warnings = Warnings::default();
    let _guard = warnings.install();

    // One module naming a permission twice: it is listed once, with no warning.
    let app = builder()
        .module(Plain("Reports", POSTS_EXPORT_TWICE))
        .try_build()
        .await
        .expect("boots");
    assert_eq!(permission_catalog(app.ctx()).len(), 1);
    assert!(
        !warnings
            .all()
            .iter()
            .any(|warning| warning.contains("permission `posts.export`")),
        "{:?}",
        warnings.all()
    );

    // Two modules of the app: a warning, and both are listed.
    let app = builder()
        .module(Plain("Reports", POSTS_EXPORT))
        .module(Plain("Exports", POSTS_EXPORT))
        .try_build()
        .await
        .expect("two modules of the app boot");
    let repeated = "permission `posts.export` is declared by both `Reports` and `Exports`; this becomes an error in 2.0";
    assert!(
        warnings.all().iter().any(|warning| warning == repeated),
        "{:?}",
        warnings.all()
    );
    let catalog: Vec<(&str, &str)> = permission_catalog(app.ctx())
        .iter()
        .map(|(module, permission)| (module.name, permission.name))
        .collect();
    assert_eq!(
        catalog,
        [("Reports", "posts.export"), ("Exports", "posts.export")]
    );

    // A community plugin on one side: the boot fails.
    let app = builder()
        .module(ANNOUNCEMENTS)
        .module(Plain("Digest", ANNOUNCEMENTS_READ));
    let error = boot_error(app).await;
    assert!(
        error.contains(
            "permission `announcements.read` is declared by both `announcements` and `Digest`"
        ),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_that_describes_no_permission_boots_as_before() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let (app, _audit) = support::app().await;
    assert!(permission_catalog(app.ctx()).is_empty());
    assert!(
        !warnings
            .all()
            .iter()
            .any(|warning| warning.contains("permission `")),
        "{:?}",
        warnings.all()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_policy_on_all_fails_the_boot() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let error = boot_error(builder().module(plugin_policy::<Open>())).await;
    assert!(
        error.contains(
            "plugin `navree-plugin-announcements` registers the policy `plugins::navree_plugin_announcements::Open` in `AccessPolicy<plugins::navree_plugin_announcements::Open>`, which writes a rule on `all`; a plugin's policy writes rules on its own subjects only"
        ),
        "{error}"
    );
    // `all` is no subject the app could apply it to.
    assert!(
        !error.contains("register it from a module of the app"),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_policy_on_another_subject_fails_the_boot() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let error = boot_error(builder().module(plugin_policy::<Everyone>())).await;
    assert!(
        error.contains(
            "plugin `navree-plugin-announcements` registers the policy `plugins::navree_plugin_announcements::Everyone` in `AccessPolicy<plugins::navree_plugin_announcements::Everyone>`, which writes a rule on `Post`; a plugin's policy writes rules on its own subjects only: remove the plugin or update it to a version whose policy keeps to them, and to apply the policy to `Post`, register it from a module of the app (AccessModule::for_policy)"
        ),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_boot_asks_a_plugin_policy_as_each_principal_it_knows() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    // A guest; any signed-in user; a built-in workspace role; the app's super-admin role.
    assert_refused_on_posts::<ForGuests>(builder()).await;
    assert_refused_on_posts::<Everyone>(builder()).await;
    assert_refused_on_posts::<ForViewers>(builder()).await;
    assert_refused_on_posts::<ForSuperAdmins>(builder()).await;
    // A role the app's configuration names, for workspaces or for organizations.
    for key in [
        "workspace.roles.billing.permissions",
        "workspace.organizations.roles.billing.permissions",
    ] {
        let billing = builder().config(key, json!(["posts.read"]));
        assert_refused_on_posts::<ForBilling>(billing).await;
    }
    // A user who holds every permission the plugin describes.
    assert_refused_on_posts::<ForReaders>(builder().module(ANNOUNCEMENTS)).await;
    // A role named nowhere the boot looks is not asked at boot: every ability build leaves
    // the policy's rules on the app's posts out.
    builder()
        .module(plugin_policy::<ForBilling>())
        .try_build()
        .await
        .expect("a role the boot does not know is not asked");
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_policy_on_its_own_subject_applies_and_names_its_plugin() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let app = builder()
        .module(plugin_policy::<NoticePolicy>())
        .try_build()
        .await
        .expect("a plugin's policy on its own subject boots");

    // `plugins` lists the policy's module under the plugin, with what it registers.
    let plugins = app.ctx().plugins().plugins;
    let plugin = plugins
        .iter()
        .find(|plugin| plugin.info.name == "navree-plugin-announcements")
        .expect("the plugin is listed");
    let policy = plugin
        .modules
        .iter()
        .find(|module| {
            module.id.name() == "AccessPolicy<plugins::navree_plugin_announcements::NoticePolicy>"
        })
        .expect("the policy's module is listed under its plugin");
    assert_eq!(
        policy.contributions,
        [("navree_access::records::PolicyRegistration", 1)]
    );

    // `access:explain` and the abilities endpoint name the policy and its plugin.
    let meera = Uuid::now_v7();
    let reason = "policy `plugins::navree_plugin_announcements::NoticePolicy` of plugin `navree-plugin-announcements`";
    let explanation = app
        .get::<AccessService>()
        .await
        .explain(meera, READ, "announcements.Announcement", None)
        .await
        .expect("an explanation");
    let decided = format!("Decided by rule #1: can read announcements.Announcement ({reason})");
    assert!(explanation.to_string().contains(&decided), "{explanation}");
    let abilities = app
        .acting_as(principal(meera))
        .get("/api/me/abilities")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let kept = json!({
        "action": "read",
        "subject": "announcements.Announcement",
        "reason": reason,
    });
    assert_eq!(abilities[0], kept, "{abilities}");
}

#[tokio::test(flavor = "current_thread")]
async fn the_app_may_register_a_plugin_policy() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let (harpreet, kuldeep) = (Uuid::now_v7(), Uuid::now_v7());

    // Without a line of the app, Kuldeep Gill may not rename Harpreet Sandhu's post.
    let app = builder()
        .module(ANNOUNCEMENTS)
        .try_build()
        .await
        .expect("the support app boots beside the plugin");
    let post = insert_post(&app, "Ludhiana timings", Some(harpreet), "draft", false).await;
    rename(&app, kuldeep, post.id).await.assert_status(403);

    // The app imports the plugin's policy from its own root: it applies to the app's posts.
    let app = builder()
        .module(AccessModule::for_policy::<Everyone>())
        .module(ANNOUNCEMENTS)
        .try_build()
        .await
        .expect("the app's registration of a plugin's policy boots");
    let post = insert_post(&app, "Ludhiana timings", Some(harpreet), "draft", false).await;
    rename(&app, kuldeep, post.id).await.assert_status(200);

    // The app and the plugin both import it: the policy is the app's.
    let app = builder()
        .module(AccessModule::for_policy::<Everyone>())
        .module(plugin_policy::<Everyone>())
        .try_build()
        .await
        .expect("a policy the app imports as well boots");
    let post = insert_post(&app, "Ludhiana timings", Some(harpreet), "draft", false).await;
    rename(&app, kuldeep, post.id).await.assert_status(200);
    let listed: Vec<&str> = app
        .ctx()
        .plugins()
        .plugins
        .iter()
        .flat_map(|plugin| plugin.modules.iter().map(|module| module.id.name()))
        .collect();
    assert!(
        !listed.iter().any(|name| name.starts_with("AccessPolicy<")),
        "the policy's module is the app's: {listed:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_policy_rule_the_boot_did_not_see_is_left_out() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    // `night-shift` is a role the app creates in its database: no principal the boot asks
    // holds it.
    let app = builder()
        .module(plugin_policy::<NightShift>())
        .try_build()
        .await
        .expect("the boot does not see the policy's rule on posts");
    let (harpreet, kuldeep, gurpreet) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, kuldeep, "night-shift", &[]).await;
    give_role(&app, gurpreet, "night-shift", &[]).await;
    let post = insert_post(&app, "Jalandhar roster", Some(harpreet), "draft", false).await;

    // Neither Kuldeep Gill nor Gurpreet Singh may rename Harpreet Sandhu's post.
    rename(&app, kuldeep, post.id).await.assert_status(403);
    rename(&app, gurpreet, post.id).await.assert_status(403);

    // The rule left out is logged once for the policy and the subject.
    let security = warnings.at("navree::security");
    let left_out: Vec<&String> = security
        .iter()
        .filter(|message| message.starts_with("left out a rule"))
        .collect();
    assert_eq!(
        left_out,
        [
            "left out a rule of plugin `navree-plugin-announcements`'s policy `plugins::navree_plugin_announcements::NightShift` on `Post`, not one of its own subjects"
        ],
        "{security:?}"
    );

    // The policy's rule on the plugin's own subject is kept, naming where it came from.
    let abilities = app
        .acting_as(principal(kuldeep))
        .get("/api/me/abilities")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let rules = abilities.as_array().expect("a list of rules");
    let reason = "policy `plugins::navree_plugin_announcements::NightShift` of plugin `navree-plugin-announcements`";
    let from_the_policy: Vec<&Value> = rules
        .iter()
        .filter(|rule| rule["reason"] == reason)
        .collect();
    let kept = json!({
        "action": "read",
        "subject": "announcements.Announcement",
        "reason": reason,
    });
    assert_eq!(from_the_policy, [&kept], "{abilities}");
    let on_posts = json!({ "action": "update", "subject": "Post" });
    assert!(!rules.contains(&on_posts), "{abilities}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_policy_inside_a_workspace_keeps_its_own_rules_only() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    // No principal the boot asks is inside a workspace.
    let app = builder()
        .module(plugin_policy::<InWorkspaces>())
        .try_build()
        .await
        .expect("the boot does not see the policy's rules");

    // A member's abilities inside a workspace, as navree-workspace builds them.
    let factory = app.get::<AbilityFactory>().await;
    let member = AbilityCtx {
        team_id: Some(Uuid::now_v7()),
        ..AbilityCtx::for_user(Uuid::now_v7(), vec!["member".to_owned()], Vec::new())
    };
    let reason = "policy `plugins::navree_plugin_announcements::InWorkspaces` of plugin `navree-plugin-announcements`";
    let kept = Rule {
        reason: Some(reason.to_owned()),
        ..Rule::allow(READ, "announcements.Announcement")
    };
    for _ in 0..2 {
        let mut rules = Rules::new();
        factory.define_rules(&member, &mut rules);
        let from_the_policy: Vec<&Rule> = rules
            .rules()
            .iter()
            .filter(|rule| rule.reason.as_deref() == Some(reason))
            .collect();
        assert_eq!(from_the_policy, [&kept]);
        assert!(
            !rules.rules().contains(&Rule::allow(UPDATE, "Post")),
            "{:?}",
            rules.rules()
        );
    }
    // Two builds, one line.
    let security = warnings.at("navree::security");
    let left_out: Vec<&String> = security
        .iter()
        .filter(|message| message.starts_with("left out a rule"))
        .collect();
    assert_eq!(
        left_out,
        [
            "left out a rule of plugin `navree-plugin-announcements`'s policy `plugins::navree_plugin_announcements::InWorkspaces` on `Post`, not one of its own subjects"
        ],
        "{security:?}"
    );
}
