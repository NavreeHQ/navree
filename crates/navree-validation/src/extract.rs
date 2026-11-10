//! The [`Valid`] and [`ValidAsync`] extractors.

use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use crate::Validate;
use http::header::CONTENT_TYPE;
use http::request::Parts;
use http::{HeaderMap, Method, StatusCode};
use navree_db::Uow;
use navree_http::body::{BodyLimit, Bytes, read_body as read_limited};
use navree_http::extract::{FromRequest, FromRequestParts, Request};
use navree_http::{AppError, Form, Json, PathParams, Query};
use navree_kernel::AppContext;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::context;
use crate::failure::{self, Failure};
use crate::partial::{self, Mendable, Pairs, Stop};
use crate::{
    AsyncRule, AsyncRules, InputContext, MessageTranslator, Messages, Violation, Violations,
};

/// Bodies of this many bytes or more are deserialized, sanitized and checked
/// on Tokio's blocking pool, not on the async worker that reads the request:
/// what a body costs follows its length, so the length of the whole body, not
/// of a field, is what says whether a hostile one can hold the worker for long.
/// Under it the most a body can ask of the worker is some milliseconds, and an
/// ordinary one takes microseconds: less than the hop to another thread costs.
const OFF_THE_WORKER_BYTES: usize = 16 * 1024;

/// Whether a body of `bytes` is read off the async worker.
fn is_long(bytes: usize) -> bool {
    bytes >= OFF_THE_WORKER_BYTES
}

/// Whether a JSON value is at least [`OFF_THE_WORKER_BYTES`] long as JSON,
/// counted until it is. Every value counts, not only text: what reading one
/// costs follows how many values it holds, and a million zeros hold no text.
fn is_long_value(value: &Value) -> bool {
    let mut bytes = least_bytes(value);
    // Counted as they are found, so what waits here stays under the length.
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Array(items) => {
                for item in items {
                    bytes = bytes.saturating_add(least_bytes(item));
                    if is_long(bytes) {
                        return true;
                    }
                    pending.push(item);
                }
            }
            Value::Object(fields) => {
                for (key, item) in fields {
                    // A key is text in quotes, with a colon after it.
                    bytes = bytes
                        .saturating_add(key.len().saturating_add(3))
                        .saturating_add(least_bytes(item));
                    if is_long(bytes) {
                        return true;
                    }
                    pending.push(item);
                }
            }
            _ => {}
        }
    }
    is_long(bytes)
}

/// The fewest bytes `value` takes as JSON, with the comma after it and
/// without what it holds: its text and quotes, or at least a digit.
fn least_bytes(value: &Value) -> usize {
    match value {
        Value::String(text) => text.len().saturating_add(3),
        _ => 2,
    }
}

/// Runs `work` on Tokio's blocking pool when `long` and there is a runtime to
/// run it on, and here otherwise. A panic in `work` is a panic here.
async fn off_the_worker<R>(
    long: bool,
    work: impl FnOnce() -> R + Send + 'static,
) -> Result<R, Invalid>
where
    R: Send + 'static,
{
    if !long || tokio::runtime::Handle::try_current().is_err() {
        return Ok(work());
    }
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => Ok(result),
        Err(error) => match error.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(_) => Err(Invalid::unavailable()),
        },
    }
}

/// Validated input: `Valid<Json<T>>`, `Valid<Query<T>>` or `Valid<Form<T>>`
/// deserializes `T`, then runs its rules.
///
/// Every failure is a 400 error with field errors
/// (`{"statusCode": 400, "message": ["email must be a valid email address"],
/// "error": "Bad Request", "code": "validation_failed", "errors": {"email":
/// ["email must be a valid email address"]}}`):
/// malformed JSON (on `body`), serde errors (at the serde path: a missing
/// field is `required`, a wrong type is `string`, `integer`, ...) and the
/// rules, with messages from the app's [`Messages`]. One answer lists them
/// all: a value of the wrong type is reported with the other fields' rule
/// failures, not before and instead of them (input under 16 KiB, up to 16
/// such values; longer input reports its first).
///
/// ```
/// use http::Method;
/// use navree_http::{Json, Query, RouteDef};
/// use navree_validation::Validate;
/// use navree_validation::Valid;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Deserialize, Serialize, Validate)]
/// struct CreateUser {
///     #[validate(email)]
///     email: String,
/// }
///
/// #[derive(Deserialize, Validate)]
/// struct Filters {
///     #[validate(range(min = 1, max = 100))]
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
/// # let _ = RouteDef::on(Method::POST, "/users", store);
/// # let _ = RouteDef::on(Method::GET, "/users", index);
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

impl<T> FromRequest for Valid<Json<T>>
where
    T: DeserializeOwned + Validate + Send + 'static,
{
    type Rejection = AppError;

    async fn from_request(req: Request) -> Result<Self, Self::Rejection> {
        let ctx = req.extensions().get::<AppContext>().cloned();
        let (mut parts, body) = json_body(req).await?;
        let long = is_long(body.len());
        let mut context = Context::new(ctx.as_ref());
        let read = context
            .run(Some(&mut parts), long, body, |body| {
                from_json_bytes::<T>(body)
            })
            .await;
        let (value, input) = match read.map(|(_, read)| read) {
            Ok(Ok(read)) => read,
            Ok(Err(invalid)) | Err(invalid) => return Err(invalid.reject(ctx.as_ref()).await),
        };
        let checked = context
            .run(Some(&mut parts), long, value, |value| check(value))
            .await;
        let (value, checked) = match checked {
            Ok(checked) => checked,
            Err(invalid) => return Err(invalid.reject(ctx.as_ref()).await),
        };
        if let Err(failures) = checked {
            let input = off_the_worker(long, move || input.into_value()).await;
            return Err(match input {
                Ok(input) => Invalid::new(failures, input, "body"),
                Err(invalid) => invalid,
            }
            .reject(ctx.as_ref())
            .await);
        }
        Ok(Valid(Json(value)))
    }
}

impl<T> FromRequestParts for Valid<Query<T>>
where
    T: DeserializeOwned + Validate + Send + 'static,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let ctx = parts.extensions.get::<AppContext>().cloned();
        let result = validate_urlencoded::<T>(ctx.as_ref(), parts, None, ["query", "query"]).await;
        match result {
            Ok(value) => Ok(Valid(Query(value))),
            Err(invalid) => Err(invalid.reject(ctx.as_ref()).await),
        }
    }
}

impl<T> FromRequest for Valid<Form<T>>
where
    T: DeserializeOwned + Validate + Send + 'static,
{
    type Rejection = AppError;

    async fn from_request(req: Request) -> Result<Self, Self::Rejection> {
        let ctx = req.extensions().get::<AppContext>().cloned();
        // Rule failures on the form as a whole are on `body`, even when a
        // `GET` form comes from the query string.
        let result = match form_body(req).await? {
            (mut parts, Some(body)) => {
                validate_urlencoded::<T>(ctx.as_ref(), &mut parts, Some(body), ["body", "body"])
                    .await
            }
            (mut parts, None) => {
                validate_urlencoded::<T>(ctx.as_ref(), &mut parts, None, ["query", "body"]).await
            }
        };
        match result {
            Ok(value) => Ok(Valid(Form(value))),
            Err(invalid) => Err(invalid.reject(ctx.as_ref()).await),
        }
    }
}

/// An input extractor from another crate that [`Valid`] and [`ValidAsync`]
/// can wrap, such as navree-micro's `Payload<T>`: it reads its input as a
/// JSON value, and validation then runs exactly as for `Valid<Json<T>>`
/// (serde errors at their path, the rules of
/// [`Target`](ValidInput::Target), the app's [`Messages`]).
///
/// ```
/// use navree_http::extract::Request;
/// use navree_validation::Validate;
/// use navree_http::{AppError, RouteDef};
/// use navree_validation::{Valid, ValidInput};
/// use serde::Deserialize;
/// use serde_json::Value;
///
/// /// The input, read from a JSON body.
/// struct Input<T>(T);
///
/// impl<T> ValidInput for Input<T>
/// where
///     T: serde::de::DeserializeOwned + Validate + Send + 'static,
/// {
///     type Target = T;
///
///     async fn read(req: Request) -> Result<Option<Value>, AppError> {
///         let bytes = navree_http::body::to_bytes(req.into_body(), 1 << 20)
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
///     #[validate(length(min = 1))]
///     name: String,
/// }
///
/// async fn rename(Valid(Input(input)): Valid<Input<Rename>>) -> String {
///     input.name
/// }
/// # let _ = RouteDef::on(http::Method::POST, "/rename", rename);
/// ```
pub trait ValidInput: Sized + Send {
    /// The type the input deserializes into.
    type Target: DeserializeOwned + Validate + Send + 'static;

    /// Reads the request's input as a JSON value. `Ok(None)` means the
    /// input is malformed and becomes the field error malformed JSON gets
    /// (`body must be a valid JSON string`); an error is returned to the
    /// caller as it is.
    fn read(req: Request) -> impl Future<Output = Result<Option<Value>, AppError>> + Send;

    /// Wraps the validated value.
    fn wrap(value: Self::Target) -> Self;
}

impl<E> FromRequest for Valid<E>
where
    E: ValidInput,
{
    type Rejection = AppError;

    async fn from_request(req: Request) -> Result<Self, Self::Rejection> {
        let ctx = req.extensions().get::<AppContext>().cloned();
        let Some(input) = E::read(req).await? else {
            return Err(Invalid::new(vec![Failure::keyed("", "json")], None, "body")
                .reject(ctx.as_ref())
                .await);
        };
        match validate_detached::<E::Target>(input, ctx.as_ref()).await {
            Ok(value) => Ok(Valid(E::wrap(value))),
            Err(invalid) => Err(invalid.reject(ctx.as_ref()).await),
        }
    }
}

impl<E> FromRequest for ValidAsync<E>
where
    E: ValidInput,
    E::Target: AsyncRules,
{
    type Rejection = AppError;

    async fn from_request(req: Request) -> Result<Self, Self::Rejection> {
        let (parts, body) = req.into_parts();
        let request = RuleRequest::of(&parts);
        let Valid(input) =
            <Valid<Checked<E>> as FromRequest>::from_request(Request::from_parts(parts, body))
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

/// The request's [`InputContext`], resolved the first time a value asks
/// for it (see [`crate::context`]): input without international types
/// never resolves it.
struct Context<'a> {
    app: Option<&'a AppContext>,
    resolved: Option<InputContext>,
}

impl<'a> Context<'a> {
    fn new(app: Option<&'a AppContext>) -> Self {
        Self {
            app,
            resolved: None,
        }
    }

    /// Runs `f` with the resolved context, or without one: `None` when a
    /// value asked for it (resolve, then run `f` again).
    fn attempt<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
        match &self.resolved {
            Some(context) => Some(context.scope(f)),
            None => match context::lazily(f) {
                (result, false) => Some(result),
                (_, true) => None,
            },
        }
    }

    /// Resolves the context from the app and the request head (`None`
    /// outside a request: the app's context as configured).
    async fn resolve(&mut self, parts: Option<&mut Parts>) {
        if self.resolved.is_none() {
            self.resolved = Some(InputContext::resolve(self.app, parts).await);
        }
    }

    /// Runs `f` with the resolved context.
    fn resolved<R>(&self, f: impl FnOnce() -> R) -> R {
        match &self.resolved {
            Some(context) => context.scope(f),
            None => f(),
        }
    }

    /// Runs `work` on `state` (deserializing or validating) in the context:
    /// first as is, and again with the resolved context when a value asked for
    /// it. On the blocking pool when `long`, which is why `state` goes there
    /// and back.
    async fn run<S, R>(
        &mut self,
        parts: Option<&mut Parts>,
        long: bool,
        state: S,
        work: fn(&S) -> R,
    ) -> Result<(S, R), Invalid>
    where
        S: Send + 'static,
        R: Send + 'static,
    {
        let state = match self.resolved.clone() {
            Some(context) => {
                return off_the_worker(long, move || {
                    let result = context.scope(|| work(&state));
                    (state, result)
                })
                .await;
            }
            None => state,
        };
        let (state, (result, asked)) = off_the_worker(long, move || {
            let result = context::lazily(|| work(&state));
            (state, result)
        })
        .await?;
        if !asked {
            return Ok((state, result));
        }
        self.resolve(parts).await;
        let context = self.resolved.clone().unwrap_or_default();
        off_the_worker(long, move || {
            let result = context.scope(|| work(&state));
            (state, result)
        })
        .await
    }
}

/// Deserializes and validates URL-encoded pairs: `body`, or the query
/// string of `parts` when there is none. `roots` name whole-input errors
/// of deserializing and of the rules.
async fn validate_urlencoded<T>(
    app: Option<&AppContext>,
    parts: &mut Parts,
    body: Option<Bytes>,
    [root, rules_root]: [&'static str; 2],
) -> Result<T, Invalid>
where
    T: DeserializeOwned + Validate + Send + 'static,
{
    let mut context = Context::new(app);
    let input = body.unwrap_or_else(|| {
        Bytes::copy_from_slice(parts.uri.query().unwrap_or_default().as_bytes())
    });
    let long = is_long(input.len());
    let (_, value) = context
        .run(Some(&mut *parts), long, input, |input| {
            urlencoded::<T>(input)
        })
        .await?;
    let value = value.map_err(|failures| Invalid::new(failures, None, root))?;
    let (value, checked) = context
        .run(Some(parts), long, value, |value| check(value))
        .await?;
    checked.map_err(|failures| Invalid::new(failures, None, rules_root))?;
    Ok(value)
}

/// Validated input with database rules: like [`Valid`], then, once the
/// validation rules pass, the [`AsyncRules`] of `T` (`unique`, `exists`) run in
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

impl<T> FromRequest for ValidAsync<Json<T>>
where
    T: DeserializeOwned + Validate + AsyncRules + Send + 'static,
{
    type Rejection = AppError;

    async fn from_request(req: Request) -> Result<Self, Self::Rejection> {
        let (parts, body) = req.into_parts();
        let request = RuleRequest::of(&parts);
        let Valid(Json(value)) =
            <Valid<Json<T>> as FromRequest>::from_request(Request::from_parts(parts, body)).await?;
        request.check(value.async_rules()).await?;
        Ok(ValidAsync(Json(value)))
    }
}

impl<T> FromRequestParts for ValidAsync<Query<T>>
where
    T: DeserializeOwned + Validate + AsyncRules + Send + 'static,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let request = RuleRequest::of(parts);
        let Valid(Query(value)) =
            <Valid<Query<T>> as FromRequestParts>::from_request_parts(parts).await?;
        request.check(value.async_rules()).await?;
        Ok(ValidAsync(Query(value)))
    }
}

impl<T> FromRequest for ValidAsync<Form<T>>
where
    T: DeserializeOwned + Validate + AsyncRules + Send + 'static,
{
    type Rejection = AppError;

    async fn from_request(req: Request) -> Result<Self, Self::Rejection> {
        let (parts, body) = req.into_parts();
        let request = RuleRequest::of(&parts);
        let Valid(Form(value)) =
            <Valid<Form<T>> as FromRequest>::from_request(Request::from_parts(parts, body)).await?;
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
    /// Reads the app, the unit of work and the route's path parameters
    /// (none outside a matched route) from the request head.
    fn of(parts: &Parts) -> Self {
        let path_params = parts
            .extensions
            .get::<PathParams>()
            .map_or_else(Vec::new, |params| {
                params
                    .iter()
                    .map(|(key, value)| (key.to_owned(), value.to_owned()))
                    .collect()
            });
        Self {
            ctx: parts.extensions.get::<AppContext>().cloned(),
            uow: parts.extensions.get::<Uow>().cloned(),
            path_params,
        }
    }

    /// Runs `rules` in the unit of work; failures are a 400.
    async fn check(self, rules: Vec<AsyncRule>) -> Result<(), AppError> {
        let violations = self.failures(rules).await?;
        if violations.is_empty() {
            Ok(())
        } else {
            Err(Invalid::body(failure::from_violations(violations))
                .reject(self.ctx.as_ref())
                .await)
        }
    }

    /// Runs `rules` in the unit of work and returns the ones that failed, each as the
    /// violation of its rule key at its field. An error when a rule can't run: a bad
    /// identifier, no unit of work, a database error (all 500s).
    async fn failures(&self, rules: Vec<AsyncRule>) -> Result<Violations, AppError> {
        // Bad identifiers fail even when no query is needed; null values pass.
        for rule in &rules {
            rule.check_identifiers().map_err(AppError::internal)?;
        }
        let rules: Vec<AsyncRule> = rules.into_iter().filter(|r| !r.is_null()).collect();
        let mut violations = Violations::new();
        if rules.is_empty() {
            return Ok(violations);
        }
        let uow = self.uow.as_ref().ok_or_else(|| {
            AppError::internal(
                "ValidAsync needs the request's unit of work: add DatabaseModule to the app",
            )
        })?;
        for rule in &rules {
            if !rule
                .passes_for(uow, &self.path_params)
                .await
                .map_err(AppError::internal)?
            {
                violations.add(rule.field(), Violation::rule(rule.rule()));
            }
        }
        Ok(violations)
    }
}

/// Runs a value's database rules ([`AsyncRules`]: `unique`, `exists`, a
/// [`UowCheck`](crate::UowCheck)) in the request's unit of work, exactly as
/// [`ValidAsync`] does after a value passes its rules: identifiers are checked first, a
/// null value's rule passes without a query, and a rule that ignores a path parameter
/// ([`AsyncRule::ignore_path_param`]) reads it from the route `parts` matched.
///
/// The rules that failed come back as [`Violations`] (the rule key, `unique` or
/// `exists`, at the rule's field), for the caller to render with the app's messages
/// ([`AppMessages`](crate::AppMessages)): Navree UI's forms show them under their
/// fields. A rule that can't run is an error: a bad identifier, a request without a
/// unit of work (the app needs `DatabaseModule`), a database error.
///
/// ```ignore
/// let violations = check_async_rules(&parts, input.async_rules()).await?;
/// if !violations.is_empty() {
///     let errors = AppMessages::of(ctx).await.render_violations(&violations, "body");
///     // ...
/// }
/// ```
pub async fn check_async_rules(
    parts: &Parts,
    rules: Vec<AsyncRule>,
) -> Result<Violations, AppError> {
    RuleRequest::of(parts).failures(rules).await
}

/// Unrendered failures and what rendering them needs.
pub(crate) struct Invalid {
    failures: Vec<Failure>,
    input: Option<Value>,
    root: &'static str,
    /// The work was cut off by the runtime shutting down: a 503, not a
    /// verdict on the input.
    unavailable: bool,
}

impl Invalid {
    pub(crate) fn new(failures: Vec<Failure>, input: Option<Value>, root: &'static str) -> Self {
        Self {
            failures,
            input,
            root,
            unavailable: false,
        }
    }

    /// The runtime shut down under the work.
    fn unavailable() -> Self {
        Self {
            unavailable: true,
            ..Self::new(Vec::new(), None, "body")
        }
    }

    fn body(failures: Vec<Failure>) -> Self {
        Self::new(failures, None, "body")
    }

    /// The 400 error, with the app's messages (through its
    /// [`MessageTranslator`], when it has one).
    pub(crate) async fn reject(self, ctx: Option<&AppContext>) -> AppError {
        if self.unavailable {
            return AppError::from_status(StatusCode::SERVICE_UNAVAILABLE)
                .with_detail("The server is shutting down");
        }
        let messages = messages(ctx).await;
        let translator = translator(ctx).await;
        failure::render(
            &self.failures,
            &messages,
            translator.as_deref(),
            self.input.as_ref(),
            self.root,
        )
        .into_app_error()
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

/// Runs the rules of `value` outside an extractor (a GraphQL
/// argument, a message payload) and renders the failures with `messages`,
/// as the extractors do: fields are dotted paths (`items.0.sku`), a rule on
/// the value as a whole is reported under `root`, and `input`, the value as
/// JSON when it is at hand, picks the `array` variant of the length
/// messages. Rules that read the [`InputContext`] (`rules::phone`) use the
/// defaults, unless the call runs in an [`InputContext::scope`].
///
/// ```
/// use navree_validation::Validate;
/// use navree_validation::{Messages, validate};
///
/// #[derive(Validate)]
/// struct MoveDeal {
///     #[validate(range(min = 0))]
///     position: i64,
/// }
///
/// let errors = validate(&MoveDeal { position: -1 }, None, &Messages::default(), "input")
///     .unwrap_err();
/// assert_eq!(
///     errors.get("position"),
///     Some(&["position must not be less than 0".to_owned()][..])
/// );
/// ```
pub fn validate<T: Validate>(
    value: &T,
    input: Option<&Value>,
    messages: &Messages,
    root: &str,
) -> Result<(), crate::ValidationErrors> {
    check(value).map_err(|failures| failure::render(&failures, messages, None, input, root))
}

/// [`validate`] with the app's [`Messages`] (`ctx`'s, or the defaults
/// without `ValidationModule`): what [`Valid`] does after deserializing a
/// body, for values that arrive another way (navree-grpc validates gRPC
/// messages with it). Fields are keyed by dotted path; rules on the value
/// as a whole are keyed `input`.
///
/// Rules that read the [`InputContext`] (`rules::phone` on a number
/// without a country code) get the app's, as configured: there is no
/// request for a [`RegionSource`](crate::RegionSource) to look at.
pub async fn validate_in<T: Validate>(
    value: &T,
    ctx: Option<&AppContext>,
) -> Result<(), crate::ValidationErrors> {
    let mut context = Context::new(ctx);
    let checked = match context.attempt(|| check(value)) {
        Some(checked) => checked,
        None => {
            context.resolve(None).await;
            context.resolved(|| check(value))
        }
    };
    match checked {
        Ok(()) => Ok(()),
        Err(failures) => {
            let messages = messages(ctx).await;
            let translator = translator(ctx).await;
            Err(failure::render(
                &failures,
                &messages,
                translator.as_deref(),
                None,
                "input",
            ))
        }
    }
}

/// Deserializes `input` into `T` and runs its rules: the checks
/// [`Valid<Json<T>>`](Valid) runs on a request body, for JSON that arrives
/// some other way (MCP tool arguments, agent tool calls, queued payloads).
///
/// Failures are the same 400 errors, with field errors at their serde path
/// (`"title should not be empty"`) and rule messages, from the app's
/// [`Messages`] when `ctx` has them (the defaults otherwise). Values that
/// read the [`InputContext`] (a `Phone` written without a country code)
/// get the app's, as configured: there is no request for a
/// [`RegionSource`](crate::RegionSource) to look at.
///
/// ```
/// use navree_validation::Validate;
/// use navree_validation::validate_value;
/// use serde::Deserialize;
/// use serde_json::json;
///
/// #[derive(Deserialize, Validate)]
/// struct LogCall {
///     #[validate(length(min = 1, max = 200))]
///     subject: String,
/// }
///
/// # tokio_test(async {
/// let call: LogCall = validate_value(json!({ "subject": "Budget approved" }), None).await.unwrap();
/// assert_eq!(call.subject, "Budget approved");
///
/// let error = validate_value::<LogCall>(json!({ "subject": "" }), None).await.err().unwrap();
/// assert_eq!(error.status, 400);
/// assert!(error.body()["errors"]["subject"].is_array());
/// # });
/// # fn tokio_test(f: impl std::future::Future<Output = ()>) {
/// #     tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
/// # }
/// ```
pub async fn validate_value<T>(input: Value, ctx: Option<&AppContext>) -> Result<T, AppError>
where
    T: DeserializeOwned + Validate + Send + 'static,
{
    match validate_detached::<T>(input, ctx).await {
        Ok(value) => Ok(value),
        Err(invalid) => Err(invalid.reject(ctx).await),
    }
}

/// Deserializes and validates JSON that comes without a request head
/// ([`ValidInput`], [`validate_value`]): the [`InputContext`] is resolved
/// only when a value asks for it, as configured (no
/// [`RegionSource`](crate::RegionSource) runs). Failures of the input as a
/// whole are on `body`.
async fn validate_detached<T>(input: Value, app: Option<&AppContext>) -> Result<T, Invalid>
where
    T: DeserializeOwned + Validate + Send + 'static,
{
    let mut context = Context::new(app);
    let long = is_long_value(&input);
    let (input, value) = context
        .run(None, long, input, |input| read_json::<T>(input))
        .await?;
    let value = match value {
        Ok(value) => value,
        Err(failures) => return Err(Invalid::new(failures, Some(input), "body")),
    };
    let (value, checked) = context.run(None, long, value, |value| check(value)).await?;
    match checked {
        Ok(()) => Ok(value),
        Err(failures) => Err(Invalid::new(failures, Some(input), "body")),
    }
}

/// The app's [`MessageTranslator`], when `ctx` has one.
pub(crate) async fn translator(ctx: Option<&AppContext>) -> Option<Arc<dyn MessageTranslator>> {
    match ctx {
        Some(ctx) => ctx.try_get::<dyn MessageTranslator>().await,
        None => None,
    }
}

/// Runs the rules of `value`.
pub(crate) fn check<T: Validate>(value: &T) -> Result<(), Vec<Failure>> {
    value.validate().map_err(failure::from_violations)
}

/// Reads a JSON body, keeping the request head. The outer error is an
/// HTTP failure (415, 413); the body is deserialized later, in the input
/// context ([`from_json_bytes`]).
async fn json_body(req: Request) -> Result<(Parts, Bytes), AppError> {
    if !is_json(req.headers()) {
        return Err(AppError::from_status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .with_detail("Expected request with `Content-Type: application/json`"));
    }
    read_body(req).await
}

/// A JSON body as the rule failures see it: its bytes, parsed into a
/// [`Value`] only when a failure needs it (the length messages pick their
/// `array` variant from it).
enum JsonInput {
    Bytes(Bytes),
    Value(Value),
}

impl JsonInput {
    fn into_value(self) -> Option<Value> {
        match self {
            JsonInput::Bytes(bytes) => serde_json::from_slice(&bytes).ok(),
            JsonInput::Value(value) => Some(value),
        }
    }
}

/// Deserializes `T` from a JSON body, failing as a validation failure.
///
/// A body is deserialized straight from its bytes. Only one that fails
/// that way is parsed into a [`Value`] and deserialized from it
/// ([`read_json`]), as every body once was: that reports every failure as
/// before (malformed JSON on `body`, serde errors at their path) and
/// accepts what the `Value` route accepts (a repeated key keeps its last
/// value).
fn from_json_bytes<T: DeserializeOwned + Validate>(
    bytes: &Bytes,
) -> Result<(T, JsonInput), Invalid> {
    if let Ok(value) = failure::encoding(|| serde_json::from_slice::<T>(bytes)) {
        return Ok((value, JsonInput::Bytes(bytes.clone())));
    }
    let input: Value = match serde_json::from_slice(bytes) {
        Ok(input) => input,
        Err(_) => {
            return Err(Invalid::new(vec![Failure::keyed("", "json")], None, "body"));
        }
    };
    match input.read::<T>() {
        Ok(value) => Ok((value, JsonInput::Value(input))),
        Err(stop) => {
            let failures = every_failure::<T, _>(&input, stop, is_long(bytes.len()));
            Err(Invalid::new(failures, Some(input), "body"))
        }
    }
}

/// Deserializes `T` from a JSON value: when it does not, every failure, as
/// [`Valid`] reports them.
pub(crate) fn read_json<T: DeserializeOwned + Validate>(input: &Value) -> Result<T, Vec<Failure>> {
    input
        .read::<T>()
        .map_err(|stop| every_failure::<T, _>(input, stop, is_long_value(input)))
}

/// The failures of `input`, which serde stopped reading at `stop`: all of
/// them ([`partial`]), or only that one for `long` input.
fn every_failure<T, I>(input: &I, stop: Stop, long: bool) -> Vec<Failure>
where
    T: DeserializeOwned + Validate,
    I: Mendable,
{
    if long {
        vec![stop.into_failure()]
    } else {
        partial::every_failure::<T, I>(input, stop)
    }
}

/// Reads a form, keeping the request head: `None` for `GET` and `HEAD`,
/// whose form is the query string (like [`Form`]), else the
/// `application/x-www-form-urlencoded` body.
async fn form_body(req: Request) -> Result<(Parts, Option<Bytes>), AppError> {
    if req.method() == Method::GET || req.method() == Method::HEAD {
        return Ok((req.into_parts().0, None));
    }
    if !has_content_type(req.headers(), "application/x-www-form-urlencoded") {
        return Err(
            AppError::from_status(StatusCode::UNSUPPORTED_MEDIA_TYPE).with_detail(
                "Expected request with `Content-Type: application/x-www-form-urlencoded`",
            ),
        );
    }
    let (parts, bytes) = read_body(req).await?;
    Ok((parts, Some(bytes)))
}

/// Buffers the body within the route's body limit, keeping the request
/// head (a region source may need it after the body is read): a body past
/// the limit is a 413, one that cannot be read a 400.
async fn read_body(req: Request) -> Result<(Parts, Bytes), AppError> {
    let limit = BodyLimit::of(req.extensions());
    let (parts, body) = req.into_parts();
    match read_limited(body, limit).await {
        Ok(bytes) => Ok((parts, bytes)),
        Err(error) => Err(rejected(error.status(), error.to_string())),
    }
}

/// Deserializes URL-encoded pairs, reporting serde errors at their path:
/// every failure, as [`Valid`] reports them.
fn urlencoded<T: DeserializeOwned + Validate>(input: &Bytes) -> Result<T, Vec<Failure>> {
    let deserializer = serde_urlencoded::Deserializer::new(form_urlencoded::parse(input));
    failure::encoding(|| serde_path_to_error::deserialize(deserializer)).map_err(|error| {
        let stop = Stop::of(error.path(), &error.inner().to_string());
        every_failure::<T, _>(&Pairs::parse(input), stop, is_long(input.len()))
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// What a value costs to read follows how many values it holds, text or
    /// not.
    #[test]
    fn a_value_is_long_by_its_values_not_only_its_text() {
        assert!(!is_long_value(
            &json!({ "name": "Gurpreet Kaur", "size": 3 })
        ));
        assert!(is_long_value(
            &json!({ "text": "x".repeat(OFF_THE_WORKER_BYTES) })
        ));
        let zeros = OFF_THE_WORKER_BYTES / 2;
        assert!(is_long_value(&json!({ "values": vec![0; zeros] })));
        assert!(!is_long_value(&json!({ "values": vec![0; zeros / 4] })));
        assert!(is_long_value(&json!(vec![json!({}); zeros])));
        assert!(is_long_value(&json!(vec![json!(null); zeros])));
        let keys: serde_json::Map<String, Value> = (0..zeros / 2)
            .map(|n| (n.to_string(), Value::Null))
            .collect();
        assert!(is_long_value(&Value::Object(keys)));
    }

    /// Nesting counts like anything else.
    #[test]
    fn deep_values_are_counted_too() {
        let mut deep = json!(0);
        for _ in 0..OFF_THE_WORKER_BYTES / 2 {
            deep = Value::Array(vec![deep]);
        }
        assert!(is_long_value(&deep));
        // serde_json drops a deep value on the stack: leave it to the heap.
        std::mem::forget(deep);
    }
}
