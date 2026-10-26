//! The [`Valid`] and [`ValidAsync`] extractors.

use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use axum::extract::{Form, FromRequest, FromRequestParts, Query, RawPathParams, Request};
use bytes::Bytes;
use garde::Validate;
use http::header::CONTENT_TYPE;
use http::request::Parts;
use http::{HeaderMap, Method, StatusCode};
use navree_db::Uow;
use navree_http::{AppError, Json};
use navree_kernel::AppContext;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::failure::{self, Failure};
use crate::{AsyncRule, AsyncRules, Messages};

/// Validated input: `Valid<Json<T>>`, `Valid<Query<T>>` or `Valid<Form<T>>`
/// deserializes `T`, then runs its garde rules.
///
/// Every failure is a 400 error with field errors
/// (`{"statusCode": 400, "message": ["email must be a valid email address"],
/// "error": "Bad Request", "errors": {"email": ["email must be a valid email
/// address"]}}`):
/// malformed JSON (on `body`), serde errors (at the serde path: a missing
/// field is `required`, a wrong type is `string`, `integer`, ...) and garde
/// rules, with messages from the app's [`Messages`].
///
/// ```
/// use axum::extract::Query;
/// use garde::Validate;
/// use navree_http::Json;
/// use navree_validation::Valid;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Deserialize, Serialize, Validate)]
/// struct CreateUser {
///     #[garde(email)]
///     email: String,
/// }
///
/// #[derive(Deserialize, Validate)]
/// struct Filters {
///     #[garde(range(min = 1, max = 100))]
///     per_page: Option<u32>,
/// }
///
/// async fn store(Valid(Json(input)): Valid<Json<CreateUser>>) -> Json<CreateUser> {
///     Json(input)
/// }
///
/// async fn index(Valid(Query(filters)): Valid<Query<Filters>>) -> String {
///     format!("{:?}", filters.per_page)
/// }
/// # let _: axum::routing::MethodRouter = axum::routing::post(store).get(index);
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct Valid<E>(pub E);

impl<E> Valid<E> {
    /// The wrapped extractor.
    pub fn into_inner(self) -> E {
        self.0
    }
}

impl<E> Deref for Valid<E> {
    type Target = E;

    fn deref(&self) -> &E {
        &self.0
    }
}

impl<E> DerefMut for Valid<E> {
    fn deref_mut(&mut self) -> &mut E {
        &mut self.0
    }
}

impl<T, S> FromRequest<S> for Valid<Json<T>>
where
    T: DeserializeOwned + Validate<Context = ()> + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let ctx = req.extensions().get::<AppContext>().cloned();
        let (value, input) = match json_body::<T, S>(req, state).await? {
            Ok(parsed) => parsed,
            Err(invalid) => return Err(invalid.reject(ctx.as_ref()).await),
        };
        if let Err(failures) = check(&value) {
            return Err(Invalid::new(failures, Some(input), "body")
                .reject(ctx.as_ref())
                .await);
        }
        Ok(Valid(Json(value)))
    }
}

impl<T, S> FromRequestParts<S> for Valid<Query<T>>
where
    T: DeserializeOwned + Validate<Context = ()> + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ctx = parts.extensions.get::<AppContext>().cloned();
        let query = parts.uri.query().unwrap_or_default();
        let result = urlencoded::<T>(query.as_bytes(), "query")
            .and_then(|value| check(&value).map(|()| value).map_err(Invalid::query));
        match result {
            Ok(value) => Ok(Valid(Query(value))),
            Err(invalid) => Err(invalid.reject(ctx.as_ref()).await),
        }
    }
}

impl<T, S> FromRequest<S> for Valid<Form<T>>
where
    T: DeserializeOwned + Validate<Context = ()> + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let ctx = req.extensions().get::<AppContext>().cloned();
        let result = form_body::<T, S>(req, state)
            .await?
            .and_then(|value| check(&value).map(|()| value).map_err(Invalid::body));
        match result {
            Ok(value) => Ok(Valid(Form(value))),
            Err(invalid) => Err(invalid.reject(ctx.as_ref()).await),
        }
    }
}

/// An input extractor from another crate that [`Valid`] and [`ValidAsync`]
/// can wrap, such as navree-micro's `Payload<T>`: it reads its input as a
/// JSON value, and validation then runs exactly as for `Valid<Json<T>>`
/// (serde errors at their path, the garde rules of
/// [`Target`](ValidInput::Target), the app's [`Messages`]).
///
/// ```
/// use axum::extract::Request;
/// use garde::Validate;
/// use navree_http::AppError;
/// use navree_validation::{Valid, ValidInput};
/// use serde::Deserialize;
/// use serde_json::Value;
///
/// /// The input, read from a JSON body.
/// struct Input<T>(T);
///
/// impl<T> ValidInput for Input<T>
/// where
///     T: serde::de::DeserializeOwned + Validate<Context = ()> + Send,
/// {
///     type Target = T;
///
///     async fn read(req: Request) -> Result<Option<Value>, AppError> {
///         let bytes = axum::body::to_bytes(req.into_body(), 1 << 20)
///             .await
///             .map_err(|_| AppError::bad_request("unreadable input"))?;
///         Ok(serde_json::from_slice(&bytes).ok())
///     }
///
///     fn wrap(value: T) -> Self {
///         Input(value)
///     }
/// }
///
/// #[derive(Deserialize, Validate)]
/// struct Rename {
///     #[garde(length(min = 1))]
///     name: String,
/// }
///
/// async fn rename(Valid(Input(input)): Valid<Input<Rename>>) -> String {
///     input.name
/// }
/// # let _: axum::routing::MethodRouter = axum::routing::post(rename);
/// ```
pub trait ValidInput: Sized + Send {
    /// The type the input deserializes into.
    type Target: DeserializeOwned + Validate<Context = ()> + Send;

    /// Reads the request's input as a JSON value. `Ok(None)` means the
    /// input is malformed and becomes the field error malformed JSON gets
    /// (`body must be a valid JSON string`); an error is returned to the
    /// caller as it is.
    fn read(req: Request) -> impl Future<Output = Result<Option<Value>, AppError>> + Send;

    /// Wraps the validated value.
    fn wrap(value: Self::Target) -> Self;
}

impl<E, S> FromRequest<S> for Valid<E>
where
    E: ValidInput,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let ctx = req.extensions().get::<AppContext>().cloned();
        let Some(input) = E::read(req).await? else {
            return Err(
                Invalid::new(vec![Failure::keyed("", "json")], None, "body")
                    .reject(ctx.as_ref())
                    .await,
            );
        };
        let value = match serde_path_to_error::deserialize::<_, E::Target>(&input) {
            Ok(value) => value,
            Err(error) => {
                let failure = failure::from_serde(error.path(), &error.inner().to_string());
                return Err(Invalid::new(vec![failure], Some(input), "body")
                    .reject(ctx.as_ref())
                    .await);
            }
        };
        if let Err(failures) = check(&value) {
            return Err(Invalid::new(failures, Some(input), "body")
                .reject(ctx.as_ref())
                .await);
        }
        Ok(Valid(E::wrap(value)))
    }
}

impl<E, S> FromRequest<S> for ValidAsync<E>
where
    E: ValidInput,
    E::Target: AsyncRules,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let (mut parts, body) = req.into_parts();
        let request = RuleRequest::of(&mut parts, state).await;
        let Valid(input) = Valid::<Checked<E>>::from_request(Request::from_parts(parts, body), state)
            .await?;
        request.check(input.0.async_rules()).await?;
        Ok(ValidAsync(E::wrap(input.0)))
    }
}

/// The validated value of a [`ValidInput`], before it is wrapped (so
/// [`ValidAsync`] can run the async rules on it first).
struct Checked<E: ValidInput>(E::Target);

impl<E: ValidInput> ValidInput for Checked<E> {
    type Target = E::Target;

    fn read(req: Request) -> impl Future<Output = Result<Option<Value>, AppError>> + Send {
        E::read(req)
    }

    fn wrap(value: Self::Target) -> Self {
        Checked(value)
    }
}

/// Validated input with database rules: like [`Valid`], then, once the
/// garde rules pass, the [`AsyncRules`] of `T` (`unique`, `exists`) run in
/// the request's unit of work ([`Uow`], so the app needs `DatabaseModule`).
/// Failed rules are 400 field errors (`"email has already been
/// taken"`); a rule that cannot run (a bad identifier, a database error)
/// is a 500.
///
/// See the [crate example](crate) and [`AsyncRule::ignore_path_param`] for
/// updates (`PATCH /users/{id}` ignoring the user's own row).
#[derive(Clone, Copy, Debug, Default)]
pub struct ValidAsync<E>(pub E);

impl<E> ValidAsync<E> {
    /// The wrapped extractor.
    pub fn into_inner(self) -> E {
        self.0
    }
}

impl<E> Deref for ValidAsync<E> {
    type Target = E;

    fn deref(&self) -> &E {
        &self.0
    }
}

impl<E> DerefMut for ValidAsync<E> {
    fn deref_mut(&mut self) -> &mut E {
        &mut self.0
    }
}

impl<T, S> FromRequest<S> for ValidAsync<Json<T>>
where
    T: DeserializeOwned + Validate<Context = ()> + AsyncRules + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let (mut parts, body) = req.into_parts();
        let request = RuleRequest::of(&mut parts, state).await;
        let Valid(Json(value)) =
            Valid::<Json<T>>::from_request(Request::from_parts(parts, body), state).await?;
        request.check(value.async_rules()).await?;
        Ok(ValidAsync(Json(value)))
    }
}

impl<T, S> FromRequestParts<S> for ValidAsync<Query<T>>
where
    T: DeserializeOwned + Validate<Context = ()> + AsyncRules + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let request = RuleRequest::of(parts, state).await;
        let Valid(Query(value)) = Valid::<Query<T>>::from_request_parts(parts, state).await?;
        request.check(value.async_rules()).await?;
        Ok(ValidAsync(Query(value)))
    }
}

impl<T, S> FromRequest<S> for ValidAsync<Form<T>>
where
    T: DeserializeOwned + Validate<Context = ()> + AsyncRules + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let (mut parts, body) = req.into_parts();
        let request = RuleRequest::of(&mut parts, state).await;
        let Valid(Form(value)) =
            Valid::<Form<T>>::from_request(Request::from_parts(parts, body), state).await?;
        request.check(value.async_rules()).await?;
        Ok(ValidAsync(Form(value)))
    }
}

/// What async rules need from the request.
struct RuleRequest {
    ctx: Option<AppContext>,
    uow: Option<Uow>,
    path_params: Vec<(String, String)>,
}

impl RuleRequest {
    async fn of<S: Send + Sync>(parts: &mut Parts, state: &S) -> Self {
        let path_params = match RawPathParams::from_request_parts(parts, state).await {
            Ok(params) => params
                .iter()
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
            Err(_) => Vec::new(),
        };
        Self {
            ctx: parts.extensions.get::<AppContext>().cloned(),
            uow: parts.extensions.get::<Uow>().cloned(),
            path_params,
        }
    }

    /// Runs `rules` in the unit of work; failures are a 400.
    async fn check(self, rules: Vec<AsyncRule>) -> Result<(), AppError> {
        // Bad identifiers fail even when no query is needed; null values pass.
        for rule in &rules {
            rule.check_identifiers().map_err(AppError::internal)?;
        }
        let rules: Vec<AsyncRule> = rules.into_iter().filter(|r| !r.is_null()).collect();
        if rules.is_empty() {
            return Ok(());
        }
        let uow = self.uow.ok_or_else(|| {
            AppError::internal(
                "ValidAsync needs the request's unit of work: add DatabaseModule to the app",
            )
        })?;
        let mut failures = Vec::new();
        {
            // Dropped before the unit of work commits.
            let tx = uow.tx().await.map_err(AppError::internal)?;
            for rule in &rules {
                let ignore = rule.ignore_for(&self.path_params);
                if !rule
                    .passes_ignoring(&*tx, ignore)
                    .await
                    .map_err(AppError::internal)?
                {
                    failures.push(Failure::keyed(rule.field(), rule.rule()));
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Invalid::body(failures).reject(self.ctx.as_ref()).await)
        }
    }
}

/// Unrendered failures and what rendering them needs.
pub(crate) struct Invalid {
    failures: Vec<Failure>,
    input: Option<Value>,
    root: &'static str,
}

impl Invalid {
    pub(crate) fn new(failures: Vec<Failure>, input: Option<Value>, root: &'static str) -> Self {
        Self {
            failures,
            input,
            root,
        }
    }

    fn body(failures: Vec<Failure>) -> Self {
        Self::new(failures, None, "body")
    }

    fn query(failures: Vec<Failure>) -> Self {
        Self::new(failures, None, "query")
    }

    /// The 400 error, with the app's messages.
    pub(crate) async fn reject(self, ctx: Option<&AppContext>) -> AppError {
        let messages = messages(ctx).await;
        failure::render(&self.failures, &messages, self.input.as_ref(), self.root).into_app_error()
    }
}

/// The app's [`Messages`], or the defaults without `ValidationModule`.
pub(crate) async fn messages(ctx: Option<&AppContext>) -> Arc<Messages> {
    match ctx {
        Some(ctx) => match ctx.try_get::<Messages>().await {
            Some(messages) => messages,
            None => Messages::shared_default(),
        },
        None => Messages::shared_default(),
    }
}

/// Runs the garde rules of `value`.
pub(crate) fn check<T: Validate<Context = ()>>(value: &T) -> Result<(), Vec<Failure>> {
    failure::scope(|| value.validate_with(&())).map_err(failure::from_report)
}

/// Reads and deserializes a JSON body. The outer error is an HTTP failure
/// (415, 413); the inner one a validation failure.
async fn json_body<T, S>(req: Request, state: &S) -> Result<Result<(T, Value), Invalid>, AppError>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    if !is_json(req.headers()) {
        return Err(AppError::from_status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .with_detail("Expected request with `Content-Type: application/json`"));
    }
    let bytes = Bytes::from_request(req, state)
        .await
        .map_err(|rejection| rejected(rejection.status(), rejection.body_text()))?;
    let input: Value = match serde_json::from_slice(&bytes) {
        Ok(input) => input,
        Err(_) => {
            return Ok(Err(Invalid::new(
                vec![Failure::keyed("", "json")],
                None,
                "body",
            )));
        }
    };
    match serde_path_to_error::deserialize::<_, T>(&input) {
        Ok(value) => Ok(Ok((value, input))),
        Err(error) => {
            let failure = failure::from_serde(error.path(), &error.inner().to_string());
            Ok(Err(Invalid::new(vec![failure], Some(input), "body")))
        }
    }
}

/// Reads and deserializes a form: from the query string for `GET` and
/// `HEAD` (like Axum's `Form`), else from an
/// `application/x-www-form-urlencoded` body.
async fn form_body<T, S>(req: Request, state: &S) -> Result<Result<T, Invalid>, AppError>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    if req.method() == Method::GET || req.method() == Method::HEAD {
        let query = req.uri().query().unwrap_or_default();
        return Ok(urlencoded(query.as_bytes(), "query"));
    }
    if !has_content_type(req.headers(), "application/x-www-form-urlencoded") {
        return Err(
            AppError::from_status(StatusCode::UNSUPPORTED_MEDIA_TYPE).with_detail(
                "Expected request with `Content-Type: application/x-www-form-urlencoded`",
            ),
        );
    }
    let bytes = Bytes::from_request(req, state)
        .await
        .map_err(|rejection| rejected(rejection.status(), rejection.body_text()))?;
    Ok(urlencoded(&bytes, "body"))
}

/// Deserializes URL-encoded pairs, reporting serde errors at their path.
fn urlencoded<T: DeserializeOwned>(input: &[u8], root: &'static str) -> Result<T, Invalid> {
    let deserializer = serde_urlencoded::Deserializer::new(form_urlencoded::parse(input));
    serde_path_to_error::deserialize(deserializer).map_err(|error| {
        let failure = failure::from_serde(error.path(), &error.inner().to_string());
        Invalid::new(vec![failure], None, root)
    })
}

/// Whether the content type (without parameters) is `expected`.
fn has_content_type(headers: &HeaderMap, expected: &str) -> bool {
    content_type(headers).is_some_and(|essence| essence == expected)
}

/// The lower-cased content type without parameters.
fn content_type(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(CONTENT_TYPE)?.to_str().ok()?;
    Some(
        value
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase(),
    )
}

/// `application/json` or `application/*+json`.
fn is_json(headers: &HeaderMap) -> bool {
    let Some(essence) = content_type(headers) else {
        return false;
    };
    essence == "application/json"
        || (essence.starts_with("application/") && essence.ends_with("+json"))
}

/// An extractor rejection as an error.
fn rejected(status: StatusCode, text: String) -> AppError {
    let error = AppError::from_status(status);
    if status.is_client_error() {
        error.with_detail(text)
    } else {
        error.with_source(text)
    }
}
