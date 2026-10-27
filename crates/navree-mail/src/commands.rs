//! The commands `mail:test`, `mail:suppressions` and `mail:domains`.

use std::fmt::Write as _;

use navree_db::{Db, PageQuery};
use navree_kernel::{AppContext, BoxError, CommandSpec};
use uuid::Uuid;

use crate::{
    CheckStatus, DomainExpectations, DomainReport, MailScope, Mailer, Message, RecordCheck,
    SendReport, SendingDomains, SuppressionReason, Suppressions,
};

/// Every command of the mail module.
pub(crate) fn commands() -> [CommandSpec; 3] {
    [
        CommandSpec::new(
            "mail:test",
            "Send a test message and print how it went (<to> [--via <transport>] [--workspace <id>] [--organization <id>] [--subject <text>])",
            test_command,
        ),
        CommandSpec::new(
            "mail:suppressions",
            "List, add or remove suppressed addresses ([list|add|remove] [<email>] [--workspace <id>] [--reason <reason>])",
            suppressions_command,
        ),
        CommandSpec::new(
            "mail:domains",
            "Check a sending domain's SPF, DKIM and DMARC records (<domain> [--selector <s>]... [--spf-include <domain>]...)",
            domains_command,
        ),
    ]
}

/// The value after `--name` (or in `--name=value`).
fn flag(args: &[String], name: &str) -> Result<Option<String>, BoxError> {
    Ok(flags(args, name)?.into_iter().next())
}

/// Every value given for `--name`.
fn flags(args: &[String], name: &str) -> Result<Vec<String>, BoxError> {
    let long = format!("--{name}");
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if *arg == long {
            values.push(
                iter.next()
                    .cloned()
                    .ok_or_else(|| format!("`{long}` needs a value"))?,
            );
        } else if let Some(value) = arg.strip_prefix(&format!("{long}=")) {
            values.push(value.to_owned());
        }
    }
    Ok(values)
}

/// The arguments that are neither flags nor flag values.
fn positionals(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg.starts_with("--") {
            if !arg.contains('=') {
                iter.next();
            }
        } else {
            out.push(arg.as_str());
        }
    }
    out
}

fn parse_id(text: &str, what: &str) -> Result<Uuid, BoxError> {
    Uuid::parse_str(text).map_err(|_| format!("`{text}` is not a {what} id").into())
}

/// What `mail:test` prints.
pub(crate) fn format_report(report: &SendReport) -> String {
    let mut out = format!("{}", report.status);
    if let Some(transport) = report.transport {
        let _ = write!(out, " via {transport}");
    }
    if let Some(tier) = report.tier {
        let _ = write!(out, " ({tier})");
    }
    if let Some(id) = &report.provider_id {
        let _ = write!(out, "  provider_id={id}");
    }
    if let Some(id) = report.delivery_id {
        let _ = write!(out, "  delivery={id}");
    }
    if !report.suppressed.is_empty() {
        let _ = write!(out, "  suppressed={}", report.suppressed.join(","));
    }
    out.push('\n');
    out
}

async fn test_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let usage = "usage: mail:test <to> [--via <transport>] [--workspace <id>] [--organization <id>] [--subject <text>]";
    let to = *positionals(&args).first().ok_or(usage)?;
    let subject = flag(&args, "subject")?.unwrap_or_else(|| "Test message".to_owned());
    let mut scope = MailScope::platform();
    if let Some(id) = flag(&args, "workspace")? {
        scope.workspace_id = Some(parse_id(&id, "workspace")?);
    }
    if let Some(id) = flag(&args, "organization")? {
        scope.organization_id = Some(parse_id(&id, "organization")?);
    }
    let message = Message::new().to(to).subject(subject).text(
        "This is a test message sent with `mail:test`. If you can read it, the transport works.",
    );
    let mailer = ctx.get::<Mailer>().await?;
    let mut scoped = mailer.for_scope(&scope);
    if let Some(via) = flag(&args, "via")? {
        scoped = scoped.via(via);
    }
    let report = scoped.deliver(&message).await?;
    print!("{}", format_report(&report));
    Ok(())
}

async fn suppressions_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let positional = positionals(&args);
    let action = positional.first().copied().unwrap_or("list");
    let workspace = match flag(&args, "workspace")? {
        Some(id) => Some(parse_id(&id, "workspace")?),
        None => None,
    };
    let suppressions = ctx.get::<Suppressions>().await?;
    match action {
        "list" => {
            let page = suppressions
                .list(
                    workspace,
                    positional.get(1).copied(),
                    PageQuery::new(1, 1000),
                )
                .await?;
            if page.data.is_empty() {
                println!("No suppressed addresses.");
            }
            for row in page.data {
                let scope = row
                    .workspace_id
                    .map_or_else(|| "everywhere".to_owned(), |id| format!("workspace {id}"));
                println!(
                    "{}  {}  {}  {}  {}",
                    row.id,
                    row.email,
                    row.reason,
                    scope,
                    row.created_at.to_rfc3339()
                );
            }
            Ok(())
        }
        "add" | "remove" => {
            let email = positional
                .get(1)
                .copied()
                .ok_or("usage: mail:suppressions add|remove <email> [--workspace <id>]")?;
            let db = ctx.get::<Db>().await?;
            let uow = db.uow(None, None).await;
            let result = if action == "add" {
                let reason = match flag(&args, "reason")? {
                    Some(reason) => reason.parse::<SuppressionReason>()?,
                    None => SuppressionReason::Manual,
                };
                suppressions
                    .add(&uow, workspace, email, reason, None)
                    .await
                    .map(|row| format!("Suppressed {} ({}).", row.email, row.reason))
            } else {
                suppressions
                    .remove_email(&uow, workspace, email)
                    .await
                    .map(|removed| {
                        if removed {
                            format!("{email} is no longer suppressed.")
                        } else {
                            format!("{email} was not suppressed.")
                        }
                    })
            };
            match result {
                Ok(line) => {
                    uow.commit().await?;
                    println!("{line}");
                    Ok(())
                }
                Err(error) => {
                    uow.rollback().await?;
                    Err(error.into())
                }
            }
        }
        other => Err(format!("unknown action `{other}`: use list, add or remove").into()),
    }
}

/// What `mail:domains` prints.
pub(crate) fn format_domain_report(report: &DomainReport) -> String {
    let mut out = format!("{}\n", report.domain);
    let mut line = |label: &str, check: &RecordCheck| {
        let status = match check.status {
            CheckStatus::Pass => "pass",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "FAIL",
        };
        let _ = writeln!(out, "  {label:<12} {status}  {}", check.name);
        if let Some(record) = &check.record {
            let _ = writeln!(out, "               {record}");
        }
        for problem in &check.problems {
            let _ = writeln!(out, "               - {problem}");
        }
    };
    line("SPF", &report.spf);
    for dkim in &report.dkim {
        line(&format!("DKIM {}", dkim.selector), &dkim.check);
    }
    line("DMARC", &report.dmarc);
    out.push_str(if report.ready() {
        "Ready to send.\n"
    } else {
        "Not ready: fix the failing records.\n"
    });
    out
}

async fn domains_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let domain = *positionals(&args)
        .first()
        .ok_or("usage: mail:domains <domain> [--selector <s>]... [--spf-include <domain>]...")?;
    let mut expect = DomainExpectations::new();
    for selector in flags(&args, "selector")? {
        expect = expect.selector(selector);
    }
    for include in flags(&args, "spf-include")? {
        expect = expect.spf_include(include);
    }
    let report = ctx
        .get::<SendingDomains>()
        .await?
        .check(domain, &expect)
        .await?;
    print!("{}", format_domain_report(&report));
    if report.ready() {
        Ok(())
    } else {
        Err(format!("{domain} is not ready to send").into())
    }
}
