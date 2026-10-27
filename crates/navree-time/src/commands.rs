//! The `time:zones` command.

use chrono::{DateTime, Utc};
use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::Zones;
use crate::controller::zone_list;

/// Every command of the time module.
pub(crate) fn commands() -> [CommandSpec; 1] {
    [CommandSpec::new(
        "time:zones",
        "List time zones with their current offsets: time:zones [search] [--country CC]",
        |ctx, args| async move { zones(ctx, args).await },
    )]
}

/// `time:zones [search] [--country CC]`.
async fn zones(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let mut search: Option<String> = None;
    let mut country: Option<String> = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--country" => {
                country = Some(
                    args.next()
                        .ok_or("time:zones: --country needs a country code")?,
                );
            }
            flag if flag.starts_with("--") => {
                return Err(format!("time:zones: unknown option `{flag}`").into());
            }
            _ if search.is_none() => search = Some(arg),
            _ => return Err(format!("time:zones: unexpected argument `{arg}`").into()),
        }
    }
    let now = match ctx.try_get::<Zones>().await {
        Some(zones) => zones.now(),
        None => Utc::now(),
    };
    println!(
        "{}",
        zones_listing(search.as_deref(), country.as_deref(), now)?
    );
    Ok(())
}

/// The `time:zones` table: name, offset at `now` and countries of every
/// zone matching `search` (any case) in `country` (any case).
pub fn zones_listing(
    search: Option<&str>,
    country: Option<&str>,
    now: DateTime<Utc>,
) -> Result<String, BoxError> {
    let zones = zone_list(country, search).map_err(|_| {
        format!(
            "time:zones: `{}` is not an ISO 3166-1 alpha-2 country code",
            country.unwrap_or_default()
        )
    })?;
    if zones.is_empty() {
        return Ok("No time zones match.".to_owned());
    }
    let mut rows = vec![[
        "zone".to_owned(),
        "offset".to_owned(),
        "countries".to_owned(),
    ]];
    for zone in zones {
        rows.push([
            zone.name().to_owned(),
            zone.offset_label(now),
            zone.countries().join(","),
        ]);
    }
    let width = rows.iter().map(|row| row[0].len()).max().unwrap_or(0);
    Ok(rows
        .iter()
        .map(|row| format!("{:width$}  {:6}  {}", row[0], row[1], row[2]).trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n"))
}
