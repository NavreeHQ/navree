//! The server's methods: lists and runs the registry's items for the
//! caller of each request, on top of the protocol layer (`crate::protocol`),
//! for both transports.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use futures::FutureExt as _;
use http::StatusCode;
use navree_access::{AbilityFactory, AuthorizedRecord, PrincipalAccess, Rules};
use navree_contracts::{AuditEntry, AuditKind, AuditSink, Clock, Principal};
use navree_db::{Db, Uow};
use navree_http::{AccessSpec, AppError, ErrorMapper};
use navree_kernel::AppContext;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::content::{PromptMessage, text_block};
use crate::context::{ContextParts, McpClient, McpContext};
use crate::item::{Call, Handler, McpOutput, PromptDef, ResourceDef, ToolDef};
use crate::notifier::{Lists, McpNotifier};
use crate::observe;
use crate::options::McpSettings;
use crate::protocol::jsonrpc::{self, RESOURCE_NOT_FOUND, Request, RpcError};
use crate::protocol::lifecycle::{
    self, Filter, META_SERVER_INFO, META_SUBSCRIPTION_ID, Prepared, ServerInfo,
};
use crate::protocol::outlet::Outlet;
use crate::protocol::{
    Implementation, Prompt, PromptArgumentInfo, Resource, ResourceTemplate, Tool, ToolAnnotations,
};
use crate::registry::McpRegistry;
use crate::schema::coerce_prompt_arguments;
use crate::session::Session;

/// What every request's handler shares: the app and its services.
pub(crate) struct Shared {
    pub(crate) app: AppContext,
    pub(crate) registry: Arc<McpRegistry>,
    pub(crate) notifier: Arc<McpNotifier>,
    factory: Option<Arc<AbilityFactory>>,
    db: Option<Arc<Db>>,
    mappers: Vec<ErrorMapper>,
    audit: Option<Arc<dyn AuditSink>>,
    clock: Option<Arc<dyn Clock>>,
    pub(crate) info: ServerInfo,
}

impl Shared {
    /// Resolves the services the handlers use; fails when an item checks
    /// roles, permissions or abilities and the app has no `AccessModule`.
    pub(crate) async fn new(
        app: &AppContext,
        registry: Arc<McpRegistry>,
        settings: &McpSettings,
        notifier: Arc<McpNotifier>,
    ) -> Result<Self, String> {
        let factory = app.try_get::<AbilityFactory>().await;
        if factory.is_none()
            && let Some(item) = registry.needs_access()
        {
            return Err(format!(
                "{item} declares roles, permissions or can: add AccessModule to the app"
            ));
        }
        let mappers = app
            .contributions::<ErrorMapper>()
            .into_iter()
            .map(|c| (*c.value).clone())
            .collect();
        Ok(Self {
            app: app.clone(),
            factory,
            db: app.try_get::<Db>().await,
            mappers,
            audit: app.try_get::<dyn AuditSink>().await,
            clock: app.try_get::<dyn Clock>().await,
            registry,
            notifier,
            info: server_info(settings),
        })
    }
}

/// What `initialize` and `server/discover` say: capabilities, server info
/// and instructions.
fn server_info(settings: &McpSettings) -> ServerInfo {
    ServerInfo {
        implementation: Implementation::new(settings.name.clone(), settings.version.clone())
            .with_title(settings.title.clone()),
        instructions: settings.instructions.clone(),
        capabilities: json!({
            "tools": { "listChanged": true },
            "resources": { "subscribe": true, "listChanged": true },
            "prompts": { "listChanged": true },
        }),
    }
}

/// Who made a request.
#[derive(Clone, Debug)]
pub(crate) struct Caller {
    pub(crate) principal: Principal,
    pub(crate) request_id: String,
    pub(crate) session_id: Option<String>,
}

/// Everything one request runs with, from its transport.
pub(crate) struct Scope {
    pub(crate) caller: Caller,
    /// The session the request belongs to (none on the stateless protocol).
    pub(crate) session: Option<Arc<Session>>,
    /// The client's name and version.
    pub(crate) client: Option<McpClient>,
    /// The request's own stream: progress and listen notifications.
    pub(crate) outlet: Outlet,
    /// Cancelled when the client cancels the request.
    pub(crate) cancel: CancellationToken,
    /// Cancelled when the app shuts down.
    pub(crate) shutdown: CancellationToken,
}

/// The server's methods, shared by every session and transport.
#[derive(Clone)]
pub(crate) struct McpHandler {
    shared: Arc<Shared>,
}

/// What a call needs besides its handler.
struct Invocation<'a> {
    access: &'a AccessSpec,
    scope: &'a Scope,
    principal_access: Arc<PrincipalAccess>,
    arguments: Value,
    params: BTreeMap<String, String>,
    progress: Option<Value>,
}

/// Unregisters a listen stream when its request ends, however it ends.
struct ListenGuard<'a> {
    notifier: &'a McpNotifier,
    id: u64,
}

impl Drop for ListenGuard<'_> {
    fn drop(&mut self) {
        self.notifier.close(self.id);
    }
}

impl McpHandler {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    /// What `initialize` and `server/discover` answer from.
    pub(crate) fn info(&self) -> &ServerInfo {
        &self.shared.info
    }

    /// The app's notifier.
    pub(crate) fn notifier(&self) -> &Arc<McpNotifier> {
        &self.shared.notifier
    }

    /// The input schema of tool `name`.
    pub(crate) fn tool_schema(&self, name: &str) -> Option<Value> {
        self.shared
            .registry
            .tool(name)
            .map(|entry| entry.def.input_schema.clone())
    }

    /// Registers `session` with the notifier, once (at
    /// `notifications/initialized`, and on any request, so restored
    /// sessions are found too).
    pub(crate) fn register(&self, session: &Arc<Session>) {
        if !session.registered.swap(true, Ordering::AcqRel) {
            self.shared.notifier.open_session(session);
        }
    }

    /// Answers one request (other than `initialize`, which the transports
    /// answer): the answer message, or `None` when the client cancelled it
    /// (no answer is sent then). A panic in the server answers with a bare
    /// internal error.
    pub(crate) async fn answer(
        &self,
        scope: &Scope,
        prepared: &Prepared,
        request: Request,
    ) -> Option<Value> {
        let id = request.id.clone();
        let method = request.method.clone();
        let work = AssertUnwindSafe(self.handle(scope, prepared, request)).catch_unwind();
        tokio::select! {
            biased;
            () = scope.cancel.cancelled() => None,
            outcome = work => Some(match outcome {
                Ok(Ok(result)) => jsonrpc::result(&id, result),
                Ok(Err(error)) => jsonrpc::error(&id, &error),
                Err(_) => {
                    tracing::error!(target: "navree::mcp", method = %method, "an MCP request panicked");
                    jsonrpc::error(&id, &RpcError::internal())
                }
            }),
        }
    }

    /// Runs one request: the lifecycle's method checks, the method, and the
    /// shapes of the request's protocol version.
    async fn handle(
        &self,
        scope: &Scope,
        prepared: &Prepared,
        request: Request,
    ) -> Result<Value, RpcError> {
        lifecycle::gate(&request.method, prepared)?;
        let Request { id, method, params } = request;
        let outcome = match method.as_str() {
            "ping" => Ok(json!({})),
            "server/discover" => Ok(self.shared.info.discover_result()),
            "tools/list" => self.list_tools(scope).await,
            "tools/call" => self.call_tool(scope, params).await,
            "resources/list" => self.list_resources(scope, false).await,
            "resources/templates/list" => self.list_resources(scope, true).await,
            "resources/read" => self.read_resource(scope, params).await,
            "resources/subscribe" => self.subscribe(scope, &params, true).await,
            "resources/unsubscribe" => self.subscribe(scope, &params, false).await,
            "subscriptions/listen" => self.listen(scope, &id, &params).await,
            "prompts/list" => self.list_prompts(scope).await,
            "prompts/get" => self.get_prompt(scope, params).await,
            // No completions are offered: an empty list for any argument.
            "completion/complete" => Ok(json!({ "completion": { "values": [] } })),
            _ => Err(RpcError::method_not_found(&method)),
        };
        match outcome {
            Ok(mut result) => {
                lifecycle::shape_result(&method, prepared, &mut result);
                Ok(result)
            }
            Err(mut error) => {
                lifecycle::shape_error(prepared, &mut error);
                Err(error)
            }
        }
    }

    /// The caller's roles, permissions and abilities.
    async fn access(&self, principal: &Principal) -> Result<Arc<PrincipalAccess>, RpcError> {
        match &self.shared.factory {
            Some(factory) => factory.access_for(Some(principal)).await.map_err(|error| {
                tracing::error!(target: "navree::mcp", %error, "loading the caller's access failed");
                RpcError::internal()
            }),
            None => Ok(Arc::new(PrincipalAccess {
                user: Some(principal.user_id),
                roles: Vec::new(),
                permissions: Vec::new(),
                version: 0,
                super_admin: false,
                ability: Arc::new(Rules::new().build()),
            })),
        }
    }

    /// Whether the caller may see (and use) an item.
    fn visible(&self, access: &PrincipalAccess, spec: &AccessSpec) -> bool {
        if !spec.needs_authorizer() {
            return spec.public || spec.authenticated;
        }
        self.shared
            .factory
            .as_ref()
            .is_some_and(|factory| factory.allows(access, spec))
    }

    /// Registers the session and, when the user's access changed since the
    /// session last read a list, tells it to read the lists again.
    fn touch(&self, scope: &Scope, access: &PrincipalAccess) {
        let Some(session) = &scope.session else {
            return;
        };
        self.register(session);
        if session.access_changed(access.version) {
            for method in [
                "notifications/tools/list_changed",
                "notifications/resources/list_changed",
                "notifications/prompts/list_changed",
            ] {
                let _ = session.notify(jsonrpc::notification(method, None));
            }
        }
    }

    /// Notes the access version a list was read at.
    fn listed(scope: &Scope, access: &PrincipalAccess) {
        if let Some(session) = &scope.session {
            session.listed(access.version);
        }
    }

    /// The caller, their access, and the session registered.
    async fn begin(&self, scope: &Scope) -> Result<Arc<PrincipalAccess>, RpcError> {
        let access = self.access(&scope.caller.principal).await?;
        self.touch(scope, &access);
        Ok(access)
    }

    /// Runs one call: the declared record check and the handler in a unit
    /// of work that commits on `Ok`. A panicking handler fails the call
    /// with an internal error (its message stays in the log).
    async fn run(
        &self,
        invocation: Invocation<'_>,
        handler: &Handler,
    ) -> Result<McpOutput, AppError> {
        let scope = invocation.scope;
        let caller = &scope.caller;
        let uow = match &self.shared.db {
            Some(db) => Some(
                db.uow(
                    Some(caller.principal.user_id),
                    Some(caller.request_id.clone()),
                )
                .await,
            ),
            None => None,
        };
        let progress = invocation
            .progress
            .map(|token| (scope.outlet.clone(), token));
        let cx = McpContext::new(ContextParts {
            app: self.shared.app.clone(),
            principal: caller.principal.clone(),
            access: invocation.principal_access.clone(),
            uow: uow.clone(),
            session_id: caller.session_id.clone(),
            client: scope.client.clone(),
            request_id: caller.request_id.clone(),
            progress,
            clock: self.shared.clock.clone(),
            cancel: scope.cancel.clone(),
        });
        let work = async {
            let record = self
                .check_record(
                    invocation.access,
                    &invocation.principal_access,
                    uow.as_ref(),
                    &invocation.arguments,
                    &invocation.params,
                )
                .await?;
            let call = handler(Call {
                cx,
                arguments: invocation.arguments,
                params: invocation.params,
                record,
            });
            match AssertUnwindSafe(call).catch_unwind().await {
                Ok(result) => result,
                Err(panic) => Err(AppError::internal(format!(
                    "the MCP handler panicked: {}",
                    panic_message(panic.as_ref())
                ))),
            }
        };
        let result = match &uow {
            Some(uow) => uow.scope(work).await,
            None => work.await,
        };
        if let Some(uow) = uow {
            finish(&uow, &result).await?;
        }
        result
    }

    /// The `can(.., from = ..)` check of a declaration: the record whose id
    /// is that argument (or URI variable), loaded and authorized.
    async fn check_record(
        &self,
        access: &AccessSpec,
        principal_access: &PrincipalAccess,
        uow: Option<&Uow>,
        arguments: &Value,
        params: &BTreeMap<String, String>,
    ) -> Result<Option<AuthorizedRecord>, AppError> {
        let Some(can) = &access.can else {
            return Ok(None);
        };
        let Some(from) = &can.from else {
            return Ok(None);
        };
        let id = match params.get(from) {
            Some(id) => id.clone(),
            None => match &arguments[from.as_str()] {
                Value::String(id) => id.clone(),
                Value::Number(id) => id.to_string(),
                _ => {
                    return Err(AppError::validation(BTreeMap::from([(
                        from.clone(),
                        vec![format!("{from} should not be empty")],
                    )])));
                }
            },
        };
        let factory = self
            .shared
            .factory
            .as_ref()
            .ok_or_else(|| AppError::internal("record checks need AccessModule in the app"))?;
        let uow =
            uow.ok_or_else(|| AppError::internal("record checks need DatabaseModule in the app"))?;
        factory
            .authorize_record(principal_access, uow, &can.action, &can.subject, &id)
            .await
            .map(Some)
    }

    /// Maps, logs and audits a failed call; returns the error to report.
    async fn report(&self, error: AppError, method: &str, name: &str, caller: &Caller) -> AppError {
        let error = if error.status == StatusCode::INTERNAL_SERVER_ERROR && error.source.is_some() {
            ErrorMapper::apply(&self.shared.mappers, &error).unwrap_or(error)
        } else {
            error
        };
        if error.status.is_server_error() {
            tracing::error!(
                target: "navree::mcp",
                request_id = %caller.request_id,
                method,
                name,
                error = %error_chain(&error),
                "MCP call failed"
            );
        }
        if error.status == StatusCode::FORBIDDEN {
            tracing::warn!(
                target: "navree::security",
                method,
                name,
                user_id = %caller.principal.user_id,
                "access denied"
            );
            if let Some(sink) = &self.shared.audit {
                let entry = AuditEntry::new(AuditKind::Security, "access.denied")
                    .actor(caller.principal.user_id)
                    .request_id(caller.request_id.clone())
                    .metadata(json!({ "check": "mcp", "method": method, "name": name }));
                if let Err(error) = sink.record(entry).await {
                    tracing::warn!(target: "navree::audit", %error, "could not write the access denial audit entry");
                }
            }
        }
        error
    }

    /// Reads the resource at `uri` for an embedded prompt message.
    async fn embedded(
        &self,
        uri: &str,
        scope: &Scope,
        access: &Arc<PrincipalAccess>,
    ) -> Result<Value, AppError> {
        let (entry, params) = self
            .shared
            .registry
            .resource(uri)
            .filter(|(entry, _)| self.visible(access, &entry.def.access))
            .ok_or_else(AppError::not_found)?;
        let output = self
            .run(
                Invocation {
                    access: &entry.def.access,
                    scope,
                    principal_access: access.clone(),
                    arguments: Value::Object(Map::new()),
                    params,
                    progress: None,
                },
                &entry.handler,
            )
            .await?;
        resource_contents(output, uri, &entry.def).map(|mut contents| contents.swap_remove(0))
    }

    async fn list_tools(&self, scope: &Scope) -> Result<Value, RpcError> {
        let access = self.begin(scope).await?;
        Self::listed(scope, &access);
        let tools: Vec<Value> = self
            .shared
            .registry
            .tools
            .iter()
            .filter(|entry| self.visible(&access, &entry.def.access))
            .map(|entry| to_json(&tool(&entry.def)))
            .collect();
        Ok(json!({ "tools": tools, "cacheScope": "private" }))
    }

    async fn call_tool(&self, scope: &Scope, params: Map<String, Value>) -> Result<Value, RpcError> {
        let started = Instant::now();
        let name = required_string(&params, "name")?;
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => Value::Object(Map::new()),
            Some(Value::Object(arguments)) => Value::Object(arguments.clone()),
            Some(_) => return Err(RpcError::invalid_params("arguments must be an object")),
        };
        let access = self.begin(scope).await?;
        let caller = &scope.caller;
        let Some(entry) = self
            .shared
            .registry
            .tool(&name)
            .filter(|entry| self.visible(&access, &entry.def.access))
        else {
            observe::call(
                "tools/call",
                None,
                &caller.principal,
                &caller.request_id,
                scope.client.as_ref(),
                started,
                "unknown",
            );
            return Err(RpcError::invalid_params(format!("Unknown tool: {name}")));
        };
        let outcome = self
            .run(
                Invocation {
                    access: &entry.def.access,
                    scope,
                    principal_access: access,
                    arguments,
                    params: BTreeMap::new(),
                    progress: lifecycle::progress_token(&params),
                },
                &entry.handler,
            )
            .await;
        let (result, label) = match outcome {
            Ok(output) => (tool_success(output), "ok"),
            Err(error) => {
                let error = self
                    .report(error, "tools/call", &entry.def.name, caller)
                    .await;
                (tool_error(&error), "error")
            }
        };
        observe::call(
            "tools/call",
            Some(&entry.def.name),
            &caller.principal,
            &caller.request_id,
            scope.client.as_ref(),
            started,
            label,
        );
        Ok(result)
    }

    async fn list_resources(&self, scope: &Scope, templates: bool) -> Result<Value, RpcError> {
        let access = self.begin(scope).await?;
        Self::listed(scope, &access);
        let visible = self
            .shared
            .registry
            .resources
            .iter()
            .filter(|resource| resource.uri.is_template() == templates)
            .filter(|resource| self.visible(&access, &resource.entry.def.access));
        Ok(if templates {
            let templates: Vec<Value> = visible
                .map(|resource| to_json(&resource_template(&resource.entry.def)))
                .collect();
            json!({ "resourceTemplates": templates, "cacheScope": "private" })
        } else {
            let resources: Vec<Value> = visible
                .map(|resource| to_json(&fixed_resource(&resource.entry.def)))
                .collect();
            json!({ "resources": resources, "cacheScope": "private" })
        })
    }

    async fn read_resource(
        &self,
        scope: &Scope,
        params: Map<String, Value>,
    ) -> Result<Value, RpcError> {
        let started = Instant::now();
        let uri = required_string(&params, "uri")?;
        let access = self.begin(scope).await?;
        let caller = &scope.caller;
        let Some((entry, uri_params)) = self
            .shared
            .registry
            .resource(&uri)
            .filter(|(entry, _)| self.visible(&access, &entry.def.access))
        else {
            observe::call(
                "resources/read",
                None,
                &caller.principal,
                &caller.request_id,
                scope.client.as_ref(),
                started,
                "unknown",
            );
            return Err(resource_not_found(&uri));
        };
        let outcome = self
            .run(
                Invocation {
                    access: &entry.def.access,
                    scope,
                    principal_access: access,
                    arguments: Value::Object(Map::new()),
                    params: uri_params,
                    progress: lifecycle::progress_token(&params),
                },
                &entry.handler,
            )
            .await
            .and_then(|output| resource_contents(output, &uri, &entry.def));
        let label = if outcome.is_ok() { "ok" } else { "error" };
        let answer = match outcome {
            Ok(contents) => Ok(json!({ "contents": contents })),
            Err(error) => {
                let error = self
                    .report(error, "resources/read", &entry.def.name, caller)
                    .await;
                Err(rpc_error(&error))
            }
        };
        observe::call(
            "resources/read",
            Some(&entry.def.name),
            &caller.principal,
            &caller.request_id,
            scope.client.as_ref(),
            started,
            label,
        );
        answer
    }

    async fn subscribe(
        &self,
        scope: &Scope,
        params: &Map<String, Value>,
        subscribed: bool,
    ) -> Result<Value, RpcError> {
        let uri = required_string(params, "uri")?;
        if subscribed {
            let access = self.begin(scope).await?;
            let visible = self
                .shared
                .registry
                .resource(&uri)
                .is_some_and(|(entry, _)| self.visible(&access, &entry.def.access));
            if !visible {
                return Err(resource_not_found(&uri));
            }
        }
        let session = scope
            .caller
            .session_id
            .as_deref()
            .ok_or_else(|| RpcError::invalid_request("subscriptions need a session"))?;
        self.shared.notifier.subscribe(session, &uri, subscribed);
        Ok(json!({}))
    }

    /// `subscriptions/listen`: acknowledges the accepted filter on the
    /// request's stream, then sends the matching notifications there until
    /// the client cancels the request (or the app shuts down, which ends it
    /// with a result).
    async fn listen(
        &self,
        scope: &Scope,
        id: &Value,
        params: &Map<String, Value>,
    ) -> Result<Value, RpcError> {
        let info = &self.shared.info;
        let accepted = Filter::requested(params).accepted(info);
        let acknowledgment = jsonrpc::notification(
            "notifications/subscriptions/acknowledged",
            Some(json!({
                "notifications": accepted.to_value(),
                "_meta": { META_SUBSCRIPTION_ID: id },
            })),
        );
        if !scope.outlet.deliver(acknowledgment).await {
            return Err(RpcError::internal());
        }
        let access = self.access(&scope.caller.principal).await?;
        let resources: BTreeSet<String> = accepted
            .resource_subscriptions
            .iter()
            .filter(|uri| {
                self.shared
                    .registry
                    .resource(uri)
                    .is_some_and(|(entry, _)| self.visible(&access, &entry.def.access))
            })
            .cloned()
            .collect();
        let lists = Lists {
            tools: accepted.tools,
            resources: accepted.resources,
            prompts: accepted.prompts,
        };
        let listener = self
            .shared
            .notifier
            .listen(scope.outlet.clone(), id.clone(), lists, resources);
        let _guard = ListenGuard {
            notifier: &self.shared.notifier,
            id: listener,
        };
        scope.shutdown.cancelled().await;
        Ok(json!({
            "resultType": "complete",
            "_meta": { META_SUBSCRIPTION_ID: id, META_SERVER_INFO: info.implementation },
        }))
    }

    async fn list_prompts(&self, scope: &Scope) -> Result<Value, RpcError> {
        let access = self.begin(scope).await?;
        Self::listed(scope, &access);
        let prompts: Vec<Value> = self
            .shared
            .registry
            .prompts
            .iter()
            .filter(|entry| self.visible(&access, &entry.def.access))
            .map(|entry| to_json(&prompt(&entry.def)))
            .collect();
        Ok(json!({ "prompts": prompts, "cacheScope": "private" }))
    }

    async fn get_prompt(&self, scope: &Scope, params: Map<String, Value>) -> Result<Value, RpcError> {
        let started = Instant::now();
        let name = required_string(&params, "name")?;
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(arguments)) => arguments.clone(),
            Some(_) => return Err(RpcError::invalid_params("arguments must be an object")),
        };
        let access = self.begin(scope).await?;
        let caller = &scope.caller;
        let Some(entry) = self
            .shared
            .registry
            .prompt(&name)
            .filter(|entry| self.visible(&access, &entry.def.access))
        else {
            observe::call(
                "prompts/get",
                None,
                &caller.principal,
                &caller.request_id,
                scope.client.as_ref(),
                started,
                "unknown",
            );
            return Err(RpcError::invalid_params(format!("Unknown prompt: {name}")));
        };
        let arguments = coerce_prompt_arguments(&entry.def.arguments_schema, arguments);
        let outcome = self
            .run(
                Invocation {
                    access: &entry.def.access,
                    scope,
                    principal_access: access.clone(),
                    arguments,
                    params: BTreeMap::new(),
                    progress: lifecycle::progress_token(&params),
                },
                &entry.handler,
            )
            .await;
        let rendered = match outcome {
            Ok(output) => {
                let mut messages = Vec::new();
                let mut failed = None;
                for message in prompt_messages(output) {
                    let embedded = match message.uri() {
                        Some(uri) => match self.embedded(uri, scope, &access).await {
                            Ok(contents) => Some(contents),
                            Err(error) => {
                                failed = Some(error);
                                break;
                            }
                        },
                        None => None,
                    };
                    messages.push(message.into_mcp(embedded));
                }
                match failed {
                    Some(error) => Err(error),
                    None => Ok(messages),
                }
            }
            Err(error) => Err(error),
        };
        let label = if rendered.is_ok() { "ok" } else { "error" };
        let answer = match rendered {
            Ok(messages) => {
                let mut result = json!({ "messages": messages });
                if let Some(description) = &entry.def.description {
                    result["description"] = json!(description);
                }
                Ok(result)
            }
            Err(error) => {
                let error = self
                    .report(error, "prompts/get", &entry.def.name, caller)
                    .await;
                Err(rpc_error(&error))
            }
        };
        observe::call(
            "prompts/get",
            Some(&entry.def.name),
            &caller.principal,
            &caller.request_id,
            scope.client.as_ref(),
            started,
            label,
        );
        answer
    }
}

/// A string parameter the method needs.
fn required_string(params: &Map<String, Value>, key: &str) -> Result<String, RpcError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| RpcError::invalid_params(format!("Invalid params: {key} must be a string")))
}

/// The `-32002` error of an unknown (or hidden) resource.
fn resource_not_found(uri: &str) -> RpcError {
    RpcError::new(
        RESOURCE_NOT_FOUND,
        "Resource not found",
        Some(json!({ "uri": uri })),
    )
}

/// The text of a panic's payload, for the log.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a non-text panic".to_owned())
}

/// Commits the unit of work after `Ok`, rolls it back after `Err`.
async fn finish<T>(uow: &Uow, result: &Result<T, AppError>) -> Result<(), AppError> {
    if result.is_ok() {
        uow.commit().await.map_err(AppError::internal)
    } else {
        if let Err(error) = uow.rollback().await {
            tracing::warn!(target: "navree::mcp", %error, "rolling back the MCP call's unit of work failed");
        }
        Ok(())
    }
}

/// The error and its source chain, for logs.
fn error_chain(error: &AppError) -> String {
    let mut out = error.to_string();
    let mut next = error
        .source
        .as_deref()
        .map(|e| e as &(dyn std::error::Error + 'static));
    while let Some(cause) = next {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        next = cause.source();
    }
    out
}

/// A JSON-RPC error for a failed resource read or prompt: 400 → invalid
/// params, 404 → resource not found, anything else → internal error; the
/// data is Navree's error body (which never carries an internal error's
/// text).
fn rpc_error(error: &AppError) -> RpcError {
    let body = error.body();
    let message = body["message"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| body["message"].to_string());
    let code = match error.status {
        StatusCode::BAD_REQUEST => jsonrpc::INVALID_PARAMS,
        StatusCode::NOT_FOUND => RESOURCE_NOT_FOUND,
        _ => jsonrpc::INTERNAL_ERROR,
    };
    RpcError::new(code, message, Some(body))
}

/// A protocol description as JSON.
fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or_else(|_| json!({}))
}

/// The protocol tool of a definition.
pub(crate) fn tool(def: &ToolDef) -> Tool {
    Tool {
        name: def.name.clone(),
        title: def.title.clone(),
        description: def.description.clone(),
        input_schema: def.input_schema.as_object().cloned().unwrap_or_default(),
        output_schema: None,
        annotations: Some(ToolAnnotations {
            title: def.title.clone(),
            read_only_hint: Some(def.read_only),
            destructive_hint: Some(def.destructive),
            idempotent_hint: Some(def.idempotent),
            open_world_hint: Some(def.open_world),
        }),
    }
}

/// The protocol resource of a fixed-URI definition.
fn fixed_resource(def: &ResourceDef) -> Resource {
    Resource {
        uri: def.uri.clone(),
        name: def.name.clone(),
        title: def.title.clone(),
        description: def.description.clone(),
        mime_type: def.mime_type.clone(),
    }
}

/// The protocol template of a templated definition.
fn resource_template(def: &ResourceDef) -> ResourceTemplate {
    ResourceTemplate {
        uri_template: def.uri.clone(),
        name: def.name.clone(),
        title: def.title.clone(),
        description: def.description.clone(),
        mime_type: def.mime_type.clone(),
    }
}

/// The protocol prompt of a definition.
fn prompt(def: &PromptDef) -> Prompt {
    Prompt {
        name: def.name.clone(),
        title: def.title.clone(),
        description: def.description.clone(),
        arguments: (!def.arguments.is_empty()).then(|| {
            def.arguments
                .iter()
                .map(|argument| PromptArgumentInfo {
                    name: argument.name.clone(),
                    description: argument.description.clone(),
                    required: Some(argument.required),
                })
                .collect()
        }),
    }
}

/// A tool's successful result: structured content and JSON text for a
/// JSON object, text for a string, `{"result": value}` as structured
/// content for other values.
fn tool_success(output: McpOutput) -> Value {
    match output {
        McpOutput::Json(Value::String(text)) => {
            json!({ "content": [text_block(text)], "isError": false })
        }
        McpOutput::Json(value @ Value::Object(_)) => json!({
            "content": [text_block(value.to_string())],
            "structuredContent": value,
            "isError": false,
        }),
        McpOutput::Json(value) => json!({
            "content": [text_block(value.to_string())],
            "structuredContent": { "result": value },
            "isError": false,
        }),
        McpOutput::Contents(contents) => {
            let content: Vec<Value> = contents
                .into_iter()
                .map(|contents| json!({ "type": "resource", "resource": contents.into_mcp("about:blank") }))
                .collect();
            json!({ "content": content, "isError": false })
        }
        McpOutput::Messages(messages) => {
            let content: Vec<Value> = messages
                .into_iter()
                .map(|message| text_block(message.text().unwrap_or_default()))
                .collect();
            json!({ "content": content, "isError": false })
        }
    }
}

/// A failed tool call: `isError`, with Navree's error body as text and as
/// structured content. The body of an internal error is generic: what the
/// error said stays in the log; a tool that wants the client to see its
/// message returns a client error (`AppError::conflict(..)`, ...).
fn tool_error(error: &AppError) -> Value {
    let body = error.body();
    json!({
        "content": [text_block(body.to_string())],
        "structuredContent": body,
        "isError": true,
    })
}

/// The contents of a resource read.
fn resource_contents(output: McpOutput, uri: &str, def: &ResourceDef) -> Result<Vec<Value>, AppError> {
    let text = |text: String, default_mime: &str| {
        json!({
            "uri": uri,
            "mimeType": def.mime_type.clone().unwrap_or_else(|| default_mime.to_owned()),
            "text": text,
        })
    };
    match output {
        McpOutput::Json(Value::String(string)) => Ok(vec![text(string, "text/plain")]),
        McpOutput::Json(value) => Ok(vec![text(value.to_string(), "application/json")]),
        McpOutput::Contents(contents) if !contents.is_empty() => Ok(contents
            .into_iter()
            .map(|contents| contents.into_mcp(uri))
            .collect()),
        McpOutput::Contents(_) => Err(AppError::internal(format!(
            "resource `{}` returned no contents",
            def.name
        ))),
        McpOutput::Messages(_) => Err(AppError::internal(format!(
            "resource `{}` returned prompt messages",
            def.name
        ))),
    }
}

/// The messages of a prompt's output.
fn prompt_messages(output: McpOutput) -> Vec<PromptMessage> {
    match output {
        McpOutput::Messages(messages) => messages,
        McpOutput::Json(Value::String(text)) => vec![PromptMessage::user(text)],
        McpOutput::Json(value) => vec![PromptMessage::user(value.to_string())],
        McpOutput::Contents(contents) => contents
            .into_iter()
            .filter_map(|contents| contents.as_text().map(PromptMessage::user))
            .collect(),
    }
}
