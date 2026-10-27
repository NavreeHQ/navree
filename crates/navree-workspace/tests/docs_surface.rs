//! Every public item the organizations and partitions docs pages name
//! (`apps/website/content/docs/workspaces/organizations.mdx` and
//! `partitions.mdx`, and their additions to `resolution.mdx`,
//! `members-and-roles.mdx` and `isolation.mdx`): services, extractors,
//! repositories, configuration keys, tables, events and the row-level
//! security SQL. A rename fails this file.

use navree_access::AbilityCtx;
use navree_config::{Config, ConfigSources};
use navree_db::Uow;
use navree_events::Event;
use navree_http::AppError;
use navree_workspace::entity::{
    access_request, organization, organization_grant, organization_member, partition,
    partition_grant, workspace,
};
use navree_workspace::events::{
    AccessRequestApproved, AccessRequestDeclined, AccessRequested, OrgMemberAdded,
    OrgMemberChanged, OrgMemberRemoved, OrganizationCreated, OrganizationDeleted,
    OrganizationOwnershipTransferred, PartitionAccessChanged, PartitionCreated, PartitionDeleted,
    WorkspaceAttached, WorkspaceDetached,
};
use navree_workspace::{
    AccessRequests, Approval, ApprovedRequest, CurrentOrganization, CurrentPartition,
    CurrentWorkspace, GrantedBy, InOrganization, NewAccessRequest, NewOrganization, NewPartition,
    OrgMemberInfo, OrgMembers, OrgMembership, OrgRepo, OrgRole, OrganizationChanges,
    Organizations, PartitionRepo, PartitionScope, PartitionScoped, PartitionSelector, Partitions,
    RlsPolicy, WorkspaceConfig, WorkspaceError, WorkspaceScoped, WorkspaceSelector, Workspaces,
};
use sea_orm::{ActiveValue::Set, EntityName, EntityTrait};
use uuid::Uuid;

/// `contacts(id, workspace_id, partition_id, name)`, as on the partitions
/// page.
mod contact {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "contacts")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub workspace_id: Uuid,
        pub partition_id: Option<Uuid>,
        pub name: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl WorkspaceScoped for contact::Entity {}
impl PartitionScoped for contact::Entity {}

type Contact = contact::Entity;

/// The `Organizations` methods of the organizations page (compiled, never
/// run).
#[allow(dead_code)]
async fn organizations_methods(
    organizations: &Organizations,
    uow: &Uow,
    user: Uuid,
    uk: Uuid,
) -> Result<(), WorkspaceError> {
    let acme = organizations
        .create(
            uow,
            user,
            NewOrganization::named("Acme Group")
                .country("gb")
                .timezone("Europe/London"),
        )
        .await?;
    let _: (String, Option<String>, Option<String>) = (acme.slug, acme.country, acme.timezone);
    organizations
        .create(uow, user, NewOrganization::named("Acme").slug("acme"))
        .await?;
    organizations
        .update(
            uow,
            acme.id,
            OrganizationChanges {
                name: Some("Acme".into()),
                country: Some(None),
                timezone: None,
            },
        )
        .await?;
    organizations.rename(uow, acme.id, "Acme").await?;
    organizations.attach(uow, acme.id, uk).await?;
    organizations.detach(uow, acme.id, uk).await?;
    organizations.transfer_ownership(uow, acme.id, user).await?;
    organizations.find(acme.id).await?;
    organizations.find_by_slug("acme").await?;
    organizations.resolve("acme").await?;
    for OrgMembership {
        organization,
        role,
        workspaces,
    } in organizations.for_user(user).await?
    {
        let _: (Uuid, OrgRole, WorkspaceSelector) = (organization.id, role, workspaces);
    }
    organizations.workspaces(acme.id).await?;
    organizations.visible_workspaces(acme.id, user).await?;
    organizations.membership(acme.id, user).await?;
    organizations.all().await?;
    organizations.enter(uow, acme.id).await?;
    organizations.delete(uow, acme.id).await
}

/// `OrgMembers` and the selectors.
#[allow(dead_code)]
async fn org_members_methods(
    members: &OrgMembers,
    uow: &Uow,
    org: Uuid,
    user: Uuid,
    uk: Uuid,
) -> Result<(), WorkspaceError> {
    members
        .add(uow, org, user, OrgRole::Admin, WorkspaceSelector::All)
        .await?;
    members
        .add(uow, org, user, OrgRole::Member, WorkspaceSelector::only([uk]))
        .await?;
    members
        .grant(uow, org, user, OrgRole::Viewer, WorkspaceSelector::All)
        .await?;
    members
        .update(uow, org, user, Some(OrgRole::Member), None)
        .await?;
    for OrgMemberInfo {
        member,
        workspaces,
        user,
    } in members.list(org).await?
    {
        let _ = (member.role, workspaces, user.map(|u| u.email));
    }
    members.find(org, user).await?;
    members.selector(org, user).await?;
    members.remove(uow, org, user).await
}

/// `AccessRequests`.
#[allow(dead_code)]
async fn access_request_methods(
    requests: &AccessRequests,
    uow: &Uow,
    org: Uuid,
    ken: Uuid,
    grace: Uuid,
    uk: Uuid,
) -> Result<(), WorkspaceError> {
    let request = requests
        .request(
            uow,
            org,
            ken,
            NewAccessRequest {
                workspace_id: Some(uk),
                message: Some("I run the UK team".into()),
            },
        )
        .await?;
    let ApprovedRequest { request: _, member } = requests
        .approve(uow, org, request.id, Some(grace), Approval::default())
        .await?;
    let _: Uuid = member.user_id;
    requests
        .approve(
            uow,
            org,
            request.id,
            None,
            Approval {
                role: OrgRole::Viewer,
                workspaces: Some(WorkspaceSelector::All),
            },
        )
        .await?;
    requests
        .decline(uow, org, request.id, Some(grace), Some("No".into()))
        .await?;
    for info in requests.pending(org).await? {
        let _ = (info.request.status(), info.user);
    }
    requests.for_user(ken).await?;
    requests.find(org, request.id).await?;
    Ok(())
}

/// Workspace resolution with organization grants.
#[allow(dead_code)]
async fn workspace_grants(
    workspaces: &Workspaces,
    ws: CurrentWorkspace,
    user: Uuid,
) -> Result<(), WorkspaceError> {
    let _: Uuid = ws.user_id();
    let _: Option<Uuid> = ws.workspace().organization_id;
    match ws.granted_by() {
        GrantedBy::Membership => {}
        GrantedBy::Organization {
            organization_id,
            role,
        } => {
            let _: (&Uuid, &OrgRole) = (organization_id, role);
        }
    }
    for membership in workspaces.for_user(user).await? {
        let _: GrantedBy = membership.granted_by;
    }
    workspaces.role_of(ws.id(), user).await?;
    Ok(())
}

/// The `CurrentOrganization` table and `OrgRepo`.
#[allow(dead_code)]
async fn current_organization(org: CurrentOrganization, uow: Uow) -> Result<(), AppError> {
    let _: Uuid = org.id();
    let _: &str = &org.slug;
    let _ = (org.organization(), org.member(), org.role(), org.workspaces());
    let ids: &[Uuid] = org.workspace_ids();
    let _: bool = org.reaches(Uuid::now_v7());
    let _: &[String] = org.permissions();
    let _: bool = org.has_permission("reports.read") && org.can("read", "Deal");
    org.authorize("reports.read")?;
    let _ = (org.ability(), org.access(), org.resolver());
    let _ = org.scope(Contact::find());
    let _ = OrgRepo::<Contact>::select(ids);
    let _ = OrgRepo::<Contact>::scope(ids, Contact::find());
    OrgRepo::<Contact>::find(&uow, ids, Uuid::now_v7()).await?;
    OrgRepo::<Contact>::find_or_fail(&uow, ids, Uuid::now_v7()).await?;
    let _ = (
        InOrganization::guard(),
        InOrganization::from_path("organization"),
    );
    Ok(())
}

/// The `Partitions` methods of the partitions page.
#[allow(dead_code)]
async fn partitions_methods(
    partitions: &Partitions,
    uow: &Uow,
    workspace: Uuid,
    grace: Uuid,
) -> Result<(), WorkspaceError> {
    let uk = partitions
        .create(uow, workspace, NewPartition::named("acme.co.uk"))
        .await?;
    let _: String = uk.slug;
    partitions
        .create(uow, workspace, NewPartition::named("acme.de").slug("de"))
        .await?;
    partitions
        .grant(uow, workspace, grace, PartitionSelector::only([uk.id]))
        .await?;
    partitions.rename(uow, workspace, uk.id, "acme.uk").await?;
    partitions.list(workspace).await?;
    partitions
        .visible(workspace, &PartitionSelector::All)
        .await?;
    partitions.find(workspace, uk.id).await?;
    partitions.resolve(workspace, "acme-co-uk").await?;
    let _: PartitionSelector = partitions.access(workspace, grace).await?;
    partitions.enter(uow, uk.id).await?;
    partitions.delete(uow, workspace, uk.id).await
}

/// The partition extractors and `PartitionRepo`.
#[allow(dead_code)]
async fn partition_requests(
    ws: CurrentWorkspace,
    scope: PartitionScope,
    partition: CurrentPartition,
    uow: Uow,
) -> Result<(), AppError> {
    let workspace: Uuid = scope.workspace_id();
    let selector: &PartitionSelector = scope.selector();
    let _ = (scope.allowed(), scope.current(), scope.includes(Uuid::now_v7()));
    let _ = scope.scope(Contact::find());
    let _: (Uuid, Uuid, &str) = (
        partition.id(),
        partition.workspace_id(),
        partition.resolver().as_str(),
    );
    let _: &str = &partition.slug;
    let _ = PartitionRepo::<Contact>::select(workspace, selector);
    let _ = PartitionRepo::<Contact>::scope(workspace, selector, Contact::find());
    PartitionRepo::<Contact>::find(&uow, workspace, selector, Uuid::now_v7()).await?;
    let found =
        PartitionRepo::<Contact>::find_or_fail(&uow, workspace, selector, Uuid::now_v7()).await?;
    let created = PartitionRepo::<Contact>::insert(
        &uow,
        ws.id(),
        partition.id(),
        contact::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set("Ada".into()),
            ..Default::default()
        },
    )
    .await?;
    let mut row: contact::ActiveModel = created.into();
    row.name = Set("Grace".into());
    PartitionRepo::<Contact>::update(&uow, workspace, selector, row).await?;
    PartitionRepo::<Contact>::delete(&uow, workspace, selector, found).await?;
    Ok(())
}

#[test]
fn the_documented_names_exist() {
    // Tables.
    assert_eq!(organization::Entity.table_name(), "organizations");
    assert_eq!(
        organization_member::Entity.table_name(),
        "organization_members"
    );
    assert_eq!(organization_grant::Entity.table_name(), "organization_grants");
    assert_eq!(access_request::Entity.table_name(), "access_requests");
    assert_eq!(partition::Entity.table_name(), "workspace_partitions");
    assert_eq!(partition_grant::Entity.table_name(), "partition_grants");
    assert_eq!(workspace::Entity.table_name(), "workspaces");
    // Events.
    for (name, expected) in [
        (OrganizationCreated::NAME, "organization.created"),
        (OrganizationDeleted::NAME, "organization.deleted"),
        (
            OrganizationOwnershipTransferred::NAME,
            "organization.ownership_transferred",
        ),
        (WorkspaceAttached::NAME, "organization.workspace_attached"),
        (WorkspaceDetached::NAME, "organization.workspace_detached"),
        (OrgMemberAdded::NAME, "organization.member_added"),
        (OrgMemberChanged::NAME, "organization.member_changed"),
        (OrgMemberRemoved::NAME, "organization.member_removed"),
        (AccessRequested::NAME, "organization.access_requested"),
        (
            AccessRequestApproved::NAME,
            "organization.access_request_approved",
        ),
        (
            AccessRequestDeclined::NAME,
            "organization.access_request_declined",
        ),
        (PartitionCreated::NAME, "workspace.partition_created"),
        (PartitionDeleted::NAME, "workspace.partition_deleted"),
        (
            PartitionAccessChanged::NAME,
            "workspace.partition_access_changed",
        ),
    ] {
        assert_eq!(name, expected);
    }
    // The abilities context carries the organization.
    let ctx = AbilityCtx::default();
    assert_eq!(ctx.organization_id, None);
    assert_eq!(ctx.team_id, None);
    // The default partition column.
    assert_eq!(<Contact as PartitionScoped>::PARTITION_COLUMN, "partition_id");
}

#[test]
fn the_documented_configuration_loads() {
    let sources = ConfigSources::new()
        .set("workspace.roles.auditor.permissions", serde_json::json!(["workspace.read", "deals.read"]))
        .set("workspace.organizations.enabled", true)
        .set(
            "workspace.organizations.resolvers",
            serde_json::json!(["header", "path", "subdomain", "claim"]),
        )
        .set("workspace.organizations.header", "X-Organization")
        .set("workspace.organizations.base_domain", "partners.example.com")
        .set("workspace.organizations.path_prefix", "/o")
        .set("workspace.organizations.claim", "organization")
        .set(
            "workspace.organizations.roles.member.permissions",
            serde_json::json!(["reports.read"]),
        )
        .set("workspace.organizations.roles.member.workspace_role", "viewer")
        .set(
            "workspace.organizations.roles.partner.permissions",
            serde_json::json!(["organization.read", "reports.read"]),
        )
        .set("workspace.organizations.roles.partner.workspace_role", "auditor")
        .set("workspace.partitions.enabled", true)
        .set("workspace.partitions.resolvers", serde_json::json!(["header", "claim"]))
        .set("workspace.partitions.header", "X-Partition")
        .set("workspace.partitions.path_prefix", "/p")
        .set("workspace.partitions.claim", "partition");
    let config = Config::load(&sources)
        .unwrap()
        .typed::<WorkspaceConfig>()
        .unwrap();
    assert!(config.organizations.enabled && config.partitions.enabled);
    // A role without `workspace_role` needs a workspace role of its name.
    let missing = Config::load(&ConfigSources::new().set(
        "workspace.organizations.roles.partner.permissions",
        serde_json::json!(["organization.read"]),
    ))
    .unwrap()
    .typed::<WorkspaceConfig>();
    assert!(missing.is_err());
    // Partitions have no subdomain resolver.
    let subdomain = Config::load(
        &ConfigSources::new().set("workspace.partitions.resolvers", serde_json::json!(["subdomain"])),
    )
    .unwrap()
    .typed::<WorkspaceConfig>();
    assert!(subdomain.is_err());
}

#[test]
fn the_documented_policies_are_the_sql_on_the_pages() {
    let workspace = "\"workspace_id\" = NULLIF(current_setting('navree.workspace', true), '')::uuid";
    let partition = "(NULLIF(current_setting('navree.partition', true), '') IS NULL OR \"partition_id\" = NULLIF(current_setting('navree.partition', true), '')::uuid)";
    // partitions.mdx
    let partitioned = RlsPolicy::new("contacts").partitioned().create_sql();
    assert!(partitioned.ends_with(&format!(
        "CREATE POLICY \"contacts_workspace_isolation\" ON \"contacts\"\n    \
         USING (({workspace}) AND {partition})\n    \
         WITH CHECK (({workspace}) AND {partition});"
    )));
    // isolation.mdx
    let organizations = RlsPolicy::new("contacts").organizations().create_sql();
    assert!(organizations.ends_with(
        "CREATE POLICY \"contacts_organization_read\" ON \"contacts\" FOR SELECT\n    \
         USING (\"workspace_id\" IN (SELECT \"id\" FROM \"workspaces\" WHERE \"organization_id\" = NULLIF(current_setting('navree.organization', true), '')::uuid));"
    ));
    let both = RlsPolicy::new("contacts")
        .organizations()
        .partitioned()
        .create_sql();
    assert!(both.contains("contacts_organization_read"));
    assert!(
        RlsPolicy::new("contacts")
            .partition_column("domain_id")
            .create_sql()
            .contains("\"domain_id\" = ")
    );
    assert_eq!(
        RlsPolicy::new("contacts").organizations().organization_name(),
        "contacts_organization_read"
    );
}
