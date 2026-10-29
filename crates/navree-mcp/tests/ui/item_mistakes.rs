//! Mistakes in items: names, URIs, `from`, parameters and signatures.

use navree_access::{Authorized, Read};
use navree_http::AppError;
use navree_macros::{injectable, mcp_server};

/// The `deals` model, the access subject `Deal`.
#[derive(Clone, Debug, Default, navree_orm::Model, serde::Serialize)]
#[model(table = "deals", subject = "Deal", permission_prefix = "deals")]
struct Deal {
    id: i64,
}

struct Args;

#[injectable]
struct Server;

#[mcp_server]
#[authenticated]
impl Server {
    #[mcp_tool(name = "has space")]
    async fn bad_name(&self) -> Result<u32, AppError> {
        Ok(1)
    }

    #[mcp_resource("deals/{id}")]
    async fn no_scheme(&self, id: String) -> Result<String, AppError> {
        Ok(id)
    }

    #[mcp_resource("crm://deals/{id}")]
    #[can(Read, Deal, from = "deal_id")]
    async fn wrong_from(&self) -> Result<String, AppError> {
        Ok(String::new())
    }

    #[mcp_resource("crm://notes/{slug}")]
    async fn unknown_variable(&self, name: String) -> Result<String, AppError> {
        Ok(name)
    }

    #[mcp_tool]
    async fn two_arguments(&self, first: Args, second: Args) -> Result<u32, AppError> {
        let _ = (first, second);
        Ok(1)
    }

    #[mcp_tool]
    async fn record_without_from(&self, deal: Authorized<Deal, Read>) -> Result<u32, AppError> {
        let _ = deal;
        Ok(1)
    }

    #[mcp_tool]
    fn not_async(&self) -> Result<u32, AppError> {
        Ok(1)
    }

    #[mcp_tool]
    async fn no_result(&self) -> u32 {
        1
    }

    #[mcp_tool(unknown_key)]
    async fn unknown_key(&self) -> Result<u32, AppError> {
        Ok(1)
    }

    #[mcp_tool]
    #[mcp_prompt]
    async fn two_kinds(&self) -> Result<u32, AppError> {
        Ok(1)
    }
}

fn main() {}
