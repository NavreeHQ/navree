//! What a server offers: [`McpItem`]s (tools, resources and prompts) with
//! their definitions, and the [`McpServer`] trait `#[mcp_server]`
//! implements.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;

use navree_access::{ActionMarker, Authorized, AuthorizedRecord, SubjectRecords};
use navree_http::{AccessSpec, AppError, CanSpec};
use navree_kernel::BoxFuture;
use navree_validation::Validate;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::content::{PromptMessage, ResourceContents};
use crate::context::McpContext;
use crate::schema::{PromptArgument, empty_object, input_schema, prompt_arguments};

/// An MCP server: a service whose tools, resources and prompts are offered
/// to MCP clients. `#[mcp_server]` implements it for an `impl` block; write
/// it by hand with [`McpItem`] and the definition builders when a macro is
/// not wanted. Register it with
/// [`ModuleBuilderMcpExt::mcp_server`](crate::ModuleBuilderMcpExt::mcp_server).
pub trait McpServer: Send + Sync + 'static {
    /// The tools, resources and prompts, each with its handler.
    fn items(self: Arc<Self>) -> Vec<McpItem>;
}

/// What a handler returns: a value, text, resource contents or prompt
/// messages.
#[derive(Clone, Debug, PartialEq)]
pub enum McpOutput {
    /// A serialized value: structured content and JSON text for tools,
    /// JSON for resources. A JSON string is sent as plain text.
    Json(Value),
    /// Resource contents.
    Contents(Vec<ResourceContents>),
    /// Prompt messages.
    Messages(Vec<PromptMessage>),
}

/// The future of a handler.
pub type HandlerFuture = BoxFuture<'static, Result<McpOutput, AppError>>;

/// A tool, resource or prompt handler.
pub type Handler = Arc<dyn Fn(Call) -> HandlerFuture + Send + Sync>;

/// A tool, a resource (or resource template) or a prompt, with its handler.
#[derive(Clone)]
pub struct McpItem {
    pub(crate) def: ItemDef,
    pub(crate) handler: Handler,
}

/// The definition of an item.
#[derive(Clone, Debug)]
pub(crate) enum ItemDef {
    Tool(ToolDef),
    Resource(ResourceDef),
    Prompt(PromptDef),
}

impl McpItem {
    /// A tool.
    pub fn tool<F, Fut>(def: ToolDef, handler: F) -> Self
    where
        F: Fn(Call) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<McpOutput, AppError>> + Send + 'static,
    {
        Self::new(ItemDef::Tool(def), handler)
    }

    /// A resource or resource template.
    pub fn resource<F, Fut>(def: ResourceDef, handler: F) -> Self
    where
        F: Fn(Call) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<McpOutput, AppError>> + Send + 'static,
    {
        Self::new(ItemDef::Resource(def), handler)
    }

    /// A prompt.
    pub fn prompt<F, Fut>(def: PromptDef, handler: F) -> Self
    where
        F: Fn(Call) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<McpOutput, AppError>> + Send + 'static,
    {
        Self::new(ItemDef::Prompt(def), handler)
    }

    fn new<F, Fut>(def: ItemDef, handler: F) -> Self
    where
        F: Fn(Call) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<McpOutput, AppError>> + Send + 'static,
    {
        Self {
            def,
            handler: Arc::new(move |call| Box::pin(handler(call))),
        }
    }

    /// `tool`, `resource` or `prompt`.
    pub fn kind(&self) -> &'static str {
        self.def.kind()
    }

    /// The item's name.
    pub fn name(&self) -> &str {
        self.def.name()
    }

    /// Who may use the item.
    pub fn access(&self) -> &AccessSpec {
        self.def.access()
    }
}

impl fmt::Debug for McpItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpItem")
            .field("def", &self.def)
            .finish_non_exhaustive()
    }
}

impl ItemDef {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            ItemDef::Tool(_) => "tool",
            ItemDef::Resource(_) => "resource",
            ItemDef::Prompt(_) => "prompt",
        }
    }

    pub(crate) fn name(&self) -> &str {
        match self {
            ItemDef::Tool(def) => &def.name,
            ItemDef::Resource(def) => &def.name,
            ItemDef::Prompt(def) => &def.name,
        }
    }

    pub(crate) fn access(&self) -> &AccessSpec {
        match self {
            ItemDef::Tool(def) => &def.access,
            ItemDef::Resource(def) => &def.access,
            ItemDef::Prompt(def) => &def.access,
        }
    }
}

/// The access builders shared by the three definitions, the same
/// declarations a route has (deny by default: an item without one fails the
/// boot).
macro_rules! access_builders {
    () => {
        /// Who may use it, as a whole.
        pub fn access(mut self, access: AccessSpec) -> Self {
            self.access = access;
            self
        }

        /// Any signed-in client may use it.
        pub fn public(mut self) -> Self {
            self.access.public = true;
            self
        }

        /// Any signed-in user may use it (no further authorization).
        pub fn authenticated(mut self) -> Self {
            self.access.authenticated = true;
            self
        }

        /// Users with any of the roles may use it.
        pub fn roles(mut self, roles: &[&str]) -> Self {
            self.access
                .roles
                .extend(roles.iter().map(|role| (*role).to_owned()));
            self
        }

        /// Users with every permission may use it.
        pub fn permissions(mut self, permissions: &[&str]) -> Self {
            self.access
                .permissions
                .extend(permissions.iter().map(|p| (*p).to_owned()));
            self
        }

        /// Users who can do `action` on `subject` may use it; with `from`,
        /// the record whose id is that argument (or URI variable) is loaded
        /// and checked on every call.
        pub fn can(mut self, action: &str, subject: &str, from: Option<&str>) -> Self {
            self.access.can = Some(CanSpec {
                action: action.to_owned(),
                subject: subject.to_owned(),
                from: from.map(str::to_owned),
            });
            self
        }
    };
}

/// A tool: an action the model can take, with typed arguments.
#[derive(Clone, Debug)]
pub struct ToolDef {
    /// The name the model calls it by (`list_deals`).
    pub name: String,
    /// A display name.
    pub title: Option<String>,
    /// What it does, what it returns and its limits.
    pub description: Option<String>,
    /// The JSON Schema of its arguments (an object).
    pub input_schema: Value,
    /// `readOnlyHint`: it changes nothing.
    pub read_only: bool,
    /// `destructiveHint`: it may overwrite or remove data.
    pub destructive: bool,
    /// `idempotentHint`: calling it twice does no more than once.
    pub idempotent: bool,
    /// `openWorldHint`: it reaches outside the app.
    pub open_world: bool,
    /// Who may use it.
    pub access: AccessSpec,
}

impl ToolDef {
    /// A tool named `name`, without arguments until
    /// [`input`](ToolDef::input).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: None,
            description: None,
            input_schema: empty_object(),
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: false,
            access: AccessSpec::default(),
        }
    }

    /// The display name.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The description the model reads.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// The arguments are a `T` (its schema is the input schema).
    pub fn input<T: utoipa::ToSchema>(mut self) -> Self {
        self.input_schema = input_schema::<T>();
        self
    }

    /// An explicit input schema.
    pub fn input_schema(mut self, schema: Value) -> Self {
        self.input_schema = schema;
        self
    }

    /// Sends `readOnlyHint: true`.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// Sends `destructiveHint: true`.
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// Sends `idempotentHint: true`.
    pub fn idempotent(mut self) -> Self {
        self.idempotent = true;
        self
    }

    /// Sends `openWorldHint: true`.
    pub fn open_world(mut self) -> Self {
        self.open_world = true;
        self
    }

    access_builders!();
}

/// A resource (fixed URI) or resource template (URI with `{variables}`).
#[derive(Clone, Debug)]
pub struct ResourceDef {
    /// The URI or URI template (`crm://deals/{id}`).
    pub uri: String,
    /// The name (default: the method name).
    pub name: String,
    /// A display name.
    pub title: Option<String>,
    /// What it holds.
    pub description: Option<String>,
    /// The MIME type of its contents (`application/json` by default for
    /// values, `text/plain` for text).
    pub mime_type: Option<String>,
    /// Who may read it.
    pub access: AccessSpec,
}

impl ResourceDef {
    /// The resource at `uri` named `name`.
    pub fn new(uri: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            name: name.into(),
            title: None,
            description: None,
            mime_type: None,
            access: AccessSpec::default(),
        }
    }

    /// The display name.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// The MIME type.
    pub fn mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    access_builders!();
}

/// A prompt: a template the user picks in the client.
#[derive(Clone, Debug)]
pub struct PromptDef {
    /// The name (`follow-up-email`).
    pub name: String,
    /// A display name (`Draft a follow-up email`).
    pub title: Option<String>,
    /// What it does.
    pub description: Option<String>,
    /// The JSON Schema of its arguments struct.
    pub arguments_schema: Value,
    /// The arguments the client fills in.
    pub arguments: Vec<PromptArgument>,
    /// Who may use it.
    pub access: AccessSpec,
}

impl PromptDef {
    /// A prompt named `name`, without arguments until
    /// [`input`](PromptDef::input).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: None,
            description: None,
            arguments_schema: empty_object(),
            arguments: Vec::new(),
            access: AccessSpec::default(),
        }
    }

    /// The display name.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// The arguments are the properties of a `T`.
    pub fn input<T: utoipa::ToSchema>(mut self) -> Self {
        self.arguments_schema = input_schema::<T>();
        self.arguments = prompt_arguments(&self.arguments_schema);
        self
    }

    access_builders!();
}

/// One call of a handler: the context, the arguments (tools and prompts),
/// the URI variables (resource templates) and the record a declared `from`
/// check loaded.
pub struct Call {
    pub(crate) cx: McpContext,
    pub(crate) arguments: Value,
    pub(crate) params: BTreeMap<String, String>,
    pub(crate) record: Option<AuthorizedRecord>,
}

impl Call {
    /// The caller.
    pub fn context(&self) -> &McpContext {
        &self.cx
    }

    /// The raw arguments (a JSON object; `{}` for resources).
    pub fn arguments(&self) -> &Value {
        &self.arguments
    }

    /// The arguments as a `T`, deserialized and validated like a request
    /// body: a 400 with field errors when they are invalid.
    pub async fn args<T: DeserializeOwned + Validate + Send + 'static>(
        &self,
    ) -> Result<T, AppError> {
        navree_validation::validate_value(self.arguments.clone(), Some(self.cx.app())).await
    }

    /// The URI variable `name` as a `T`: a 404 when the resource URI does not
    /// carry a valid `T` there.
    pub fn param<T: FromStr>(&self, name: &str) -> Result<T, AppError> {
        self.params
            .get(name)
            .and_then(|value| value.parse().ok())
            .ok_or_else(AppError::not_found)
    }

    /// The record the item's `can(.., from = ..)` declaration loaded and
    /// authorized; a 500 when the item declares no such check on `E` and
    /// `A`.
    pub fn record<E, A>(&self) -> Result<Authorized<E, A>, AppError>
    where
        E: SubjectRecords,
        A: ActionMarker,
    {
        self.record
            .clone()
            .and_then(AuthorizedRecord::into_authorized::<E, A>)
            .ok_or_else(|| {
                AppError::internal(format!(
                    "Authorized<{}, {}> needs the item to declare `can({}, {}, from = ..)`",
                    E::SUBJECT,
                    A::NAME,
                    A::NAME,
                    E::SUBJECT
                ))
            })
    }
}

impl fmt::Debug for Call {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Call")
            .field("arguments", &self.arguments)
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}
