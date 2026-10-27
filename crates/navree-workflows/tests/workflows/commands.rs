//! The `workflows:*` and `automations:*` commands.

use navree_workflows::RunStatus;

use crate::support::{DAY, Greet, Onboarding, harness};

#[tokio::test]
async fn commands_inspect_and_change_runs() {
    let h = harness(|b| b).await;
    let sleeping = h.start(Onboarding { user_id: 1 }).await;
    let done = h.start(Greet { name: "a".into() }).await;
    let runs: [(&str, &[&str]); 6] = [
        ("workflows:list", &[]),
        (
            "workflows:list",
            &["--workflow", "greet", "--status", "completed", "--limit", "5"],
        ),
        ("workflows:tick", &[]),
        ("workflows:prune", &["--days", "1"]),
        ("automations:list", &[]),
        ("automations:catalog", &[]),
    ];
    for (name, args) in runs {
        h.app.run_command(name, args).await.unwrap();
    }
    let id = sleeping.to_string();
    h.app.run_command("workflows:show", &[&id]).await.unwrap();
    h.app.run_command("workflows:cancel", &[&id]).await.unwrap();
    assert_eq!(h.status(sleeping).await, RunStatus::Cancelled);
    // Cancelling again, a bad id and a missing id are errors.
    assert!(h.app.run_command("workflows:cancel", &[&id]).await.is_err());
    assert!(h.app.run_command("workflows:show", &["nope"]).await.is_err());
    assert!(h.app.run_command("workflows:retry", &[]).await.is_err());
    assert!(
        h.app
            .run_command("workflows:list", &["--status", "done"])
            .await
            .is_err()
    );
    // Prune with the configured retention (30 days).
    h.clock.advance(31 * DAY);
    h.app.run_command("workflows:prune", &[]).await.unwrap();
    assert!(h.workflows().await.run(done).await.is_err());
}
