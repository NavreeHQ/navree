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
use serde_json::Value;

use crate::authorizer::AuthorizedRecord;
use crate::engine::{Ability, ActionMarker, SqlColumns, SqlFilter, Subject};
use crate::factory::{AbilityFactory, PrincipalAccess};
use crate::records::SubjectRecords;

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
/// async fn show(Abilities(ability): Abilities, post: Authorized<Post, Read>) -> .. {
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

/// The record of a `can(A, S, from = "id")` route, loaded and authorized
/// by the route authorizer before the handler runs. `S` is any
/// [`SubjectRecords`]: an ORM model declared with `#[model(subject =
/// "...")]` (the record is an ORM record) or a navree-db record that
/// implements it.
///
/// A 500 if the route does not declare that check (action `A::NAME` on
/// subject `S::SUBJECT` with `from`).
pub struct Authorized<S: SubjectRecords, A> {
    model: Arc<S::Record>,
    record: Arc<Value>,
    ability: Arc<Ability>,
    _action: PhantomData<fn() -> A>,
}

impl<S: SubjectRecords, A> Authorized<S, A> {
    pub(crate) fn from_parts(
        model: Arc<S::Record>,
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

impl<S, A> Authorized<S, A>
where
    S: SubjectRecords,
    A: ActionMarker,
{
    /// Loads the record of `S` whose id is `id` in `uow` (`FOR UPDATE`
    /// where supported) and checks `A` on it with `ability`, outside an HTTP
    /// request (MCP tools, agent tools, jobs). A 404 when there is no such
    /// record (or `id` cannot name one), a 403 when `ability` does not allow
    /// `A` on it. `S` needs no
    /// [`AccessModule::for_feature`](crate::AccessModule::for_feature)
    /// registration.
    pub async fn load(uow: &Uow, ability: Arc<Ability>, id: &str) -> Result<Self, AppError> {
        let model = S::load(uow, id)
            .await
            .map_err(AppError::internal)?
            .ok_or_else(AppError::not_found)?;
        Self::check(model, ability)
    }

    /// Checks action `A` on `model` with `ability` outside a route (in an
    /// agent's tool, a job or a service): the record, as conditions see it
    /// ([`SubjectRecords::record_json`]), or a 403 when `ability` does not
    /// allow `A` on it.
    pub fn check(model: S::Record, ability: Arc<Ability>) -> Result<Self, AppError> {
        let record = S::record_json(&model);
        if !ability.can(A::NAME, S::SUBJECT, Some(&record)) {
            return Err(AppError::forbidden());
        }
        Ok(Self {
            model: Arc::new(model),
            record: Arc::new(record),
            ability,
            _action: PhantomData,
        })
    }

    /// The record.
    pub fn into_inner(self) -> S::Record {
        Arc::try_unwrap(self.model).unwrap_or_else(|shared| (*shared).clone())
    }

    /// The record.
    pub fn model(&self) -> &S::Record {
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
    /// (column names) of this record with action `A`; a 403
    /// `forbidden_fields` naming the fields they may not change otherwise.
    pub fn check_fields(&self, changed: &[&str]) -> Result<(), AppError> {
        let denied: Vec<&str> = changed
            .iter()
            .copied()
            .filter(|field| {
                !self
                    .ability
                    .can_field(A::NAME, S::SUBJECT, Some(&self.record), field)
            })
            .collect();
        if denied.is_empty() {
            Ok(())
        } else {
            Err(AppError::forbidden()
                .with_code("forbidden_fields")
                .with_detail(format!(
                    "You may not change these fields: {}.",
                    denied.join(", ")
                )))
        }
    }
}

impl<S: SubjectRecords, A> Deref for Authorized<S, A> {
    type Target = S::Record;

    fn deref(&self) -> &S::Record {
        &self.model
    }
}

impl<S: SubjectRecords, A> Clone for Authorized<S, A> {
    fn clone(&self) -> Self {
        Self {
            model: self.model.clone(),
            record: self.record.clone(),
            ability: self.ability.clone(),
            _action: PhantomData,
        }
    }
}

impl<S: SubjectRecords, A> fmt::Debug for Authorized<S, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Authorized")
            .field("subject", &S::SUBJECT)
            .field("record", &self.record)
            .finish_non_exhaustive()
    }
}

impl<E, A, St> FromRequestParts<St> for Authorized<E, A>
where
    E: SubjectRecords,
    A: ActionMarker,
    St: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &St) -> Result<Self, Self::Rejection> {
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
        record.into_authorized::<E, A>().ok_or_else(missing)
    }
}

/// A list filter: the rows of `E` on which the principal may do `A`. Its
/// SQL is compiled from the same rules as record checks (last match wins),
/// so a record is listed exactly when `can(A, E, record)` holds.
///
/// For an ORM model (`Accessible<Deal, Read>`), navree-orm's
/// `AccessibleQuery` applies it to the model's queries:
///
/// ```ignore
/// async fn index(db: Db, deals: Accessible<Deal, Read>, page: PageQuery) -> .. {
///     Ok(Json(deals.query().paginate(&db, page.page).await?))
/// }
/// ```
///
/// Anywhere else, [`sql_filter`](Accessible::sql_filter) compiles it with a
/// column mapping, and [`SqlFilter::to_statement`] writes it for a backend
/// as a portable fragment:
///
/// ```ignore
/// let columns = TypedColumns::new("deals").with("owner_id", Kind::Uuid);
/// let (sql, values) = deals.sql_filter(&columns).to_statement(uow.backend()).into_parts();
/// let rows = DealRow::select().filter(Filter::all().raw(sql, values)).all(&uow).await?;
/// ```
pub struct Accessible<E, A> {
    ability: Arc<Ability>,
    _marker: PhantomData<fn() -> (E, A)>,
}

impl<E, A> Accessible<E, A>
where
    E: Subject,
    A: ActionMarker,
{
    /// The filter for `ability`.
    pub fn new(ability: Arc<Ability>) -> Self {
        Self {
            ability,
            _marker: PhantomData,
        }
    }

    /// The principal's abilities.
    pub fn ability(&self) -> &Arc<Ability> {
        &self.ability
    }

    /// The list filter with `columns` mapping rule fields onto SQL (see
    /// [`Ability::sql_filter_with`]).
    pub fn sql_filter<C: SqlColumns + ?Sized>(&self, columns: &C) -> SqlFilter {
        self.ability.sql_filter_with(A::NAME, E::SUBJECT, columns)
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
    E: Subject,
    A: ActionMarker,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let access = request_access(parts, None).await?;
        Ok(Self::new(access.ability.clone()))
    }
}
