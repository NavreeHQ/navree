//! The commands: `billing:plans`, `billing:webhook`, `billing:report-usage`,
//! `billing:sync`, `billing:events` and `billing:prune-events`.

use navree_kernel::{AppContext, BoxError, CommandSpec};
use uuid::Uuid;

use crate::billable::BillableRef;
use crate::billing::Billing;

/// Every command of the billing module.
pub(crate) fn commands() -> [CommandSpec; 6] {
    [
        CommandSpec::new(
            "billing:plans",
            "List the plans and check every price id against the provider",
            plans_command,
        ),
        CommandSpec::new(
            "billing:webhook",
            "Create the provider's webhook endpoint for <url> with the events Navree handles",
            webhook_command,
        ),
        CommandSpec::new(
            "billing:report-usage",
            "Report unreported metered usage to the provider",
            report_usage_command,
        ),
        CommandSpec::new(
            "billing:sync",
            "Pull a billable's subscriptions from the provider: <type> <id>",
            sync_command,
        ),
        CommandSpec::new(
            "billing:events",
            "List received webhook events ([--failed] for the ones that failed)",
            events_command,
        ),
        CommandSpec::new(
            "billing:prune-events",
            "Delete processed webhook events older than N days ([--days N], default 90)",
            prune_command,
        ),
    ]
}

async fn plans_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let billing = ctx.get::<Billing>().await?;
    if billing.plans().is_empty() {
        println!("No plans: add them under [billing.plans.<key>].");
        return Ok(());
    }
    let mut missing = 0;
    for plan in billing.plans() {
        println!(
            "{} ({}), trial {} day(s){}",
            plan.key,
            plan.name,
            plan.trial_days,
            if plan.seats { ", per seat" } else { "" }
        );
        let prices = plan
            .monthly_price
            .iter()
            .map(|price| ("monthly".to_owned(), price.clone()))
            .chain(
                plan.yearly_price
                    .iter()
                    .map(|price| ("yearly".to_owned(), price.clone())),
            )
            .chain(
                plan.meters
                    .iter()
                    .map(|(meter, price)| (format!("meter {meter}"), price.clone())),
            )
            .collect::<Vec<_>>();
        for (kind, price) in prices {
            let found = billing.provider().price_exists(&price).await?;
            if !found {
                missing += 1;
            }
            println!("  {kind}: {price} {}", if found { "ok" } else { "MISSING" });
        }
    }
    if missing > 0 {
        return Err(format!("{missing} price id(s) do not exist at the provider").into());
    }
    Ok(())
}

async fn webhook_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let Some(url) = args.first() else {
        return Err(
            "usage: billing:webhook <url> (https://api.example.com/api/billing/webhook)".into(),
        );
    };
    let billing = ctx.get::<Billing>().await?;
    let endpoint = billing.provider().create_webhook_endpoint(url).await?;
    println!("Created webhook endpoint {} for {url}", endpoint.id);
    println!("Events: {}", endpoint.events.join(", "));
    if let Some(secret) = endpoint.secret {
        let key = match billing.provider().name() {
            // The fake signs like Stripe.
            "stripe" | "fake" => "APP_BILLING__STRIPE__WEBHOOK_SECRET".to_owned(),
            name => format!(
                "APP_BILLING__{}__WEBHOOK_SECRET",
                name.to_ascii_uppercase().replace('-', "_")
            ),
        };
        println!("Put its signing secret in .env:\n{key}={secret}");
    }
    Ok(())
}

async fn report_usage_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let billing = ctx.get::<Billing>().await?;
    let summary = billing.report_pending_usage().await?;
    println!(
        "Reported {} batch(es), {} unit(s); {} failed; {} row(s) skipped.",
        summary.reported, summary.units, summary.failed, summary.skipped
    );
    Ok(())
}

async fn sync_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let [billable_type, id] = args.as_slice() else {
        return Err("usage: billing:sync <type> <id> (billing:sync workspace 0199...)".into());
    };
    let id: Uuid = id.parse().map_err(|_| format!("`{id}` is not a UUID"))?;
    let billing = ctx.get::<Billing>().await?;
    let synced = billing
        .sync(&BillableRef::new(billable_type.clone(), id))
        .await?;
    for sub in &synced {
        println!(
            "{} {} {} {} x{}",
            sub.provider_id.as_deref().unwrap_or("-"),
            sub.plan,
            sub.interval,
            sub.status,
            sub.quantity
        );
    }
    println!("Synced {} subscription(s).", synced.len());
    Ok(())
}

async fn events_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let failed = args.iter().any(|arg| arg == "--failed");
    let billing = ctx.get::<Billing>().await?;
    let events = billing.events(failed).await?;
    if events.is_empty() {
        println!("No events.");
    }
    for event in events {
        let state = match (&event.processed_at, &event.error) {
            (Some(at), _) => format!("processed {}", at.to_rfc3339()),
            (None, Some(error)) => format!("failed ({} attempt(s)): {error}", event.attempts),
            (None, None) => "pending".to_owned(),
        };
        println!(
            "{} {} {} {} {state}",
            event.received_at.to_rfc3339(),
            event.provider,
            event.provider_event_id,
            event.event_type
        );
    }
    Ok(())
}

async fn prune_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let days = match args.iter().position(|arg| arg == "--days") {
        Some(index) => args
            .get(index + 1)
            .and_then(|days| days.parse::<u32>().ok())
            .ok_or("usage: billing:prune-events [--days N]")?,
        None => 90,
    };
    let billing = ctx.get::<Billing>().await?;
    let pruned = billing.prune_events(days).await?;
    println!("Pruned {pruned} processed event(s) older than {days} day(s).");
    Ok(())
}
