//! [`Workspaces`]: create, rename, list, transfer and delete workspaces.

use std::fmt;
use std::sync::Arc;

use navree_contracts::ZoneOwner;
use navree_db::{Repo, Uow};
use navree_time::Zone;
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde::Serialize;
use uuid::Uuid;

use crate::core::Core;
use crate::entity::{Member, Workspace, member, workspace};
use crate::events::{MemberAdded, OwnershipTransferred, WorkspaceCreated, WorkspaceDeleted};
use crate::{WorkspaceError, WorkspaceRole};

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
}

/// Creates, renames, lists, transfers and deletes workspaces. Provided and
/// exported by the workspace module.
///
/// Writes take the [`Uow`] to run in (an HTTP request's, or one from
/// `Db::uow` in jobs and commands), go through `navree_db::Repo` (so the
/// audit trail records them) and emit their events once it commits. Reads
/// use `Db::reader`: the current unit of work's open transaction, else the
/// pool.
pub struct Workspaces {
    core: Arc<Core>,
}

impl Workspaces {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }

    /// Creates a workspace owned by `owner`, who joins it with the `owner`
    /// role. A taken or malformed slug is a 400 on `slug`; an empty or long
    /// name a 400 on `name`. Under schema isolation the workspace's schema
    /// is created and migrated in the same transaction.
    pub async fn create(
        &self,
        uow: &Uow,
        owner: Uuid,
        input: NewWorkspace,
    ) -> Result<Workspace, WorkspaceError> {
        let name = check_name(&input.name)?;
        let tx = uow.tx().await?;
        let slug = match input.slug {
            Some(slug) => {
                let slug = slug.trim().to_owned();
                check_slug(&slug)?;
                if slug_taken(&*tx, &slug, None).await? {
                    return Err(WorkspaceError::invalid(
                        "slug",
                        "slug has already been taken",
                    ));
                }
                slug
            }
            None => free_slug(&*tx, &slugify(&name)).await?,
        };
        drop(tx);
        let now = self.core.now();
        let id = Uuid::now_v7();
        let created = Repo::<workspace::Entity>::insert(
            uow,
            workspace::ActiveModel {
                id: Set(id),
                name: Set(name.clone()),
                slug: Set(slug.clone()),
                owner_id: Set(owner),
                timezone: Set(None),
                country: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            },
        )
        .await?;
        Repo::<member::Entity>::insert(
            uow,
            member::ActiveModel {
                id: Set(Uuid::now_v7()),
                workspace_id: Set(id),
                user_id: Set(owner),
                role: Set(WorkspaceRole::Owner.as_str().to_owned()),
                created_at: Set(now),
                updated_at: Set(now),
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
        let current = self.core.workspace_in(&*uow.tx().await?, id).await?;
        let mut row = current.into_active_model();
        row.name = Set(name);
        row.updated_at = Set(self.core.now());
        Ok(Repo::<workspace::Entity>::update(uow, row).await?)
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
        let current = self.core.workspace_in(&*uow.tx().await?, id).await?;
        let mut row = current.into_active_model();
        row.timezone = Set(timezone);
        row.country = Set(country);
        row.updated_at = Set(self.core.now());
        Ok(Repo::<workspace::Entity>::update(uow, row).await?)
    }

    /// Every workspace's id, time zone and country (for navree-time).
    pub(crate) async fn zone_owners(&self) -> Result<Vec<ZoneOwner>, WorkspaceError> {
        let rows: Vec<(Uuid, Option<String>, Option<String>)> = workspace::Entity::find()
            .select_only()
            .column(workspace::Column::Id)
            .column(workspace::Column::Timezone)
            .column(workspace::Column::Country)
            .into_tuple()
            .all(&self.core.db.reader().await)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(id, zone, country)| ZoneOwner { id, zone, country })
            .collect())
    }

    /// The workspaces `user` belongs to, with their role in each, by name.
    pub async fn for_user(&self, user: Uuid) -> Result<Vec<Membership>, WorkspaceError> {
        let rows = member::Entity::find()
            .filter(member::Column::UserId.eq(user))
            .find_also_related(workspace::Entity)
            .order_by_asc(workspace::Column::Name)
            .all(&self.core.db.reader().await)
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(membership, workspace)| {
                workspace.map(|workspace| Membership {
                    role: membership.role(),
                    workspace,
                })
            })
            .collect())
    }

    /// The workspace with `id`.
    pub async fn find(&self, id: Uuid) -> Result<Option<Workspace>, WorkspaceError> {
        Ok(workspace::Entity::find_by_id(id)
            .one(&self.core.db.reader().await)
            .await?)
    }

    /// The workspace with `slug`.
    pub async fn find_by_slug(&self, slug: &str) -> Result<Option<Workspace>, WorkspaceError> {
        Ok(workspace::Entity::find()
            .filter(workspace::Column::Slug.eq(slug))
            .one(&self.core.db.reader().await)
            .await?)
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
        self.core
            .member_in(&self.core.db.reader().await, id, user)
            .await
    }

    /// Makes `to` (already a member) the owner of workspace `id`: they get
    /// the `owner` role, and the previous owner becomes an `admin`.
    pub async fn transfer_ownership(
        &self,
        uow: &Uow,
        id: Uuid,
        to: Uuid,
    ) -> Result<Workspace, WorkspaceError> {
        let tx = uow.tx().await?;
        let current = self.core.workspace_in(&*tx, id).await?;
        let from = current.owner_id;
        if from == to {
            return Ok(current);
        }
        let new_owner = self.core.member_in(&*tx, id, to).await?.ok_or_else(|| {
            WorkspaceError::invalid("user_id", "user_id must be a member of the workspace")
        })?;
        let old_owner = self.core.member_in(&*tx, id, from).await?;
        drop(tx);
        let now = self.core.now();
        set_role(uow, new_owner, WorkspaceRole::Owner, now).await?;
        if let Some(old_owner) = old_owner {
            set_role(uow, old_owner, WorkspaceRole::Admin, now).await?;
        }
        let mut row = current.into_active_model();
        row.owner_id = Set(to);
        row.updated_at = Set(now);
        let updated = Repo::<workspace::Entity>::update(uow, row).await?;
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
        let current = self.core.workspace_in(&*uow.tx().await?, id).await?;
        let slug = current.slug.clone();
        Repo::<workspace::Entity>::delete(uow, current).await?;
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
        Ok(workspace::Entity::find()
            .order_by_asc(workspace::Column::CreatedAt)
            .all(&self.core.db.reader().await)
            .await?)
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
    let mut row = membership.into_active_model();
    row.role = Set(role.as_str().to_owned());
    row.updated_at = Set(now);
    Ok(Repo::<member::Entity>::update(uow, row).await?)
}

/// A trimmed, checked workspace name.
fn check_name(name: &str) -> Result<String, WorkspaceError> {
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
    let timezone = match timezone.map(str::trim).filter(|z| !z.is_empty()) {
        Some(name) => Some(
            Zone::canonical_name(name)
                .ok_or_else(|| {
                    WorkspaceError::invalid(
                        "timezone",
                        "timezone must be a valid IANA time zone name",
                    )
                })?
                .to_owned(),
        ),
        None => None,
    };
    let country = match country.map(str::trim).filter(|c| !c.is_empty()) {
        Some(code) if Zone::for_country(code).is_some() => Some(code.to_ascii_uppercase()),
        Some(_) => {
            return Err(WorkspaceError::invalid(
                "country",
                "country must be a valid ISO 3166-1 alpha-2 country code",
            ));
        }
        None => None,
    };
    Ok((timezone, country))
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
        "workspace".to_owned()
    } else {
        slug
    }
}

async fn slug_taken<C: ConnectionTrait>(
    conn: &C,
    slug: &str,
    except: Option<Uuid>,
) -> Result<bool, WorkspaceError> {
    let mut query = workspace::Entity::find().filter(workspace::Column::Slug.eq(slug));
    if let Some(id) = except {
        query = query.filter(workspace::Column::Id.ne(id));
    }
    Ok(query.one(conn).await?.is_some())
}

/// `base`, or `base-2`, `base-3`, ... whichever is free.
async fn free_slug<C: ConnectionTrait>(conn: &C, base: &str) -> Result<String, WorkspaceError> {
    if !slug_taken(conn, base, None).await? {
        return Ok(base.to_owned());
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !slug_taken(conn, &candidate, None).await? {
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
