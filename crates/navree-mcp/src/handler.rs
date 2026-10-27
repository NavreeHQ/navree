//! The protocol side: an rmcp `ServerHandler` that lists and runs the
//! registry's items for the caller of each request.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use http::StatusCode;
use navree_access::{AbilityFactory, AuthorizedRecord, PrincipalAccess, Rules};
use navree_contracts::{AuditEntry, AuditKind, AuditSink, AuthState, Clock, Principal, RequestId};
use navree_db::{Db, Uow};
use navree_http::{AccessSpec, AppError, ErrorMapper};
use navree_kernel::AppContext;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorCode,
    ErrorData, Extensions, GetPromptRequestParams, GetPromptResponse, GetPromptResult,
    Implementation, InitializeResult, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, Prompt,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
    ResourceTemplate, ServerCapabilities, SubscribeRequestParams, SubscriptionFilter, Tool,
    UnsubscribeRequestParams,
};
use rmcp::service::{NotificationContext, RequestContext, SubscriptionContext};
use rmcp::{Peer, RoleServer, ServerHandler};
use serde_json::{Map, Value, json};

use crate::content::PromptMessage;
use crate::context::{ContextParts, McpClient, McpContext};
use crate::item::{Call, Handler, McpOutput, PromptDef, ResourceDef, ToolDef};
use crate::notifier::{Lists, McpNotifier};
use crate::observe;
use crate::options::McpSettings;
use crate::registry::McpRegistry;
use crate::schema::coerce_prompt_arguments;

/// What every session's handler shares: the app and its services.
pub(crate) struct Shared {
    pub(crate) app: AppContext,
    pub(crate) registry: Arc<McpRegistry>,
    pub(crate) notifier: Arc<McpNotifier>,
    factory: Option<Arc<AbilityFactory>>,
    db: Option<Arc<Db>>,
    mappers: Vec<ErrorMapper>,
    audit: Option<Arc<dyn AuditSink>>,
    clock: Option<Arc<dyn Clock>>,
    info: InitializeResult,
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

/// The `initialize` answer: capabilities, server info and instructions.
fn server_info(settings: &McpSettings) -> InitializeResult {
    let capabilities: ServerCapabilities = serde_json::from_value(json!({
        "tools": { "listChanged": true },
        "resources": { "subscribe": true, "listChanged": true },
        "prompts": { "listChanged": true },
    }))
    .expect("valid server capabilities");
    let mut info = InitializeResult::new(capabilities).with_server_info(
        Implementation::new(settings.name.clone(), settings.version.clone())
            .with_title(settings.title.clone()),
    );
    if let Some(instructions) = &settings.instructions {
        info = info.with_instructions(instructions.clone());
    }
    info
}

/// The process user of the stdio transport.
#[derive(Clone)]
pub(crate) struct LocalCaller {
    pub(crate) principal: Principal,
    pub(crate) session_id: String,
}

/// One session's handler (the HTTP transport builds one per session; the
/// stdio transport one for the process).
#[derive(Clone)]
pub(crate) struct McpHandler {
    shared: Arc<Shared>,
    local: Option<LocalCaller>,
    session: Arc<SessionSlot>,
}

/// Per-session state: the notifier registration and the access version of
/// the last list the session read.
struct SessionSlot {
    notifier: Arc<McpNotifier>,
    listener: Mutex<Option<(String, u64)>>,
    listed_version: AtomicI64,
}

/// No list read yet.
const NEVER_LISTED: i64 = i64::MIN;

impl Drop for SessionSlot {
    fn drop(&mut self) {
        let listener = self
            .listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some((_, id)) = listener {
            self.notifier.close(id);
        }
    }
}

/// Who made a request.
struct Caller {
    principal: Principal,
    request_id: String,
    session_id: Option<String>,
}

/// What a call needs besides its handler.
struct Invocation<'a> {
    access: &'a AccessSpec,
    caller: &'a Caller,
    principal_access: Arc<PrincipalAccess>,
    arguments: Value,
    params: BTreeMap<String, String>,
    client: Option<McpClient>,
    peer: &'a Peer<RoleServer>,
    progress: Option<rmcp::model::ProgressToken>,
}

impl McpHandler {
    pub(crate) fn new(shared: Arc<Shared>, local: Option<LocalCaller>) -> Self {
        let notifier = shared.notifier.clone();
        Self {
            shared,
            local,
            session: Arc::new(SessionSlot {
                notifier,
                listener: Mutex::new(None),
                listed_version: AtomicI64::new(NEVER_LISTED),
            }),
        }
    }

    /// The caller of a request: the stdio user, or the principal the HTTP
    /// pipeline authenticated.
    fn caller(&self, extensions: &Extensions) -> Result<Caller, ErrorData> {
        if let Some(local) = &self.local {
            return Ok(Caller {
                principal: local.principal.clone(),
                request_id: uuid::Uuid::now_v7().to_string(),
                session_id: Some(local.session_id.clone()),
            });
        }
        let parts = extensions.get::<http::request::Parts>().ok_or_else(|| {
            ErrorData::internal_error("the MCP request has no HTTP request", None)
        })?;
        let principal = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.clone())
            .ok_or_else(|| ErrorData::invalid_request("Unauthorized", None))?;
        let request_id = parts
            .extensions
            .get::<RequestId>()
            .map(|id| id.as_str().to_owned())
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let session_id = parts
            .headers
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        Ok(Caller {
            principal,
            request_id,
            session_id,
        })
    }

    /// The caller's roles, permissions and abilities.
    async fn access(&self, principal: &Principal) -> Result<Arc<PrincipalAccess>, ErrorData> {
        match &self.shared.factory {
            Some(factory) => factory
                .access_for(Some(principal))
                .await
                .map_err(|error| ErrorData::internal_error(error.to_string(), None)),
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

    /// Registers the session with the notifier, once (at
    /// `notifications/initialized`, and on any request, so restored sessions
    /// are found too).
    fn register(&self, caller: &Caller, peer: &Peer<RoleServer>) {
        let Some(session) = &caller.session_id else {
            return;
        };
        let mut listener = self
            .session
            .listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if listener.as_ref().is_none_or(|(id, _)| id != session) {
            let id = self.shared.notifier.open_session(session, peer.clone());
            *listener = Some((session.clone(), id));
        }
    }

    /// Registers the session and, when the user's access changed since the
    /// session last read a list, tells it to read the lists again.
    async fn touch(&self, caller: &Caller, access: &PrincipalAccess, peer: &Peer<RoleServer>) {
        self.register(caller, peer);
        let listed = self.session.listed_version.load(Ordering::Relaxed);
        if listed != NEVER_LISTED
            && listed != access.version
            && self
                .session
                .listed_version
                .compare_exchange(listed, access.version, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            let _ = peer.notify_tool_list_changed().await;
            let _ = peer.notify_resource_list_changed().await;
            let _ = peer.notify_prompt_list_changed().await;
        }
    }

    /// Notes the access version a list was read at.
    fn listed(&self, access: &PrincipalAccess) {
        self.session
            .listed_version
            .store(access.version, Ordering::Relaxed);
    }

    /// The client's name and version, when it sent them.
    fn client(peer: &Peer<RoleServer>) -> Option<McpClient> {
        peer.peer_info().map(|info| McpClient {
            name: info.client_info.name.clone(),
            version: info.client_info.version.clone(),
        })
    }

    /// Runs one call: the declared record check and the handler in a unit
    /// of work that commits on `Ok`.
    async fn run(
        &self,
        invocation: Invocation<'_>,
        handler: &Handler,
    ) -> Result<McpOutput, AppError> {
        let caller = invocation.caller;
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
            .map(|token| (invocation.peer.clone(), token));
        let cx = McpContext::new(ContextParts {
            app: self.shared.app.clone(),
            principal: caller.principal.clone(),
            access: invocation.principal_access.clone(),
            uow: uow.clone(),
            session_id: caller.session_id.clone(),
            client: invocation.client,
            request_id: caller.request_id.clone(),
            progress,
            clock: self.shared.clock.clone(),
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
            handler(Call {
                cx,
                arguments: invocation.arguments,
                params: invocation.params,
                record,
            })
            .await
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
        caller: &Caller,
        access: &Arc<PrincipalAccess>,
        peer: &Peer<RoleServer>,
    ) -> Result<rmcp::model::ResourceContents, AppError> {
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
                    caller,
                    principal_access: access.clone(),
                    arguments: Value::Object(Map::new()),
                    params,
                    client: Self::client(peer),
                    peer,
                    progress: None,
                },
                &entry.handler,
            )
            .await?;
        resource_contents(output, uri, &entry.def).map(|mut contents| contents.swap_remove(0))
    }
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
/// data is Navree's error body.
fn rpc_error(error: &AppError) -> ErrorData {
    let body = error.body();
    let message = body["message"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| body["message"].to_string());
    let code = match error.status {
        StatusCode::BAD_REQUEST => ErrorCode::INVALID_PARAMS,
        StatusCode::NOT_FOUND => ErrorCode::RESOURCE_NOT_FOUND,
        _ => ErrorCode::INTERNAL_ERROR,
    };
    ErrorData::new(code, message, Some(body))
}

/// The protocol tool of a definition.
fn tool(def: &ToolDef) -> Tool {
    let mut annotations = json!({
        "readOnlyHint": def.read_only,
        "destructiveHint": def.destructive,
        "idempotentHint": def.idempotent,
        "openWorldHint": def.open_world,
    });
    if let Some(title) = &def.title {
        annotations["title"] = json!(title);
    }
    let mut value = json!({
        "name": def.name,
        "inputSchema": def.input_schema,
        "annotations": annotations,
    });
    if let Some(title) = &def.title {
        value["title"] = json!(title);
    }
    if let Some(description) = &def.description {
        value["description"] = json!(description);
    }
    serde_json::from_value(value).expect("a valid tool definition")
}

/// The fixed resource or template of a definition, with its common fields.
fn resource_fields(def: &ResourceDef, uri_key: &str) -> Value {
    let mut value = json!({ uri_key: def.uri, "name": def.name });
    if let Some(title) = &def.title {
        value["title"] = json!(title);
    }
    if let Some(description) = &def.description {
        value["description"] = json!(description);
    }
    if let Some(mime) = &def.mime_type {
        value["mimeType"] = json!(mime);
    }
    value
}

/// The protocol prompt of a definition.
fn prompt(def: &PromptDef) -> Prompt {
    let mut value = json!({ "name": def.name });
    if let Some(title) = &def.title {
        value["title"] = json!(title);
    }
    if let Some(description) = &def.description {
        value["description"] = json!(description);
    }
    if !def.arguments.is_empty() {
        value["arguments"] = def
            .arguments
            .iter()
            .map(|argument| {
                let mut value = json!({ "name": argument.name, "required": argument.required });
                if let Some(description) = &argument.description {
                    value["description"] = json!(description);
                }
                value
            })
            .collect();
    }
    serde_json::from_value(value).expect("a valid prompt definition")
}

/// A tool's successful result: structured content and JSON text for a
/// JSON object, text for a string, `{"result": value}` as structured
/// content for other values.
fn tool_success(output: McpOutput) -> CallToolResult {
    match output {
        McpOutput::Json(Value::String(text)) => {
            CallToolResult::success(vec![ContentBlock::text(text)])
        }
        McpOutput::Json(value @ Value::Object(_)) => CallToolResult::structured(value),
        McpOutput::Json(value) => {
            let mut result = CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
            result.structured_content = Some(json!({ "result": value }));
            result
        }
        McpOutput::Contents(contents) => CallToolResult::success(
            contents
                .into_iter()
                .map(|contents| ContentBlock::resource(contents.into_mcp("about:blank")))
                .collect(),
        ),
        McpOutput::Messages(messages) => CallToolResult::success(
            messages
                .into_iter()
                .map(|message| ContentBlock::text(message.text().unwrap_or_default().to_owned()))
                .collect(),
        ),
    }
}

/// A failed tool call: `isError`, with Navree's error body as text and as
/// structured content.
fn tool_error(error: &AppError) -> CallToolResult {
    let body = error.body();
    let mut result = CallToolResult::error(vec![ContentBlock::text(body.to_string())]);
    result.structured_content = Some(body);
    result
}

/// The contents of a resource read.
fn resource_contents(
    output: McpOutput,
    uri: &str,
    def: &ResourceDef,
) -> Result<Vec<rmcp::model::ResourceContents>, AppError> {
    let text =
        |text: String, default_mime: &str| rmcp::model::ResourceContents::TextResourceContents {
            uri: uri.to_owned(),
            mime_type: Some(
                def.mime_type
                    .clone()
                    .unwrap_or_else(|| default_mime.to_owned()),
            ),
            text,
            meta: None,
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

impl ServerHandler for McpHandler {
    fn get_info(&self) -> InitializeResult {
        self.shared.info.clone()
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        if let Ok(caller) = self.caller(&context.extensions) {
            self.register(&caller, &context.peer);
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let caller = self.caller(&context.extensions)?;
        let access = self.access(&caller.principal).await?;
        self.touch(&caller, &access, &context.peer).await;
        self.listed(&access);
        let tools = self
            .shared
            .registry
            .tools
            .iter()
            .filter(|entry| self.visible(&access, &entry.def.access))
            .map(|entry| tool(&entry.def))
            .collect();
        let mut result = ListToolsResult::with_all_items(tools);
        result.cache_scope = Some(CacheScope::Private);
        Ok(result)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let started = Instant::now();
        let caller = self.caller(&context.extensions)?;
        let access = self.access(&caller.principal).await?;
        self.touch(&caller, &access, &context.peer).await;
        let client = Self::client(&context.peer);
        let name = request.name.to_string();
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
                client.as_ref(),
                started,
                "unknown",
            );
            return Err(ErrorData::invalid_params(
                format!("Unknown tool: {name}"),
                None,
            ));
        };
        let outcome = self
            .run(
                Invocation {
                    access: &entry.def.access,
                    caller: &caller,
                    principal_access: access,
                    arguments: Value::Object(request.arguments.unwrap_or_default()),
                    params: BTreeMap::new(),
                    client: client.clone(),
                    peer: &context.peer,
                    progress: context.meta.get_progress_token(),
                },
                &entry.handler,
            )
            .await;
        let (result, label) = match outcome {
            Ok(output) => (tool_success(output), "ok"),
            Err(error) => {
                let error = self
                    .report(error, "tools/call", &entry.def.name, &caller)
                    .await;
                (tool_error(&error), "error")
            }
        };
        observe::call(
            "tools/call",
            Some(&entry.def.name),
            &caller.principal,
            &caller.request_id,
            client.as_ref(),
            started,
            label,
        );
        Ok(CallToolResponse::Complete(result))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let caller = self.caller(&context.extensions)?;
        let access = self.access(&caller.principal).await?;
        self.touch(&caller, &access, &context.peer).await;
        self.listed(&access);
        let resources: Vec<Resource> = self
            .shared
            .registry
            .resources
            .iter()
            .filter(|resource| !resource.uri.is_template())
            .filter(|resource| self.visible(&access, &resource.entry.def.access))
            .map(|resource| {
                serde_json::from_value(resource_fields(&resource.entry.def, "uri"))
                    .expect("a valid resource")
            })
            .collect();
        let mut result = ListResourcesResult::with_all_items(resources);
        result.cache_scope = Some(CacheScope::Private);
        Ok(result)
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        let caller = self.caller(&context.extensions)?;
        let access = self.access(&caller.principal).await?;
        self.touch(&caller, &access, &context.peer).await;
        self.listed(&access);
        let templates: Vec<ResourceTemplate> = self
            .shared
            .registry
            .resources
            .iter()
            .filter(|resource| resource.uri.is_template())
            .filter(|resource| self.visible(&access, &resource.entry.def.access))
            .map(|resource| {
                serde_json::from_value(resource_fields(&resource.entry.def, "uriTemplate"))
                    .expect("a valid resource template")
            })
            .collect();
        let mut result = ListResourceTemplatesResult::with_all_items(templates);
        result.cache_scope = Some(CacheScope::Private);
        Ok(result)
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let started = Instant::now();
        let caller = self.caller(&context.extensions)?;
        let access = self.access(&caller.principal).await?;
        self.touch(&caller, &access, &context.peer).await;
        let client = Self::client(&context.peer);
        let uri = request.uri;
        let Some((entry, params)) = self
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
                client.as_ref(),
                started,
                "unknown",
            );
            return Err(ErrorData::resource_not_found(
                "Resource not found",
                Some(json!({ "uri": uri })),
            ));
        };
        let outcome = self
            .run(
                Invocation {
                    access: &entry.def.access,
                    caller: &caller,
                    principal_access: access,
                    arguments: Value::Object(Map::new()),
                    params,
                    client: client.clone(),
                    peer: &context.peer,
                    progress: context.meta.get_progress_token(),
                },
                &entry.handler,
            )
            .await
            .and_then(|output| resource_contents(output, &uri, &entry.def));
        match outcome {
            Ok(contents) => {
                observe::call(
                    "resources/read",
                    Some(&entry.def.name),
                    &caller.principal,
                    &caller.request_id,
                    client.as_ref(),
                    started,
                    "ok",
                );
                Ok(ReadResourceResponse::Complete(ReadResourceResult::new(
                    contents,
                )))
            }
            Err(error) => {
                let error = self
                    .report(error, "resources/read", &entry.def.name, &caller)
                    .await;
                observe::call(
                    "resources/read",
                    Some(&entry.def.name),
                    &caller.principal,
                    &caller.request_id,
                    client.as_ref(),
                    started,
                    "error",
                );
                Err(rpc_error(&error))
            }
        }
    }

    #[allow(deprecated)]
    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let caller = self.caller(&context.extensions)?;
        let access = self.access(&caller.principal).await?;
        self.touch(&caller, &access, &context.peer).await;
        let visible = self
            .shared
            .registry
            .resource(&request.uri)
            .is_some_and(|(entry, _)| self.visible(&access, &entry.def.access));
        if !visible {
            return Err(ErrorData::resource_not_found(
                "Resource not found",
                Some(json!({ "uri": request.uri })),
            ));
        }
        let session = caller
            .session_id
            .ok_or_else(|| ErrorData::invalid_request("subscriptions need a session", None))?;
        self.shared.notifier.subscribe(&session, &request.uri, true);
        Ok(())
    }

    #[allow(deprecated)]
    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let caller = self.caller(&context.extensions)?;
        if let Some(session) = caller.session_id {
            self.shared
                .notifier
                .subscribe(&session, &request.uri, false);
        }
        Ok(())
    }

    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        Some(requested.clone())
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        let request = context.request_context();
        let caller = self.caller(&request.extensions)?;
        let access = self.access(&caller.principal).await?;
        let accepted = context.accepted();
        let resources: BTreeSet<String> = accepted
            .resource_subscriptions
            .iter()
            .flatten()
            .filter(|uri| {
                self.shared
                    .registry
                    .resource(uri)
                    .is_some_and(|(entry, _)| self.visible(&access, &entry.def.access))
            })
            .cloned()
            .collect();
        let lists = Lists {
            tools: accepted.tools_list_changed == Some(true),
            resources: accepted.resources_list_changed == Some(true),
            prompts: accepted.prompts_list_changed == Some(true),
        };
        let id = self
            .shared
            .notifier
            .listen(context.sink().clone(), lists, resources);
        context.cancelled().await;
        self.shared.notifier.close(id);
        Ok(())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let caller = self.caller(&context.extensions)?;
        let access = self.access(&caller.principal).await?;
        self.touch(&caller, &access, &context.peer).await;
        self.listed(&access);
        let prompts = self
            .shared
            .registry
            .prompts
            .iter()
            .filter(|entry| self.visible(&access, &entry.def.access))
            .map(|entry| prompt(&entry.def))
            .collect();
        let mut result = ListPromptsResult::with_all_items(prompts);
        result.cache_scope = Some(CacheScope::Private);
        Ok(result)
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        let started = Instant::now();
        let caller = self.caller(&context.extensions)?;
        let access = self.access(&caller.principal).await?;
        self.touch(&caller, &access, &context.peer).await;
        let client = Self::client(&context.peer);
        let Some(entry) = self
            .shared
            .registry
            .prompt(&request.name)
            .filter(|entry| self.visible(&access, &entry.def.access))
        else {
            observe::call(
                "prompts/get",
                None,
                &caller.principal,
                &caller.request_id,
                client.as_ref(),
                started,
                "unknown",
            );
            return Err(ErrorData::invalid_params(
                format!("Unknown prompt: {}", request.name),
                None,
            ));
        };
        let arguments = coerce_prompt_arguments(
            &entry.def.arguments_schema,
            request.arguments.unwrap_or_default(),
        );
        let outcome = self
            .run(
                Invocation {
                    access: &entry.def.access,
                    caller: &caller,
                    principal_access: access.clone(),
                    arguments,
                    params: BTreeMap::new(),
                    client: client.clone(),
                    peer: &context.peer,
                    progress: context.meta.get_progress_token(),
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
                        Some(uri) => {
                            match self.embedded(uri, &caller, &access, &context.peer).await {
                                Ok(contents) => Some(contents),
                                Err(error) => {
                                    failed = Some(error);
                                    break;
                                }
                            }
                        }
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
        match rendered {
            Ok(messages) => {
                observe::call(
                    "prompts/get",
                    Some(&entry.def.name),
                    &caller.principal,
                    &caller.request_id,
                    client.as_ref(),
                    started,
                    "ok",
                );
                let mut result = GetPromptResult::new(messages);
                if let Some(description) = &entry.def.description {
                    result = result.with_description(description.clone());
                }
                Ok(GetPromptResponse::Complete(result))
            }
            Err(error) => {
                let error = self
                    .report(error, "prompts/get", &entry.def.name, &caller)
                    .await;
                observe::call(
                    "prompts/get",
                    Some(&entry.def.name),
                    &caller.principal,
                    &caller.request_id,
                    client.as_ref(),
                    started,
                    "error",
                );
                Err(rpc_error(&error))
            }
        }
    }
}
