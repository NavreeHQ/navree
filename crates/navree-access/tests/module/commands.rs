//! `access:role`, `access:grant` and `access:explain`.

use navree_access::AccessService;
use uuid::Uuid;

use crate::support::{app, give_role, insert_post};

#[tokio::test]
async fn role_and_grant_commands_create_roles_and_grant_permissions() {
    let (app, _audit) = app().await;
    app.run_command("access:role", &["support"]).await.unwrap();
    // Running it again is fine (the role already exists).
    app.run_command("access:role", &["support"]).await.unwrap();
    app.run_command("access:grant", &["support", "posts.read"])
        .await
        .unwrap();

    let service = app.get::<AccessService>().await;
    let role = service
        .role("support")
        .await
        .unwrap()
        .expect("role created");
    assert_eq!(role.permissions, ["posts.read"]);

    assert!(app.run_command("access:role", &[]).await.is_err());
    assert!(app.run_command("access:role", &["bad name"]).await.is_err());
    assert!(app.run_command("access:grant", &["support"]).await.is_err());
    let error = app
        .run_command("access:grant", &["ghost", "posts.read"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("ghost"), "{error}");
}

#[tokio::test]
async fn explain_command_prints_matching_rule() {
    let (app, _audit) = app().await;
    let (author, stranger) = (Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, author, "writer", &[]).await;
    let locked = insert_post(&app, "Locked", Some(author), "locked", false).await;
    let open = insert_post(&app, "Open", Some(author), "open", false).await;
    let service = app.get::<AccessService>().await;

    let denied = service
        .explain(author, "delete", "Post", Some(&locked.id.to_string()))
        .await
        .unwrap();
    assert!(!denied.allowed);
    let text = denied.to_string();
    assert!(
        text.contains(&format!(
            "User {author} may not `delete` Post {}",
            locked.id
        )),
        "{text}"
    );
    assert!(
        text.contains(
            "rule #6: cannot delete Post where {\"status\":{\"$eq\":\"locked\"}} (locked posts cannot be deleted)"
        ) || text.contains(
            "rule #6: cannot delete Post where {\"status\":\"locked\"} (locked posts cannot be deleted)"
        ),
        "{text}"
    );
    assert!(text.contains("Roles: writer"), "{text}");

    let allowed = service
        .explain(author, "delete", "Post", Some(&open.id.to_string()))
        .await
        .unwrap();
    assert!(allowed.allowed);
    assert_eq!(allowed.rule.as_ref().map(|(n, _)| *n), Some(4));
    assert!(
        allowed
            .to_string()
            .contains("rule #4: can delete Post where")
    );

    let nothing = service
        .explain(stranger, "archive", "Post", None)
        .await
        .unwrap();
    assert!(!nothing.allowed && nothing.rule.is_none());
    assert!(
        nothing
            .to_string()
            .contains("No rule matches: denied by default."),
        "{nothing}"
    );

    // The command prints the same explanation.
    let (user, id) = (author.to_string(), locked.id.to_string());
    app.run_command("access:explain", &[&user, "delete", "Post", &id])
        .await
        .unwrap();
    app.run_command("access:explain", &[&user, "update", "Post"])
        .await
        .unwrap();
    assert!(
        app.run_command("access:explain", &["not-a-uuid", "read", "Post"])
            .await
            .is_err()
    );
    assert!(
        app.run_command(
            "access:explain",
            &[&user, "read", "Post", &Uuid::now_v7().to_string()]
        )
        .await
        .is_err()
    );
}
