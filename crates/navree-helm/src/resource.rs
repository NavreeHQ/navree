//! The [`Resource`] trait and [`FormData`].

use axum::extract::FromRequest;
use navree_access::Subject;
use navree_http::{AppError, Json};
use navree_kernel::{AppContext, BoxFuture};
use navree_validation::Valid;
use sea_orm::{EntityTrait, IntoActiveModel, Select};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::{Action, Field, Filter, Metric, SaveContext, save};

/// The model of resource `R`'s entity.
pub type ModelOf<R> = <<R as Resource>::Entity as EntityTrait>::Model;

/// One entity in the admin panel: which fields to show and edit, how to
/// filter and search it, which actions run on it and which numbers
/// matter. Register it with `m.resource::<R>()`
/// ([`ModuleBuilderHelmExt::resource`](crate::ModuleBuilderHelmExt::resource)),
/// which also registers `R` as a provider of the module.
///
/// ```ignore
/// #[injectable]
/// pub struct DealResource;
///
/// impl Resource for DealResource {
///     type Entity = deal::Entity;
///
///     const LABEL: &'static str = "Deals";
///     const SINGULAR: &'static str = "Deal";
///     const TITLE: &'static str = "title";
///     const SEARCH: &'static [&'static str] = &["title"];
///
///     fn fields(&self) -> Vec<Field> {
///         vec![Field::text("title").required().sortable()]
///     }
/// }
/// ```
///
/// Every screen starts from [`query`](Resource::query), then applies the
/// user's access filter, so a resource never shows a row its rules hide.
/// Writes go through [`create`](Resource::create),
/// [`update`](Resource::update) and [`delete`](Resource::delete), which by
/// default write the form's values to the entity's columns with
/// `navree_db::Repo` in the request's unit of work (audited).
pub trait Resource: Send + Sync + 'static {
    /// The SeaORM entity. It implements navree-access's `Subject`, which it
    /// already does if routes check it with `#[can]`.
    type Entity: EntityTrait<
            Model: Serialize
                       + IntoActiveModel<<Self::Entity as EntityTrait>::ActiveModel>
                       + Clone
                       + Send
                       + Sync,
            ActiveModel: Send + Sync,
        > + Subject
        + Send
        + Sync;

    /// The plural name on screen (`"Deals"`).
    const LABEL: &'static str;
    /// The singular name on screen (`"Deal"`); defaults to `LABEL`.
    const SINGULAR: &'static str = Self::LABEL;
    /// The menu group; resources without one go under **Resources**.
    const GROUP: &'static str = "";
    /// A Lucide icon name for custom front ends (the metadata API's `icon`).
    const ICON: &'static str = "";
    /// The column that names a record in headings, breadcrumbs and
    /// relationship fields; defaults to `id`.
    const TITLE: &'static str = "id";
    /// The columns the search box matches, case-insensitively.
    const SEARCH: &'static [&'static str] = &[];
    /// The path segment (`/admin/resources/deals`); defaults to the table
    /// name.
    const SLUG: &'static str = "";
    /// Rows per page on the list; `0` (the default) uses `helm.per_page`.
    const PER_PAGE: u64 = 0;
    /// The list's default order, a sortable column with a leading `-` for
    /// descending (`"-amount_cents"`); defaults to newest first
    /// (`-created_at` when there is such a column, else the primary key).
    const DEFAULT_SORT: &'static str = "";
    /// The position in the menu within the group (lower first, then by
    /// label).
    const ORDER: i32 = 0;

    /// The fields, in display order.
    fn fields(&self) -> Vec<Field>;

    /// The filters of the list's side panel.
    fn filters(&self) -> Vec<Filter<Self::Entity>> {
        Vec::new()
    }

    /// The actions on the list and on records.
    fn actions(&self) -> Vec<Action<Self::Entity>> {
        Vec::new()
    }

    /// The metric cards.
    fn metrics(&self) -> Vec<Metric<Self::Entity>> {
        Vec::new()
    }

    /// The base query of every screen, metric and relationship picker
    /// (hide soft-deleted rows here). Helm applies the user's access filter
    /// after it, so it never widens what a user may see.
    fn query(&self, select: Select<Self::Entity>) -> Select<Self::Entity> {
        select
    }

    /// Creates a record from a create form (already checked against the
    /// fields' rules). By default: a new active model with the form's
    /// values, a UUID v7 primary key when the key is a UUID, `created_at`
    /// and `updated_at` set to now when the entity has them, inserted with
    /// `Repo::insert`.
    fn create<'a>(
        &'a self,
        ctx: &'a SaveContext,
        form: FormData,
    ) -> BoxFuture<'a, Result<ModelOf<Self>, AppError>> {
        Box::pin(save::create::<Self::Entity>(ctx, form))
    }

    /// Updates `record` from an edit form (the submitted fields only). By
    /// default: the form's values set on the record, `updated_at` set to
    /// now when the entity has it, saved with `Repo::update`.
    fn update<'a>(
        &'a self,
        ctx: &'a SaveContext,
        record: ModelOf<Self>,
        form: FormData,
    ) -> BoxFuture<'a, Result<ModelOf<Self>, AppError>> {
        Box::pin(save::update::<Self::Entity>(ctx, record, form))
    }

    /// Deletes `record`. By default with `Repo::delete`.
    fn delete<'a>(
        &'a self,
        ctx: &'a SaveContext,
        record: ModelOf<Self>,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(save::delete::<Self::Entity>(ctx, record))
    }
}

/// The values of a submitted form: one per field, keyed by the field's
/// name, already checked against the fields' rules. Money is in minor
/// units, dates are `YYYY-MM-DD`, date-times RFC 3339 and booleans
/// `true`/`false`.
#[derive(Clone, Debug, Default)]
pub struct FormData {
    values: Map<String, Value>,
    app: Option<AppContext>,
    tz_offset: Option<i32>,
}

impl FormData {
    /// Form data holding `values`.
    pub fn new(values: Map<String, Value>) -> Self {
        Self {
            values,
            app: None,
            tz_offset: None,
        }
    }

    pub(crate) fn with_app(mut self, app: AppContext, tz_offset: Option<i32>) -> Self {
        self.app = Some(app);
        self.tz_offset = tz_offset;
        self
    }

    /// The submitted values.
    pub fn values(&self) -> &Map<String, Value> {
        &self.values
    }

    /// The value of field `name`, if it was submitted.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }

    /// Whether field `name` was submitted.
    pub fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// The browser's offset from UTC in minutes, as `Date.getTimezoneOffset()`
    /// gives it, when the form sent one.
    pub(crate) fn tz_offset(&self) -> Option<i32> {
        self.tz_offset
    }

    /// Deserializes the values into your DTO and validates it with its
    /// garde rules, exactly as `Valid<Json<T>>` does in your API: a 400
    /// whose `errors` name the fields, shown under them in the panel.
    ///
    /// ```ignore
    /// let input: UpdateDeal = form.parse().await?;
    /// ```
    pub async fn parse<T>(&self) -> Result<T, AppError>
    where
        T: DeserializeOwned + garde::Validate<Context = ()> + Send,
    {
        let body = serde_json::to_vec(&self.values).map_err(AppError::internal)?;
        let mut request = http::Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .map_err(AppError::internal)?;
        if let Some(app) = &self.app {
            request.extensions_mut().insert(app.clone());
        }
        let Valid(Json(value)) = Valid::<Json<T>>::from_request(request, &()).await?;
        Ok(value)
    }
}
