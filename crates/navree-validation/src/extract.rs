//! The [`Valid`] and [`ValidAsync`] extractors.

use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use axum::RequestExt;
use axum::extract::{Form, FromRequest, FromRequestParts, Query, RawPathParams, Request};
use bytes::Bytes;
use garde::Validate;
use http::header::CONTENT_TYPE;
use http::request::Parts;
use http::{HeaderMap, Method, StatusCode};
use http_body_util::BodyExt;
use navree_db::Uow;
use navree_http::{AppError, Json};
use navree_kernel::AppContext;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::context;
use crate::failure::{self, Failure};
use crate::{AsyncRule, AsyncRules, InputContext, MessageTranslator, Messages};

/// Runs `$f` (deserializing or validating) in `$context` (a [`Context`]):
/// first as is, and again with the resolved context when a value asked for
/// it. A macro, so no closure (and no borrow it holds) lives across the
/// `await`.
macro_rules! in_context {
    ($context:expr, $parts:expr, $f:expr) => {
        match $context.attempt($f) {
            Some(result) => result,
            None => {
                $context.resolve(Some(&mut *$parts)).await;
                $context.resolved($f)
            }
        }
    };
}

/// Validated input: `Valid<Json<T>>`, `Valid<Query<T>>` or `Valid<Form<T>>`
/// deserializes `T`, then runs its garde rules.
///
/// Every failure is a 400 error with field errors
/// (`{"statusCode": 400, "message": ["email must be a valid email address"],
/// "error": "Bad Request", "code": "validation_failed", "errors": {"email":
/// ["email must be a valid email address"]}}`):
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

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let ctx = req.extensions().get::<AppContext>().cloned();
        let (mut parts, body) = json_body(req).await?;
        let mut context = Context::new(ctx.as_ref());
        let (value, input) = match in_context!(context, &mut parts, || from_json_bytes::<T>(&body))
        {
            Ok(read) => read,
            Err(invalid) => return Err(invalid.reject(ctx.as_ref()).await),
        };
        if let Err(failures) = in_context!(context, &mut parts, || check(&value)) {
            return Err(Invalid::new(failures, input.into_value(), "body")
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
        let result = validate_urlencoded::<T>(ctx.as_ref(), parts, None, ["query", "query"]).await;
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

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let ctx = req.extensions().get::<AppContext>().cloned();
        // Rule failures on the form as a whole are on `body`, even when a
        // `GET` form comes from the query string.
        let result = match form_body(req).await? {
            (mut parts, Some(body)) => {
                validate_urlencoded::<T>(ctx.as_ref(), &mut parts, Some(&body), ["body", "body"])
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
        let Valid(input) =
            Valid::<Checked<E>>::from_request(Request::from_parts(parts, body), state).await?;
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
}

/// Deserializes and validates URL-encoded pairs: `body`, or the query
/// string of `parts` when there is none. `roots` name whole-input errors
/// of deserializing and of the rules.
async fn validate_urlencoded<T>(
    app: Option<&AppContext>,
    parts: &mut Parts,
    body: Option<&[u8]>,
    [root, rules_root]: [&'static str; 2],
) -> Result<T, Invalid>
where
    T: DeserializeOwned + Validate<Context = ()>,
{
    let mut context = Context::new(app);
    let query = parts.uri.query().unwrap_or_default();
    let input = body.unwrap_or(query.as_bytes());
    let value = match context.attempt(|| urlencoded::<T>(input)) {
        Some(value) => value,
        None => {
            // Resolving the context needs the request head, so the input is
            // copied, only in this case.
            let input = input.to_owned();
            context.resolve(Some(&mut *parts)).await;
            context.resolved(|| urlencoded::<T>(&input))
        }
    };
    let value = value.map_err(|failure| Invalid::new(vec![failure], None, root))?;
    in_context!(context, parts, || check(&value))
        .map_err(|failures| Invalid::new(failures, None, rules_root))?;
    Ok(value)
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
        for rule in &rules {
            if !rule
                .passes_for(&uow, &self.path_params)
                .await
                .map_err(AppError::internal)?
            {
                failures.push(Failure::keyed(rule.field(), rule.rule()));
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

    /// The 400 error, with the app's messages (through its
    /// [`MessageTranslator`], when it has one).
    pub(crate) async fn reject(self, ctx: Option<&AppContext>) -> AppError {
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

/// Runs the garde rules of `value` outside an extractor (a GraphQL
/// argument, a message payload) and renders the failures with `messages`,
/// as the extractors do: fields are dotted paths (`items.0.sku`), a rule on
/// the value as a whole is reported under `root`, and `input`, the value as
/// JSON when it is at hand, picks the `array` variant of the length
/// messages. Rules that read the [`InputContext`] (`rules::phone`) use the
/// defaults, unless the call runs in an [`InputContext::scope`].
///
/// ```
/// use garde::Validate;
/// use navree_validation::{Messages, validate};
///
/// #[derive(Validate)]
/// struct MoveDeal {
///     #[garde(range(min = 0))]
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
pub fn validate<T: Validate<Context = ()>>(
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
pub async fn validate_in<T: Validate<Context = ()>>(
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

/// Deserializes `input` into `T` and runs its garde rules: the checks
/// [`Valid<Json<T>>`](Valid) runs on a request body, for JSON that arrives
/// some other way (MCP tool arguments, agent tool calls, queued payloads).
///
/// Failures are the same 400 errors, with field errors at their serde path
/// (`"title should not be empty"`) and garde messages, from the app's
/// [`Messages`] when `ctx` has them (the defaults otherwise). Values that
/// read the [`InputContext`] (a `Phone` written without a country code)
/// get the app's, as configured: there is no request for a
/// [`RegionSource`](crate::RegionSource) to look at.
///
/// ```
/// use garde::Validate;
/// use navree_validation::validate_value;
/// use serde::Deserialize;
/// use serde_json::json;
///
/// #[derive(Deserialize, Validate)]
/// struct LogCall {
///     #[garde(length(min = 1, max = 200))]
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
    T: DeserializeOwned + Validate<Context = ()>,
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
    T: DeserializeOwned + Validate<Context = ()>,
{
    let mut context = Context::new(app);
    let value = match context.attempt(|| from_json::<T>(&input)) {
        Some(value) => value,
        None => {
            context.resolve(None).await;
            context.resolved(|| from_json::<T>(&input))
        }
    };
    let value = match value {
        Ok(value) => value,
        Err(failure) => return Err(Invalid::new(vec![failure], Some(input), "body")),
    };
    let checked = match context.attempt(|| check(&value)) {
        Some(checked) => checked,
        None => {
            context.resolve(None).await;
            context.resolved(|| check(&value))
        }
    };
    match checked {
        Ok(()) => Ok(value),
        Err(failures) => Err(Invalid::new(failures, Some(input), "body")),
    }
}

/// The app's [`MessageTranslator`], when `ctx` has one.
async fn translator(ctx: Option<&AppContext>) -> Option<Arc<dyn MessageTranslator>> {
    match ctx {
        Some(ctx) => ctx.try_get::<dyn MessageTranslator>().await,
        None => None,
    }
}

/// Runs the garde rules of `value`.
pub(crate) fn check<T: Validate<Context = ()>>(value: &T) -> Result<(), Vec<Failure>> {
    failure::scope(|| value.validate_with(&())).map_err(failure::from_report)
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

/// A JSON body as the garde failures see it: its bytes, parsed into a
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
/// ([`from_json`]), as every body once was: that reports every failure as
/// before (malformed JSON on `body`, serde errors at their path) and
/// accepts what the `Value` route accepts (a repeated key keeps its last
/// value).
fn from_json_bytes<T: DeserializeOwned>(bytes: &Bytes) -> Result<(T, JsonInput), Invalid> {
    if let Ok(value) = failure::encoding(|| serde_json::from_slice::<T>(bytes)) {
        return Ok((value, JsonInput::Bytes(bytes.clone())));
    }
    let input: Value = match serde_json::from_slice(bytes) {
        Ok(input) => input,
        Err(_) => {
            return Err(Invalid::new(vec![Failure::keyed("", "json")], None, "body"));
        }
    };
    match from_json::<T>(&input) {
        Ok(value) => Ok((value, JsonInput::Value(input))),
        Err(failure) => Err(Invalid::new(vec![failure], Some(input), "body")),
    }
}

/// Deserializes `T` from a JSON value, reporting serde errors at their path.
pub(crate) fn from_json<T: DeserializeOwned>(input: &Value) -> Result<T, Failure> {
    failure::encoding(|| serde_path_to_error::deserialize::<_, T>(input))
        .map_err(|error| failure::from_serde(error.path(), &error.inner().to_string()))
}

/// Reads a form, keeping the request head: `None` for `GET` and `HEAD`,
/// whose form is the query string (like Axum's `Form`), else the
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
/// head (a region source may need it after the body is read). Failures
/// read like Axum's `Bytes` rejections.
async fn read_body(req: Request) -> Result<(Parts, Bytes), AppError> {
    let (parts, body) = req.with_limited_body().into_parts();
    match body.collect().await {
        Ok(collected) => Ok((parts, collected.to_bytes())),
        Err(error) => {
            let too_large = std::iter::successors(
                Some(&error as &(dyn std::error::Error + 'static)),
                |error| error.source(),
            )
            .any(|error| error.is::<http_body_util::LengthLimitError>());
            Err(if too_large {
                rejected(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Failed to buffer the request body: length limit exceeded".to_owned(),
                )
            } else {
                rejected(
                    StatusCode::BAD_REQUEST,
                    format!("Failed to buffer the request body: {error}"),
                )
            })
        }
    }
}

/// Deserializes URL-encoded pairs, reporting serde errors at their path.
fn urlencoded<T: DeserializeOwned>(input: &[u8]) -> Result<T, Failure> {
    let deserializer = serde_urlencoded::Deserializer::new(form_urlencoded::parse(input));
    failure::encoding(|| serde_path_to_error::deserialize(deserializer))
        .map_err(|error| failure::from_serde(error.path(), &error.inner().to_string()))
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
