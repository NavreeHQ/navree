//! Resolving flags through the `Flags` service: scopes, overrides, the
//! global state, rollouts, plans, the kill switch and the default, on the
//! database store (the default) and the memory store.

use navree_flags::{Flag, FlagScope, Flags, FlagsError, Rollout, bucket};
use uuid::Uuid;

use crate::support::{AiSummaries, DealExport, Forgotten, NewPipeline, World, builder};

/// An id whose bucket for `F` is (or is not) below `percent`.
fn id_in<F: Flag>(inside: bool, percent: u8) -> Uuid {
    loop {
        let id = Uuid::now_v7();
        if (bucket(F::NAME, id) < percent) == inside {
            return id;
        }
    }
}

async fn defaults_and_overrides(flags: &Flags) {
    let (ada, grace, acme, globex) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );

    // The defaults of `#[flag]`.
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::user(ada))
            .await
            .unwrap()
    );
    assert!(
        flags
            .active::<DealExport>(FlagScope::user(ada))
            .await
            .unwrap()
    );

    // On for one workspace, and for one user wherever they work.
    flags
        .activate::<NewPipeline>(FlagScope::workspace(acme))
        .await
        .unwrap();
    flags
        .activate::<NewPipeline>(FlagScope::user(ada))
        .await
        .unwrap();
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::user(grace).workspace(acme))
            .await
            .unwrap(),
        "grace gets what acme gets"
    );
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::user(ada).workspace(globex))
            .await
            .unwrap(),
        "ada has it in every workspace"
    );
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::user(grace).workspace(globex))
            .await
            .unwrap()
    );

    // Off for one user beats the workspace; forgetting returns to it.
    flags
        .deactivate::<NewPipeline>(FlagScope::user(grace))
        .await
        .unwrap();
    let grace_at_acme = FlagScope::user(grace).workspace(acme);
    assert!(!flags.active::<NewPipeline>(&grace_at_acme).await.unwrap());
    flags
        .forget::<NewPipeline>(FlagScope::user(grace))
        .await
        .unwrap();
    assert!(flags.active::<NewPipeline>(&grace_at_acme).await.unwrap());

    // Released to everyone, then off for everyone but the overrides.
    flags
        .activate::<NewPipeline>(FlagScope::everyone())
        .await
        .unwrap();
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::user(grace).workspace(globex))
            .await
            .unwrap()
    );
    flags
        .deactivate::<NewPipeline>(FlagScope::everyone())
        .await
        .unwrap();
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::user(grace).workspace(globex))
            .await
            .unwrap()
    );
    assert!(flags.active::<NewPipeline>(&grace_at_acme).await.unwrap());

    // A default-on flag turned off for everyone.
    flags
        .deactivate::<DealExport>(FlagScope::everyone())
        .await
        .unwrap();
    assert!(
        !flags
            .active::<DealExport>(FlagScope::user(ada))
            .await
            .unwrap()
    );
    flags
        .forget::<DealExport>(FlagScope::everyone())
        .await
        .unwrap();
    assert!(
        flags
            .active::<DealExport>(FlagScope::user(ada))
            .await
            .unwrap()
    );

    // The kill switch beats every override; revive restores them.
    flags.kill::<NewPipeline>().await.unwrap();
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::user(ada))
            .await
            .unwrap()
    );
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );
    assert!(flags.state::<NewPipeline>().await.unwrap().killed);
    flags.revive::<NewPipeline>().await.unwrap();
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::user(ada))
            .await
            .unwrap()
    );
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::workspace(acme))
            .await
            .unwrap()
    );

    // Every registered flag's value at once.
    let all = flags.all(FlagScope::user(ada)).await.unwrap();
    assert_eq!(
        all.into_iter().collect::<Vec<_>>(),
        [
            ("ai-summaries".to_owned(), false),
            ("deal-export".to_owned(), true),
            ("new-pipeline".to_owned(), true),
        ]
    );
}

#[tokio::test]
async fn overrides_global_state_kill_switch_and_defaults_on_the_database() {
    let world = World::new().await;
    defaults_and_overrides(&*world.flags().await).await;
}

#[tokio::test]
async fn overrides_global_state_kill_switch_and_defaults_in_memory() {
    let world = World::with(builder().config("flags.store", "memory")).await;
    defaults_and_overrides(&*world.flags().await).await;
}

#[tokio::test]
async fn a_scope_with_a_user_and_a_workspace_is_not_a_write_target() {
    let world = World::new().await;
    let flags = world.flags().await;
    let both = FlagScope::user(Uuid::now_v7()).workspace(Uuid::now_v7());
    let error = flags.activate::<NewPipeline>(both).await.unwrap_err();
    assert!(matches!(error, FlagsError::InvalidScope(_)), "{error}");
}

#[tokio::test]
async fn percentage_rollouts_bucket_users_or_workspaces() {
    let world = World::new().await;
    let flags = world.flags().await;
    let (inside, outside) = (
        id_in::<NewPipeline>(true, 25),
        id_in::<NewPipeline>(false, 25),
    );

    // By user (the default unit).
    flags
        .rollout::<NewPipeline>(Rollout::percent(25))
        .await
        .unwrap();
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::user(inside))
            .await
            .unwrap()
    );
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::user(outside))
            .await
            .unwrap()
    );
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(inside))
            .await
            .unwrap(),
        "a user rollout needs a user"
    );

    // By workspace.
    flags
        .rollout::<NewPipeline>(Rollout::percent(25).of_workspaces())
        .await
        .unwrap();
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::user(outside).workspace(inside))
            .await
            .unwrap()
    );
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::user(inside).workspace(outside))
            .await
            .unwrap()
    );
    assert!(
        Rollout::percent(25)
            .of_workspaces()
            .includes::<NewPipeline>(inside)
    );

    // Raising the percentage keeps everyone who had it.
    let at_25: Vec<Uuid> = (0..200)
        .map(|_| Uuid::now_v7())
        .filter(|id| Rollout::percent(25).includes::<NewPipeline>(*id))
        .collect();
    flags
        .rollout::<NewPipeline>(Rollout::percent(50).of_workspaces())
        .await
        .unwrap();
    for id in at_25 {
        assert!(
            flags
                .active::<NewPipeline>(FlagScope::workspace(id))
                .await
                .unwrap()
        );
    }

    // An override beats the rollout; stopping it leaves the default.
    flags
        .deactivate::<NewPipeline>(FlagScope::workspace(inside))
        .await
        .unwrap();
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(inside))
            .await
            .unwrap()
    );
    flags.stop_rollout::<NewPipeline>().await.unwrap();
    let state = flags.state::<NewPipeline>().await.unwrap();
    assert_eq!(state.rollout, None);
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::workspace(inside))
            .await
            .unwrap()
    );

    // 0% and 100%.
    flags
        .rollout::<NewPipeline>(Rollout::percent(100))
        .await
        .unwrap();
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::user(outside))
            .await
            .unwrap()
    );
    flags
        .rollout::<NewPipeline>(Rollout::percent(0))
        .await
        .unwrap();
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::user(inside))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn invalid_rollouts_are_refused() {
    let world = World::new().await;
    let flags = world.flags().await;
    let error = flags
        .rollout::<NewPipeline>(Rollout::percent(101))
        .await
        .unwrap_err();
    assert!(matches!(error, FlagsError::InvalidRollout(_)), "{error}");
    let error = flags
        .rollout::<NewPipeline>(Rollout::plans(["pro", ""]))
        .await
        .unwrap_err();
    assert!(matches!(error, FlagsError::InvalidRollout(_)), "{error}");
    assert_eq!(flags.state::<NewPipeline>().await.unwrap().rollout, None);
}

#[tokio::test]
async fn plan_rollouts_read_the_workspace_plan() {
    let world = World::new().await;
    let flags = world.flags().await;
    let plans = world.plans().await;
    let (pro, business, free, unknown) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    plans.set(pro, "pro");
    plans.set(business, "business");
    plans.set(free, "free");

    flags
        .rollout::<AiSummaries>(Rollout::plans(["pro", "business"]))
        .await
        .unwrap();
    assert!(
        flags
            .active::<AiSummaries>(FlagScope::workspace(pro))
            .await
            .unwrap()
    );
    assert!(
        flags
            .active::<AiSummaries>(FlagScope::workspace(business))
            .await
            .unwrap()
    );
    assert!(
        !flags
            .active::<AiSummaries>(FlagScope::workspace(free))
            .await
            .unwrap()
    );
    assert!(
        !flags
            .active::<AiSummaries>(FlagScope::workspace(unknown))
            .await
            .unwrap()
    );
    assert!(
        !flags
            .active::<AiSummaries>(FlagScope::user(Uuid::now_v7()))
            .await
            .unwrap(),
        "no workspace, no plan"
    );

    // The plan is cached: a second check asks the resolver nothing.
    let lookups = plans.lookups();
    assert!(
        flags
            .active::<AiSummaries>(FlagScope::workspace(pro))
            .await
            .unwrap()
    );
    assert_eq!(plans.lookups(), lookups);

    // Half of the Pro and Business workspaces.
    let inside = id_in::<AiSummaries>(true, 50);
    let outside = id_in::<AiSummaries>(false, 50);
    plans.set(inside, "pro");
    plans.set(outside, "pro");
    flags
        .rollout::<AiSummaries>(
            Rollout::percent(50)
                .of_workspaces()
                .on_plans(["pro", "business"]),
        )
        .await
        .unwrap();
    assert!(
        flags
            .active::<AiSummaries>(FlagScope::workspace(inside))
            .await
            .unwrap()
    );
    assert!(
        !flags
            .active::<AiSummaries>(FlagScope::workspace(outside))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn plan_rollouts_need_a_plan_resolver() {
    use navree_access::{AccessModule, AccessOptions};
    use navree_cache::{CacheModule, CacheOptions};
    use navree_db::DatabaseModule;
    use navree_flags::FlagsModule;
    use navree_testing::TestApp;

    let app = TestApp::builder()
        .temp_database()
        .config("flags.admin_routes", false)
        .module(DatabaseModule::from_config())
        .module(CacheModule::for_root(CacheOptions::default()))
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(FlagsModule::for_feature::<AiSummaries>())
        .module(FlagsModule::from_config())
        .build()
        .await;
    let flags = app.get::<Flags>().await;
    let error = flags
        .rollout::<AiSummaries>(Rollout::plans(["pro"]))
        .await
        .unwrap_err();
    assert!(matches!(error, FlagsError::NoPlanResolver), "{error}");
    flags
        .rollout::<AiSummaries>(Rollout::percent(10))
        .await
        .expect("a rollout without plans needs no resolver");
}

#[tokio::test]
async fn checks_without_a_scope_follow_missing_scope() {
    let world = World::new().await;
    let flags = world.flags().await;
    assert!(
        flags
            .active::<DealExport>(FlagScope::everyone())
            .await
            .unwrap()
    );
    flags
        .rollout::<NewPipeline>(Rollout::percent(100))
        .await
        .unwrap();
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::everyone())
            .await
            .unwrap(),
        "nobody to bucket"
    );

    let strict = World::with(builder().config("flags.missing_scope", "off")).await;
    let flags = strict.flags().await;
    assert!(
        !flags
            .active::<DealExport>(FlagScope::everyone())
            .await
            .unwrap(),
        "off unless on for everyone"
    );
    flags
        .activate::<DealExport>(FlagScope::everyone())
        .await
        .unwrap();
    assert!(
        flags
            .active::<DealExport>(FlagScope::everyone())
            .await
            .unwrap()
    );
    assert!(
        flags
            .active::<DealExport>(FlagScope::user(Uuid::now_v7()))
            .await
            .unwrap(),
        "a check with a user is not a missing scope"
    );
}

#[tokio::test]
async fn unregistered_flags_are_errors() {
    let world = World::new().await;
    let flags = world.flags().await;
    let error = flags
        .active::<Forgotten>(FlagScope::everyone())
        .await
        .unwrap_err();
    match &error {
        FlagsError::Unregistered { name, .. } => assert_eq!(name, "forgotten"),
        other => panic!("expected Unregistered, got {other}"),
    }
    assert!(
        error
            .to_string()
            .contains("FlagsModule::for_feature::<Forgotten>()")
    );
    assert!(
        flags
            .activate::<Forgotten>(FlagScope::everyone())
            .await
            .is_err()
    );
    let names: Vec<&str> = flags.registered().iter().map(|d| d.name).collect();
    assert_eq!(names, ["ai-summaries", "deal-export", "new-pipeline"]);
}

#[tokio::test]
async fn users_principals_and_workspaces_convert_into_scopes() {
    use navree_contracts::{AuthVia, Principal};

    let world = World::new().await;
    let flags = world.flags().await;
    let owner = world.user("Ada").await;
    let acme = world.workspace(owner, "Acme").await;
    flags.activate::<NewPipeline>(&acme).await.unwrap();
    assert!(flags.active::<NewPipeline>(&acme).await.unwrap());

    let principal = Principal {
        user_id: owner,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    };
    flags.activate::<AiSummaries>(&principal).await.unwrap();
    assert!(
        flags
            .active::<AiSummaries>(FlagScope::user(owner))
            .await
            .unwrap()
    );
}
