//! [`Partitions`]: named slices of a workspace's data, and the partitions
//! each member sees.

use std::fmt;
use std::sync::Arc;

use navree_db::Uow;
use navree_orm::{Executor, Model as _, Record};
use uuid::Uuid;

use crate::core::Core;
use crate::entity::{Member, Partition};
use crate::events::{PartitionAccessChanged, PartitionCreated, PartitionDeleted};
use crate::grants::{partition_selector, set_partition_selector};
use crate::store;
use crate::workspaces::{check_name, check_slug, slugify_or};
use crate::{PartitionSelector, WorkspaceError, WorkspaceRole};

/// The input of [`Partitions::create`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewPartition {
    /// The display name (1 to 100 characters), such as a domain
    /// (`acme.co.uk`) or a region.
    pub name: String,
    /// The slug, unique in the workspace; derived from the name when `None`
    /// (`acme.co.uk` → `acme-co-uk`, with `-2`, `-3`, ... when taken).
    pub slug: Option<String>,
}

impl NewPartition {
    /// A partition named `name` with a derived slug.
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

/// Creates, renames and deletes the partitions of a workspace, and sets
/// which partitions each member sees. Provided and exported by the
/// workspace module; its methods answer [`WorkspaceError::FeatureDisabled`]
/// unless `workspace.partitions.enabled` is on.
///
/// A partition is a named slice of a workspace's data: tables that are
/// [`PartitionScoped`](crate::PartitionScoped) carry a `partition_id`, and
/// [`PartitionScope`](crate::PartitionScope) and
/// [`PartitionRepo`](crate::PartitionRepo) keep each member to the
/// partitions they see: `All` (the default) or `Only` the granted ones.
/// The workspace owner and members who enter through an organization see
/// every partition.
pub struct Partitions {
    core: Arc<Core>,
}

impl Partitions {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }

    /// Creates a partition in `workspace`. A taken or malformed slug is a
    /// 400 on `slug`; an empty or long name a 400 on `name`.
    pub async fn create(
        &self,
        uow: &Uow,
        workspace: Uuid,
        input: NewPartition,
    ) -> Result<Partition, WorkspaceError> {
        self.core.require_partitions()?;
        let name = check_name(&input.name)?;
        self.core.workspace_in(uow, workspace).await?;
        let slug = match input.slug {
            Some(slug) => {
                let slug = slug.trim().to_owned();
                check_slug(&slug)?;
                if slug_taken(uow, workspace, &slug).await? {
                    return Err(WorkspaceError::invalid(
                        "slug",
                        "slug has already been taken",
                    ));
                }
                slug
            }
            None => free_slug(uow, workspace, &slugify_or(&name, "partition")).await?,
        };
        let now = self.core.now();
        let created = store::insert(
            uow,
            Partition {
                id: Uuid::now_v7(),
                workspace_id: workspace,
                name: name.clone(),
                slug: slug.clone(),
                created_at: now,
                updated_at: now,
            },
        )
        .await?;
        self.core
            .emit_after_commit(
                uow,
                PartitionCreated {
                    workspace_id: workspace,
                    partition_id: created.id,
                    name,
                    slug,
                },
            )
            .await;
        Ok(created)
    }

    /// Renames partition `id` of `workspace` (the slug stays).
    pub async fn rename(
        &self,
        uow: &Uow,
        workspace: Uuid,
        id: Uuid,
        name: &str,
    ) -> Result<Partition, WorkspaceError> {
        self.core.require_partitions()?;
        let name = check_name(name)?;
        let current = self.partition_in(uow, workspace, id).await?;
        let now = self.core.now();
        store::update(uow, current, |row| {
            row.name = name;
            row.updated_at = now;
        })
        .await
    }

    /// Deletes partition `id` of `workspace` with its grants. Rows of
    /// partition-scoped tables keep their `partition_id`; delete or move
    /// them first.
    pub async fn delete(&self, uow: &Uow, workspace: Uuid, id: Uuid) -> Result<(), WorkspaceError> {
        self.core.require_partitions()?;
        let current = self.partition_in(uow, workspace, id).await?;
        let slug = current.slug.clone();
        store::delete(uow, current).await?;
        self.core
            .emit_after_commit(
                uow,
                PartitionDeleted {
                    workspace_id: workspace,
                    partition_id: id,
                    slug,
                },
            )
            .await;
        Ok(())
    }

    /// The partitions of `workspace`, by name.
    pub async fn list(&self, workspace: Uuid) -> Result<Vec<Partition>, WorkspaceError> {
        self.core.require_partitions()?;
        Ok(Partition::query()
            .where_(Partition::WORKSPACE_ID.eq(workspace))
            .order_by(Partition::NAME)
            .order_by(Partition::ID)
            .get(&*self.core.db)
            .await?
            .into_models())
    }

    /// The partitions of `workspace` that `selector` sees, by name.
    pub async fn visible(
        &self,
        workspace: Uuid,
        selector: &PartitionSelector,
    ) -> Result<Vec<Partition>, WorkspaceError> {
        Ok(self
            .list(workspace)
            .await?
            .into_iter()
            .filter(|partition| selector.includes(partition.id))
            .collect())
    }

    /// Partition `id` of `workspace`.
    pub async fn find(
        &self,
        workspace: Uuid,
        id: Uuid,
    ) -> Result<Option<Partition>, WorkspaceError> {
        self.core.require_partitions()?;
        Ok(Partition::query()
            .where_key(id)
            .where_(Partition::WORKSPACE_ID.eq(workspace))
            .first(&*self.core.db)
            .await?
            .map(Record::into_inner))
    }

    /// The partition of `workspace` an identifier names: an id (UUID) or a
    /// slug. [`WorkspaceError::PartitionNotFound`] when there is none.
    pub async fn resolve(
        &self,
        workspace: Uuid,
        identifier: &str,
    ) -> Result<Partition, WorkspaceError> {
        self.core.require_partitions()?;
        let mut select = Partition::query().where_(Partition::WORKSPACE_ID.eq(workspace));
        select = match Uuid::parse_str(identifier) {
            Ok(id) => select.where_key(id),
            Err(_) => select.where_(Partition::SLUG.eq(identifier)),
        };
        select
            .first(&*self.core.db)
            .await?
            .map(Record::into_inner)
            .ok_or_else(|| WorkspaceError::PartitionNotFound(identifier.to_owned()))
    }

    /// The partitions member `user` of `workspace` sees.
    /// [`WorkspaceError::MemberNotFound`] if they are not a member.
    pub async fn access(
        &self,
        workspace: Uuid,
        user: Uuid,
    ) -> Result<PartitionSelector, WorkspaceError> {
        self.core.require_partitions()?;
        let db = &*self.core.db;
        let member = self
            .core
            .member_in(db, workspace, user)
            .await?
            .ok_or(WorkspaceError::MemberNotFound)?;
        partition_selector(db, &member).await
    }

    /// The partitions membership `member` sees.
    pub(crate) async fn member_access(
        &self,
        member: &Member,
    ) -> Result<PartitionSelector, WorkspaceError> {
        partition_selector(&*self.core.db, member).await
    }

    /// Sets the partitions member `user` of `workspace` sees: `All`, or
    /// `Only` the listed partitions (which must be the workspace's, a 400
    /// on `partitions` otherwise). [`WorkspaceError::MemberNotFound`] if
    /// they are not a member; [`WorkspaceError::OwnerRequired`] when limiting
    /// the workspace's owner.
    pub async fn grant(
        &self,
        uow: &Uow,
        workspace: Uuid,
        user: Uuid,
        partitions: PartitionSelector,
    ) -> Result<Member, WorkspaceError> {
        self.core.require_partitions()?;
        self.core.workspace_in(uow, workspace).await?;
        let member = self
            .core
            .member_in(uow, workspace, user)
            .await?
            .ok_or(WorkspaceError::MemberNotFound)?;
        if let Some(ids) = partitions.ids() {
            let known: Vec<Uuid> = Partition::query()
                .where_(Partition::WORKSPACE_ID.eq(workspace))
                .get(uow)
                .await?
                .into_models()
                .iter()
                .map(|p| p.id)
                .collect();
            if !ids.iter().all(|id| known.contains(id)) {
                return Err(WorkspaceError::invalid(
                    "partitions",
                    "partitions must list partitions of the workspace",
                ));
            }
        }
        if member.role() == WorkspaceRole::Owner {
            if partitions.is_all() {
                return Ok(member);
            }
            return Err(WorkspaceError::OwnerRequired);
        }
        let changed = set_partition_selector(uow, member, &partitions, self.core.now()).await?;
        self.core
            .emit_after_commit(
                uow,
                PartitionAccessChanged {
                    workspace_id: workspace,
                    user_id: user,
                    partitions,
                },
            )
            .await;
        Ok(changed)
    }

    /// Scopes `uow` (a unit of work on the app's database, for a job or a
    /// command) to partition `id`, as a request naming it is: on PostgreSQL
    /// it sets `navree.partition` for its transaction, which row-level
    /// security policies built with
    /// [`RlsPolicy::partitioned`](crate::RlsPolicy::partitioned) read. Enter
    /// the workspace too ([`Workspaces::enter`](crate::Workspaces::enter)).
    pub async fn enter(&self, uow: &Uow, id: Uuid) -> Result<(), WorkspaceError> {
        self.core.require_partitions()?;
        self.core
            .isolation
            .apply_setting(uow, "navree.partition", id)
            .await
    }

    async fn partition_in<E: Executor + ?Sized>(
        &self,
        exec: &E,
        workspace: Uuid,
        id: Uuid,
    ) -> Result<Partition, WorkspaceError> {
        Partition::query()
            .where_key(id)
            .where_(Partition::WORKSPACE_ID.eq(workspace))
            .first(exec)
            .await?
            .map(Record::into_inner)
            .ok_or_else(|| WorkspaceError::PartitionNotFound(id.to_string()))
    }
}

impl fmt::Debug for Partitions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Partitions").finish_non_exhaustive()
    }
}

async fn slug_taken<E: Executor + ?Sized>(
    exec: &E,
    workspace: Uuid,
    slug: &str,
) -> Result<bool, WorkspaceError> {
    Ok(Partition::query()
        .where_(Partition::WORKSPACE_ID.eq(workspace))
        .where_(Partition::SLUG.eq(slug))
        .exists(exec)
        .await?)
}

/// `base`, or `base-2`, `base-3`, ... whichever is free in `workspace`.
async fn free_slug<E: Executor + ?Sized>(
    exec: &E,
    workspace: Uuid,
    base: &str,
) -> Result<String, WorkspaceError> {
    if !slug_taken(exec, workspace, base).await? {
        return Ok(base.to_owned());
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !slug_taken(exec, workspace, &candidate).await? {
            return Ok(candidate);
        }
    }
    let suffix = Uuid::now_v7().simple().to_string();
    Ok(format!("{base}-{}", &suffix[suffix.len() - 8..]))
}
