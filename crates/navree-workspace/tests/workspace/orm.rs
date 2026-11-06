//! The workspace tables as navree-orm models: workspaces, memberships,
//! invitations, organizations and partitions written by the services read
//! back as models, queried with typed columns.

use navree_orm::Model as _;
use navree_workspace::WorkspaceSelector;
use navree_workspace::entity::{
    Member, OrgMember, Organization, Partition, Workspace, invitation, organization_grant,
};

use crate::support::{World, org_builder};

#[tokio::test]
async fn workspaces_and_memberships_are_orm_models() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra Inc.").await;
    let gulati = world.workspace(grace, "Gulati").await;
    world.join(agra.id, grace, "member").await;
    let db = world.db().await;

    assert_eq!(Workspace::table_name(), "workspaces");
    let found = Workspace::query()
        .where_(Workspace::SLUG.eq("agra-inc"))
        .first(&db)
        .await
        .unwrap()
        .expect("agra");
    assert_eq!(found.model(), &agra);

    let members = Member::query()
        .where_(Member::WORKSPACE_ID.eq(agra.id))
        .order_by(Member::CREATED_AT)
        .order_by(Member::ID)
        .pluck(&db, Member::USER_ID)
        .await
        .unwrap();
    assert_eq!(members, [ada, grace]);
    let graces = Member::query()
        .where_(Member::USER_ID.eq(grace))
        .pluck(&db, Member::WORKSPACE_ID)
        .await
        .unwrap();
    assert_eq!(graces.len(), 2);
    assert!(graces.contains(&agra.id) && graces.contains(&gulati.id));
    assert_eq!(
        Member::query()
            .where_(Member::ROLE.eq("owner"))
            .count(&db)
            .await
            .unwrap(),
        2
    );
    assert!(
        invitation::Model::query()
            .where_(invitation::Model::WORKSPACE_ID.eq(agra.id))
            .doesnt_exist(&db)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn organizations_and_partitions_are_orm_models() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let group = world.organization(ada, "Agra Group").await;
    let agra = world.workspace(ada, "Agra").await;
    world.attach(group.id, agra.id).await;
    world
        .grant(
            group.id,
            grace,
            "member",
            WorkspaceSelector::only([agra.id]),
        )
        .await;
    let uk = world.partition(agra.id, "agra.co.uk").await;
    let db = world.db().await;

    let found = Organization::find(&db, group.id)
        .await
        .unwrap()
        .expect("the organization");
    assert_eq!(found.slug, "agra-group");
    assert_eq!(
        Workspace::query()
            .where_(Workspace::ORGANIZATION_ID.eq(group.id))
            .pluck(&db, Workspace::ID)
            .await
            .unwrap(),
        [agra.id]
    );
    let limited = OrgMember::query()
        .where_(OrgMember::ORGANIZATION_ID.eq(group.id))
        .where_(OrgMember::SCOPE.eq("only"))
        .get(&db)
        .await
        .unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].user_id, grace);
    assert_eq!(
        organization_grant::Model::query()
            .where_(organization_grant::Model::MEMBER_ID.eq(limited[0].id))
            .pluck(&db, organization_grant::Model::WORKSPACE_ID)
            .await
            .unwrap(),
        [agra.id]
    );
    assert_eq!(
        Partition::query()
            .where_(Partition::WORKSPACE_ID.eq(agra.id))
            .first(&db)
            .await
            .unwrap()
            .expect("the partition")
            .model(),
        &uk
    );
}
