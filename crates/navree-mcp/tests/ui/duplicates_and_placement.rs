//! Two items of a kind with one name, and attributes out of place.

use navree_http::AppError;
use navree_macros::{injectable, mcp_server, mcp_tool};

#[injectable]
struct Server;

#[mcp_server]
#[public]
impl Server {
    #[mcp_tool(name = "list")]
    async fn first(&self) -> Result<u32, AppError> {
        Ok(1)
    }

    #[mcp_tool(name = "list")]
    async fn second(&self) -> Result<u32, AppError> {
        Ok(2)
    }
}

#[injectable]
struct Other;

#[mcp_server]
#[mcp_tool]
impl Other {}

struct Plain;

impl Plain {
    #[mcp_tool]
    async fn outside(&self) -> Result<u32, AppError> {
        Ok(1)
    }
}

fn main() {}
