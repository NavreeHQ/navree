//! The `flags:*` commands.

use navree_flags::{FlagScope, FlagState, FlagStore, Rollout, ScopeType};
use uuid::Uuid;

use crate::support::{AiSummaries, NewPipeline, World};

#[tokio::test]
async fn on_off_and_forget_target_users_workspaces_or_everyone() {
    let world = World::new().await;
    let flags = world.flags().await;
    let owner = world.user("Ada").await;
    let acme = world.workspace(owner, "Acme").await;
    let grace = Uuid::now_v7();
    let grace_arg = grace.to_string();
    let app = &world.app;

    app.run_command("flags:on", &["new-pipeline", "--workspace", "acme"])
        .await
        .unwrap();
    assert!(flags.active::<NewPipeline>(&acme).await.unwrap());
    let acme_id = acme.id.to_string();
    app.run_command("flags:off", &["new-pipeline", &format!("--workspace={acme_id}")])
        .await
        .unwrap();
    assert!(!flags.active::<NewPipeline>(&acme).await.unwrap());

    app.run_command("flags:on", &["new-pipeline", "--user", &grace_arg])
        .await
        .unwrap();
    assert!(flags.active::<NewPipeline>(FlagScope::user(grace)).await.unwrap());
    app.run_command("flags:forget", &["new-pipeline", "--user", &grace_arg])
        .await
        .unwrap();
    assert!(!flags.active::<NewPipeline>(FlagScope::user(grace)).await.unwrap());

    app.run_command("flags:on", &["new-pipeline"]).await.unwrap();
    assert_eq!(flags.state::<NewPipeline>().await.unwrap().everyone, Some(true));
    app.run_command("flags:forget", &["new-pipeline"]).await.unwrap();
    assert_eq!(flags.state::<NewPipeline>().await.unwrap().everyone, None);

    // Mistakes are errors, not silent no-ops.
    assert!(app.run_command("flags:on", &["nope"]).await.is_err());
    assert!(app.run_command("flags:on", &[]).await.is_err());
    assert!(
        app.run_command("flags:on", &["new-pipeline", "--workspace", "missing"])
            .await
            .is_err()
    );
    assert!(
        app.run_command("flags:on", &["new-pipeline", "--user", "not-a-uuid"])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn rollouts_and_the_kill_switch() {
    let world = World::new().await;
    let flags = world.flags().await;
    let app = &world.app;

    app.run_command("flags:rollout", &["new-pipeline", "25", "--of", "workspaces"])
        .await
        .unwrap();
    assert_eq!(
        flags.state::<NewPipeline>().await.unwrap().rollout,
        Some(Rollout::percent(25).of_workspaces())
    );
    app.run_command(
        "flags:rollout",
        &["ai-summaries", "100", "--of", "workspaces", "--plans", "pro,business"],
    )
    .await
    .unwrap();
    assert_eq!(
        flags.state::<AiSummaries>().await.unwrap().rollout,
        Some(Rollout::plans(["pro", "business"]))
    );
    app.run_command("flags:rollout", &["new-pipeline", "off"])
        .await
        .unwrap();
    assert_eq!(flags.state::<NewPipeline>().await.unwrap().rollout, None);
    assert!(
        app.run_command("flags:rollout", &["new-pipeline", "120"])
            .await
            .is_err()
    );

    app.run_command("flags:kill", &["new-pipeline"]).await.unwrap();
    assert!(flags.state::<NewPipeline>().await.unwrap().killed);
    app.run_command("flags:revive", &["new-pipeline"])
        .await
        .unwrap();
    assert!(!flags.state::<NewPipeline>().await.unwrap().killed);
    assert!(app.run_command("flags:kill", &[]).await.is_err());

    app.run_command("flags:list", &[]).await.unwrap();
    assert!(app.run_command("flags:list", &["--all"]).await.is_err());
}

#[tokio::test]
async fn purge_deletes_flags_no_module_registers() {
    let world = World::new().await;
    let store = world.app.get::<dyn FlagStore>().await;
    let flags = world.flags().await;
    let ada = Uuid::now_v7();
    flags
        .activate::<NewPipeline>(FlagScope::user(ada))
        .await
        .unwrap();
    // State left behind by a flag deleted from the code.
    store
        .save_state(
            None,
            &FlagState {
                everyone: Some(true),
                ..FlagState::unset("old-board")
            },
        )
        .await
        .unwrap();
    store
        .set_override(
            None,
            &navree_flags::FlagOverride {
                flag: "old-board".to_owned(),
                scope_type: ScopeType::User,
                scope_id: ada,
                active: true,
            },
        )
        .await
        .unwrap();

    world
        .app
        .run_command("flags:purge", &["--dry-run"])
        .await
        .unwrap();
    assert!(store.state(None, "old-board").await.unwrap().is_some());

    let mut events = world.events(&["flag.changed"]).await;
    world.app.run_command("flags:purge", &[]).await.unwrap();
    assert_eq!(store.state(None, "old-board").await.unwrap(), None);
    assert!(store.overrides(None, "old-board").await.unwrap().is_empty());
    assert!(
        flags.active::<NewPipeline>(FlagScope::user(ada)).await.unwrap(),
        "registered flags keep their state"
    );
    let events = events.drain();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1["flag"], "old-board");
    assert_eq!(events[0].1["change"], "purged");

    assert!(world.app.run_command("flags:purge", &["--now"]).await.is_err());
}
