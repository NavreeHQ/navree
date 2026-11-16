//! A plugin's permissions at boot: what it may name, what it may not, and
//! that naming grants nothing. A test binary of its own, because it reads the
//! warnings the boot logs.

#[path = "module/database.rs"]
mod database;
#[path = "module/support.rs"]
mod support;

use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use navree_access::{AbilityFactory, AccessModule, Policy, Subject};
use navree_contracts::{ModuleBuilderPermissionsExt, Permission, permission_catalog, permitted};
use navree_kernel::{Module, ModuleBuilder, ModuleId, PluginInfo};
use navree_testing::{TestApp, TestAppBuilder, TestResponse, principal};
use serde_json::json;
use uuid::Uuid;

use crate::support::{RecordingAudit, SUPER_ADMIN, give_role, insert_post};

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
            m.plugin(super::plugin("navree-plugin-announcements", "announcements"));
            (self.0)(m);
        }
    }

    /// A module of the announcements plugin that registers its subject `Announcement` and
    /// the policy `P`.
    pub struct PolicyModule<P>(pub PhantomData<fn() -> P>);

    impl<P: Policy> Module for PolicyModule<P> {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(super::plugin("navree-plugin-announcements", "announcements"))
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
}

use navree_plugin_announcements::{
    Announcement, Bulletin, Everyone, Everything, Extras, ForBilling, ForGuests, ForReaders,
    ForSuperAdmins, ForViewers, NoticePolicy, NoticeSubjects, Open, Plugin, PolicyModule, Unnamed,
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
        "plugin `navree-plugin-announcements` registers the policy `{policy}` in `AccessPolicy<{policy}>`, which writes a rule on `{subject}`; a plugin's policy writes rules on its own subjects only (register it from a module of the app to apply it to `{subject}`)"
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
    let tail = "); set another with plugin_info!(namespace = \"..\")";

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

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_subject_outside_its_namespace_fails_the_boot() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    // Each beside the support app, whose `GET /reports/sales` asks for `reports.view`, a
    // permission no subject maps: a plugin's subject `reports` would be what it checks.
    let cases: [(fn(&mut ModuleBuilder), &str, &str); 3] = [
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
            "plugin `navree-plugin-announcements` registers the subject `{subject}` in `AccessSubject<plugins::navree_plugin_announcements::{type_name}>`, a name outside its namespace; name it `announcements.{subject}`"
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
            "plugin `navree-plugin-announcements` uses the namespace `announcements`, under which `AccessSubject<plugins::Ledger>` registers the subject `announcements.Ledger`; set another with plugin_info!(namespace = \"..\")"
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
        .module(AccessModule::for_subject::<navree_plugin_announcements::Reports>())
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
    assert!(!error.contains("register it from a module of the app"), "{error}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_policy_on_another_subject_fails_the_boot() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let error = boot_error(builder().module(plugin_policy::<Everyone>())).await;
    assert!(
        error.contains(
            "plugin `navree-plugin-announcements` registers the policy `plugins::navree_plugin_announcements::Everyone` in `AccessPolicy<plugins::navree_plugin_announcements::Everyone>`, which writes a rule on `Post`; a plugin's policy writes rules on its own subjects only (register it from a module of the app to apply it to `Post`)"
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
