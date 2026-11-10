//! The organization services: `Organizations`, `OrgMembers` and
//! `AccessRequests`.

use navree_workspace::{
    AccessRequestStatus, Approval, GrantedBy, NewAccessRequest, NewOrganization, OrgRole,
    OrganizationChanges, Selector, WorkspaceError, WorkspaceRole, WorkspaceSelector,
};
use serde_json::json;

use crate::support::{World, org_builder};

async fn world() -> World {
    World::with(org_builder()).await
}

#[tokio::test]
async fn organizations_are_created_updated_listed_and_deleted() {
    let world = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let organizations = world.organizations().await;
    let mut events = world
        .events(&[
            "organization.created",
            "organization.member_added",
            "organization.deleted",
        ])
        .await;

    let agra = world.organization(ada, "Agra Group").await;
    assert_eq!(agra.slug, "agra-group");
    assert_eq!(agra.owner_id, ada);
    assert_eq!(agra.country, None);
    let owner = world
        .org_members()
        .await
        .find(agra.id, ada)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owner.role(), OrgRole::Owner);
    assert!(!owner.is_limited());
    let drained = events.drain();
    assert_eq!(
        drained
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["organization.created", "organization.member_added"]
    );
    assert_eq!(drained[0].1["slug"], json!("agra-group"));
    assert_eq!(drained[1].1["role"], json!("owner"));
    assert_eq!(drained[1].1["workspaces"], json!("all"));

    // Slugs, names, countries and time zones are checked.
    let service = organizations.clone();
    let taken = world
        .work(|uow| async move {
            service
                .create(
                    &uow,
                    ada,
                    NewOrganization::named("Other").slug("agra-group"),
                )
                .await
        })
        .await;
    assert!(matches!(
        taken,
        Err(WorkspaceError::Invalid { field: "slug", .. })
    ));
    for (input, field) in [
        (NewOrganization::named("  "), "name"),
        (NewOrganization::named("X").country("Britain"), "country"),
        (
            NewOrganization::named("X").timezone("not a zone"),
            "timezone",
        ),
    ] {
        let service = organizations.clone();
        let result = world
            .work(|uow| async move { service.create(&uow, ada, input).await })
            .await;
        assert!(
            matches!(result, Err(WorkspaceError::Invalid { field: f, .. }) if f == field),
            "{field}: {result:?}"
        );
    }
    let second = world.organization(ada, "Agra Group").await;
    assert_eq!(second.slug, "agra-group-2");

    // Updates normalize the country code; `None` leaves a value, `Some(None)`
    // clears it.
    let service = organizations.clone();
    let updated = world
        .work(|uow| async move {
            service
                .update(
                    &uow,
                    agra.id,
                    OrganizationChanges {
                        name: Some("Agra Holdings".into()),
                        country: Some(Some("gb".into())),
                        timezone: Some(Some("Europe/London".into())),
                    },
                )
                .await
        })
        .await
        .unwrap();
    assert_eq!(updated.name, "Agra Holdings");
    assert_eq!(updated.country.as_deref(), Some("GB"));
    assert_eq!(updated.timezone.as_deref(), Some("Europe/London"));
    let service = organizations.clone();
    let renamed = world
        .work(|uow| async move { service.rename(&uow, agra.id, "Agra").await })
        .await
        .unwrap();
    assert_eq!(renamed.name, "Agra");
    assert_eq!(renamed.country.as_deref(), Some("GB"));
    let service = organizations.clone();
    let cleared = world
        .work(|uow| async move {
            service
                .update(
                    &uow,
                    agra.id,
                    OrganizationChanges {
                        timezone: Some(None),
                        ..OrganizationChanges::default()
                    },
                )
                .await
        })
        .await
        .unwrap();
    assert_eq!(cleared.timezone, None);
    assert_eq!(cleared.country.as_deref(), Some("GB"));

    // Lookups.
    assert_eq!(
        organizations.resolve("agra-group").await.unwrap().id,
        agra.id
    );
    assert_eq!(
        organizations
            .resolve(&agra.id.to_string())
            .await
            .unwrap()
            .id,
        agra.id
    );
    assert!(matches!(
        organizations.resolve("nope").await,
        Err(WorkspaceError::OrganizationNotFound(_))
    ));
    let mine = organizations.for_user(ada).await.unwrap();
    assert_eq!(
        mine.iter()
            .map(|m| m.organization.name.as_str())
            .collect::<Vec<_>>(),
        ["Agra", "Agra Group"]
    );
    assert_eq!(mine[0].role, OrgRole::Owner);
    assert_eq!(mine[0].workspaces, Selector::All);

    // Deleting an organization keeps its workspaces, detached.
    let ws = world.workspace(ada, "Agra Pune").await;
    world.attach(agra.id, ws.id).await;
    events.drain();
    let service = organizations.clone();
    world
        .work(|uow| async move { service.delete(&uow, agra.id).await })
        .await
        .unwrap();
    assert!(organizations.find(agra.id).await.unwrap().is_none());
    let kept = world.workspaces().await.find(ws.id).await.unwrap().unwrap();
    assert_eq!(kept.organization_id, None);
    let drained = events.drain();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].0, "organization.deleted");
    assert_eq!(drained[0].1["detached"], json!([ws.id]));
}

#[tokio::test]
async fn workspaces_are_attached_and_detached() {
    let world = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.organization(ada, "Agra").await;
    let other = world.organization(ada, "Other").await;
    let pune = world.workspace(ada, "Agra Pune").await;
    let de = world.workspace(ada, "Agra DE").await;
    let organizations = world.organizations().await;
    let mut events = world
        .events(&[
            "organization.workspace_attached",
            "organization.workspace_detached",
        ])
        .await;

    world.attach(agra.id, pune.id).await;
    world.attach(agra.id, de.id).await;
    // Attaching again is a no-op; attaching to another organization a 409.
    world.attach(agra.id, pune.id).await;
    let service = organizations.clone();
    let taken = world
        .work(|uow| async move { service.attach(&uow, other.id, pune.id).await })
        .await;
    assert!(matches!(taken, Err(WorkspaceError::AlreadyAttached)));
    assert_eq!(
        organizations
            .workspaces(agra.id)
            .await
            .unwrap()
            .iter()
            .map(|w| w.slug.as_str())
            .collect::<Vec<_>>(),
        ["agra-de", "agra-pune"]
    );
    let attached = world
        .workspaces()
        .await
        .find(pune.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(attached.organization_id, Some(agra.id));
    // Mail sent for the workspace uses its organization's transport too.
    assert_eq!(
        navree_mail::MailScope::from(&attached),
        navree_mail::MailScope::workspace(pune.id).with_organization(agra.id)
    );

    // Detaching drops the grants that named the workspace.
    world
        .grant(
            agra.id,
            grace,
            "member",
            WorkspaceSelector::only([pune.id, de.id]),
        )
        .await;
    let service = organizations.clone();
    world
        .work(|uow| async move { service.detach(&uow, agra.id, pune.id).await })
        .await
        .unwrap();
    assert_eq!(
        world
            .org_members()
            .await
            .selector(agra.id, grace)
            .await
            .unwrap(),
        Some(Selector::Only(vec![de.id]))
    );
    let service = organizations.clone();
    let missing = world
        .work(|uow| async move { service.detach(&uow, agra.id, pune.id).await })
        .await;
    assert!(matches!(missing, Err(WorkspaceError::NotAttached)));
    assert_eq!(
        events.names(),
        [
            "organization.workspace_attached",
            "organization.workspace_attached",
            "organization.workspace_detached"
        ]
    );
}

#[tokio::test]
async fn org_members_reach_all_or_some_workspaces_with_the_mapped_role() {
    let world = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let linus = world.user("Laksh", "laksh@example.com").await;
    let ken = world.user("Ken", "ken@example.com").await;
    let agra = world.organization(ada, "Agra").await;
    let pune = world.workspace(ada, "Agra Pune").await;
    let de = world.workspace(ada, "Agra DE").await;
    let outside = world.workspace(ada, "Outside").await;
    world.attach(agra.id, pune.id).await;
    world.attach(agra.id, de.id).await;
    let workspaces = world.workspaces().await;

    world
        .grant(agra.id, grace, "admin", WorkspaceSelector::All)
        .await;
    world
        .grant(agra.id, linus, "member", WorkspaceSelector::only([pune.id]))
        .await;
    world
        .grant(agra.id, ken, "partner", WorkspaceSelector::All)
        .await;

    let via = |role: OrgRole| GrantedBy::Organization {
        organization_id: agra.id,
        role,
    };
    assert_eq!(
        workspaces.role_of(pune.id, grace).await.unwrap(),
        Some((WorkspaceRole::Admin, via(OrgRole::Admin)))
    );
    assert_eq!(
        workspaces.role_of(de.id, grace).await.unwrap(),
        Some((WorkspaceRole::Admin, via(OrgRole::Admin)))
    );
    assert_eq!(
        workspaces.role_of(pune.id, linus).await.unwrap(),
        Some((WorkspaceRole::Member, via(OrgRole::Member)))
    );
    assert_eq!(workspaces.role_of(de.id, linus).await.unwrap(), None);
    assert_eq!(
        workspaces.role_of(de.id, ken).await.unwrap(),
        Some((
            WorkspaceRole::Viewer,
            via(OrgRole::Custom("partner".into()))
        ))
    );
    // Workspaces outside the organization are out of reach.
    assert_eq!(workspaces.role_of(outside.id, grace).await.unwrap(), None);
    // A membership of the workspace wins over the organization grant.
    world.join(de.id, grace, "viewer").await;
    assert_eq!(
        workspaces.role_of(de.id, grace).await.unwrap(),
        Some((WorkspaceRole::Viewer, GrantedBy::Membership))
    );

    // `for_user` lists the workspaces either way.
    let listed = workspaces.for_user(linus).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].workspace.id, pune.id);
    assert_eq!(listed[0].role, WorkspaceRole::Member);
    assert_eq!(listed[0].granted_by, via(OrgRole::Member));
    let grace_sees: Vec<_> = workspaces
        .for_user(grace)
        .await
        .unwrap()
        .into_iter()
        .map(|m| (m.workspace.slug, m.role))
        .collect();
    assert_eq!(
        grace_sees,
        [
            ("agra-de".to_owned(), WorkspaceRole::Viewer),
            ("agra-pune".to_owned(), WorkspaceRole::Admin)
        ]
    );
    assert_eq!(
        world
            .organizations()
            .await
            .visible_workspaces(agra.id, linus)
            .await
            .unwrap()
            .iter()
            .map(|w| w.id)
            .collect::<Vec<_>>(),
        [pune.id]
    );

    // Removing the member removes the reach.
    let members = world.org_members().await;
    world
        .work(|uow| async move { members.remove(&uow, agra.id, linus).await })
        .await
        .unwrap();
    assert_eq!(workspaces.role_of(pune.id, linus).await.unwrap(), None);
}

#[tokio::test]
async fn organization_grants_do_nothing_while_organizations_are_off() {
    let on = world().await;
    let ada = on.user("Anu", "anu@example.com").await;
    let grace = on.user("Gauri", "gauri@example.com").await;
    let agra = on.organization(ada, "Agra").await;
    let pune = on.workspace(ada, "Agra Pune").await;
    on.attach(agra.id, pune.id).await;
    on.grant(agra.id, grace, "admin", WorkspaceSelector::All)
        .await;
    assert!(
        on.workspaces()
            .await
            .role_of(pune.id, grace)
            .await
            .unwrap()
            .is_some()
    );

    let off = World::new().await;
    let ada = off.user("Anu", "anu@example.com").await;
    let organizations = off.organizations().await;
    let refused = off
        .work(|uow| async move {
            organizations
                .create(&uow, ada, NewOrganization::named("Agra"))
                .await
        })
        .await;
    assert!(matches!(
        refused,
        Err(WorkspaceError::FeatureDisabled(
            "workspace.organizations.enabled"
        ))
    ));
    let partitions = off.partitions().await;
    let ws = off.workspace(ada, "Agra").await;
    assert!(matches!(
        partitions.list(ws.id).await,
        Err(WorkspaceError::FeatureDisabled(
            "workspace.partitions.enabled"
        ))
    ));
}

#[tokio::test]
async fn members_are_added_changed_and_removed_and_the_owner_is_kept() {
    let world = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.organization(ada, "Agra").await;
    let pune = world.workspace(ada, "Agra Pune").await;
    let outside = world.workspace(ada, "Outside").await;
    world.attach(agra.id, pune.id).await;
    let members = world.org_members().await;
    let mut events = world
        .events(&[
            "organization.member_added",
            "organization.member_changed",
            "organization.member_removed",
        ])
        .await;

    let service = members.clone();
    let added = world
        .work(|uow| async move {
            service
                .add(
                    &uow,
                    agra.id,
                    grace,
                    OrgRole::Member,
                    WorkspaceSelector::All,
                )
                .await
        })
        .await
        .unwrap();
    assert_eq!(added.role(), OrgRole::Member);
    let service = members.clone();
    let twice = world
        .work(|uow| async move {
            service
                .add(
                    &uow,
                    agra.id,
                    grace,
                    OrgRole::Member,
                    WorkspaceSelector::All,
                )
                .await
        })
        .await;
    assert!(matches!(
        twice,
        Err(WorkspaceError::AlreadyOrganizationMember)
    ));

    let service = members.clone();
    let changed = world
        .work(|uow| async move {
            service
                .update(
                    &uow,
                    agra.id,
                    grace,
                    Some(OrgRole::Admin),
                    Some(WorkspaceSelector::only([pune.id])),
                )
                .await
        })
        .await
        .unwrap();
    assert_eq!(changed.role(), OrgRole::Admin);
    assert!(changed.is_limited());
    let listed = members.list(agra.id).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].member.user_id, ada);
    assert_eq!(listed[0].user.as_ref().unwrap().name, "Anu");
    assert_eq!(listed[1].workspaces, Selector::Only(vec![pune.id]));

    // Bad input.
    for (role, selector, field) in [
        (
            OrgRole::Custom("ghost".into()),
            WorkspaceSelector::All,
            "role",
        ),
        (OrgRole::Owner, WorkspaceSelector::All, "role"),
        (
            OrgRole::Member,
            WorkspaceSelector::only([outside.id]),
            "workspaces",
        ),
    ] {
        let service = members.clone();
        let result = world
            .work(|uow| async move { service.grant(&uow, agra.id, grace, role, selector).await })
            .await;
        assert!(
            matches!(result, Err(WorkspaceError::Invalid { field: f, .. }) if f == field),
            "{field}: {result:?}"
        );
    }
    // The owner can't be removed, demoted or limited.
    for (role, selector) in [
        (Some(OrgRole::Admin), None),
        (None, Some(WorkspaceSelector::only([pune.id]))),
    ] {
        let service = members.clone();
        let result = world
            .work(|uow| async move { service.update(&uow, agra.id, ada, role, selector).await })
            .await;
        assert!(matches!(
            result,
            Err(WorkspaceError::OrganizationOwnerRequired)
        ));
    }
    let service = members.clone();
    let result = world
        .work(|uow| async move { service.remove(&uow, agra.id, ada).await })
        .await;
    assert!(matches!(
        result,
        Err(WorkspaceError::OrganizationOwnerRequired)
    ));

    // Transferring ownership makes the old owner an admin.
    let organizations = world.organizations().await;
    let transferred = world
        .work(|uow| async move { organizations.transfer_ownership(&uow, agra.id, grace).await })
        .await
        .unwrap();
    assert_eq!(transferred.owner_id, grace);
    let grace_now = members.find(agra.id, grace).await.unwrap().unwrap();
    assert_eq!(grace_now.role(), OrgRole::Owner);
    assert!(!grace_now.is_limited());
    assert_eq!(
        members.find(agra.id, ada).await.unwrap().unwrap().role(),
        OrgRole::Admin
    );

    let service = members.clone();
    world
        .work(|uow| async move { service.remove(&uow, agra.id, ada).await })
        .await
        .unwrap();
    let service = members.clone();
    let gone = world
        .work(|uow| async move { service.remove(&uow, agra.id, ada).await })
        .await;
    assert!(matches!(gone, Err(WorkspaceError::MemberNotFound)));
    // Ownership transfers emit `organization.ownership_transferred` only.
    assert_eq!(
        events.names(),
        [
            "organization.member_added",
            "organization.member_changed",
            "organization.member_removed"
        ]
    );
}

#[tokio::test]
async fn access_requests_are_requested_approved_and_declined() {
    let world = world().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let linus = world.user("Laksh", "laksh@example.com").await;
    let agra = world.organization(ada, "Agra").await;
    let pune = world.workspace(ada, "Agra Pune").await;
    let outside = world.workspace(ada, "Outside").await;
    world.attach(agra.id, pune.id).await;
    let requests = world.access_requests().await;
    let mut events = world
        .events(&[
            "organization.access_requested",
            "organization.access_request_approved",
            "organization.access_request_declined",
            "organization.member_added",
        ])
        .await;

    let service = requests.clone();
    let asked = world
        .work(|uow| async move {
            service
                .request(
                    &uow,
                    agra.id,
                    grace,
                    NewAccessRequest {
                        workspace_id: Some(pune.id),
                        message: Some("  I run the Pune team.  ".into()),
                    },
                )
                .await
        })
        .await
        .unwrap();
    assert_eq!(asked.status(), AccessRequestStatus::Pending);
    assert_eq!(asked.message.as_deref(), Some("I run the Pune team."));

    // One pending request per user; members and other workspaces refused.
    for (user, input, expected) in [
        (grace, NewAccessRequest::default(), "pending"),
        (ada, NewAccessRequest::default(), "member"),
        (
            linus,
            NewAccessRequest {
                workspace_id: Some(outside.id),
                message: None,
            },
            "workspace",
        ),
    ] {
        let service = requests.clone();
        let result = world
            .work(|uow| async move { service.request(&uow, agra.id, user, input).await })
            .await;
        match expected {
            "pending" => assert!(matches!(result, Err(WorkspaceError::AccessRequestPending))),
            "member" => assert!(matches!(
                result,
                Err(WorkspaceError::AlreadyOrganizationMember)
            )),
            _ => assert!(matches!(
                result,
                Err(WorkspaceError::Invalid {
                    field: "workspace_id",
                    ..
                })
            )),
        }
    }
    let pending = requests.pending(agra.id).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].user.as_ref().unwrap().email, "gauri@example.com");

    // Approving gives the requested workspace with the approval's role.
    let service = requests.clone();
    let approved = world
        .work(|uow| async move {
            service
                .approve(&uow, agra.id, asked.id, Some(ada), Approval::default())
                .await
        })
        .await
        .unwrap();
    assert_eq!(approved.request.status(), AccessRequestStatus::Approved);
    assert_eq!(approved.request.decided_by, Some(ada));
    assert_eq!(approved.request.role.as_deref(), Some("member"));
    assert_eq!(approved.member.role(), OrgRole::Member);
    assert_eq!(
        world
            .org_members()
            .await
            .selector(agra.id, grace)
            .await
            .unwrap(),
        Some(Selector::Only(vec![pune.id]))
    );
    let service = requests.clone();
    let again = world
        .work(|uow| async move {
            service
                .approve(&uow, agra.id, asked.id, Some(ada), Approval::default())
                .await
        })
        .await;
    assert!(matches!(again, Err(WorkspaceError::AccessRequestDecided)));

    // Declining records the reason.
    let service = requests.clone();
    let linus_asks = world
        .work(|uow| async move {
            service
                .request(&uow, agra.id, linus, NewAccessRequest::default())
                .await
        })
        .await
        .unwrap();
    let service = requests.clone();
    let declined = world
        .work(|uow| async move {
            service
                .decline(
                    &uow,
                    agra.id,
                    linus_asks.id,
                    Some(ada),
                    Some("Not yet.".into()),
                )
                .await
        })
        .await
        .unwrap();
    assert_eq!(declined.status(), AccessRequestStatus::Declined);
    assert_eq!(declined.reason.as_deref(), Some("Not yet."));
    assert!(requests.pending(agra.id).await.unwrap().is_empty());
    assert_eq!(requests.for_user(linus).await.unwrap().len(), 1);
    let service = requests.clone();
    let unknown = world
        .work(|uow| async move {
            service
                .decline(&uow, agra.id, uuid::Uuid::now_v7(), None, None)
                .await
        })
        .await;
    assert!(matches!(
        unknown,
        Err(WorkspaceError::AccessRequestNotFound)
    ));

    // After a decline, a new request can be made and approved for every
    // workspace with another role.
    let service = requests.clone();
    let retry = world
        .work(|uow| async move {
            service
                .request(&uow, agra.id, linus, NewAccessRequest::default())
                .await
        })
        .await
        .unwrap();
    let service = requests.clone();
    let approved = world
        .work(|uow| async move {
            service
                .approve(
                    &uow,
                    agra.id,
                    retry.id,
                    Some(ada),
                    Approval {
                        role: OrgRole::Viewer,
                        workspaces: None,
                    },
                )
                .await
        })
        .await
        .unwrap();
    assert_eq!(approved.member.role(), OrgRole::Viewer);
    assert!(!approved.member.is_limited());

    let names = events.names();
    assert_eq!(
        names
            .iter()
            .filter(|n| *n == "organization.access_requested")
            .count(),
        3
    );
    assert_eq!(
        names
            .iter()
            .filter(|n| *n == "organization.access_request_approved")
            .count(),
        2
    );
    assert_eq!(
        names
            .iter()
            .filter(|n| *n == "organization.access_request_declined")
            .count(),
        1
    );
    assert_eq!(
        names
            .iter()
            .filter(|n| *n == "organization.member_added")
            .count(),
        2
    );
}
