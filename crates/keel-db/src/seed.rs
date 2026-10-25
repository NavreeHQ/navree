//! [`Seeder`]s and model [`Factory`]s.

use keel_kernel::{AppContext, BoxError, BoxFuture};
use sea_orm::{ActiveModelTrait, ConnectionTrait, DbErr, EntityTrait, IntoActiveModel};

use crate::Db;

/// Fills the database with data (`db:seed`, `migrate:fresh --seed`).
/// Registered with [`ModuleBuilderDbExt::seeder`](crate::ModuleBuilderDbExt::seeder);
/// seeders run in module dependency order, then registration order.
pub trait Seeder: Send + Sync + 'static {
    /// The name `db:seed <name>` selects.
    fn name(&self) -> &'static str;

    /// Seeds the database.
    fn run<'a>(&'a self, ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>>;
}

/// Runs the registered seeders (all of them when `names` is empty, else the
/// named ones), in order; returns the names that ran.
pub async fn run_seeders(
    ctx: &AppContext,
    names: &[String],
) -> Result<Vec<&'static str>, BoxError> {
    let seeders = ctx.get_all::<dyn Seeder>().await?;
    if let Some(unknown) = names
        .iter()
        .find(|name| !seeders.iter().any(|s| s.name() == name.as_str()))
    {
        let known: Vec<&str> = seeders.iter().map(|s| s.name()).collect();
        return Err(format!(
            "unknown seeder `{unknown}`; registered seeders: {}",
            if known.is_empty() {
                "none".to_owned()
            } else {
                known.join(", ")
            }
        )
        .into());
    }
    let db = ctx.get::<Db>().await?;
    let mut ran = Vec::new();
    for seeder in seeders
        .iter()
        .filter(|s| names.is_empty() || names.iter().any(|n| n == s.name()))
    {
        tracing::info!(target: "keel_db::seed", seeder = seeder.name(), "seeding");
        seeder
            .run(ctx, &db)
            .await
            .map_err(|error| format!("seeder `{}` failed: {error}", seeder.name()))?;
        ran.push(seeder.name());
    }
    Ok(ran)
}

/// Builds records of one entity for tests and seeders.
///
/// ```ignore
/// struct UserFactory;
///
/// impl Factory for UserFactory {
///     type Entity = user::Entity;
///
///     fn definition(&self, n: usize) -> user::ActiveModel {
///         user::ActiveModel {
///             id: Set(Uuid::now_v7()),
///             email: Set(format!("user{n}@example.com")),
///             ..Default::default()
///         }
///     }
/// }
///
/// let users = create_many(&UserFactory, db.conn(), 10).await?;
/// ```
pub trait Factory: Send + Sync {
    /// The entity built.
    type Entity: EntityTrait;

    /// The `n`-th record of a batch (`n` counts from 0 in each
    /// [`create_many`] call; use it for unique values).
    fn definition(&self, n: usize) -> <Self::Entity as EntityTrait>::ActiveModel;
}

/// Inserts `n` records from `factory` (definitions `0..n`), in order.
pub async fn create_many<F, C>(
    factory: &F,
    conn: &C,
    n: usize,
) -> Result<Vec<<F::Entity as EntityTrait>::Model>, DbErr>
where
    F: Factory,
    C: ConnectionTrait,
    <F::Entity as EntityTrait>::Model: IntoActiveModel<<F::Entity as EntityTrait>::ActiveModel>,
    <F::Entity as EntityTrait>::ActiveModel: Send,
{
    let mut created = Vec::with_capacity(n);
    for i in 0..n {
        created.push(factory.definition(i).insert(conn).await?);
    }
    Ok(created)
}

/// Inserts one record from `factory` (definition 0).
pub async fn create<F, C>(factory: &F, conn: &C) -> Result<<F::Entity as EntityTrait>::Model, DbErr>
where
    F: Factory,
    C: ConnectionTrait,
    <F::Entity as EntityTrait>::Model: IntoActiveModel<<F::Entity as EntityTrait>::ActiveModel>,
    <F::Entity as EntityTrait>::ActiveModel: Send,
{
    factory.definition(0).insert(conn).await
}
