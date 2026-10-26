//! [`McpRegistry`]: every tool, resource and prompt of the app, collected
//! from the servers modules register, checked once at boot.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use navree_http::AccessSpec;
use navree_kernel::{AppContext, BoxFuture, Injectable, KernelError, ModuleId, Resolver};

use crate::item::{Handler, ItemDef, McpItem, McpServer, PromptDef, ResourceDef, ToolDef};
use crate::uri::UriTemplate;

/// A server registered by a module (a contribution of
/// `m.mcp_server::<S>()`).
#[derive(Clone)]
pub(crate) struct ServerRegistration {
    pub(crate) module: ModuleId,
    pub(crate) type_name: &'static str,
    pub(crate) resolve: ResolveItems,
}

type ResolveItems =
    Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<Vec<McpItem>, KernelError>> + Send + Sync>;

impl ServerRegistration {
    pub(crate) fn of<S: McpServer + Injectable>(module: ModuleId) -> Self {
        Self {
            module,
            type_name: std::any::type_name::<S>(),
            resolve: Arc::new(|r: Resolver| {
                Box::pin(async move {
                    let server = r.get::<S>().await?;
                    Ok(server.items())
                })
            }),
        }
    }
}

/// One registered item and where it came from.
pub(crate) struct Entry<D> {
    pub(crate) def: D,
    pub(crate) handler: Handler,
}

/// A resource with its parsed URI.
pub(crate) struct ResourceEntry {
    pub(crate) entry: Entry<ResourceDef>,
    pub(crate) uri: UriTemplate,
}

/// Every tool, resource and prompt of the app. Provided and exported by
/// `McpModule`; built at boot, where two items of a kind with the same name
/// (or two resources with the same URI), an item without an access
/// declaration and a malformed resource URI fail the boot.
pub struct McpRegistry {
    pub(crate) tools: Vec<Entry<ToolDef>>,
    pub(crate) resources: Vec<ResourceEntry>,
    pub(crate) prompts: Vec<Entry<PromptDef>>,
}

impl McpRegistry {
    /// Collects the servers registered by the modules of `ctx`.
    pub(crate) async fn collect(ctx: &AppContext) -> Result<Self, String> {
        let mut items = Vec::new();
        for contributed in ctx.contributions::<ServerRegistration>() {
            let registration = &contributed.value;
            let resolver = ctx.resolver_for(registration.module);
            let server_items = (registration.resolve)(resolver).await.map_err(|error| {
                format!("MCP server `{}`: {error}", short(registration.type_name))
            })?;
            for item in server_items {
                items.push((contributed.module.name, short(registration.type_name), item));
            }
        }
        Self::build(items)
    }

    /// Checks and indexes `items` (module name, server name, item).
    pub(crate) fn build(items: Vec<(&'static str, &'static str, McpItem)>) -> Result<Self, String> {
        let mut errors = Vec::new();
        let mut registry = McpRegistry {
            tools: Vec::new(),
            resources: Vec::new(),
            prompts: Vec::new(),
        };
        let mut seen: BTreeMap<(String, String), (&'static str, &'static str)> = BTreeMap::new();
        for (module, server, item) in items {
            let kind = item.def.kind();
            let name = item.def.name().to_owned();
            if let Some(problem) = access_problem(item.def.access()) {
                errors.push(format!(
                    "MCP {kind} `{name}` of `{server}` ({module}) {problem}"
                ));
            }
            let mut keys = vec![(kind.to_owned(), name.clone())];
            if let ItemDef::Resource(def) = &item.def {
                keys.push(("resource URI".to_owned(), def.uri.clone()));
            }
            for (key_kind, key) in keys {
                match seen.get(&(key_kind.clone(), key.clone())) {
                    Some((first_module, first_server)) => errors.push(format!(
                        "two MCP {key_kind}s named `{key}`: one in `{first_server}` ({first_module}), one in `{server}` ({module})"
                    )),
                    None => {
                        seen.insert((key_kind, key), (module, server));
                    }
                }
            }
            match item.def {
                ItemDef::Tool(def) => {
                    if let Some(problem) = tool_name_problem(&def.name) {
                        errors.push(format!(
                            "MCP tool `{}` of `{server}` ({module}): {problem}",
                            def.name
                        ));
                    }
                    registry.tools.push(Entry {
                        def,
                        handler: item.handler,
                    });
                }
                ItemDef::Resource(def) => match UriTemplate::parse(&def.uri) {
                    Ok(uri) => {
                        if let Some(from) = def.access.can.as_ref().and_then(|c| c.from.as_ref())
                            && !uri.variables().contains(&from.as_str())
                        {
                            errors.push(format!(
                                "MCP resource `{}` of `{server}` ({module}) checks the record from `{from}`, which is not a variable of `{}`",
                                def.name, def.uri
                            ));
                        }
                        registry.resources.push(ResourceEntry {
                            entry: Entry {
                                def,
                                handler: item.handler,
                            },
                            uri,
                        });
                    }
                    Err(problem) => errors.push(format!(
                        "MCP resource `{}` of `{server}` ({module}): {problem}",
                        def.name
                    )),
                },
                ItemDef::Prompt(def) => registry.prompts.push(Entry {
                    def,
                    handler: item.handler,
                }),
            }
        }
        if errors.is_empty() {
            Ok(registry)
        } else {
            Err(errors.join("\n"))
        }
    }

    /// The tool named `name`.
    pub(crate) fn tool(&self, name: &str) -> Option<&Entry<ToolDef>> {
        self.tools.iter().find(|entry| entry.def.name == name)
    }

    /// The resource whose URI (or template) matches `uri`, with its
    /// variables; fixed URIs win over templates.
    pub(crate) fn resource(
        &self,
        uri: &str,
    ) -> Option<(&Entry<ResourceDef>, BTreeMap<String, String>)> {
        let fixed = self
            .resources
            .iter()
            .filter(|resource| !resource.uri.is_template());
        let templates = self
            .resources
            .iter()
            .filter(|resource| resource.uri.is_template());
        fixed.chain(templates).find_map(|resource| {
            resource
                .uri
                .matches(uri)
                .map(|params| (&resource.entry, params))
        })
    }

    /// The prompt named `name`.
    pub(crate) fn prompt(&self, name: &str) -> Option<&Entry<PromptDef>> {
        self.prompts.iter().find(|entry| entry.def.name == name)
    }

    /// Every tool definition.
    pub fn tools(&self) -> impl Iterator<Item = &ToolDef> {
        self.tools.iter().map(|entry| &entry.def)
    }

    /// Every resource and resource template definition.
    pub fn resources(&self) -> impl Iterator<Item = &ResourceDef> {
        self.resources.iter().map(|resource| &resource.entry.def)
    }

    /// Every prompt definition.
    pub fn prompts(&self) -> impl Iterator<Item = &PromptDef> {
        self.prompts.iter().map(|entry| &entry.def)
    }

    /// Whether any item checks roles, permissions or abilities (the app then
    /// needs `AccessModule`).
    pub(crate) fn needs_access(&self) -> Option<String> {
        let tools = self
            .tools
            .iter()
            .map(|e| ("tool", &e.def.name, &e.def.access));
        let resources = self
            .resources
            .iter()
            .map(|r| ("resource", &r.entry.def.name, &r.entry.def.access));
        let prompts = self
            .prompts
            .iter()
            .map(|e| ("prompt", &e.def.name, &e.def.access));
        tools
            .chain(resources)
            .chain(prompts)
            .find(|(_, _, access)| access.needs_authorizer())
            .map(|(kind, name, _)| format!("MCP {kind} `{name}`"))
    }
}

impl fmt::Debug for McpRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpRegistry")
            .field("tools", &self.tools().map(|t| &t.name).collect::<Vec<_>>())
            .field(
                "resources",
                &self.resources().map(|r| &r.uri).collect::<Vec<_>>(),
            )
            .field(
                "prompts",
                &self.prompts().map(|p| &p.name).collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// What is wrong with an access declaration (deny by default).
fn access_problem(access: &AccessSpec) -> Option<&'static str> {
    if !access.is_declared() {
        return Some(
            "has no access declaration: add public, authenticated, roles, permissions or can (deny by default)",
        );
    }
    if access.public && (access.authenticated || access.needs_authorizer()) {
        return Some("is public but also declares a restriction: remove one of them");
    }
    None
}

/// MCP tool names: 1 to 128 of `A-Z a-z 0-9 _ - .`.
fn tool_name_problem(name: &str) -> Option<&'static str> {
    let valid = (1..=128).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    (!valid).then_some("a tool name is 1 to 128 letters, digits, `_`, `-` or `.`")
}

/// `crm_deals::mcp::CrmMcp` → `CrmMcp`.
fn short(type_name: &'static str) -> &'static str {
    let base = type_name.split('<').next().unwrap_or(type_name);
    base.rsplit("::").next().unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::McpOutput;
    use serde_json::json;

    fn tool(name: &str) -> McpItem {
        McpItem::tool(ToolDef::new(name).authenticated(), |_| async {
            Ok(McpOutput::Json(json!(null)))
        })
    }

    fn resource(uri: &str, name: &str) -> McpItem {
        McpItem::resource(ResourceDef::new(uri, name).authenticated(), |_| async {
            Ok(McpOutput::Json(json!(null)))
        })
    }

    #[test]
    fn duplicates_fail_naming_both_modules() {
        let error = McpRegistry::build(vec![
            ("DealsModule", "CrmMcp", tool("list_deals")),
            ("ReportsModule", "Reports", tool("list_deals")),
        ])
        .unwrap_err();
        assert!(
            error.contains("two MCP tools named `list_deals`"),
            "{error}"
        );
        assert!(
            error.contains("DealsModule") && error.contains("ReportsModule"),
            "{error}"
        );

        let error = McpRegistry::build(vec![
            ("A", "S", resource("crm://pipeline", "pipeline")),
            ("B", "T", resource("crm://pipeline", "board")),
        ])
        .unwrap_err();
        assert!(
            error.contains("resource URIs named `crm://pipeline`"),
            "{error}"
        );
    }

    #[test]
    fn items_without_access_or_with_bad_names_fail() {
        let undeclared = McpItem::tool(ToolDef::new("open"), |_| async {
            Ok(McpOutput::Json(json!(null)))
        });
        let error = McpRegistry::build(vec![("M", "S", undeclared)]).unwrap_err();
        assert!(error.contains("no access declaration"), "{error}");

        let both = McpItem::tool(ToolDef::new("both").public().roles(&["admin"]), |_| async {
            Ok(McpOutput::Json(json!(null)))
        });
        let error = McpRegistry::build(vec![("M", "S", both)]).unwrap_err();
        assert!(error.contains("public but also"), "{error}");

        let error = McpRegistry::build(vec![("M", "S", tool("has space"))]).unwrap_err();
        assert!(error.contains("1 to 128"), "{error}");

        let error =
            McpRegistry::build(vec![("M", "S", resource("deals/{id}", "deal"))]).unwrap_err();
        assert!(error.contains("no scheme"), "{error}");

        let from = McpItem::resource(
            ResourceDef::new("crm://deals/{id}", "deal").can("read", "Deal", Some("deal_id")),
            |_| async { Ok(McpOutput::Json(json!(null))) },
        );
        let error = McpRegistry::build(vec![("M", "S", from)]).unwrap_err();
        assert!(error.contains("not a variable"), "{error}");
    }

    #[test]
    fn fixed_resources_win_over_templates() {
        let registry = McpRegistry::build(vec![
            ("M", "S", resource("crm://deals/{id}", "deal")),
            ("M", "S", resource("crm://deals/board", "board")),
        ])
        .unwrap();
        let (entry, params) = registry.resource("crm://deals/board").unwrap();
        assert_eq!(entry.def.name, "board");
        assert!(params.is_empty());
        let (entry, params) = registry.resource("crm://deals/0199").unwrap();
        assert_eq!(entry.def.name, "deal");
        assert_eq!(params["id"], "0199");
        assert!(registry.resource("crm://contacts/1").is_none());
        assert_eq!(registry.needs_access(), None);
    }

    #[test]
    fn short_type_names() {
        assert_eq!(short("crm_deals::mcp::CrmMcp"), "CrmMcp");
        assert_eq!(short("a::Generic<b::C>"), "Generic");
    }
}
