//! [`AccessModule`]: wiring of the access tables, services, authorizer,
//! endpoints and commands.

use std::marker::PhantomData;
use std::sync::Arc;

use navree_contracts::PermissionCheck;
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::{ModuleBuilderHttpExt, RouteAuthorizer};
use navree_kernel::{Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::authorizer::AccessAuthorizer;
use crate::commands::commands;
use crate::controller::{AbilitiesController, AccessAdminController};
use crate::engine::Subject;
use crate::records::{Policy, PolicyRegistration, SubjectRecords};
use crate::entity::role;
use crate::error::{AccessError, map_access_error};
use crate::factory::AbilityFactory;
use crate::migration::migrations;
use crate::options::AccessOptions;
use crate::permission_check::AccessPermissionCheck;
use crate::service::AccessService;
use crate::subject::{SubjectRegistration, Subjects};

/// Roles, permissions and abilities.
///
/// Global; provides and exports [`AccessService`] and [`AbilityFactory`],
/// registers the app's `dyn RouteAuthorizer` ([`AccessAuthorizer`](crate::AccessAuthorizer)),
/// a `dyn PermissionCheck` (so `navree_contracts::permitted` answers with
/// the user's app-wide roles and permissions), the `access` migrations
/// (after `auth`) and the `AccessError` → error mapping. Needs
/// `DatabaseModule`.
///
/// ```no_run
/// use navree_access::{AbilityCtx, AccessModule, AccessOptions, READ, Rules};
/// use navree_kernel::Navree;
///
/// fn define(ctx: &AbilityCtx, rules: &mut Rules) {
///     ctx.grant_permissions(rules);
///     if ctx.user.is_some() {
///         rules.can(READ, "Post");
///     }
/// }
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     // ConfigModule, HttpModule, DatabaseModule, ...
///     .module(AccessModule::for_root(AccessOptions {
///         abilities: define,
///         super_admin_role: Some("admin".into()),
///     }))
///     .run()
/// # }
/// ```
///
/// Imported plainly (`AccessModule`) it reuses the app's configured
/// instance, or uses [`AccessOptions::default`] when there is none.
/// Feature modules register their entities with
/// [`for_feature`](AccessModule::for_feature) so `can(.., from = "id")`
/// routes can load their records.
#[derive(Clone, Copy, Debug, Default)]
pub struct AccessModule;

impl AccessModule {
    /// The module with `options` (configure it once, in the app root).
    pub fn for_root(options: AccessOptions) -> impl Module {
        ConfiguredAccessModule { options }
    }

    /// Registers `S` as a subject: its permission prefix maps to its
    /// subject type, `can(.., S::SUBJECT, from = "id")` routes load its
    /// records ([`SubjectRecords::load`]: an entity by primary key, an ORM
    /// model by route key, in the request's unit of work), and its policy,
    /// if it has one ([`SubjectRecords::policy`]), joins every principal's
    /// rules.
    pub fn for_feature<S: SubjectRecords>() -> impl Module {
        AccessFeature::<S>(PhantomData)
    }

    /// Registers a subject that is not an entity (only its permission
    /// prefix, e.g. `reports` → `Report`).
    pub fn for_subject<S: Subject + Send + Sync + 'static>() -> impl Module {
        AccessSubject::<S>(PhantomData)
    }

    /// Registers policy `P`: its rules join every principal's rules, before
    /// the app's abilities function.
    pub fn for_policy<P: Policy>() -> impl Module {
        AccessPolicy::<P>(PhantomData)
    }
}

impl Module for AccessModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, AccessOptions::default());
    }
}

/// `AccessModule::for_root`: same module id, configured.
struct ConfiguredAccessModule {
    options: AccessOptions,
}

impl Module for ConfiguredAccessModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<AccessModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: AccessOptions) {
    m.global()
        .value(options)
        .contribute(SubjectRegistration::entity::<role::Entity>())
        .factory(
            Factory::<AbilityFactory>::new(|r: Resolver| async move {
                let db = r.get::<Db>().await?;
                let options = r.get::<AccessOptions>().await?;
                let subjects = Subjects::collect(&r.app())?;
                Ok(Arc::new(AbilityFactory::new(
                    db,
                    options,
                    Arc::new(subjects),
                )))
            })
            .deps(vec![
                ProviderKey::of::<Db>(),
                ProviderKey::of::<AccessOptions>(),
            ]),
        )
        .exports::<AbilityFactory>()
        .factory(
            Factory::<AccessService>::new(|r: Resolver| async move {
                let db = r.get::<Db>().await?;
                let factory = r.get::<AbilityFactory>().await?;
                Ok(Arc::new(AccessService::new(
                    db,
                    factory,
                    r.app().downgrade(),
                )))
            })
            .deps(vec![
                ProviderKey::of::<Db>(),
                ProviderKey::of::<AbilityFactory>(),
            ]),
        )
        .exports::<AccessService>()
        .factory(
            Factory::<dyn RouteAuthorizer>::new(|r: Resolver| async move {
                let factory = r.get::<AbilityFactory>().await?;
                Ok(Arc::new(AccessAuthorizer::new(factory)) as Arc<dyn RouteAuthorizer>)
            })
            .deps(vec![ProviderKey::of::<AbilityFactory>()]),
        )
        .exports::<dyn RouteAuthorizer>()
        // Permission questions asked outside a route (`permitted`).
        .multi(
            Factory::<dyn PermissionCheck>::new(|r: Resolver| async move {
                let factory = r.get::<AbilityFactory>().await?;
                Ok(Arc::new(AccessPermissionCheck(factory)) as Arc<dyn PermissionCheck>)
            })
            .deps(vec![ProviderKey::of::<AbilityFactory>()]),
        )
        .controller::<AbilitiesController>()
        .controller::<AccessAdminController>()
        .migrations(migrations())
        .map_error::<AccessError>(map_access_error);
    for command in commands() {
        m.command(command);
    }
}

/// `AccessModule::for_feature::<S>()`.
struct AccessFeature<S>(PhantomData<fn() -> S>);

impl<S: SubjectRecords> Module for AccessFeature<S> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(SubjectRegistration::records::<S>());
        if let Some(define) = S::policy() {
            m.contribute(PolicyRegistration {
                name: std::any::type_name::<S>(),
                define,
            });
        }
    }
}

/// `AccessModule::for_policy::<P>()`.
struct AccessPolicy<P>(PhantomData<fn() -> P>);

impl<P: Policy> Module for AccessPolicy<P> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(PolicyRegistration::of::<P>());
    }
}

/// `AccessModule::for_subject::<S>()`.
struct AccessSubject<S>(PhantomData<fn() -> S>);

impl<S: Subject + Send + Sync + 'static> Module for AccessSubject<S> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(SubjectRegistration::of::<S>());
    }
}
