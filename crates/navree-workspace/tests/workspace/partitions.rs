//! The `Partitions` service: partitions inside a workspace and the
//! partitions each member sees.

use navree_workspace::{NewPartition, PartitionSelector, Selector, WorkspaceError};
use serde_json::json;

use crate::support::{World, org_builder};

#[tokio::test]
async fn partitions_are_created_renamed_listed_and_deleted() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let other = world.workspace(ada, "Other").await;
    let partitions = world.partitions().await;
    let mut events = world
        .events(&["workspace.partition_created", "workspace.partition_deleted"])
        .await;

    let uk = world.partition(agra.id, "agra.co.uk").await;
    assert_eq!(uk.slug, "agra-co-uk");
    assert_eq!(uk.name, "agra.co.uk");
    assert_eq!(uk.workspace_id, agra.id);
    let de = world.partition(agra.id, "agra.de").await;
    // Slugs are unique per workspace, not across workspaces.
    let elsewhere = world.partition(other.id, "agra.co.uk").await;
    assert_eq!(elsewhere.slug, "agra-co-uk");
    let service = partitions.clone();
    let taken = world
        .work(|uow| async move {
            service
                .create(&uow, agra.id, NewPartition::named("UK").slug("agra-co-uk"))
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
                .create(&uow, agra.id, NewPartition::named("agra.co.uk"))
                .await
        })
        .await
        .unwrap();
    assert_eq!(derived.slug, "agra-co-uk-2");
    let service = partitions.clone();
    let empty = world
        .work(|uow| async move {
            service
                .create(&uow, agra.id, NewPartition::named(" "))
                .await
        })
        .await;
    assert!(matches!(
        empty,
        Err(WorkspaceError::Invalid { field: "name", .. })
    ));

    assert_eq!(
        partitions
            .list(agra.id)
            .await
            .unwrap()
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["agra.co.uk", "agra.co.uk", "agra.de"]
    );
    assert_eq!(
        partitions.resolve(agra.id, "agra-de").await.unwrap().id,
        de.id
    );
    assert_eq!(
        partitions
            .resolve(agra.id, &uk.id.to_string())
            .await
            .unwrap()
            .id,
        uk.id
    );
    // Another workspace's partition is not found here.
    assert!(matches!(
        partitions.resolve(agra.id, &elsewhere.id.to_string()).await,
        Err(WorkspaceError::PartitionNotFound(_))
    ));

    let service = partitions.clone();
    let renamed = world
        .work(|uow| async move { service.rename(&uow, agra.id, de.id, "agra.de (EU)").await })
        .await
        .unwrap();
    assert_eq!(renamed.name, "agra.de (EU)");
    assert_eq!(renamed.slug, "agra-de");

    let service = partitions.clone();
    world
        .work(|uow| async move { service.delete(&uow, agra.id, derived.id).await })
        .await
        .unwrap();
    let service = partitions.clone();
    let wrong_workspace = world
        .work(|uow| async move { service.delete(&uow, agra.id, elsewhere.id).await })
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
    assert_eq!(drained[0].1["slug"], json!("agra-co-uk"));
}

#[tokio::test]
async fn members_see_all_partitions_or_only_their_grants() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let other = world.workspace(ada, "Other").await;
    world.join(agra.id, grace, "member").await;
    let uk = world.partition(agra.id, "agra.co.uk").await;
    let de = world.partition(agra.id, "agra.de").await;
    let foreign = world.partition(other.id, "other.com").await;
    let partitions = world.partitions().await;
    let mut events = world.events(&["workspace.partition_access_changed"]).await;

    assert_eq!(
        partitions.access(agra.id, grace).await.unwrap(),
        Selector::All
    );
    world
        .limit_partitions(agra.id, grace, PartitionSelector::only([uk.id]))
        .await;
    assert_eq!(
        partitions.access(agra.id, grace).await.unwrap(),
        Selector::Only(vec![uk.id])
    );
    assert_eq!(
        partitions
            .visible(agra.id, &partitions.access(agra.id, grace).await.unwrap())
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
        .find(agra.id, grace)
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
                .grant(&uow, agra.id, grace, PartitionSelector::only([foreign.id]))
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
                .grant(&uow, agra.id, ada, PartitionSelector::only([uk.id]))
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
        .work(|uow| async move { service.delete(&uow, agra.id, uk.id).await })
        .await
        .unwrap();
    assert_eq!(
        partitions.access(agra.id, grace).await.unwrap(),
        Selector::Only(vec![])
    );
    world
        .limit_partitions(agra.id, grace, PartitionSelector::All)
        .await;
    assert_eq!(
        partitions.access(agra.id, grace).await.unwrap(),
        Selector::All
    );
    let _ = de;
    let drained = events.drain();
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[0].1["partitions"], json!([uk.id]));
    assert_eq!(drained[1].1["partitions"], json!("all"));
}
