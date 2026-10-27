//! Seeding a [`TestApp`] (feature `testing`).

use navree_db::{Seeder, SeederCalls, run_seeders};
use navree_kernel::{BoxError, BoxFuture, Injectable};
use navree_testing::TestApp;

/// Seeders for a booted [`TestApp`].
///
/// ```no_run
/// # use navree_db::{Db, Seeder};
/// # use navree_kernel::{AppContext, BoxError, BoxFuture, Injectable, KernelError, Resolver};
/// # struct DemoSeeder;
/// # impl Injectable for DemoSeeder {
/// #     fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> { Box::pin(async { Ok(DemoSeeder) }) }
/// # }
/// # impl Seeder for DemoSeeder {
/// #     fn name(&self) -> &'static str { "demo" }
/// #     fn run<'a>(&'a self, _ctx: &'a AppContext, _db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> { Box::pin(async { Ok(()) }) }
/// # }
/// use navree_orm::testing::TestAppExt;
///
/// # async fn demo(app: navree_testing::TestApp) -> Result<(), BoxError> {
/// app.seed::<DemoSeeder>().await?;   // one seeder (registered or not)
/// app.seed_all().await?;             // what `db:seed` runs
/// # Ok(()) }
/// ```
pub trait TestAppExt {
    /// Runs seeder `S` on the app's database: the registered instance, or
    /// one built with its [`Injectable`] constructor.
    fn seed<S: Seeder + Injectable>(&self) -> BoxFuture<'_, Result<(), BoxError>>;

    /// Runs the app's seeders as `db:seed` does (the root seeders, or
    /// every seeder); returns their names.
    fn seed_all(&self) -> BoxFuture<'_, Result<Vec<&'static str>, BoxError>>;
}

impl TestAppExt for TestApp {
    fn seed<S: Seeder + Injectable>(&self) -> BoxFuture<'_, Result<(), BoxError>> {
        self.ctx().call::<S>()
    }

    fn seed_all(&self) -> BoxFuture<'_, Result<Vec<&'static str>, BoxError>> {
        Box::pin(run_seeders(self.ctx(), &[]))
    }
}
