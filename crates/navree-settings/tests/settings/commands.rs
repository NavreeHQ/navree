//! The `settings:*` commands.

use navree_settings::{Scope, Source};

use crate::support::{PortalSettings, ViewPreferences, World};

#[tokio::test]
async fn set_show_and_inherit_at_every_level() {
    let world = World::new().await;
    let settings = world.settings().await;
    let ada = world.user("Ada").await;
    let holding = world.organization(ada, "Holding").await;
    let acme = world.workspace(ada, "Acme").await;
    world.attach(holding.id, acme.id).await;
    let app = &world.app;

    app.run_command(
        "settings:set",
        &["portal", "page_size=50", "welcome_text=Hello there"],
    )
    .await
    .unwrap();
    app.run_command(
        "settings:set",
        &[
            "portal",
            "show_leaderboard=true",
            "--organization",
            "holding",
        ],
    )
    .await
    .unwrap();
    app.run_command(
        "settings:set",
        &["portal", "welcome_text=Hi Acme", "--workspace=acme"],
    )
    .await
    .unwrap();
    let ada_arg = ada.to_string();
    app.run_command(
        "settings:set",
        &[
            "portal",
            "page_size=10",
            "--user",
            &ada_arg,
            "--workspace",
            "acme",
        ],
    )
    .await
    .unwrap();

    let global = settings
        .resolve::<PortalSettings>(Scope::global())
        .await
        .unwrap();
    assert_eq!(global.welcome_text, "Hello there");
    assert_eq!(global.page_size, 50);
    let mine = settings
        .resolve::<PortalSettings>(Scope::user(ada).in_workspace(acme.id))
        .await
        .unwrap();
    assert_eq!(mine.source("show_leaderboard"), Some(Source::Organization));
    assert_eq!(mine.source("welcome_text"), Some(Source::Workspace));
    assert_eq!(mine.page_size, 10);
    // The user's values went to the user's level, not the workspace's.
    assert!(
        settings
            .stored::<PortalSettings>(Scope::workspace(acme.id))
            .await
            .unwrap()
            .get("page_size")
            .is_none()
    );

    // show reads every level; by id or by slug.
    for args in [
        vec!["portal"],
        vec!["portal", "--workspace", "acme"],
        vec!["portal", "--organization", "holding"],
        vec!["portal", "--user", ada_arg.as_str(), "--workspace", "acme"],
    ] {
        app.run_command("settings:show", &args).await.unwrap();
    }
    let acme_id = acme.id.to_string();
    app.run_command("settings:show", &["portal", "--workspace", &acme_id])
        .await
        .unwrap();
    let holding_id = holding.id.to_string();
    app.run_command("settings:show", &["portal", "--organization", &holding_id])
        .await
        .unwrap();

    app.run_command(
        "settings:inherit",
        &["portal", "welcome_text", "--workspace", "acme"],
    )
    .await
    .unwrap();
    let workspace = settings
        .resolve::<PortalSettings>(Scope::workspace(acme.id))
        .await
        .unwrap();
    assert_eq!(workspace.source("welcome_text"), Some(Source::Global));
    app.run_command("settings:inherit", &["portal", "--all"])
        .await
        .unwrap();
    let global = settings
        .resolve::<PortalSettings>(Scope::global())
        .await
        .unwrap();
    assert!(global.overridden.is_empty());
    assert_eq!(global.page_size, 25);

    app.run_command("settings:list", &[]).await.unwrap();

    // User-only settings by user id.
    app.run_command(
        "settings:set",
        &["view-preferences", "theme=dark", "--user", &ada_arg],
    )
    .await
    .unwrap();
    assert_eq!(
        settings
            .get::<ViewPreferences>(Scope::user(ada))
            .await
            .unwrap()
            .theme,
        "dark"
    );
}

#[tokio::test]
async fn mistakes_are_errors_naming_the_problem() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    let holding = world.organization(ada, "Holding").await;
    world.workspace(ada, "Acme").await;
    let other = world.workspace(ada, "Globex").await;
    world.attach(holding.id, other.id).await;
    let app = &world.app;
    let error = |result: Result<(), navree_kernel::BoxError>| result.unwrap_err().to_string();

    assert!(error(app.run_command("settings:show", &[]).await).contains("name the settings"));
    assert_eq!(
        error(app.run_command("settings:show", &["nope"]).await),
        "unknown settings `nope`"
    );
    assert!(
        error(
            app.run_command("settings:show", &["portal", "--workspace", "missing"])
                .await
        )
        .contains("missing")
    );
    assert!(
        error(
            app.run_command("settings:show", &["portal", "--user", "ada"])
                .await
        )
        .contains("is not a user id")
    );
    // The workspace is not in that organization.
    assert!(
        error(
            app.run_command(
                "settings:show",
                &["portal", "--workspace", "acme", "--organization", "holding"],
            )
            .await
        )
        .contains("is not in the organization")
    );
    assert!(
        error(app.run_command("settings:set", &["portal"]).await)
            .contains("give at least one <field>=<value>")
    );
    let invalid = error(
        app.run_command("settings:set", &["portal", "page_size=0"])
            .await,
    );
    assert!(invalid.contains("page_size"), "{invalid}");
    assert!(
        error(
            app.run_command(
                "settings:set",
                &[
                    "notifications",
                    "daily_digest=true",
                    "--organization",
                    "holding"
                ]
            )
            .await
        )
        .contains("cannot be stored at the organization level")
    );
    assert!(
        error(app.run_command("settings:inherit", &["portal"]).await)
            .contains("name the fields, or pass --all")
    );
    assert!(
        error(
            app.run_command("settings:inherit", &["portal", "page_size", "--all"])
                .await
        )
        .contains("not both")
    );
    assert!(
        error(app.run_command("settings:list", &["portal"]).await).contains("unexpected argument")
    );
}
