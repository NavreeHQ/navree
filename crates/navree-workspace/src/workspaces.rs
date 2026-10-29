//! [`Workspaces`]: create, rename, list, transfer and delete workspaces.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use navree_contracts::ZoneOwner;
use navree_db::Uow;
use navree_orm::{Executor, Model as _, Record};
use navree_time::Zone;
use serde::Serialize;
use uuid::Uuid;

use crate::core::Core;
use crate::entity::{Member, OrgMember, Organization, Workspace, WorkspaceKind};
use crate::events::{MemberAdded, OwnershipTransferred, WorkspaceCreated, WorkspaceDeleted};
use crate::grants::{org_grant, org_selector, reached};
use crate::org_role::SCOPE_ALL;
use crate::store;
use crate::{OrgRole, WorkspaceError, WorkspaceRole};

/// The longest workspace name, in characters.
pub const MAX_NAME_LEN: usize = 100;
/// The longest slug, in bytes.
pub const MAX_SLUG_LEN: usize = 63;

/// The input of [`Workspaces::create`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewWorkspace {
    /// The display name (1 to 100 characters).
    pub name: String,
    /// The slug; derived from the name when `None` (`Acme Inc.` → `acme-inc`,
    /// with `-2`, `-3`, ... when taken).
    pub slug: Option<String>,
}

impl NewWorkspace {
    /// A workspace named `name` with a derived slug.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            slug: None,
        }
    }

    /// Sets the slug.
    pub fn slug(mut self, slug: impl Into<String>) -> Self {
        self.slug = Some(slug.into());
        self
    }
}

/// A workspace with the role a user has in it ([`Workspaces::for_user`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Membership {
    /// The workspace.
    #[serde(flatten)]
    pub workspace: Workspace,
    /// The user's role in it.
    pub role: WorkspaceRole,
    /// How the user has it: a membership, or a grant of the workspace's
    /// organization.
    pub granted_by: GrantedBy,
}

/// How a user may enter a workspace: as one of its members, or through the
/// organization the workspace belongs to (organizations on).
///
/// In JSON it is `"membership"` or
/// `{"organization": {"organization_id": "..", "role": "admin"}}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantedBy {
    /// A row of `workspace_members`.
    Membership,
    /// A membership of the workspace's organization whose role maps to a
    /// workspace role and whose workspaces include this one.
    Organization {
        /// The organization.
        organization_id: Uuid,
        /// The organization role that gives the workspace role.
        role: OrgRole,
    },
}

/// A user admitted to a workspace: the membership (built from the
/// organization membership for organization grants), the role and how.
#[derive(Clone, Debug)]
pub(crate) struct Admission {
    pub(crate) member: Member,
    pub(crate) role: WorkspaceRole,
    pub(crate) granted_by: GrantedBy,
}

/// Creates, renames, lists, transfers and deletes workspaces. Provided and
/// exported by the workspace module.
///
/// Writes take the [`Uow`] to run in (an HTTP request's, or one from
/// `Db::uow` in jobs and commands), record their changes in it (so the
/// audit trail sees them) and emit their events once it commits. Reads go
/// where the ORM sends them: the current unit of work's open transaction,
/// else the pool.
pub struct Workspaces {
    core: Arc<Core>,
}

impl Workspaces {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }

    /// Creates a (live) workspace owned by `owner`, who joins it with the
    /// `owner` role. A taken or malformed slug is a 400 on `slug`; an empty
    /// or long name a 400 on `name`. Under schema isolation the workspace's
    /// schema is created and migrated in the same transaction.
    /// [`DemoWorkspaces::create`](crate::DemoWorkspaces::create) makes demo
    /// workspaces.
    pub async fn create(
        &self,
        uow: &Uow,
        owner: Uuid,
        input: NewWorkspace,
    ) -> Result<Workspace, WorkspaceError> {
        self.create_kind(uow, owner, input, WorkspaceKind::Live)
            .await
    }

    /// [`create`](Self::create) for a workspace of `kind`.
    pub(crate) async fn create_kind(
        &self,
        uow: &Uow,
        owner: Uuid,
        input: NewWorkspace,
        kind: WorkspaceKind,
    ) -> Result<Workspace, WorkspaceError> {
        let name = check_name(&input.name)?;
        let slug = match input.slug {
            Some(slug) => {
                let slug = slug.trim().to_owned();
                check_slug(&slug)?;
                if slug_taken(uow, &slug, None).await? {
                    return Err(WorkspaceError::invalid(
                        "slug",
                        "slug has already been taken",
                    ));
                }
                slug
            }
            None => free_slug(uow, &slugify(&name)).await?,
        };
        let now = self.core.now();
        let id = Uuid::now_v7();
        let created = store::insert(
            uow,
            Workspace {
                id,
                name: name.clone(),
                slug: slug.clone(),
                owner_id: owner,
                organization_id: None,
                timezone: None,
                country: None,
                kind: kind.as_str().to_owned(),
                created_at: now,
                updated_at: now,
            },
        )
        .await?;
        store::insert(
            uow,
            Member {
                id: Uuid::now_v7(),
                workspace_id: id,
                user_id: owner,
                role: WorkspaceRole::Owner.as_str().to_owned(),
                partition_scope: SCOPE_ALL.to_owned(),
                created_at: now,
                updated_at: now,
            },
        )
        .await?;
        self.core.isolation.created(uow, id).await?;
        self.core
            .emit_after_commit(
                uow,
                WorkspaceCreated {
                    workspace_id: id,
                    name,
                    slug,
                    owner_id: owner,
                },
            )
            .await;
        self.core
            .emit_after_commit(
                uow,
                MemberAdded {
                    workspace_id: id,
                    user_id: owner,
                    role: WorkspaceRole::Owner.as_str().to_owned(),
                },
            )
            .await;
        Ok(created)
    }

    /// Renames workspace `id`.
    pub async fn rename(
        &self,
        uow: &Uow,
        id: Uuid,
        name: &str,
    ) -> Result<Workspace, WorkspaceError> {
        let name = check_name(name)?;
        let current = self.core.workspace_in(uow, id).await?;
        let now = self.core.now();
        store::update(uow, current, |row| {
            row.name = name;
            row.updated_at = now;
        })
        .await
    }

    /// Sets the time zone and country of workspace `id` (`None` clears
    /// one). The zone is any IANA name, stored under its current name
    /// (`Asia/Calcutta` is stored as `Asia/Kolkata`); the country is an ISO
    /// 3166-1 alpha-2 code, stored upper-case. An unknown zone is a 400 on
    /// `timezone`, an unknown country a 400 on `country`.
    ///
    /// navree-time reads both: a request in this workspace (and every job
    /// run for it) uses the zone, else the country's primary zone, else the
    /// app's default zone.
    pub async fn set_time_zone(
        &self,
        uow: &Uow,
        id: Uuid,
        timezone: Option<&str>,
        country: Option<&str>,
    ) -> Result<Workspace, WorkspaceError> {
        let (timezone, country) = check_time_zone(timezone, country)?;
        let current = self.core.workspace_in(uow, id).await?;
        let now = self.core.now();
        store::update(uow, current, |row| {
            row.timezone = timezone;
            row.country = country;
            row.updated_at = now;
        })
        .await
    }

    /// Every workspace's id, time zone and country (for navree-time), with
    /// [`zone_owner`](Self::zone_owner)'s inheritance.
    pub(crate) async fn zone_owners(&self) -> Result<Vec<ZoneOwner>, WorkspaceError> {
        self.zone_rows(None).await
    }

    /// Workspace `id`'s time zone and country (for navree-time). With
    /// organizations on, a workspace with neither inherits its
    /// organization's.
    pub(crate) async fn zone_owner(&self, id: Uuid) -> Result<Option<ZoneOwner>, WorkspaceError> {
        Ok(self.zone_rows(Some(id)).await?.pop())
    }

    async fn zone_rows(&self, id: Option<Uuid>) -> Result<Vec<ZoneOwner>, WorkspaceError> {
        type Row = (Uuid, Option<String>, Option<String>, Option<Uuid>);
        let db = &*self.core.db;
        let mut query = Workspace::query();
        if let Some(id) = id {
            query = query.where_key(id);
        }
        let rows: Vec<Row> = query
            .get(db)
            .await?
            .into_models()
            .into_iter()
            .map(|w| (w.id, w.timezone, w.country, w.organization_id))
            .collect();
        let unset = |value: &Option<String>| value.as_deref().is_none_or(|v| v.trim().is_empty());
        let inheriting: HashSet<Uuid> = if self.core.config.organizations.enabled {
            rows.iter()
                .filter(|(_, zone, country, _)| unset(zone) && unset(country))
                .filter_map(|(_, _, _, organization)| *organization)
                .collect()
        } else {
            HashSet::new()
        };
        let organizations: HashMap<Uuid, (Option<String>, Option<String>)> =
            if inheriting.is_empty() {
                HashMap::new()
            } else {
                Organization::query()
                    .where_key_in(inheriting)
                    .get(db)
                    .await?
                    .into_models()
                    .into_iter()
                    .map(|org| (org.id, (org.timezone, org.country)))
                    .collect()
            };
        Ok(rows
            .into_iter()
            .map(|(id, zone, country, organization)| {
                match organization.and_then(|org| organizations.get(&org)) {
                    Some((org_zone, org_country)) if unset(&zone) && unset(&country) => ZoneOwner {
                        id,
                        zone: org_zone.clone(),
                        country: org_country.clone(),
                    },
                    _ => ZoneOwner { id, zone, country },
                }
            })
            .collect())
    }

    /// The workspaces `user` may enter, with their role in each, by name:
    /// the ones they belong to and, with organizations on, the ones their
    /// organization memberships reach (a membership of the workspace wins
    /// over an organization grant).
    pub async fn for_user(&self, user: Uuid) -> Result<Vec<Membership>, WorkspaceError> {
        let db = &*self.core.db;
        let roles: HashMap<Uuid, WorkspaceRole> = Member::query()
            .where_(Member::USER_ID.eq(user))
            .get(db)
            .await?
            .into_models()
            .into_iter()
            .map(|membership| (membership.workspace_id, membership.role()))
            .collect();
        let mut memberships: Vec<Membership> = if roles.is_empty() {
            Vec::new()
        } else {
            Workspace::query()
                .where_key_in(roles.keys().copied())
                .order_by(Workspace::NAME)
                .get(db)
                .await?
                .into_models()
                .into_iter()
                .filter_map(|workspace| {
                    roles.get(&workspace.id).map(|role| Membership {
                        role: role.clone(),
                        workspace,
                        granted_by: GrantedBy::Membership,
                    })
                })
                .collect()
        };
        if !self.core.config.organizations.enabled {
            return Ok(memberships);
        }
        let mut seen: HashSet<Uuid> = memberships.iter().map(|m| m.workspace.id).collect();
        let org_memberships = OrgMember::query()
            .where_(OrgMember::USER_ID.eq(user))
            .get(db)
            .await?
            .into_models();
        for org_member in org_memberships {
            let role = org_member.role();
            let Some(mapped) = self.core.org_roles.workspace_role(&role).cloned() else {
                continue;
            };
            let selector = org_selector(db, &org_member).await?;
            for workspace in reached(db, org_member.organization_id, &selector).await? {
                if seen.insert(workspace.id) {
                    memberships.push(Membership {
                        workspace,
                        role: mapped.clone(),
                        granted_by: GrantedBy::Organization {
                            organization_id: org_member.organization_id,
                            role: role.clone(),
                        },
                    });
                }
            }
        }
        memberships.sort_by(|a, b| {
            (&a.workspace.name, a.workspace.id).cmp(&(&b.workspace.name, b.workspace.id))
        });
        Ok(memberships)
    }

    /// The role `user` has in workspace `id` and how: their membership,
    /// else (with organizations on) a grant of the workspace's
    /// organization. `None` when they may not enter it;
    /// [`WorkspaceError::WorkspaceNotFound`] for an unknown workspace.
    pub async fn role_of(
        &self,
        id: Uuid,
        user: Uuid,
    ) -> Result<Option<(WorkspaceRole, GrantedBy)>, WorkspaceError> {
        let workspace = self.core.workspace_in(&*self.core.db, id).await?;
        Ok(self
            .admission(&workspace, user)
            .await?
            .map(|admission| (admission.role, admission.granted_by)))
    }

    /// How `user` enters `workspace`, if they may.
    pub(crate) async fn admission(
        &self,
        workspace: &Workspace,
        user: Uuid,
    ) -> Result<Option<Admission>, WorkspaceError> {
        let db = &*self.core.db;
        if let Some(member) = self.core.member_in(db, workspace.id, user).await? {
            return Ok(Some(Admission {
                role: member.role(),
                member,
                granted_by: GrantedBy::Membership,
            }));
        }
        if !self.core.config.organizations.enabled {
            return Ok(None);
        }
        let Some(grant) = org_grant(&self.core, db, workspace, user).await? else {
            return Ok(None);
        };
        let member = Member {
            id: grant.member.id,
            workspace_id: workspace.id,
            user_id: user,
            role: grant.workspace_role.as_str().to_owned(),
            partition_scope: SCOPE_ALL.to_owned(),
            created_at: grant.member.created_at,
            updated_at: grant.member.updated_at,
        };
        Ok(Some(Admission {
            member,
            role: grant.workspace_role,
            granted_by: GrantedBy::Organization {
                organization_id: grant.member.organization_id,
                role: grant.role,
            },
        }))
    }

    /// The workspace with `id`.
    pub async fn find(&self, id: Uuid) -> Result<Option<Workspace>, WorkspaceError> {
        Ok(Workspace::find(&*self.core.db, id)
            .await?
            .map(Record::into_inner))
    }

    /// The workspace with `slug`.
    pub async fn find_by_slug(&self, slug: &str) -> Result<Option<Workspace>, WorkspaceError> {
        Ok(Workspace::query()
            .where_(Workspace::SLUG.eq(slug))
            .first(&*self.core.db)
            .await?
            .map(Record::into_inner))
    }

    /// The workspace an identifier names: an id (UUID) or a slug.
    /// [`WorkspaceError::WorkspaceNotFound`] when there is none.
    pub async fn resolve(&self, identifier: &str) -> Result<Workspace, WorkspaceError> {
        let found = match Uuid::parse_str(identifier) {
            Ok(id) => self.find(id).await?,
            Err(_) => self.find_by_slug(identifier).await?,
        };
        found.ok_or_else(|| WorkspaceError::WorkspaceNotFound(identifier.to_owned()))
    }

    /// The membership of `user` in workspace `id`, if any.
    pub async fn membership(&self, id: Uuid, user: Uuid) -> Result<Option<Member>, WorkspaceError> {
        self.core.member_in(&*self.core.db, id, user).await
    }

    /// Makes `to` (already a member) the owner of workspace `id`: they get
    /// the `owner` role, and the previous owner becomes an `admin`.
    pub async fn transfer_ownership(
        &self,
        uow: &Uow,
        id: Uuid,
        to: Uuid,
    ) -> Result<Workspace, WorkspaceError> {
        let current = self.core.workspace_in(uow, id).await?;
        let from = current.owner_id;
        if from == to {
            return Ok(current);
        }
        let new_owner = self.core.member_in(uow, id, to).await?.ok_or_else(|| {
            WorkspaceError::invalid("user_id", "user_id must be a member of the workspace")
        })?;
        let old_owner = self.core.member_in(uow, id, from).await?;
        let now = self.core.now();
        set_role(uow, new_owner, WorkspaceRole::Owner, now).await?;
        if let Some(old_owner) = old_owner {
            set_role(uow, old_owner, WorkspaceRole::Admin, now).await?;
        }
        let updated = store::update(uow, current, |row| {
            row.owner_id = to;
            row.updated_at = now;
        })
        .await?;
        self.core
            .emit_after_commit(
                uow,
                OwnershipTransferred {
                    workspace_id: id,
                    from,
                    to,
                },
            )
            .await;
        Ok(updated)
    }

    /// Deletes workspace `id` with its members and invitations. Under
    /// schema isolation its schema is dropped in the same transaction;
    /// under database isolation its pool is closed after the commit and the
    /// database is left in place.
    pub async fn delete(&self, uow: &Uow, id: Uuid) -> Result<(), WorkspaceError> {
        let current = self.core.workspace_in(uow, id).await?;
        let slug = current.slug.clone();
        store::delete(uow, current).await?;
        self.core.isolation.deleted(uow, id).await?;
        self.core
            .emit_after_commit(
                uow,
                WorkspaceDeleted {
                    workspace_id: id,
                    slug,
                },
            )
            .await;
        Ok(())
    }

    /// Scopes `uow` (a unit of work on the app's database, for a job or a
    /// command) to workspace `id`, as a workspace request is: on PostgreSQL
    /// it sets `navree.workspace` (row-level security) and, under schema
    /// isolation, the `search_path` for its transaction. Under database
    /// isolation, use [`WorkspaceDatabases`](crate::WorkspaceDatabases) for
    /// the workspace's own database.
    pub async fn enter(&self, uow: &Uow, id: Uuid) -> Result<(), WorkspaceError> {
        self.core.isolation.apply(uow, id).await
    }

    /// Runs the pending workspace migrations of workspace `id` (schema or
    /// database isolation; nothing under row isolation); returns
    /// `module/name` of each.
    pub async fn migrate(&self, id: Uuid) -> Result<Vec<String>, WorkspaceError> {
        self.core.isolation.migrate(&self.core.db, id).await
    }

    /// Every workspace, by creation (for commands that visit them all).
    pub async fn all(&self) -> Result<Vec<Workspace>, WorkspaceError> {
        Ok(Workspace::query()
            .order_by(Workspace::CREATED_AT)
            .get(&*self.core.db)
            .await?
            .into_models())
    }
}

impl fmt::Debug for Workspaces {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Workspaces").finish_non_exhaustive()
    }
}

/// Sets `membership`'s role through `uow`.
pub(crate) async fn set_role(
    uow: &Uow,
    membership: Member,
    role: WorkspaceRole,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Member, WorkspaceError> {
    store::update(uow, membership, |row| {
        row.role = role.as_str().to_owned();
        row.updated_at = now;
    })
    .await
}

/// A trimmed, checked name (of a workspace, organization or partition).
pub(crate) fn check_name(name: &str) -> Result<String, WorkspaceError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(WorkspaceError::invalid("name", "name should not be empty"));
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(WorkspaceError::invalid(
            "name",
            format!("name must be shorter than or equal to {MAX_NAME_LEN} characters"),
        ));
    }
    Ok(name.to_owned())
}

/// A checked time zone (its current IANA name) and country (upper-case);
/// blank values count as unset.
pub(crate) fn check_time_zone(
    timezone: Option<&str>,
    country: Option<&str>,
) -> Result<(Option<String>, Option<String>), WorkspaceError> {
    Ok((check_zone_name(timezone)?, check_country_code(country)?))
}

/// A checked time zone: its current IANA name (`asia/calcutta` gives
/// `Asia/Kolkata`); blank counts as unset, anything else is a 400 on
/// `timezone`. Workspaces and organizations store zones through it.
pub(crate) fn check_zone_name(timezone: Option<&str>) -> Result<Option<String>, WorkspaceError> {
    match timezone.map(str::trim).filter(|z| !z.is_empty()) {
        Some(name) => Zone::canonical_name(name)
            .map(|name| Some(name.to_owned()))
            .ok_or_else(|| {
                WorkspaceError::invalid("timezone", "timezone must be a valid IANA time zone name")
            }),
        None => Ok(None),
    }
}

/// A checked country: an ISO 3166-1 alpha-2 code that has a time zone,
/// upper-case; blank counts as unset, anything else is a 400 on `country`.
pub(crate) fn check_country_code(country: Option<&str>) -> Result<Option<String>, WorkspaceError> {
    match country.map(str::trim).filter(|c| !c.is_empty()) {
        Some(code) if code.len() == 2 && Zone::for_country(code).is_some() => {
            Ok(Some(code.to_ascii_uppercase()))
        }
        Some(_) => Err(WorkspaceError::invalid(
            "country",
            "country must be a valid ISO 3166-1 alpha-2 country code",
        )),
        None => Ok(None),
    }
}

/// Checks a slug: lower-case ASCII letters and digits in groups separated
/// by single hyphens, 1 to 63 bytes, and not a UUID (the resolvers read
/// UUIDs as ids).
pub(crate) fn check_slug(slug: &str) -> Result<(), WorkspaceError> {
    let valid = !slug.is_empty()
        && slug.len() <= MAX_SLUG_LEN
        && slug.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
    if !valid || Uuid::parse_str(slug).is_ok() {
        return Err(WorkspaceError::invalid(
            "slug",
            "slug must contain only lower-case letters, digits and single hyphens (at most 63 characters)",
        ));
    }
    Ok(())
}

/// A slug derived from `name`: lower-case ASCII letters and digits, other
/// runs as one hyphen, at most 48 bytes; `workspace` when nothing is left.
pub fn slugify(name: &str) -> String {
    slugify_or(name, "workspace")
}

/// [`slugify`] with `fallback` when nothing is left.
pub(crate) fn slugify_or(name: &str, fallback: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-').to_owned();
    if slug.is_empty() || Uuid::parse_str(&slug).is_ok() {
        fallback.to_owned()
    } else {
        slug
    }
}

async fn slug_taken<E: Executor + ?Sized>(
    exec: &E,
    slug: &str,
    except: Option<Uuid>,
) -> Result<bool, WorkspaceError> {
    let mut query = Workspace::query().where_(Workspace::SLUG.eq(slug));
    if let Some(id) = except {
        query = query.where_key_not(id);
    }
    Ok(query.exists(exec).await?)
}

/// `base`, or `base-2`, `base-3`, ... whichever is free.
async fn free_slug<E: Executor + ?Sized>(exec: &E, base: &str) -> Result<String, WorkspaceError> {
    if !slug_taken(exec, base, None).await? {
        return Ok(base.to_owned());
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !slug_taken(exec, &candidate, None).await? {
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
    fn slugs_are_derived_from_names() {
        assert_eq!(slugify("Acme Inc."), "acme-inc");
        assert_eq!(slugify("  Zürich -- Sales  "), "z-rich-sales");
        assert_eq!(slugify("???"), "workspace");
        assert_eq!(slugify(&"a".repeat(80)).len(), 48);
    }

    #[test]
    fn slugs_are_checked() {
        assert!(check_slug("acme").is_ok());
        assert!(check_slug("acme-2").is_ok());
        assert!(check_slug("Acme").is_err());
        assert!(check_slug("acme--x").is_err());
        assert!(check_slug("-acme").is_err());
        assert!(check_slug("").is_err());
        assert!(check_slug(&"a".repeat(64)).is_err());
        assert!(check_slug("0199a5b2-0000-7000-8000-000000000001").is_err());
    }

    #[test]
    fn names_are_trimmed_and_checked() {
        assert_eq!(check_name("  Acme ").unwrap(), "Acme");
        assert!(check_name("   ").is_err());
        assert!(check_name(&"x".repeat(101)).is_err());
    }
}
