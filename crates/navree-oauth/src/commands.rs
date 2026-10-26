//! The commands `oauth:client`, `oauth:clients`, `oauth:client:secret`,
//! `oauth:client:revoke`, `oauth:keys` and `oauth:prune`.

use std::fmt::Write as _;

use chrono::TimeDelta;
use navree_auth::AuthService;
use navree_db::{Db, Uow};
use navree_kernel::{AppContext, BoxError, CommandSpec};
use navree_workspace::Workspaces;
use sea_orm::sea_query::Condition;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::core::Core;
use crate::entity::{
    access_token, authorization_request, code, device_code, personal_access_token, refresh_token,
};
use crate::keys::SigningKey25519;
use crate::{NewClient, OAuthClients};

/// Every command of the OAuth server.
pub(crate) fn commands() -> [CommandSpec; 6] {
    [
        CommandSpec::new(
            "oauth:client",
            "Create an OAuth client: <name> [--redirect-uri <uri>]... [--scopes a,b] [--public] [--grant client_credentials] [--owner <email>] [--device] [--trusted] [--workspace <slug>] [--introspect]",
            |ctx, args| async move {
                print!("{}", create_client(&ctx, &args).await?);
                Ok(())
            },
        ),
        CommandSpec::new(
            "oauth:clients",
            "List OAuth clients",
            |ctx, _args| async move {
                print!("{}", list_clients(&ctx).await?);
                Ok(())
            },
        ),
        CommandSpec::new(
            "oauth:client:secret",
            "Rotate a confidential OAuth client's secret: <id>",
            |ctx, args| async move {
                print!("{}", rotate_secret(&ctx, &args).await?);
                Ok(())
            },
        ),
        CommandSpec::new(
            "oauth:client:revoke",
            "Revoke an OAuth client and every token it holds: <id>",
            |ctx, args| async move {
                print!("{}", revoke_client(&ctx, &args).await?);
                Ok(())
            },
        ),
        CommandSpec::new(
            "oauth:keys",
            "Generate an Ed25519 signing key for ID tokens (APP_OAUTH__SIGNING_KEY)",
            |_ctx, _args| async move {
                print!("{}", generate_keys());
                Ok(())
            },
        ),
        CommandSpec::new(
            "oauth:prune",
            "Delete expired OAuth codes, requests and tokens: [--days N] (default 7)",
            |ctx, args| async move {
                print!("{}", prune(&ctx, &args).await?);
                Ok(())
            },
        ),
    ]
}

/// The value after `--flag` (or in `--flag=value`).
fn flag_values(args: &[String], flag: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == flag {
            if let Some(value) = iter.next() {
                values.push(value.clone());
            }
        } else if let Some(value) = arg.strip_prefix(&format!("{flag}=")) {
            values.push(value.to_owned());
        }
    }
    values
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

/// The flags that take a value.
const VALUE_FLAGS: [&str; 8] = [
    "--redirect-uri",
    "--scopes",
    "--grant",
    "--owner",
    "--workspace",
    "--logo-url",
    "--website",
    "--days",
];

/// The positional arguments (not flags or flag values).
fn positionals(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if VALUE_FLAGS.contains(&arg.as_str()) {
            iter.next();
        } else if !arg.starts_with("--") {
            out.push(arg.clone());
        }
    }
    out
}

/// Runs `f` in a unit of work and commits it (rolls back on error).
async fn in_uow<T, F, Fut>(ctx: &AppContext, f: F) -> Result<T, BoxError>
where
    F: FnOnce(Uow) -> Fut,
    Fut: std::future::Future<Output = Result<T, BoxError>>,
{
    let db = ctx.get::<Db>().await?;
    let uow = db.uow(None, None).await;
    match uow.scope(f(uow.clone())).await {
        Ok(value) => {
            uow.commit().await?;
            Ok(value)
        }
        Err(error) => {
            uow.rollback().await?;
            Err(error)
        }
    }
}

/// `oauth:client <name> [options]`: the output.
pub(crate) async fn create_client(ctx: &AppContext, args: &[String]) -> Result<String, BoxError> {
    let name = positionals(args).into_iter().next().ok_or(
        "usage: oauth:client <name> [--redirect-uri <uri>]... [--scopes a,b] [--public] ...",
    )?;
    let mut new = if has_flag(args, "--public") {
        NewClient::public(name)
    } else {
        NewClient::confidential(name)
    };
    for uri in flag_values(args, "--redirect-uri") {
        new = new.redirect_uri(uri);
    }
    let scopes: Vec<String> = flag_values(args, "--scopes")
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .map(str::to_owned)
        .collect();
    new = new.scopes(scopes);
    for grant in flag_values(args, "--grant") {
        new = new.grant(grant);
    }
    if has_flag(args, "--device") {
        new = new.device();
    }
    if has_flag(args, "--trusted") {
        new = new.trusted();
    }
    if has_flag(args, "--introspect") {
        new = new.introspect();
    }
    if let Some(url) = flag_values(args, "--logo-url").pop() {
        new = new.logo_url(url);
    }
    if let Some(url) = flag_values(args, "--website").pop() {
        new = new.website(url);
    }
    if let Some(email) = flag_values(args, "--owner").pop() {
        let auth = ctx
            .try_get::<AuthService>()
            .await
            .ok_or("--owner needs AuthModule in the app")?;
        let user = auth
            .find_by_email(&email)
            .await?
            .ok_or_else(|| format!("no user with the email address {email}"))?;
        new = new.owner(user.id);
    }
    if let Some(slug) = flag_values(args, "--workspace").pop() {
        let workspaces = ctx
            .try_get::<Workspaces>()
            .await
            .ok_or("--workspace needs WorkspaceModule in the app")?;
        let workspace = workspaces.resolve(&slug).await?;
        new = new.workspace(workspace.id);
    }
    let clients = ctx.get::<OAuthClients>().await?;
    let created = in_uow(ctx, |uow| async move {
        clients.create(&uow, new).await.map_err(BoxError::from)
    })
    .await?;
    let client = &created.client;
    let mut out = String::new();
    let kind = if client.is_public() {
        "public"
    } else {
        "confidential"
    };
    writeln!(out, "Created {kind} client \"{}\".", client.name)?;
    writeln!(out, "client_id:     {}", client.id)?;
    if let Some(secret) = &created.secret {
        writeln!(out, "client_secret: {secret}")?;
        writeln!(out, "Store the secret now: it is shown once.")?;
    }
    Ok(out)
}

/// `oauth:clients`: the output.
pub(crate) async fn list_clients(ctx: &AppContext) -> Result<String, BoxError> {
    let clients = ctx.get::<OAuthClients>().await?.list().await?;
    let mut out = String::new();
    if clients.is_empty() {
        writeln!(out, "No OAuth clients.")?;
        return Ok(out);
    }
    for client in clients {
        let kind = if client.is_public() {
            "public"
        } else {
            "confidential"
        };
        let state = if client.is_revoked() {
            " (revoked)"
        } else {
            ""
        };
        writeln!(
            out,
            "{}  {kind:<12}  {}{state}  scopes: {}  grants: {}",
            client.id,
            client.name,
            if client.scopes.is_empty() {
                "-"
            } else {
                &client.scopes
            },
            client.grants
        )?;
    }
    Ok(out)
}

fn client_id(args: &[String], usage: &str) -> Result<Uuid, BoxError> {
    let id = positionals(args)
        .into_iter()
        .next()
        .ok_or(usage.to_owned())?;
    Ok(Uuid::parse_str(&id).map_err(|_| format!("`{id}` is not a client id"))?)
}

/// `oauth:client:secret <id>`: the output.
pub(crate) async fn rotate_secret(ctx: &AppContext, args: &[String]) -> Result<String, BoxError> {
    let id = client_id(args, "usage: oauth:client:secret <id>")?;
    let clients = ctx.get::<OAuthClients>().await?;
    let secret = in_uow(ctx, |uow| async move {
        clients
            .rotate_secret(&uow, id)
            .await
            .map_err(BoxError::from)
    })
    .await?;
    Ok(format!(
        "New client_secret for {id}: {secret}\nStore the secret now: it is shown once. The old secret no longer works.\n"
    ))
}

/// `oauth:client:revoke <id>`: the output.
pub(crate) async fn revoke_client(ctx: &AppContext, args: &[String]) -> Result<String, BoxError> {
    let id = client_id(args, "usage: oauth:client:revoke <id>")?;
    let clients = ctx.get::<OAuthClients>().await?;
    in_uow(ctx, |uow| async move {
        clients.revoke(&uow, id).await.map_err(BoxError::from)
    })
    .await?;
    Ok(format!("Revoked client {id} and its tokens.\n"))
}

/// `oauth:keys`: a new key as an environment line.
pub(crate) fn generate_keys() -> String {
    let key = SigningKey25519::generate();
    format!("APP_OAUTH__SIGNING_KEY={}\n", key.encode())
}

/// `oauth:prune [--days N]`: the output.
pub(crate) async fn prune(ctx: &AppContext, args: &[String]) -> Result<String, BoxError> {
    let days: i64 = match flag_values(args, "--days").pop() {
        Some(days) => days
            .parse()
            .ok()
            .filter(|days| *days >= 0)
            .ok_or_else(|| format!("--days must be a whole number of days, not `{days}`"))?,
        None => 7,
    };
    let core = ctx.get::<Core>().await?;
    let cutoff = core.now() - TimeDelta::days(days);
    let deleted = in_uow(ctx, |uow| async move {
        let tx = uow.tx().await?;
        let mut total = 0;
        total += authorization_request::Entity::delete_many()
            .filter(authorization_request::Column::ExpiresAt.lt(cutoff))
            .exec(&*tx)
            .await?
            .rows_affected;
        total += code::Entity::delete_many()
            .filter(code::Column::ExpiresAt.lt(cutoff))
            .exec(&*tx)
            .await?
            .rows_affected;
        total += device_code::Entity::delete_many()
            .filter(device_code::Column::ExpiresAt.lt(cutoff))
            .exec(&*tx)
            .await?
            .rows_affected;
        total += access_token::Entity::delete_many()
            .filter(
                Condition::any()
                    .add(access_token::Column::ExpiresAt.lt(cutoff))
                    .add(access_token::Column::RevokedAt.lt(cutoff)),
            )
            .exec(&*tx)
            .await?
            .rows_affected;
        // Rotated tokens stay until they expire: reuse detection needs them.
        total += refresh_token::Entity::delete_many()
            .filter(
                Condition::any()
                    .add(refresh_token::Column::ExpiresAt.lt(cutoff))
                    .add(refresh_token::Column::RevokedAt.lt(cutoff)),
            )
            .exec(&*tx)
            .await?
            .rows_affected;
        total += personal_access_token::Entity::delete_many()
            .filter(
                Condition::any()
                    .add(personal_access_token::Column::ExpiresAt.lt(cutoff))
                    .add(personal_access_token::Column::RevokedAt.lt(cutoff)),
            )
            .exec(&*tx)
            .await?
            .rows_affected;
        Ok(total)
    })
    .await?;
    Ok(format!(
        "Pruned {deleted} expired or revoked OAuth record(s) older than {days} day(s).\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn flags_and_positionals_are_read() {
        let list = args(&[
            "Zapier",
            "--redirect-uri",
            "https://a.test/cb",
            "--redirect-uri=https://b.test/cb",
            "--scopes",
            "deals:read,contacts:read",
            "--public",
        ]);
        assert_eq!(positionals(&list), ["Zapier"]);
        assert_eq!(
            flag_values(&list, "--redirect-uri"),
            ["https://a.test/cb", "https://b.test/cb"]
        );
        assert_eq!(flag_values(&list, "--scopes"), ["deals:read,contacts:read"]);
        assert!(has_flag(&list, "--public"));
        assert!(!has_flag(&list, "--trusted"));
    }

    #[test]
    fn keys_print_an_environment_line() {
        let line = generate_keys();
        assert!(line.starts_with("APP_OAUTH__SIGNING_KEY=ed25519:MC4CAQAwBQYDK2VwBCIEI"));
        let value = line.trim().split_once('=').unwrap().1;
        assert!(SigningKey25519::parse(value).is_ok());
    }
}
