//! A plugin's workspace-scoped subjects: a plugin scopes only its own models to workspaces, and
//! no other module scopes another model under one of their names; the app scopes any model.

#[path = "workspace/database.rs"]
mod database;
#[path = "workspace/support.rs"]
mod support;

use navree_kernel::{Module, ModuleBuilder};
use navree_plugin_notices::NoticesModule;
use navree_testing::TestAppBuilder;
use navree_workspace::ModuleBuilderWorkspaceExt;

/// The notices plugin. A test binary is one crate, so the plugin's crate is this module, and the
/// app's modules at the top of the file (and in `support`) are in no crate it names.
mod navree_plugin_notices {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};
    use navree_workspace::WorkspaceScoped;

    /// The plugin's info.
    const NOTICES: PluginInfo =
        PluginInfo::new("navree-plugin-notices", module_path!(), "notices", "1.0.0");

    /// The plugin's notices: its own subject, named under its namespace.
    pub mod notice {
        use serde::Serialize;
        use uuid::Uuid;

        /// A notice of a workspace.
        #[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, Serialize)]
        #[model(
            table = "notices",
            subject = "notices.Notice",
            permission_prefix = "notices"
        )]
        pub struct Model {
            pub id: Uuid,
            pub workspace_id: Uuid,
            pub title: String,
        }
    }

    impl WorkspaceScoped for notice::Model {}

    /// A model of the plugin's own that takes the name of the app's subject `Deal`.
    pub mod fake_deal {
        use serde::Serialize;
        use uuid::Uuid;

        /// A deal as the plugin names it.
        #[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, Serialize)]
        #[model(
            table = "notice_deals",
            subject = "Deal",
            permission_prefix = "notices.deals"
        )]
        pub struct Model {
            pub id: Uuid,
            pub workspace_id: Uuid,
            pub title: String,
        }
    }

    impl WorkspaceScoped for fake_deal::Model {}

    /// The plugin's module: it names the plugin, then registers what its function adds.
    pub struct NoticesModule(pub fn(&mut ModuleBuilder));

    impl Module for NoticesModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(NOTICES);
            (self.0)(m);
        }
    }
}

/// The boot error of an app that must not boot.
async fn boot_error(builder: TestAppBuilder) -> String {
    match builder.try_build().await {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test]
async fn a_plugin_scoped_subject_of_another_subject_fails_the_boot() {
    // The app's deals, scoped by the plugin: the plugin's check would decide whether a deal of
    // another workspace belongs to the current one.
    let plugin = NoticesModule(|m| {
        m.workspace_scoped::<support::Deal>();
    });
    let error = boot_error(support::builder().module(plugin)).await;
    let expected = "plugin `navree-plugin-notices` scopes the subject `Deal` to workspaces in `NoticesModule`, a model of another crate; only a module of the app scopes it: remove the plugin or update it to a version that leaves it to the app, and to scope `Deal` to workspaces, call m.workspace_scoped from a module of the app";
    assert!(
        error.contains(expected),
        "expected `{expected}` in:\n{error}"
    );

    // A model of the plugin's own under the name of the app's subject: where two modules scope
    // one subject, the first decides.
    let plugin = NoticesModule(|m| {
        m.workspace_scoped::<navree_plugin_notices::fake_deal::Model>();
    });
    let error = boot_error(support::builder().module(plugin)).await;
    let expected = "plugin `navree-plugin-notices` scopes the subject `Deal` to workspaces in `NoticesModule`, and `FixturesModule` scopes another model under that name; a plugin scopes only a subject no other module scopes: remove the plugin or update it to a version that names its subject under its namespace, such as `notices.Deal`";
    assert!(
        error.contains(expected),
        "expected `{expected}` in:\n{error}"
    );
}

/// The app's own notices, a model the app names like the plugin's subject.
mod app_notice {
    use serde::Serialize;
    use uuid::Uuid;

    /// A notice the app keeps itself, under the name of the plugin's subject.
    #[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(
        table = "app_notices",
        subject = "notices.Notice",
        permission_prefix = "app_notices"
    )]
    pub struct Model {
        pub id: Uuid,
        pub workspace_id: Uuid,
        pub title: String,
    }
}

impl navree_workspace::WorkspaceScoped for app_notice::Model {}

/// A module of the app: it scopes the app's own notices.
struct AppNotices;

impl Module for AppNotices {
    fn register(&self, m: &mut ModuleBuilder) {
        m.workspace_scoped::<app_notice::Model>();
    }
}

#[tokio::test]
async fn a_subject_of_the_app_under_a_plugins_namespace_is_for_the_app_to_rename() {
    // The plugin scopes its own `notices.Notice`, and the app another model under that name,
    // under the plugin's namespace: the app's module renames its subject.
    let plugin = NoticesModule(|m| {
        m.workspace_scoped::<navree_plugin_notices::notice::Model>();
    });
    let app = support::builder().module(plugin).module(AppNotices);
    let error = boot_error(app).await;
    let expected = "plugin `navree-plugin-notices` scopes the subject `notices.Notice` to workspaces in `NoticesModule`, and `AppNotices` scopes another model under that name; a plugin scopes only a subject no other module scopes: the names under `notices` are the plugin's, so name the subject of the model `AppNotices` scopes outside `notices.`, or remove the plugin";
    assert!(
        error.contains(expected),
        "expected `{expected}` in:\n{error}"
    );
}

#[tokio::test]
async fn a_plugin_scopes_its_own_subject() {
    let plugin = NoticesModule(|m| {
        m.workspace_scoped::<navree_plugin_notices::notice::Model>();
    });
    let app = support::builder().module(plugin).build().await;
    // The plugin is in the app and is not one of the framework's own: its module was checked.
    let plugins = app.ctx().plugins().plugins;
    assert_eq!(plugins.len(), 1);
    assert!(!plugins[0].first_party);
    app.shutdown().await.unwrap();
}

/// A second module of the app: it scopes the app's deals, which `FixturesModule` scopes too, and
/// the plugin's notices, which the plugin scopes too.
struct AppScopes;

impl Module for AppScopes {
    fn register(&self, m: &mut ModuleBuilder) {
        m.workspace_scoped::<support::Deal>()
            .workspace_scoped::<navree_plugin_notices::notice::Model>();
    }
}

#[tokio::test]
async fn the_app_may_scope_any_subject_to_workspaces() {
    // Two modules of the app scope one subject, as in 1.2 (the first decides), and one model is
    // scoped by the plugin and by the app: it decides the same way whichever is first.
    let plugin = NoticesModule(|m| {
        m.workspace_scoped::<navree_plugin_notices::notice::Model>();
    });
    let app = support::builder()
        .module(plugin)
        .module(AppScopes)
        .build()
        .await;
    assert_eq!(app.ctx().plugins().plugins.len(), 1);
    app.shutdown().await.unwrap();
}
