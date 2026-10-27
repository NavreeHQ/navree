//! Extractors: [`Abilities`], [`CurrentAccess`], [`Authorized`] and
//! [`Accessible`].

use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use http::request::Parts;
use navree_contracts::AuthState;
use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::AppContext;
use sea_orm::{EntityTrait, QueryFilter, Select};
use sea_query::SimpleExpr;
use serde_json::Value;

use crate::authorizer::AuthorizedRecord;
use crate::engine::{Ability, ActionMarker, Subject};
use crate::factory::{AbilityFactory, PrincipalAccess};
use crate::subject::{EntityColumns, EntityLoader, SubjectLoader};

/// The current principal's access, computed once per request (by the route
/// authorizer or the first extractor that needs it) and kept in the request
/// extensions.
pub(crate) async fn request_access(
    parts: &mut Parts,
    factory: Option<&AbilityFactory>,
) -> Result<Arc<PrincipalAccess>, AppError> {
    if let Some(access) = parts.extensions.get::<Arc<PrincipalAccess>>() {
        return Ok(access.clone());
    }
    let principal = parts
        .extensions
        .get::<AuthState>()
        .and_then(|state| state.0.clone());
    let access = match factory {
        Some(factory) => factory.access_for(principal.as_ref()).await,
        None => {
            let ctx = parts
                .extensions
                .get::<AppContext>()
                .cloned()
                .ok_or_else(|| {
                    AppError::internal("access extractors need the navree request pipeline")
                })?;
            let factory = ctx
                .get::<AbilityFactory>()
                .await
                .map_err(AppError::internal)?;
            factory.access_for(principal.as_ref()).await
        }
    }
    .map_err(AppError::internal)?;
    parts.extensions.insert(access.clone());
    Ok(access)
}

/// The current principal's abilities (a guest's for unauthenticated
/// requests).
///
/// ```ignore
/// async fn show(Abilities(ability): Abilities, post: Authorized<post::Entity, Read>) -> .. {
///     let can_delete = ability.can(DELETE, "Post", Some(post.record()));
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Abilities(pub Arc<Ability>);

impl Deref for Abilities {
    type Target = Ability;

    fn deref(&self) -> &Ability {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Abilities {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Abilities(
            request_access(parts, None).await?.ability.clone(),
        ))
    }
}

/// The current principal's roles, permissions, access version and
/// abilities.
#[derive(Clone, Debug)]
pub struct CurrentAccess(pub Arc<PrincipalAccess>);

impl Deref for CurrentAccess {
    type Target = PrincipalAccess;

    fn deref(&self) -> &PrincipalAccess {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentAccess {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(CurrentAccess(request_access(parts, None).await?))
    }
}

/// The record of a `can(A, E, from = "id")` route, loaded and authorized
/// by the route authorizer before the handler runs.
///
/// A 500 if the route does not declare that check (action `A::NAME` on
/// subject `E::SUBJECT` with `from`).
pub struct Authorized<E: EntityTrait, A> {
    model: Arc<E::Model>,
    record: Arc<Value>,
    ability: Arc<Ability>,
    _action: PhantomData<fn() -> A>,
}

impl<E: EntityTrait, A> Authorized<E, A> {
    pub(crate) fn from_parts(
        model: Arc<E::Model>,
        record: Arc<Value>,
        ability: Arc<Ability>,
    ) -> Self {
        Self {
            model,
            record,
            ability,
            _action: PhantomData,
        }
    }
}

impl<E, A> Authorized<E, A>
where
    E: EntityTrait + Subject,
    E::Model: Send + Sync + 'static,
    A: ActionMarker,
{
    /// Loads the record of `E` whose primary key is `id` in `uow` (`FOR
    /// UPDATE` where supported) and checks `A` on it with `ability`, outside
    /// an HTTP request (MCP tools, agent tools, jobs). A 404 when there is no
    /// such record (or `id` cannot be a key), a 403 when `ability` does not
    /// allow `A` on it. `E` needs no
    /// [`AccessModule::for_feature`](crate::AccessModule::for_feature)
    /// registration.
    pub async fn load(uow: &Uow, ability: Arc<Ability>, id: &str) -> Result<Self, AppError> {
        let record = EntityLoader::<E>::new()
            .load(uow, id)
            .await
            .map_err(AppError::internal)?
            .ok_or_else(AppError::not_found)?;
        if !ability.can(A::NAME, E::SUBJECT, Some(&record.json)) {
            return Err(AppError::forbidden());
        }
        let model = record
            .model
            .downcast::<E::Model>()
            .map_err(|_| AppError::internal("the loaded record is not the entity's model"))?;
        Ok(Self::from_parts(model, Arc::new(record.json), ability))
    }
}

impl<E, A> Authorized<E, A>
where
    E: EntityTrait + Subject,
    A: ActionMarker,
{
    /// The model.
    pub fn into_inner(self) -> E::Model {
        Arc::try_unwrap(self.model).unwrap_or_else(|shared| (*shared).clone())
    }

    /// The model.
    pub fn model(&self) -> &E::Model {
        &self.model
    }

    /// The record as conditions see it (see [`record_json`](crate::record_json)).
    pub fn record(&self) -> &Value {
        &self.record
    }

    /// The principal's abilities.
    pub fn ability(&self) -> &Arc<Ability> {
        &self.ability
    }

    /// Checks that the principal may change each of the `changed` fields
    /// (column names) of this record with action `A`; a 403 naming the
    /// fields they may not change otherwise.
    pub fn check_fields(&self, changed: &[&str]) -> Result<(), AppError> {
        let denied: Vec<&str> = changed
            .iter()
            .copied()
            .filter(|field| {
                !self
                    .ability
                    .can_field(A::NAME, E::SUBJECT, Some(&self.record), field)
            })
            .collect();
        if denied.is_empty() {
            Ok(())
        } else {
            Err(AppError::forbidden().with_detail(format!(
                "You may not change these fields: {}.",
                denied.join(", ")
            )))
        }
    }
}

impl<E: EntityTrait, A> Deref for Authorized<E, A> {
    type Target = E::Model;

    fn deref(&self) -> &E::Model {
        &self.model
    }
}

impl<E: EntityTrait, A> Clone for Authorized<E, A> {
    fn clone(&self) -> Self {
        Self {
            model: self.model.clone(),
            record: self.record.clone(),
            ability: self.ability.clone(),
            _action: PhantomData,
        }
    }
}

impl<E: EntityTrait, A> fmt::Debug for Authorized<E, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Authorized")
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl<E, A, S> FromRequestParts<S> for Authorized<E, A>
where
    E: EntityTrait + Subject,
    E::Model: Sync,
    A: ActionMarker,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let missing = || {
            AppError::internal(format!(
                "Authorized<{}, {}> needs the route to declare `can({}, {}, from = ..)`",
                E::SUBJECT,
                A::NAME,
                A::NAME,
                E::SUBJECT
            ))
        };
        let record = parts
            .extensions
            .get::<AuthorizedRecord>()
            .cloned()
            .ok_or_else(missing)?;
        if record.subject != E::SUBJECT || record.action != A::NAME {
            return Err(missing());
        }
        let model = record.model.downcast::<E::Model>().map_err(|_| missing())?;
        Ok(Self::from_parts(model, record.json, record.ability))
    }
}

/// A list filter: the rows of `E` on which the principal may do `A`. Its
/// SQL is compiled from the same rules as record checks (last match wins),
/// so a record is listed exactly when `can(A, E, record)` holds.
///
/// ```ignore
/// async fn index(uow: Uow, posts: Accessible<post::Entity, Read>, page: PageQuery) -> .. {
///     let select = posts.apply(post::Entity::find());
///     Ok(paginate(select, &*uow.tx().await?, page).await?)
/// }
/// ```
pub struct Accessible<E, A> {
    ability: Arc<Ability>,
    _marker: PhantomData<fn() -> (E, A)>,
}

impl<E, A> Accessible<E, A>
where
    E: EntityTrait + Subject,
    A: ActionMarker,
{
    /// The filter for `ability`.
    pub fn new(ability: Arc<Ability>) -> Self {
        Self {
            ability,
            _marker: PhantomData,
        }
    }

    /// The SQL condition (see [`Ability::sql_filter_with`] and
    /// [`EntityColumns`]).
    pub fn condition(&self) -> SimpleExpr {
        self.ability
            .sql_filter_with(A::NAME, E::SUBJECT, &EntityColumns::<E>::new())
    }

    /// `select` restricted to the accessible rows.
    pub fn apply(self, select: Select<E>) -> Select<E> {
        let condition = self.condition();
        select.filter(condition)
    }

    /// The principal's abilities.
    pub fn ability(&self) -> &Arc<Ability> {
        &self.ability
    }
}

impl<E, A> Clone for Accessible<E, A> {
    fn clone(&self) -> Self {
        Self {
            ability: self.ability.clone(),
            _marker: PhantomData,
        }
    }
}

impl<E, A> fmt::Debug for Accessible<E, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Accessible").finish_non_exhaustive()
    }
}

impl<E, A, S> FromRequestParts<S> for Accessible<E, A>
where
    E: EntityTrait + Subject,
    A: ActionMarker,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let access = request_access(parts, None).await?;
        Ok(Self::new(access.ability.clone()))
    }
}
