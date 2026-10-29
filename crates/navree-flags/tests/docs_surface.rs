//! Every public item the feature flags docs page names
//! (`apps/website/content/docs/packages/feature-flags.mdx`): types,
//! methods, configuration keys, tables, commands, routes, the permission,
//! the event and the OpenAPI extension. A rename fails this file.

use navree_access::{AccessModule, AccessOptions};
use navree_db::DatabaseModule;
use navree_events::Event;
use navree_flags::entity::{feature_flag, feature_flag_override};
use navree_flags::{
    CurrentFlags, FLAG_EXTENSION, FLAGS_MANAGE_PERMISSION, FLAGS_MIGRATIONS, Flag, FlagChanged,
    FlagScope, FlagState, FlagStoreKind, Flags, FlagsError, FlagsFake, FlagsModule, FlagsOptions,
    MissingScope, PlanResolver, RequireFlag, Rollout, UserScope, requires_flag,
};
use navree_testing::TestApp;
use navree_orm::Model as _;

/// A flag for the signatures below.
struct NewPipeline;

impl Flag for NewPipeline {
    const NAME: &'static str = "new-pipeline";
}

/// The methods taking `impl Into<FlagScope>`, called with every documented
/// argument kind (compiled, never run).
async fn flags_methods(
    flags: &Flags,
    scope: FlagScope,
    workspace: &navree_workspace::Workspace,
    user: uuid::Uuid,
) -> Result<(), FlagsError> {
    flags.active::<NewPipeline>(&scope).await?;
    flags.active::<NewPipeline>(workspace).await?;
    flags.all(&scope).await?;
    flags
        .activate::<NewPipeline>(FlagScope::workspace(workspace.id))
        .await?;
    flags
        .deactivate::<NewPipeline>(FlagScope::user(user))
        .await?;
    flags.forget::<NewPipeline>(FlagScope::user(user)).await?;
    flags.activate::<NewPipeline>(FlagScope::everyone()).await?;
    Ok(())
}

/// `flags.active::<F>(&user)` with navree-auth's user (the `auth` feature).
#[cfg(feature = "auth")]
async fn user_scopes(
    flags: &Flags,
    user: &navree_auth::User,
    current: &navree_auth::CurrentUser,
) -> Result<bool, FlagsError> {
    Ok(flags.active::<NewPipeline>(user).await? && flags.active::<NewPipeline>(current).await?)
}

#[test]
fn the_documented_api_exists() {
    // FlagsModule: for_root, from_config, for_feature.
    let _ = FlagsModule::for_root(FlagsOptions::default());
    let _ = FlagsModule::from_config();
    let _ = FlagsModule::for_feature::<NewPipeline>();

    // The `flags` section and its keys.
    let FlagsOptions {
        store,
        cache_ttl_secs,
        routes,
        admin_routes,
        missing_scope,
    } = FlagsOptions::default();
    assert_eq!(
        (store, cache_ttl_secs, routes, admin_routes, missing_scope),
        (
            FlagStoreKind::Database,
            60,
            true,
            true,
            MissingScope::Default
        )
    );
    let _ = (FlagStoreKind::Memory, MissingScope::Off);
    assert_eq!(<FlagsOptions as navree_config::ConfigSection>::KEY, "flags");

    // `#[flag]` arguments are the trait's constants.
    let _ = (
        NewPipeline::NAME,
        NewPipeline::DEFAULT,
        NewPipeline::EXPOSE,
        NewPipeline::DESCRIPTION,
    );

    // The Flags methods of the reference table (see also `flags_methods`).
    let _ = flags_methods;
    #[cfg(feature = "auth")]
    let _ = user_scopes;
    let _ = Flags::rollout::<NewPipeline>;
    let _ = Flags::stop_rollout::<NewPipeline>;
    let _ = Flags::kill::<NewPipeline>;
    let _ = Flags::revive::<NewPipeline>;
    let _ = Flags::state::<NewPipeline>;
    let _: fn(&FlagState) -> bool = |state| state.killed;

    // CurrentFlags.
    let _ = CurrentFlags::active::<NewPipeline>;
    let _ = CurrentFlags::all;

    // Scopes: user, workspace, both, everyone.
    let id = uuid::Uuid::now_v7();
    let user: UserScope = FlagScope::user(id);
    let _: FlagScope = user.workspace(id);
    let _: FlagScope = FlagScope::workspace(id);
    let _: FlagScope = FlagScope::everyone();

    // Rollouts.
    let _ = Rollout::percent(10).of_workspaces().of_users();
    let _ = Rollout::plans(["pro", "business"]);
    let _ = Rollout::percent(50)
        .of_workspaces()
        .on_plans(["pro", "business"]);
    let _ = Rollout::percent(25)
        .of_workspaces()
        .includes::<NewPipeline>(id);
    let _: Option<&dyn PlanResolver> = None;

    // Errors, the event, the guard and the fake.
    let _ = |error: FlagsError| matches!(error, FlagsError::Unregistered { .. });
    assert_eq!(FlagChanged::NAME, "flag.changed");
    let _ = RequireFlag::guard::<NewPipeline>;
    let _ = requires_flag::<NewPipeline>;
    assert_eq!(FLAG_EXTENSION, "x-navree-flag");
    let fake = FlagsFake::new().on::<NewPipeline>().off::<NewPipeline>();
    let _: Flags = fake.flags();
    let _ = FlagsFake::assert_checked::<NewPipeline>;
    let _ = FlagsFake::assert_not_checked::<NewPipeline>;

    // Tables, migration set and permission.
    assert_eq!(feature_flag::Model::table_name(), "feature_flags");
    assert_eq!(
        feature_flag_override::Model::table_name(),
        "feature_flag_overrides"
    );
    assert_eq!(FLAGS_MIGRATIONS, "flags");
    assert_eq!(FLAGS_MANAGE_PERMISSION, "flags.manage");
}

#[tokio::test]
async fn the_documented_commands_and_routes_exist() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(FlagsModule::from_config())
        .build()
        .await;
    let specs = app.ctx().commands();
    let commands: Vec<&str> = specs.iter().map(|c| c.name).collect();
    for command in [
        "flags:list",
        "flags:on",
        "flags:off",
        "flags:forget",
        "flags:rollout",
        "flags:kill",
        "flags:revive",
        "flags:purge",
    ] {
        assert!(commands.contains(&command), "{command} in {commands:?}");
    }

    let routes = navree_http::route_table(app.ctx()).await.unwrap();
    let mut listed: Vec<String> = routes
        .iter()
        .filter(|route| route.full_path.contains("/flags"))
        .map(|route| format!("{} {}", route.method, route.full_path))
        .collect();
    listed.sort();
    assert_eq!(
        listed,
        [
            "DELETE /api/admin/flags/{name}/overrides",
            "GET /api/admin/flags",
            "GET /api/admin/flags/{name}",
            "GET /api/flags",
            "PUT /api/admin/flags/{name}",
            "PUT /api/admin/flags/{name}/overrides",
        ]
    );
    for route in routes.iter().filter(|r| r.full_path.contains("/admin/")) {
        assert_eq!(route.access.permissions, [FLAGS_MANAGE_PERMISSION]);
    }
}
