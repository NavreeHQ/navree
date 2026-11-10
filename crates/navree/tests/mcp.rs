//! The `mcp` feature: `navree::mcp` re-exports navree-mcp and the
//! `#[mcp_server]` macros, whose expansion names `::navree::mcp`. Runs with
//! `cargo test -p navree --features mcp`.

// Inside this crate's own tests the macros name `crate::<module>`.
use navree::{access, kernel, mcp};

use navree::mcp::testing::McpTestClient;
use navree::mcp::{McpContext, PromptMessage, mcp_server};
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use navree::validation::Validate;
use serde::Deserialize;
use serde_json::json;

/// The arguments of `greet` and `greet-someone`.
#[derive(Debug, Deserialize, Validate, navree::openapi::ToSchema)]
pub struct Greet {
    /// Who to greet.
    #[validate(length(min = 1, max = 40))]
    pub name: String,
}

/// A server with one tool, one resource and one prompt.
#[injectable]
pub struct HelloMcp;

#[mcp_server]
#[authenticated]
impl HelloMcp {
    /// Greets someone by name.
    #[mcp_tool(read_only, title = "Say hello")]
    async fn greet(&self, cx: &McpContext, args: Greet) -> Result<String> {
        Ok(format!("Hello, {}! (from {})", args.name, cx.user_id()))
    }

    /// The greeting of the day.
    #[mcp_resource("hello://today", mime = "text/plain")]
    async fn today(&self) -> Result<String> {
        Ok("Good morning".to_owned())
    }

    /// Starts a greeting.
    #[mcp_prompt(name = "greet-someone")]
    async fn greeting(&self, args: Greet) -> Result<Vec<PromptMessage>> {
        Ok(vec![PromptMessage::user(format!(
            "Greet {} warmly.",
            args.name
        ))])
    }
}

/// Registers [`HelloMcp`].
#[module(register = Self::wire)]
pub struct HelloModule;

impl HelloModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.mcp_server::<HelloMcp>();
    }
}

#[test]
fn the_module_and_its_types_are_reexported() {
    let _ = (access::READ, kernel::ModuleId::of::<HelloModule>());
    assert_eq!(
        std::any::TypeId::of::<mcp::McpModule>(),
        std::any::TypeId::of::<McpModule>()
    );
}

#[tokio::test]
async fn an_app_serves_its_mcp_server_through_the_facade() {
    let app = TestApp::builder()
        .module(McpModule::from_config())
        .module(HelloModule)
        .build()
        .await;
    let user = principal(uuid::Uuid::now_v7());
    let mcp = McpTestClient::new(app.acting_as(user.clone()))
        .initialize()
        .await;

    let tools = mcp.list_tools().await;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "greet");
    let greeted = mcp
        .call_tool("greet", json!({ "name": "Anu" }))
        .await
        .assert_ok();
    assert_eq!(
        greeted.text(),
        format!("Hello, Anu! (from {})", user.user_id)
    );
    mcp.call_tool("greet", json!({ "name": "" }))
        .await
        .assert_error(400, None);

    let today = mcp.read_resource("hello://today").await.assert_ok();
    assert_eq!(today.text().as_deref(), Some("Good morning"));
    let prompt = mcp
        .get_prompt("greet-someone", json!({ "name": "Gauri" }))
        .await
        .assert_ok();
    assert_eq!(prompt.text(0).as_deref(), Some("Greet Gauri warmly."));
}
