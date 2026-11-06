//! The `Invitations` service: invite, mail, accept, revoke, expire.

use std::time::Duration;

use navree_crypto::sha256_hex;
use navree_orm::Model as _;
use navree_workspace::{Invitation, WorkspaceError, WorkspaceRole};

use crate::support::World;

#[tokio::test]
async fn an_invitation_is_mailed_with_a_token_stored_hashed() {
    let world = World::new().await;
    let mut events = world.events(&["workspace.invitation_sent"]).await;
    let ada = world.user("Anu Lakhotia", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;

    let invitations = world.invitations().await;
    let sent = world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    agra.id,
                    " Gauri@Example.com ",
                    WorkspaceRole::Admin,
                    Some(ada),
                )
                .await
        })
        .await
        .unwrap();

    assert_eq!(sent.invitation.email, "gauri@example.com");
    assert_eq!(sent.invitation.role, "admin");
    assert_eq!(sent.token.len(), 48);
    assert_eq!(
        sent.invitation.expires_at,
        crate::support::start() + chrono::TimeDelta::days(7)
    );
    let stored = Invitation::find(&*world.db().await, sent.invitation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.token_hash, sha256_hex(sent.token.as_bytes()));
    assert_ne!(stored.token_hash, sent.token);
    let serialized = serde_json::to_value(&stored).unwrap();
    assert!(serialized.get("token_hash").is_none());

    let mails = world.mails().await;
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].to, ["gauri@example.com"]);
    assert_eq!(mails[0].subject, "You're invited to join Agra");
    let text = mails[0].text.clone().unwrap();
    assert!(
        text.contains("Anu Lakhotia invited you to join Agra as admin."),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "https://app.test/invitations/accept?token={}",
            sent.token
        )),
        "{text}"
    );
    let payloads = events.drain();
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0].1["email"], "gauri@example.com");
    assert!(payloads[0].1.get("token").is_none());
}

#[tokio::test]
async fn accepting_adds_the_member_with_the_invited_role() {
    let world = World::new().await;
    let mut events = world
        .events(&["workspace.invitation_accepted", "workspace.member_added"])
        .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    events.drain();
    let invitations = world.invitations().await;
    let sent = world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    agra.id,
                    "gauri@example.com",
                    WorkspaceRole::Viewer,
                    Some(ada),
                )
                .await
        })
        .await
        .unwrap();

    let invitations = world.invitations().await;
    let token = sent.token.clone();
    let accepted = world
        .work(|uow| async move { invitations.accept(&uow, &token, grace).await })
        .await
        .unwrap();
    assert_eq!(accepted.workspace.id, agra.id);
    assert_eq!(accepted.member.role, "viewer");
    assert_eq!(accepted.member.user_id, grace);
    let stored = Invitation::find(&*world.db().await, sent.invitation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.accepted_by, Some(grace));
    assert!(stored.accepted_at.is_some());
    assert_eq!(
        events.names(),
        ["workspace.invitation_accepted", "workspace.member_added"]
    );

    // A used token is gone.
    let invitations = world.invitations().await;
    let token = sent.token.clone();
    let reused = world
        .work(|uow| async move { invitations.accept(&uow, &token, grace).await })
        .await
        .unwrap_err();
    assert!(matches!(reused, WorkspaceError::InvitationNotFound));
}

#[tokio::test]
async fn only_the_invited_address_may_accept() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let eve = world.user("Ela", "ela@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let invitations = world.invitations().await;
    let sent = world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    agra.id,
                    "gauri@example.com",
                    WorkspaceRole::Member,
                    Some(ada),
                )
                .await
        })
        .await
        .unwrap();
    let invitations = world.invitations().await;
    let token = sent.token.clone();
    let refused = world
        .work(|uow| async move { invitations.accept(&uow, &token, eve).await })
        .await
        .unwrap_err();
    assert!(matches!(refused, WorkspaceError::InvitationEmailMismatch));
}

#[tokio::test]
async fn the_address_check_can_be_turned_off() {
    let world = World::with(
        crate::support::builder().config("workspace.invitations.require_matching_email", false),
    )
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let eve = world.user("Ela", "ela@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let invitations = world.invitations().await;
    let sent = world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    agra.id,
                    "gauri@example.com",
                    WorkspaceRole::Member,
                    Some(ada),
                )
                .await
        })
        .await
        .unwrap();
    let invitations = world.invitations().await;
    let token = sent.token.clone();
    let accepted = world
        .work(|uow| async move { invitations.accept(&uow, &token, eve).await })
        .await
        .unwrap();
    assert_eq!(accepted.member.user_id, eve);
}

#[tokio::test]
async fn invitations_expire() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let invitations = world.invitations().await;
    let sent = world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    agra.id,
                    "gauri@example.com",
                    WorkspaceRole::Member,
                    Some(ada),
                )
                .await
        })
        .await
        .unwrap();
    assert_eq!(
        world
            .invitations()
            .await
            .pending(agra.id)
            .await
            .unwrap()
            .len(),
        1
    );

    world.clock.advance(Duration::from_secs(7 * 24 * 3600));
    let invitations = world.invitations().await;
    let token = sent.token.clone();
    let expired = world
        .work(|uow| async move { invitations.accept(&uow, &token, grace).await })
        .await
        .unwrap_err();
    assert!(matches!(expired, WorkspaceError::InvitationExpired));
    assert!(
        world
            .invitations()
            .await
            .pending(agra.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        world
            .invitations()
            .await
            .find_by_token(&sent.token)
            .await
            .unwrap()
            .is_none()
    );

    world
        .app
        .run_command("workspace:prune-invitations", &[])
        .await
        .unwrap();
    let left = Invitation::query().count(&*world.db().await).await.unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn a_revoked_invitation_cannot_be_accepted() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let invitations = world.invitations().await;
    let sent = world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    agra.id,
                    "gauri@example.com",
                    WorkspaceRole::Member,
                    Some(ada),
                )
                .await
        })
        .await
        .unwrap();
    assert!(
        world
            .invitations()
            .await
            .find_by_token(&sent.token)
            .await
            .unwrap()
            .is_some()
    );
    let invitations = world.invitations().await;
    let id = sent.invitation.id;
    world
        .work(|uow| async move { invitations.revoke(&uow, agra.id, id).await })
        .await
        .unwrap();
    let invitations = world.invitations().await;
    let token = sent.token.clone();
    let revoked = world
        .work(|uow| async move { invitations.accept(&uow, &token, grace).await })
        .await
        .unwrap_err();
    assert!(matches!(revoked, WorkspaceError::InvitationNotFound));
    let invitations = world.invitations().await;
    let again = world
        .work(|uow| async move { invitations.revoke(&uow, agra.id, id).await })
        .await
        .unwrap_err();
    assert!(matches!(again, WorkspaceError::InvitationNotFound));
}

#[tokio::test]
async fn inviting_again_replaces_the_pending_invitation() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let invite = |role: WorkspaceRole| {
        let world = &world;
        async move {
            let invitations = world.invitations().await;
            world
                .work(|uow| async move {
                    invitations
                        .invite(&uow, agra.id, "gauri@example.com", role, Some(ada))
                        .await
                })
                .await
        }
    };
    let first = invite(WorkspaceRole::Member).await.unwrap();
    let second = invite(WorkspaceRole::Admin).await.unwrap();
    let pending = world.invitations().await.pending(agra.id).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, second.invitation.id);
    assert!(
        world
            .invitations()
            .await
            .find_by_token(&first.token)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn members_and_owners_are_not_invited() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let invitations = world.invitations().await;
    let member = world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    agra.id,
                    "ANU@example.com",
                    WorkspaceRole::Member,
                    Some(ada),
                )
                .await
        })
        .await
        .unwrap_err();
    assert!(matches!(member, WorkspaceError::AlreadyMember));
    let invitations = world.invitations().await;
    let owner = world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    agra.id,
                    "gauri@example.com",
                    WorkspaceRole::Owner,
                    Some(ada),
                )
                .await
        })
        .await
        .unwrap_err();
    assert!(matches!(
        owner,
        WorkspaceError::Invalid { field: "role", .. }
    ));
    let invitations = world.invitations().await;
    let malformed = world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    agra.id,
                    "not-an-email",
                    WorkspaceRole::Member,
                    Some(ada),
                )
                .await
        })
        .await
        .unwrap_err();
    assert!(matches!(
        malformed,
        WorkspaceError::Invalid { field: "email", .. }
    ));
}

/// Templates for the invitation mail.
struct InvitationTemplates;

impl navree_kernel::Module for InvitationTemplates {
    fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
        use navree_view::ModuleBuilderViewExt;
        m.templates(&[(
            "mail/invite.txt",
            "{{ inviter }} invited {{ email }} to {{ workspace }} as {{ role }}: {{ url }} ({{ expires_in_days }} days, {{ token|length }} chars)",
        )]);
    }
}

#[tokio::test]
async fn the_mail_can_come_from_a_template() {
    let world = World::with(
        crate::support::builder()
            .config("workspace.invitations.mail_template", "mail/invite")
            .config("workspace.invitations.ttl_secs", 2 * 86_400)
            .module(InvitationTemplates),
    )
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let invitations = world.invitations().await;
    let sent = world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    agra.id,
                    "gauri@example.com",
                    WorkspaceRole::Viewer,
                    Some(ada),
                )
                .await
        })
        .await
        .unwrap();
    let mails = world.mails().await;
    assert_eq!(mails[0].subject, "You're invited to join Agra");
    assert_eq!(
        mails[0].text.as_deref(),
        Some(
            format!(
                "Anu invited gauri@example.com to Agra as viewer: https://app.test/invitations/accept?token={} (2 days, 48 chars)",
                sent.token
            )
            .as_str()
        )
    );
}
