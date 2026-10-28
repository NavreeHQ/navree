//! The commands `domains:list`, `domains:verify` and
//! `domains:certificates`.

use navree_kernel::{AppContext, BoxError, CommandSpec};
use uuid::Uuid;

use crate::types::{CustomDomain, DomainStatus};
use crate::{Domains, DomainsError};

/// The module's commands.
pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(
            "domains:list",
            "List custom domains [--workspace <id>] [--status pending|verified|failed]",
            list,
        ),
        CommandSpec::new(
            "domains:verify",
            "Check custom domains' DNS: <host-or-id> | --due | --all",
            verify,
        ),
        CommandSpec::new(
            "domains:certificates",
            "Issue, renew or probe certificates and report the ones close to expiry: [<host>] [--due]",
            certificates,
        ),
    ]
}

/// The value after `name` in `args`.
fn value<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, BoxError> {
    match args.iter().position(|arg| arg == name) {
        Some(at) => args
            .get(at + 1)
            .map(|value| Some(value.as_str()))
            .ok_or_else(|| format!("{name} needs a value").into()),
        None => Ok(None),
    }
}

/// `host  status  tls  expires  workspace  last error`.
pub(crate) fn line(domain: &CustomDomain) -> String {
    let expires = domain
        .certificate_expires_at
        .map_or_else(|| "-".to_owned(), |at| at.format("%Y-%m-%d").to_string());
    let mut line = format!(
        "{}  {}  tls:{}  expires:{}  workspace:{}",
        domain.host,
        domain.status.as_str(),
        domain.tls.as_str(),
        expires,
        domain.workspace_id
    );
    if domain.primary {
        line.push_str("  primary");
    }
    if let Some(error) = &domain.last_error {
        line.push_str("  ");
        line.push_str(error);
    }
    line
}

/// `domains:list [--workspace <id>] [--status <status>]`.
async fn list(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let domains = ctx.get::<Domains>().await?;
    let status = match value(&args, "--status")? {
        Some("pending") => Some(DomainStatus::Pending),
        Some("verified") => Some(DomainStatus::Verified),
        Some("failed") => Some(DomainStatus::Failed),
        Some(other) => return Err(format!("unknown status `{other}`").into()),
        None => None,
    };
    let rows = match value(&args, "--workspace")? {
        Some(id) => {
            let id: Uuid = id
                .parse()
                .map_err(|_| format!("`{id}` is not a workspace id"))?;
            domains
                .list(id)
                .await?
                .into_iter()
                .filter(|domain| status.is_none_or(|status| domain.status == status))
                .collect()
        }
        None => domains.all(status).await?,
    };
    if rows.is_empty() {
        println!("No custom domains.");
    }
    for domain in &rows {
        println!("{}", line(domain));
    }
    Ok(())
}

/// `domains:verify <host-or-id> | --due | --all`.
async fn verify(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let domains = ctx.get::<Domains>().await?;
    if args.iter().any(|arg| arg == "--due") {
        let checked = domains.verify_due().await?;
        let issued = domains.issue_pending().await?;
        let verified = checked.iter().filter(|domain| domain.is_verified()).count();
        if !checked.is_empty() || !issued.is_empty() {
            println!(
                "Checked {} domain(s), {verified} verified; {} certificate(s) issued",
                checked.len(),
                issued.len()
            );
        }
        return Ok(());
    }
    let targets: Vec<Uuid> = if args.iter().any(|arg| arg == "--all") {
        domains
            .all(None)
            .await?
            .into_iter()
            .map(|domain| domain.id)
            .collect()
    } else {
        let Some(target) = args.iter().find(|arg| !arg.starts_with("--")) else {
            return Err("name a domain (host or id), or pass --due or --all".into());
        };
        let id = match target.parse::<Uuid>() {
            Ok(id) => id,
            Err(_) => {
                domains
                    .find(target)
                    .await?
                    .ok_or_else(|| format!("no custom domain `{target}`"))?
                    .id
            }
        };
        vec![id]
    };
    let mut failed = false;
    for id in targets {
        match domains.verify(id).await {
            Ok(domain) => println!("{}", line(&domain)),
            Err(error @ DomainsError::Dns { .. }) => {
                failed = true;
                println!("{id}  {error}");
            }
            Err(error) => return Err(error.into()),
        }
    }
    if failed {
        return Err("a DNS lookup failed".into());
    }
    Ok(())
}

/// `domains:certificates [<host>] [--due]`.
async fn certificates(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let domains = ctx.get::<Domains>().await?;
    let host = args.iter().find(|arg| !arg.starts_with("--"));
    let run = domains.check_certificates(host.map(String::as_str)).await?;
    for host in &run.issued {
        println!("issued    {host}");
    }
    for host in &run.renewed {
        println!("renewed   {host}");
    }
    for (host, expires) in &run.probed {
        println!("probed    {host}  expires {}", expires.format("%Y-%m-%d"));
    }
    for (host, expires) in &run.expiring {
        println!("EXPIRING  {host}  {}", expires.format("%Y-%m-%d %H:%M UTC"));
    }
    for (host, reason) in &run.failed {
        println!("FAILED    {host}  {reason}");
    }
    if !run.failed.is_empty() && host.is_some() {
        return Err("the certificate could not be issued or read".into());
    }
    Ok(())
}
