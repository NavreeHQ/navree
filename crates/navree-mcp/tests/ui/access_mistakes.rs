//! Deny by default: every item declares who may use it.

use navree_http::AppError;
use navree_macros::{injectable, mcp_server};

#[injectable]
struct Server;

#[mcp_server]
impl Server {
    /// No declaration.
    #[mcp_tool]
    async fn open_tool(&self) -> Result<u32, AppError> {
        Ok(1)
    }

    /// Public and restricted.
    #[mcp_prompt]
    #[public]
    #[roles("admin")]
    async fn mixed(&self) -> Result<String, AppError> {
        Ok(String::new())
    }

    /// An access attribute on a plain method.
    #[authenticated]
    fn helper(&self) {}
}

fn main() {}
