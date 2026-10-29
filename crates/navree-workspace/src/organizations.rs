//! [`Organizations`]: create, update, attach and detach workspaces,
//! transfer and delete organizations.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use navree_contracts::ZoneOwner;
use navree_db::Uow;
use navree_orm::{Executor, Model as _, Record};
use serde::Serialize;
use uuid::Uuid;

use crate::core::Core;
use crate::entity::{OrgMember, Organization, Workspace, organization_grant};
use crate::store;
use crate::events::{
    OrgMemberAdded, OrganizationCreated, OrganizationDeleted, OrganizationOwnershipTransferred,
    WorkspaceAttached, WorkspaceDetached,
};
use crate::grants::{attached, org_selector, reached, set_org_selector};
use crate::org_role::SCOPE_ALL;
use crate::workspaces::{
    check_country_code as check_country, check_name, check_slug, check_zone_name as check_timezone,
    slugify_or,
};
use crate::{OrgRole, Selector, WorkspaceError, WorkspaceSelector};

/// The input of [`Organizations::create`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewOrganization {
    /// The display name (1 to 100 characters).
    pub name: String,
    /// The slug; derived from the name when `None` (`Acme Group` →
    /// `acme-group`, with `-2`, `-3`, ... when taken).
    pub slug: Option<String>,
    /// An ISO 3166-1 alpha-2 country code (`GB`, any case), stored
    /// upper-case.
    pub country: Option<String>,
    /// An IANA time zone name (`Europe/London`, any case), stored under
    /// its current name (`Asia/Calcutta` is stored as `Asia/Kolkata`). The
    /// organization's zone is the zone of its workspaces that have none
    /// (navree-time).
    pub timezone: Option<String>,
}

impl NewOrganization {
    /// An organization named `name` with a derived slug.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Sets the slug.
    pub fn slug(mut self, slug: impl Into<String>) -> Self {
        self.slug = Some(slug.into());
        self
    }

    /// Sets the country.
    pub fn country(mut self, country: impl Into<String>) -> Self {
        self.country = Some(country.into());
        self
    }

    /// Sets the time zone.
    pub fn timezone(mut self, timezone: impl Into<String>) -> Self {
        self.timezone = Some(timezone.into());
        self
    }
}

/// The input of [`Organizations::update`]: `None` leaves a field as it is;
/// for `country` and `timezone`, `Some(None)` clears it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrganizationChanges {
    /// The new name.
    pub name: Option<String>,
    /// The new country code, or `Some(None)` to clear it.
    pub country: Option<Option<String>>,
    /// The new time zone, or `Some(None)` to clear it.
    pub timezone: Option<Option<String>>,
}

/// An organization with the role a user has in it and the workspaces they
/// reach ([`Organizations::for_user`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OrgMembership {
    /// The organization.
    #[serde(flatten)]
    pub organization: Organization,
    /// The user's organization role.
    pub role: OrgRole,
    /// The organization's workspaces they reach.
    pub workspaces: WorkspaceSelector,
}

/// Creates, updates, transfers and deletes organizations, and attaches
/// workspaces to them. Provided and exported by the workspace module; its
/// methods answer [`WorkspaceError::FeatureDisabled`] unless
/// `workspace.organizations.enabled` is on.
///
/// A workspace belongs to at most one organization. The organization's
/// members reach all of its workspaces or only some
/// ([`OrgMembers`](crate::OrgMembers)), and enter them with the workspace
/// role their organization role maps to. Writes take the [`Uow`] to run
/// in, record their changes in it (audited) and emit their events once it
/// commits.
pub struct Organizations {
    core: Arc<Core>,
}

impl Organizations {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }

    /// Creates an organization owned by `owner`, who joins it with the
    /// `owner` role. A taken or malformed slug is a 400 on `slug`; a bad
    /// name, country or time zone a 400 on that field.
    pub async fn create(
        &self,
        uow: &Uow,
        owner: Uuid,
        input: NewOrganization,
    ) -> Result<Organization, WorkspaceError> {
        self.core.require_organizations()?;
        let name = check_name(&input.name)?;
        let country = check_country(input.country.as_deref())?;
        let timezone = check_timezone(input.timezone.as_deref())?;
        let slug = match input.slug {
            Some(slug) => {
                let slug = slug.trim().to_owned();
                check_slug(&slug)?;
                if slug_taken(uow, &slug).await? {
                    return Err(WorkspaceError::invalid(
                        "slug",
                        "slug has already been taken",
                    ));
                }
                slug
            }
            None => free_slug(uow, &slugify_or(&name, "organization")).await?,
        };
        let now = self.core.now();
        let id = Uuid::now_v7();
        let created = store::insert(
            uow,
            Organization {
                id,
                name: name.clone(),
                slug: slug.clone(),
                owner_id: owner,
                country,
                timezone,
                created_at: now,
                updated_at: now,
            },
        )
        .await?;
        store::insert(
            uow,
            OrgMember {
                id: Uuid::now_v7(),
                organization_id: id,
                user_id: owner,
                role: OrgRole::Owner.as_str().to_owned(),
                scope: SCOPE_ALL.to_owned(),
                created_at: now,
                updated_at: now,
            },
        )
        .await?;
        self.core
            .emit_after_commit(
                uow,
                OrganizationCreated {
                    organization_id: id,
                    name,
                    slug,
                    owner_id: owner,
                },
            )
            .await;
        self.core
            .emit_after_commit(
                uow,
                OrgMemberAdded {
                    organization_id: id,
                    user_id: owner,
                    role: OrgRole::Owner.as_str().to_owned(),
                    workspaces: Selector::All,
                },
            )
            .await;
        Ok(created)
    }

    /// Renames organization `id`.
    pub async fn rename(
        &self,
        uow: &Uow,
        id: Uuid,
        name: &str,
    ) -> Result<Organization, WorkspaceError> {
        self.update(
            uow,
            id,
            OrganizationChanges {
                name: Some(name.to_owned()),
                ..OrganizationChanges::default()
            },
        )
        .await
    }

    /// Changes organization `id`'s name, country or time zone.
    pub async fn update(
        &self,
        uow: &Uow,
        id: Uuid,
        changes: OrganizationChanges,
    ) -> Result<Organization, WorkspaceError> {
        self.core.require_organizations()?;
        let name = changes.name.as_deref().map(check_name).transpose()?;
        let country = changes
            .country
            .map(|country| check_country(country.as_deref()))
            .transpose()?;
        let timezone = changes
            .timezone
            .map(|timezone| check_timezone(timezone.as_deref()))
            .transpose()?;
        let current = self.core.organization_in(uow, id).await?;
        let now = self.core.now();
        store::update(uow, current, |row| {
            if let Some(name) = name {
                row.name = name;
            }
            if let Some(country) = country {
                row.country = country;
            }
            if let Some(timezone) = timezone {
                row.timezone = timezone;
            }
            row.updated_at = now;
        })
        .await
    }

    /// Attaches `workspace` to organization `org`: the organization's
    /// members reach it from now on. Attaching it again does nothing;
    /// [`WorkspaceError::AlreadyAttached`] (a 409) when it belongs to
    /// another organization.
    pub async fn attach(
        &self,
        uow: &Uow,
        org: Uuid,
        workspace: Uuid,
    ) -> Result<Workspace, WorkspaceError> {
        self.core.require_organizations()?;
        self.core.organization_in(uow, org).await?;
        let current = self.core.workspace_in(uow, workspace).await?;
        match current.organization_id {
            Some(existing) if existing == org => return Ok(current),
            Some(_) => return Err(WorkspaceError::AlreadyAttached),
            None => {}
        }
        let now = self.core.now();
        let attached = store::update(uow, current, |row| {
            row.organization_id = Some(org);
            row.updated_at = now;
        })
        .await?;
        self.core
            .emit_after_commit(
                uow,
                WorkspaceAttached {
                    organization_id: org,
                    workspace_id: workspace,
                },
            )
            .await;
        Ok(attached)
    }

    /// Detaches `workspace` from organization `org`; the grants that named
    /// it go too. The workspace stays, with its own members.
    /// [`WorkspaceError::NotAttached`] (a 404) when it is not attached to
    /// `org`.
    pub async fn detach(
        &self,
        uow: &Uow,
        org: Uuid,
        workspace: Uuid,
    ) -> Result<Workspace, WorkspaceError> {
        self.core.require_organizations()?;
        let current = self.core.workspace_in(uow, workspace).await?;
        if current.organization_id != Some(org) {
            return Err(WorkspaceError::NotAttached);
        }
        type Grant = organization_grant::Model;
        let grants = Grant::query()
            .where_(Grant::ORGANIZATION_ID.eq(org))
            .where_(Grant::WORKSPACE_ID.eq(workspace))
            .get(uow)
            .await?
            .into_models();
        for grant in grants {
            store::delete(uow, grant).await?;
        }
        let detached = detach_row(uow, current, self.core.now()).await?;
        self.core
            .emit_after_commit(
                uow,
                WorkspaceDetached {
                    organization_id: org,
                    workspace_id: workspace,
                },
            )
            .await;
        Ok(detached)
    }

    /// Makes `to` (already a member) the owner of organization `id`: they
    /// get the `owner` role and every workspace, and the previous owner
    /// becomes an `admin`.
    pub async fn transfer_ownership(
        &self,
        uow: &Uow,
        id: Uuid,
        to: Uuid,
    ) -> Result<Organization, WorkspaceError> {
        self.core.require_organizations()?;
        let current = self.core.organization_in(uow, id).await?;
        let from = current.owner_id;
        if from == to {
            return Ok(current);
        }
        let new_owner = self
            .core
            .org_member_in(uow, id, to)
            .await?
            .ok_or_else(|| {
                WorkspaceError::invalid("user_id", "user_id must be a member of the organization")
            })?;
        let old_owner = self.core.org_member_in(uow, id, from).await?;
        let now = self.core.now();
        let new_owner = set_org_role(uow, new_owner, OrgRole::Owner, now).await?;
        set_org_selector(uow, new_owner, &Selector::All, now).await?;
        if let Some(old_owner) = old_owner {
            set_org_role(uow, old_owner, OrgRole::Admin, now).await?;
        }
        let updated = store::update(uow, current, |row| {
            row.owner_id = to;
            row.updated_at = now;
        })
        .await?;
        self.core
            .emit_after_commit(
                uow,
                OrganizationOwnershipTransferred {
                    organization_id: id,
                    from,
                    to,
                },
            )
            .await;
        Ok(updated)
    }

    /// Deletes organization `id` with its members, grants and access
    /// requests. Its workspaces are detached and kept.
    pub async fn delete(&self, uow: &Uow, id: Uuid) -> Result<(), WorkspaceError> {
        self.core.require_organizations()?;
        let current = self.core.organization_in(uow, id).await?;
        let workspaces = attached(uow, id).await?;
        let now = self.core.now();
        let mut detached = Vec::with_capacity(workspaces.len());
        for workspace in workspaces {
            detached.push(workspace.id);
            detach_row(uow, workspace, now).await?;
        }
        let slug = current.slug.clone();
        store::delete(uow, current).await?;
        self.core
            .emit_after_commit(
                uow,
                OrganizationDeleted {
                    organization_id: id,
                    slug,
                    detached,
                },
            )
            .await;
        Ok(())
    }

    /// The organization with `id`.
    pub async fn find(&self, id: Uuid) -> Result<Option<Organization>, WorkspaceError> {
        self.core.require_organizations()?;
        Ok(Organization::find(&*self.core.db, id)
            .await?
            .map(Record::into_inner))
    }

    /// The organization with `slug`.
    pub async fn find_by_slug(&self, slug: &str) -> Result<Option<Organization>, WorkspaceError> {
        self.core.require_organizations()?;
        Ok(Organization::query()
            .where_(Organization::SLUG.eq(slug))
            .first(&*self.core.db)
            .await?
            .map(Record::into_inner))
    }

    /// The organization an identifier names: an id (UUID) or a slug.
    /// [`WorkspaceError::OrganizationNotFound`] when there is none.
    pub async fn resolve(&self, identifier: &str) -> Result<Organization, WorkspaceError> {
        let found = match Uuid::parse_str(identifier) {
            Ok(id) => self.find(id).await?,
            Err(_) => self.find_by_slug(identifier).await?,
        };
        found.ok_or_else(|| WorkspaceError::OrganizationNotFound(identifier.to_owned()))
    }

    /// The membership of `user` in organization `id`, if any.
    pub async fn membership(
        &self,
        id: Uuid,
        user: Uuid,
    ) -> Result<Option<OrgMember>, WorkspaceError> {
        self.core.require_organizations()?;
        self.core.org_member_in(&*self.core.db, id, user).await
    }

    /// The organizations `user` belongs to, with their role and workspaces
    /// in each, by name.
    pub async fn for_user(&self, user: Uuid) -> Result<Vec<OrgMembership>, WorkspaceError> {
        self.core.require_organizations()?;
        let db = &*self.core.db;
        let mut members: HashMap<Uuid, OrgMember> = OrgMember::query()
            .where_(OrgMember::USER_ID.eq(user))
            .get(db)
            .await?
            .into_models()
            .into_iter()
            .map(|member| (member.organization_id, member))
            .collect();
        if members.is_empty() {
            return Ok(Vec::new());
        }
        let organizations = Organization::query()
            .where_key_in(members.keys().copied())
            .order_by(Organization::NAME)
            .order_by(Organization::ID)
            .get(db)
            .await?
            .into_models();
        let mut memberships = Vec::with_capacity(organizations.len());
        for organization in organizations {
            let Some(member) = members.remove(&organization.id) else {
                continue;
            };
            memberships.push(OrgMembership {
                workspaces: org_selector(db, &member).await?,
                role: member.role(),
                organization,
            });
        }
        Ok(memberships)
    }

    /// The workspaces attached to organization `id`, by name.
    pub async fn workspaces(&self, id: Uuid) -> Result<Vec<Workspace>, WorkspaceError> {
        self.core.require_organizations()?;
        attached(&*self.core.db, id).await
    }

    /// The workspaces of organization `id` that `user` reaches as a member
    /// of it, by name (none for non-members).
    pub async fn visible_workspaces(
        &self,
        id: Uuid,
        user: Uuid,
    ) -> Result<Vec<Workspace>, WorkspaceError> {
        self.core.require_organizations()?;
        let db = &*self.core.db;
        let Some(member) = self.core.org_member_in(db, id, user).await? else {
            return Ok(Vec::new());
        };
        let selector = org_selector(db, &member).await?;
        reached(db, id, &selector).await
    }

    /// The selector of organization member `member` and the attached
    /// workspaces it reaches.
    pub(crate) async fn reach(
        &self,
        member: &OrgMember,
    ) -> Result<(WorkspaceSelector, Vec<Workspace>), WorkspaceError> {
        let db = &*self.core.db;
        let selector = org_selector(db, member).await?;
        let workspaces = reached(db, member.organization_id, &selector).await?;
        Ok((selector, workspaces))
    }

    /// Whether organizations are on (`workspace.organizations.enabled`).
    pub(crate) fn enabled(&self) -> bool {
        self.core.config.organizations.enabled
    }

    /// Every organization's id, time zone and country (for navree-time).
    pub(crate) async fn zone_owners(&self) -> Result<Vec<ZoneOwner>, WorkspaceError> {
        Ok(Organization::query()
            .order_by(Organization::CREATED_AT)
            .get(&*self.core.db)
            .await?
            .into_models()
            .into_iter()
            .map(|org| ZoneOwner {
                id: org.id,
                zone: org.timezone,
                country: org.country,
            })
            .collect())
    }

    /// Every organization, by creation.
    pub async fn all(&self) -> Result<Vec<Organization>, WorkspaceError> {
        self.core.require_organizations()?;
        Ok(Organization::query()
            .order_by(Organization::CREATED_AT)
            .get(&*self.core.db)
            .await?
            .into_models())
    }

    /// Scopes `uow` (a unit of work on the app's database, for a job or a
    /// command) to organization `id`, as an organization request is: on
    /// PostgreSQL it sets `navree.organization` for its transaction, which
    /// row-level security policies built with
    /// [`RlsPolicy::organizations`](crate::RlsPolicy::organizations) read.
    pub async fn enter(&self, uow: &Uow, id: Uuid) -> Result<(), WorkspaceError> {
        self.core.require_organizations()?;
        self.core
            .isolation
            .apply_setting(uow, "navree.organization", id)
            .await
    }
}

impl fmt::Debug for Organizations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Organizations").finish_non_exhaustive()
    }
}

/// Clears `workspace`'s organization through `uow`.
async fn detach_row(
    uow: &Uow,
    workspace: Workspace,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Workspace, WorkspaceError> {
    store::update(uow, workspace, |row| {
        row.organization_id = None;
        row.updated_at = now;
    })
    .await
}

/// Sets organization member `member`'s role through `uow`.
pub(crate) async fn set_org_role(
    uow: &Uow,
    member: OrgMember,
    role: OrgRole,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<OrgMember, WorkspaceError> {
    if member.role == role.as_str() {
        return Ok(member);
    }
    store::update(uow, member, |row| {
        row.role = role.as_str().to_owned();
        row.updated_at = now;
    })
    .await
}

async fn slug_taken<E: Executor + ?Sized>(exec: &E, slug: &str) -> Result<bool, WorkspaceError> {
    Ok(Organization::query()
        .where_(Organization::SLUG.eq(slug))
        .exists(exec)
        .await?)
}

/// `base`, or `base-2`, `base-3`, ... whichever is free.
async fn free_slug<E: Executor + ?Sized>(exec: &E, base: &str) -> Result<String, WorkspaceError> {
    if !slug_taken(exec, base).await? {
        return Ok(base.to_owned());
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !slug_taken(exec, &candidate).await? {
            return Ok(candidate);
        }
    }
    let suffix = Uuid::now_v7().simple().to_string();
    Ok(format!("{base}-{}", &suffix[suffix.len() - 8..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countries_are_alpha_2_codes() {
        assert_eq!(check_country(Some(" gb ")).unwrap().as_deref(), Some("GB"));
        assert_eq!(check_country(None).unwrap(), None);
        assert_eq!(check_country(Some("")).unwrap(), None);
        for country in ["GBR", "G1", "XX", "Britain"] {
            assert!(check_country(Some(country)).is_err(), "{country}");
        }
    }

    #[test]
    fn time_zones_are_iana_names_stored_under_their_current_name() {
        for zone in [
            "UTC",
            "Europe/London",
            "America/Argentina/Buenos_Aires",
            "Etc/GMT+5",
            "America/Port-au-Prince",
        ] {
            assert_eq!(check_timezone(Some(zone)).unwrap().as_deref(), Some(zone));
        }
        assert_eq!(
            check_timezone(Some(" asia/calcutta ")).unwrap().as_deref(),
            Some("Asia/Kolkata")
        );
        assert_eq!(check_timezone(Some("  ")).unwrap(), None);
        for zone in [
            "not a zone",
            "/London",
            "Europe//London",
            "Europe/",
            "1/2",
            "Mars/Base",
        ] {
            assert!(check_timezone(Some(zone)).is_err(), "{zone}");
        }
    }
}
