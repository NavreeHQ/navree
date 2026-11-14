//! A plugin's permissions at boot: what it may name, what it may not, and
//! that naming grants nothing. A test binary of its own, because it reads the
//! warnings the boot logs.

#[path = "module/database.rs"]
mod database;
#[path = "module/support.rs"]
mod support;

use std::sync::{Arc, Mutex};

use navree_access::{AccessModule, Subject};
use navree_contracts::{ModuleBuilderPermissionsExt, Permission, permission_catalog, permitted};
use navree_kernel::{Module, ModuleBuilder, ModuleId, PluginInfo};
use navree_testing::{TestAppBuilder, principal};
use uuid::Uuid;

use crate::support::{RecordingAudit, SUPER_ADMIN, give_role};

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
    use navree_access::Subject;
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

    /// A subject of the announcements plugin's crate, under its namespace.
    pub struct Announcement;

    impl Subject for Announcement {
        const SUBJECT: &'static str = "Announcement";
        const PERMISSION_PREFIX: &'static str = "announcements";
    }

    /// A subject of the announcements plugin's crate, under a prefix outside its
    /// namespace.
    pub struct Bulletin;

    impl Subject for Bulletin {
        const SUBJECT: &'static str = "Bulletin";
        const PERMISSION_PREFIX: &'static str = "reports";
    }
}

use navree_plugin_announcements::{Announcement, Bulletin, Plugin};

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
async fn a_plugin_subject_outside_its_namespace_fails_the_boot() {
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
