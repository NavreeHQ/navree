//! [`SubjectRecords`]: subjects whose records routes load and check (SeaORM
//! entities, and ORM models with `#[model(subject = "...")]`), and
//! [`Policy`]: rules attached to a subject.

use std::marker::PhantomData;
use std::sync::Arc;

use navree_db::Uow;
use navree_kernel::{BoxError, BoxFuture};
use sea_orm::{DbErr, EntityTrait};
use serde_json::Value;

use crate::context::AbilityCtx;
use crate::engine::{Rules, Subject};
use crate::subject::{EntityLoader, LoadedRecord, SubjectLoader};

/// A subject whose records can be loaded by the id a route names and
/// checked against ability conditions: what `can(.., from = "id")` routes,
/// [`Authorized<S, A>`](crate::Authorized) and
/// [`AccessModule::for_feature`](crate::AccessModule::for_feature) work
/// with.
///
/// Every SeaORM entity that is a [`Subject`] is one (its records are its
/// models); ORM models get an implementation from `#[derive(Model)]` with
/// `#[model(subject = "...")]` (their records are ORM records).
pub trait SubjectRecords: Subject + Send + Sync + 'static {
    /// A loaded record.
    type Record: Clone + Send + Sync + 'static;

    /// The record whose id (a route's path parameter) is `id`, read in
    /// `uow`'s transaction (locked `FOR UPDATE` where the database supports
    /// it); `None` when there is none or `id` cannot name one.
    fn load<'a>(uow: &'a Uow, id: &'a str)
    -> BoxFuture<'a, Result<Option<Self::Record>, BoxError>>;

    /// The record as ability conditions see it: one member per column,
    /// UUIDs as hyphenated strings, timestamps as RFC 3339 UTC strings with
    /// microseconds (see [`record_json`](crate::record_json)), so a record
    /// check agrees with the SQL list filter.
    fn record_json(record: &Self::Record) -> Value;

    /// The rules attached to the subject ([`Policy::define`]), added to
    /// every principal's rules once the subject is registered with
    /// `AccessModule::for_feature`. None by default.
    fn policy() -> Option<fn(&AbilityCtx, &mut Rules)> {
        None
    }
}

impl<E> SubjectRecords for E
where
    E: EntityTrait + Subject,
    E::Model: Sync,
{
    type Record = E::Model;

    fn load<'a>(
        uow: &'a Uow,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<Self::Record>, BoxError>> {
        Box::pin(async move {
            let loaded = EntityLoader::<E>::new().load(uow, id).await?;
            Ok(loaded.and_then(|record| {
                record
                    .model
                    .downcast::<E::Model>()
                    .ok()
                    .map(|model| (*model).clone())
            }))
        })
    }

    fn record_json(record: &Self::Record) -> Value {
        crate::subject::record_json(record)
    }
}

/// Ability rules about one subject, kept next to it: attach it to an ORM
/// model with `#[model(policy = DealPolicy)]`, or register it with
/// [`AccessModule::for_policy`](crate::AccessModule::for_policy). Its rules
/// are added to every principal's rules before the app's abilities
/// function, so the app can still override them.
///
/// ```
/// use navree_access::{AbilityCtx, Policy, Rules, READ, UPDATE, field};
///
/// pub struct DealPolicy;
///
/// impl Policy for DealPolicy {
///     fn define(ctx: &AbilityCtx, rules: &mut Rules) {
///         let Some(user) = &ctx.user else { return };
///         rules.can(READ, "Deal");
///         rules.can(UPDATE, "Deal").when(field("owner_id").eq(user.id));
///     }
/// }
/// ```
pub trait Policy: Send + Sync + 'static {
    /// Adds the rules for the principal of `ctx`.
    fn define(ctx: &AbilityCtx, rules: &mut Rules);
}

/// A policy registered with the access module (a module contribution).
pub(crate) struct PolicyRegistration {
    pub(crate) name: &'static str,
    pub(crate) define: fn(&AbilityCtx, &mut Rules),
}

impl PolicyRegistration {
    pub(crate) fn of<P: Policy>() -> Self {
        PolicyRegistration {
            name: std::any::type_name::<P>(),
            define: P::define,
        }
    }
}

impl std::fmt::Debug for PolicyRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PolicyRegistration")
            .field(&self.name)
            .finish()
    }
}

/// Loads records of any [`SubjectRecords`] for the route authorizer.
pub(crate) struct RecordsLoader<S>(PhantomData<fn() -> S>);

impl<S> RecordsLoader<S> {
    pub(crate) fn new() -> Self {
        RecordsLoader(PhantomData)
    }
}

impl<S: SubjectRecords> SubjectLoader for RecordsLoader<S> {
    fn load<'a>(
        &'a self,
        uow: &'a Uow,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<LoadedRecord>, DbErr>> {
        Box::pin(async move {
            let record = S::load(uow, id).await.map_err(|error| {
                DbErr::Custom(format!("cannot load {} `{id}`: {error}", S::SUBJECT))
            })?;
            Ok(record.map(|record| LoadedRecord {
                json: S::record_json(&record),
                model: Arc::new(record),
            }))
        })
    }
}
