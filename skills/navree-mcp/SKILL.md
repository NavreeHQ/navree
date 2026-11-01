---
name: navree-mcp
description: "Adds an MCP server to a Navree app with navree-mcp: `McpModule` and the `mcp.*` keys, `#[mcp_server]` with `#[mcp_tool]`, `#[mcp_resource]` and `#[mcp_prompt]`, typed and validated arguments (`Deserialize` + `ToSchema` + `#[derive(Validate)]`), per-item access attributes, `McpContext`, `McpNotifier`, streamable HTTP at `/mcp`, stdio with `mcp:serve --user`, bearer or OAuth (RFC 9728) auth, and `McpTestClient`. Use when the product should expose tools, resources or prompts to Claude, ChatGPT, Cursor or any MCP client, when `navree make:mcp` was run, or when an MCP tool is missing from a client's list, answers \"Unknown tool\", a 401, 403 or 404, or a server fails to boot."
---

# MCP servers with navree-mcp

`navree-mcp` (facade feature `mcp`, opt-in: not in `full`) turns methods on
an injectable into an MCP server. Every module may register servers; they
merge into one endpoint. HTTP clients reach it at `/mcp` (authenticated,
always); local clients start `<app> mcp:serve --user <email or id>`. The
protocol layer (JSON-RPC, versions `2024-11-05` to `2025-11-25` with the
`initialize` handshake plus the stateless `2026-07-28`, both transports) is
Navree's own, in `navree-mcp`; protocol types are in `navree::mcp::protocol`
(`Tool`, `Resource`, `ResourceTemplate`, `Prompt`, `SUPPORTED_VERSIONS`).

This is the MCP server of the **product**. The coding agent's server is
Navigator (`navree mcp`), configured by nothing in the app.

## Steps

1. Dependencies (`Cargo.toml`):

   ```toml
   navree = { git = "https://github.com/NavreeHQ/navree", features = ["mcp"] }
   ```

2. Add the module in `src/app.rs` (after `HttpModule`, `DatabaseModule`,
   `AuthModule`, `AccessModule`):

   ```rust
   .module(McpModule::from_config()) // section `mcp`; serves /mcp and mcp:serve
   ```

3. Generate a server in a feature module, then edit it:

   ```sh
   navree make:mcp deals crm   # src/modules/deals/mcp/crm.rs: CrmMcp, tool `crm_echo`
   ```

   It registers `navree::mcp::ModuleBuilderMcpExt::mcp_server::<mcp::crm::CrmMcp>(m);`
   above `// navree:wire` and prints the steps above when they are missing.

4. Configure (`config/app.toml`) and test (below).

## Server template

A complete, tested server is in the reference CRM:
`apps/crm-api/crates/crm-deals/src/mcp.rs` (`DealsMcp`: `list_deals`,
`move_deal`, `crm://deals/{id}`, `crm://pipeline`, the `follow-up-email`
prompt), `apps/crm-api/crates/crm-activities/src/mcp.rs` (`ActivitiesMcp`:
`log_activity`) and its suite `apps/crm-api/tests/mcp.rs`. The template
below calls service methods of your own (`DealService::list`, `rename`,
`pdf`):

```rust
use navree::mcp::{McpContext, PromptMessage, ResourceContents, mcp_server};
use navree::prelude::*;
use serde::Deserialize;
use navree::openapi::ToSchema;

/// Tool arguments: doc comments become the schema's descriptions; the rules run
/// before the method (a failure is a tool error with a 400 body).
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct ListDeals {
    /// Words from the deal title.
    #[validate(length(max = 100))]
    pub query: Option<String>,
    /// Only the user's own deals.
    #[validate(skip)]
    #[serde(default)]
    pub mine: bool,
}

#[injectable]
pub struct DealsMcp {
    deals: Arc<DealService>, // injected like any provider
}

#[mcp_server]
impl DealsMcp {
    /// Lists the deals the user can see, newest first. Returns at most 25.
    #[mcp_tool(read_only)]                       // the doc comment is the description
    #[can(Read, Deal)]                           // REQUIRED: an access attribute
    async fn list_deals(&self, cx: &McpContext, args: ListDeals) -> Result<Vec<DealResource>> {
        let filter = DealFilter { search: args.query, owner_id: args.mine.then(|| cx.user_id()), ..DealFilter::default() };
        let page = PageQuery { per_page: 25, ..PageQuery::default() };
        Ok(self.deals.list(cx.uow(), cx.accessible::<Deal, Read>(), filter, page).await?.data)
    }

    /// Renames a deal the user may update.
    #[mcp_tool(idempotent, title = "Rename a deal")]
    #[can(Update, Deal, from = "deal_id")]       // loads + authorizes args.deal_id
    async fn rename_deal(&self, cx: &McpContext, deal: Authorized<Deal, Update>, args: RenameDeal) -> Result<DealResource> {
        self.deals.rename(cx.uow(), deal.into_inner(), args.title).await
    }

    /// A deal by id.
    #[mcp_resource("crm://deals/{id}", mime = "application/json")]
    #[can(Read, Deal, from = "id")]              // `from` names a URI variable
    async fn deal(&self, deal: Authorized<Deal, Read>) -> Result<DealResource> {
        Ok(deal.into_inner().into())
    }

    /// A file, as binary contents.
    #[mcp_resource("crm://files/{name}", mime = "application/pdf")]
    #[authenticated]
    async fn file(&self, name: String) -> Result<ResourceContents> { // URI variable by name, any FromStr
        Ok(ResourceContents::blob(self.deals.pdf(&name).await?, "application/pdf"))
    }

    /// Drafts a follow-up email for a deal.
    #[mcp_prompt(name = "follow-up-email", title = "Draft a follow-up email")]
    #[can(Read, Deal)]
    async fn follow_up(&self, cx: &McpContext, args: FollowUpArgs) -> Result<Vec<PromptMessage>> {
        let deal = cx.authorize::<Deal, Read>(args.deal_id).await?; // 404 / 403
        Ok(vec![
            PromptMessage::resource(format!("crm://deals/{}", deal.id)), // embeds current contents
            PromptMessage::user("Draft a short follow-up email. End with one next step."),
        ])
    }
}

// In the module's wire:
m.mcp_server::<DealsMcp>(); // prelude imports ModuleBuilderMcpExt
```

Item rules (checked at compile time by `#[mcp_server]`):

- Non-generic `async fn` taking `&self`, returning a `Result`.
- Other parameters, any order: `&McpContext`; one arguments struct (tools,
  prompts); URI variables by name (resource templates); `Authorized<E, A>`
  for the item's `#[can(A, E, from = "..")]`.
- Access: `#[public]` (any signed-in client), `#[authenticated]`,
  `#[roles("..")]`, `#[permissions("..")]`, `#[can(..)]`, on the method or
  the `impl` block. None at all does not compile.
- Tool hints: `read_only`, `destructive`, `idempotent`, `open_world`;
  keys `name`, `title` (tools, prompts), `name`, `title`, `mime` (resources).
- Tool names: 1 to 128 of `A-Z a-z 0-9 _ - .`. Resource URIs need a
  scheme. Duplicate names in one block do not compile; across modules they
  fail the boot naming both modules.

Results: a JSON object is structured content + JSON text; a `String` is
text; other values are JSON text with structured content
`{"result": value}`. Resources: serializable values as JSON (`mime`
default `application/json`), `String` as text (`text/plain`),
`ResourceContents` or `Vec<ResourceContents>` as given. Prompts:
`Vec<PromptMessage>`, `PromptMessage` or `String`.

## McpContext

`user_id()`, `principal()`, `user().await` (needs `AuthModule`),
`access()` (`&PrincipalAccess`), `ability()`, `accessible::<E, A>()`,
`authorize::<E, A>(id).await`, `uow()` (panics without `DatabaseModule`;
commits on `Ok`, rolls back on `Err`), `now()`, `session_id()`,
`client()` (name, version), `request_id()`, `app()`, `wants_progress()`,
`progress(done, Some(total), "message").await`, `is_cancelled()` (the
client cancelled; the call's future is also dropped at its next `.await`).

## McpNotifier

Inject `Arc<McpNotifier>`:

```rust
self.notifier.resource_updated(format!("crm://deals/{}", deal.id)).await; // subscribed sessions
self.notifier.tools_changed().await; // every open session; also resources_changed(), prompts_changed()
```

A user's role or permission change is announced to their open sessions on
their next request; no call needed.

## Configuration

```toml
[mcp]
# path = "/mcp"                    # absolute, no API prefix
name = "navree-crm"                # default: app.name, else the executable name
title = "Navree CRM"               # default: name
# version = "1.0.0"                # default: app.version, else 0.0.0
instructions = "Tools for the CRM. Amounts are in cents."
auth = "bearer"                    # or "oauth"
allowed_origins = ["https://crm.example.com"]  # browsers only; no Origin header passes
# session_ttl_secs = 3600
# max_request_bytes = 1048576      # larger: 413

[mcp.oauth]                        # with auth = "oauth"
# resource = "https://crm.example.com/mcp"           # default: app.url + path
# authorization_servers = ["https://crm.example.com"] # default: [app.url]
# scopes = ["deals:read", "activities:write"]        # default: the keys of oauth.scopes
```

With `OAuthServerModule` in the app, `auth = "oauth"` and `app.url` are
enough: the metadata names the app as the authorization server and lists
its `oauth.scopes`. Personal access tokens (`nvp_…`) and OAuth access
tokens are bearer tokens on `/mcp`, narrowed to their scopes' permissions.

Env: `APP_MCP__AUTH=oauth`, `APP_MCP__ALLOWED_ORIGINS=https://a.example,https://b.example`.
Code: `McpModule::for_root(McpOptions { name: Some("crm".into()), ..McpOptions::default() })`.

## Testing

```rust
use navree::mcp::testing::McpTestClient;
use navree::testing::{TestApp, principal};
use serde_json::json;

#[tokio::test]
async fn reps_see_and_call_their_tools() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(AccessModule::for_root(access_options()))
        .module(McpModule::from_config())
        .module(DealsModule)
        .build()
        .await;
    let mcp = McpTestClient::new(app.acting_as(principal(user_id)))
        .initialize()
        .await;
    assert!(mcp.list_tools().await.iter().any(|t| t.name == "list_deals"));
    mcp.call_tool("rename_deal", json!({ "deal_id": theirs, "title": "x" }))
        .await
        .assert_error(403, "Forbidden");
    mcp.call_tool("list_deals", json!({}))
        .await
        .assert_ok()
        .assert_json_path("0.title", json!("Acme renewal"));
    mcp.read_resource(&format!("crm://deals/{mine}")).await.assert_ok();
    mcp.get_prompt("follow-up-email", json!({ "deal_id": mine.to_string() })).await.assert_ok();
}
```

With a real token (navree-auth): `McpTestClient::new(app.client()).bearer(&tokens.access_token)`.
Also: `list_resources()`, `list_resource_templates()`, `list_prompts()`,
`call_tool_with_progress`, `subscribe(uri)`, `listen()` (GET stream,
`next(wait)`), `notifications()`, `request(method, params)`, `close()`,
`ToolResult::{json::<T>(), text(), structured(), is_error()}`.

## Checks

- `cargo test` covers each item with one user who may and one who may not,
  and the list each of them sees.
- `navree routes` lists `POST`, `GET` and `DELETE /mcp`, authenticated,
  named `mcp.message`, `mcp.stream` and `mcp.close` (and, with
  `auth = "oauth"`, the public `/.well-known/oauth-protected-resource`
  routes `mcp.metadata` and `mcp.metadata.endpoint`). The OpenAPI document
  shows them under the tag `MCP`.
- Inspect by hand: `npx @modelcontextprotocol/inspector`, URL
  `http://127.0.0.1:8080/mcp`, header `Authorization: Bearer <token>`.
- stdio: `cargo run -- mcp:serve --user ada@example.com` (logs on stderr).
- Navree's own conformance run with the official TypeScript SDK client:
  `npm ci --prefix e2e/mcp-sdk`, then
  `cargo test -p navree-mcp --test sdk_conformance -- --ignored`.

## Gotchas

- `#[mcp_server]` names `::navree::mcp`: the `mcp` feature must be on, and
  import the macro from `navree::mcp` (the item attributes need no import).
- `ToSchema` is `navree::openapi::ToSchema`, also in the prelude; the app needs no other crate for it.
- A tool the user may not use is invisible: calling it is `-32602 Unknown
  tool`, not a 403. Check roles/permissions/abilities first when a client
  can't see a tool. Token abilities (PATs, OAuth scopes) narrow the list.
- Roles, permissions or `can` without `AccessModule` fail the boot.
- `cx.uow()` panics without `DatabaseModule`. Writes commit only when the
  method returns `Ok`.
- Browser requests need `mcp.allowed_origins`; by default every `Origin`
  is refused (403). Non-browser clients send none.
- `mcp.auth = "oauth"` needs `app.url` (or `mcp.oauth.resource` and
  `authorization_servers`) or the boot fails. The metadata is at
  `/.well-known/oauth-protected-resource` and `...-resource/mcp`.
- A session belongs to the user who opened it (another user: 404). Without
  `CacheModule`, sessions live in one process.
- The endpoint also speaks the stateless protocol `2026-07-28`: no
  `initialize` (clients ask `server/discover`), no session id, the protocol
  version in every request's `_meta` (missing: 400, `-32602`), and
  notifications on a `subscriptions/listen` stream. Test it with
  `McpTestClient::new(client).protocol_version("2026-07-28")`.
- Never log arguments yourself in tools; Navree's `mcp call` line leaves
  them out on purpose.
- Errors: 5xx details (and panics) go to the log; the client reads
  `"Internal server error"`. Return a 4xx `AppError` for a message the
  model should see. Resource and prompt failures are JSON-RPC errors (404
  `-32002`, 400 `-32602`); unknown methods are `-32601`.
- Limits: bodies over `mcp.max_request_bytes` get a 413; JSON nested deeper
  than 64 levels gets `-32600`; batches only on a `2025-03-26` session; at
  most 64 messages wait for a slow client (notifications beyond are
  dropped).
- Protocol types (`Tool`, `Resource`, `ResourceTemplate`, `Prompt`) are in
  `navree::mcp::protocol`; `McpTestClient`'s `list_*` methods return them.
