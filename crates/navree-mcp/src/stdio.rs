//! The stdio transport: `mcp:serve --user <email>`.

use std::sync::Arc;

use navree_auth::AuthService;
use navree_contracts::{AuthVia, Principal};
use navree_kernel::{AppContext, BoxError, CommandSpec};
use rmcp::ServiceExt;
use tokio::io::{AsyncRead, AsyncWrite};
use uuid::Uuid;

use crate::handler::{LocalCaller, McpHandler, Shared};
use crate::notifier::McpNotifier;
use crate::options::McpSettings;
use crate::registry::McpRegistry;

/// `mcp:serve --user <email or id>`: the MCP server on stdin and stdout.
pub(crate) fn serve_command() -> CommandSpec {
    CommandSpec::new(
        "mcp:serve",
        "Serve the MCP server on stdin/stdout as a user: mcp:serve --user <email or id>",
        |ctx, args| async move {
            let user = user_argument(&args)?;
            serve_stdio(&ctx, &user, tokio::io::stdin(), tokio::io::stdout()).await
        },
    )
    .stdio()
}

/// The value of `--user` (`--user ada@example.com` or `--user=...`).
fn user_argument(args: &[String]) -> Result<String, BoxError> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--user=") {
            return Ok(value.to_owned());
        }
        if arg == "--user"
            && let Some(value) = args.next()
        {
            return Ok(value.clone());
        }
    }
    Err("usage: mcp:serve --user <email or user id>".into())
}

/// Serves the app's MCP server on `reader` and `writer` (JSON-RPC messages,
/// one per line) as `user`, an email address or a user id, until the
/// client closes the stream or the app shuts down: what `mcp:serve` runs
/// on stdin and stdout. The process acts as that user, with their roles
/// and permissions.
pub async fn serve_stdio<R, W>(ctx: &AppContext, user: &str, reader: R, writer: W) -> Result<(), BoxError>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let principal = Principal {
        user_id: find_user(ctx, user).await?,
        via: AuthVia::Token,
        token_abilities: None,
        session_id: None,
    };
    let shared = Shared::new(
        ctx,
        ctx.get::<McpRegistry>().await?,
        &*ctx.get::<McpSettings>().await?,
        ctx.get::<McpNotifier>().await?,
    )
    .await?;
    let handler = McpHandler::new(
        Arc::new(shared),
        Some(LocalCaller {
            principal,
            session_id: format!("stdio-{}", Uuid::now_v7()),
        }),
    );
    let service = handler
        .serve((reader, writer))
        .await
        .map_err(|error| format!("the MCP client did not initialize: {error}"))?;
    let shutdown = ctx.shutdown_token();
    tokio::select! {
        result = service.waiting() => {
            result.map_err(|error| format!("the MCP stdio session failed: {error}"))?;
        }
        () = shutdown.cancelled() => {}
    }
    Ok(())
}

/// The id of `user`: a user id as is (checked when the app has
/// `AuthModule`), or the account with that email address.
async fn find_user(ctx: &AppContext, user: &str) -> Result<Uuid, BoxError> {
    let auth = ctx.try_get::<AuthService>().await;
    if let Ok(id) = Uuid::parse_str(user) {
        if let Some(auth) = auth
            && auth.find_user(id).await?.is_none()
        {
            return Err(format!("no user with id {id}").into());
        }
        return Ok(id);
    }
    let auth = auth.ok_or("mcp:serve --user <email> needs AuthModule in the app (or pass a user id)")?;
    let found = auth
        .find_by_email(user)
        .await?
        .ok_or_else(|| format!("no user with email {user}"))?;
    Ok(found.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_user_comes_from_the_flag() {
        assert_eq!(user_argument(&args(&["--user", "ada@example.com"])).unwrap(), "ada@example.com");
        assert_eq!(user_argument(&args(&["--user=ada@example.com"])).unwrap(), "ada@example.com");
        assert!(user_argument(&args(&[])).is_err());
        assert!(user_argument(&args(&["--user"])).is_err());
    }

    #[test]
    fn the_command_owns_stdout() {
        let command = serve_command();
        assert_eq!(command.name, "mcp:serve");
        assert!(command.stdio);
    }
}
