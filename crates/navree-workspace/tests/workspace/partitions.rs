//! The `Partitions` service: partitions inside a workspace and the
//! partitions each member sees.

use navree_workspace::{NewPartition, PartitionSelector, Selector, WorkspaceError};
use serde_json::json;

use crate::support::{World, org_builder};

#[tokio::test]
async fn partitions_are_created_renamed_listed_and_deleted() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let other = world.workspace(ada, "Other").await;
    let partitions = world.partitions().await;
    let mut events = world
        .events(&["workspace.partition_created", "workspace.partition_deleted"])
        .await;

    let uk = world.partition(acme.id, "acme.co.uk").await;
    assert_eq!(uk.slug, "acme-co-uk");
    assert_eq!(uk.name, "acme.co.uk");
    assert_eq!(uk.workspace_id, acme.id);
    let de = world.partition(acme.id, "acme.de").await;
    // Slugs are unique per workspace, not across workspaces.
    let elsewhere = world.partition(other.id, "acme.co.uk").await;
    assert_eq!(elsewhere.slug, "acme-co-uk");
    let service = partitions.clone();
    let taken = world
        .work(|uow| async move {
            service
                .create(&uow, acme.id, NewPartition::named("UK").slug("acme-co-uk"))
                .await
        })
        .await;
    assert!(matches!(
        taken,
        Err(WorkspaceError::Invalid { field: "slug", .. })
    ));
    let service = partitions.clone();
    let derived = world
        .work(|uow| async move {
            service
                .create(&uow, acme.id, NewPartition::named("acme.co.uk"))
                .await
        })
        .await
        .unwrap();
    assert_eq!(derived.slug, "acme-co-uk-2");
    let service = partitions.clone();
    let empty = world
        .work(|uow| async move {
            service
                .create(&uow, acme.id, NewPartition::named(" "))
                .await
        })
        .await;
    assert!(matches!(
        empty,
        Err(WorkspaceError::Invalid { field: "name", .. })
    ));

    assert_eq!(
        partitions
            .list(acme.id)
            .await
            .unwrap()
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["acme.co.uk", "acme.co.uk", "acme.de"]
    );
    assert_eq!(
        partitions.resolve(acme.id, "acme-de").await.unwrap().id,
        de.id
    );
    assert_eq!(
        partitions
            .resolve(acme.id, &uk.id.to_string())
            .await
            .unwrap()
            .id,
        uk.id
    );
    // Another workspace's partition is not found here.
    assert!(matches!(
        partitions.resolve(acme.id, &elsewhere.id.to_string()).await,
        Err(WorkspaceError::PartitionNotFound(_))
    ));

    let service = partitions.clone();
    let renamed = world
        .work(|uow| async move { service.rename(&uow, acme.id, de.id, "acme.de (EU)").await })
        .await
        .unwrap();
    assert_eq!(renamed.name, "acme.de (EU)");
    assert_eq!(renamed.slug, "acme-de");

    let service = partitions.clone();
    world
        .work(|uow| async move { service.delete(&uow, acme.id, derived.id).await })
        .await
        .unwrap();
    let service = partitions.clone();
    let wrong_workspace = world
        .work(|uow| async move { service.delete(&uow, acme.id, elsewhere.id).await })
        .await;
    assert!(matches!(
        wrong_workspace,
        Err(WorkspaceError::PartitionNotFound(_))
    ));
    let drained = events.drain();
    assert_eq!(
        drained.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        [
            "workspace.partition_created",
            "workspace.partition_created",
            "workspace.partition_created",
            "workspace.partition_created",
            "workspace.partition_deleted"
        ]
    );
    assert_eq!(drained[0].1["slug"], json!("acme-co-uk"));
}

#[tokio::test]
async fn members_see_all_partitions_or_only_their_grants() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let other = world.workspace(ada, "Other").await;
    world.join(acme.id, grace, "member").await;
    let uk = world.partition(acme.id, "acme.co.uk").await;
    let de = world.partition(acme.id, "acme.de").await;
    let foreign = world.partition(other.id, "other.com").await;
    let partitions = world.partitions().await;
    let mut events = world.events(&["workspace.partition_access_changed"]).await;

    assert_eq!(
        partitions.access(acme.id, grace).await.unwrap(),
        Selector::All
    );
    world
        .limit_partitions(acme.id, grace, PartitionSelector::only([uk.id]))
        .await;
    assert_eq!(
        partitions.access(acme.id, grace).await.unwrap(),
        Selector::Only(vec![uk.id])
    );
    assert_eq!(
        partitions
            .visible(acme.id, &partitions.access(acme.id, grace).await.unwrap())
            .await
            .unwrap()
            .iter()
            .map(|p| p.id)
            .collect::<Vec<_>>(),
        [uk.id]
    );
    let member = world
        .members()
        .await
        .find(acme.id, grace)
        .await
        .unwrap()
        .unwrap();
    assert!(member.partitions_limited());

    // Another workspace's partitions can't be granted; the owner can't be
    // limited; non-members have no access.
    let service = partitions.clone();
    let foreign_grant = world
        .work(|uow| async move {
            service
                .grant(&uow, acme.id, grace, PartitionSelector::only([foreign.id]))
                .await
        })
        .await;
    assert!(matches!(
        foreign_grant,
        Err(WorkspaceError::Invalid {
            field: "partitions",
            ..
        })
    ));
    let service = partitions.clone();
    let owner = world
        .work(|uow| async move {
            service
                .grant(&uow, acme.id, ada, PartitionSelector::only([uk.id]))
                .await
        })
        .await;
    assert!(matches!(owner, Err(WorkspaceError::OwnerRequired)));
    assert!(matches!(
        partitions.access(other.id, grace).await,
        Err(WorkspaceError::MemberNotFound)
    ));

    // Deleting a partition drops its grants.
    let service = partitions.clone();
    world
        .work(|uow| async move { service.delete(&uow, acme.id, uk.id).await })
        .await
        .unwrap();
    assert_eq!(
        partitions.access(acme.id, grace).await.unwrap(),
        Selector::Only(vec![])
    );
    world
        .limit_partitions(acme.id, grace, PartitionSelector::All)
        .await;
    assert_eq!(
        partitions.access(acme.id, grace).await.unwrap(),
        Selector::All
    );
    let _ = de;
    let drained = events.drain();
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[0].1["partitions"], json!([uk.id]));
    assert_eq!(drained[1].1["partitions"], json!("all"));
}
