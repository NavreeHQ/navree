//! The `media:regenerate` command.

use navree_kernel::{AppContext, BoxError, CommandSpec};
use uuid::Uuid;

use crate::{Media, Regenerate};

const USAGE: &str = "usage: media:regenerate [--model <Type>] [--collection <name>] \
                     [--id <uuid>]... [--only <conversion,...>] [--missing] [--queue]";

/// Every command of the media module.
pub(crate) fn commands() -> [CommandSpec; 1] {
    [CommandSpec::new(
        "media:regenerate",
        "Generate media conversions again: media:regenerate [--model <Type>] \
         [--collection <name>] [--id <uuid>]... [--only <conversion,...>] [--missing] [--queue]",
        |ctx, args| async move {
            println!("{}", regenerate(ctx, args).await?);
            Ok(())
        },
    )]
}

/// The arguments of `media:regenerate`.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct RegenerateArgs {
    pub(crate) filter: Regenerate,
    pub(crate) queue: bool,
}

pub(crate) fn parse_regenerate_args(args: &[String]) -> Result<RegenerateArgs, BoxError> {
    let mut parsed = RegenerateArgs::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .cloned()
                .ok_or_else(|| format!("media:regenerate: {name} needs a value; {USAGE}"))
        };
        match arg.as_str() {
            "--model" => parsed.filter.model_type = Some(value("--model")?),
            "--collection" => parsed.filter.collection = Some(value("--collection")?),
            "--id" => {
                let id = value("--id")?;
                let id = Uuid::parse_str(&id)
                    .map_err(|_| format!("media:regenerate: `{id}` is not a UUID"))?;
                parsed.filter.ids.push(id);
            }
            "--only" => parsed.filter.conversions.extend(
                value("--only")?
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned),
            ),
            "--missing" => parsed.filter.missing_only = true,
            "--queue" => parsed.queue = true,
            other => {
                return Err(format!("media:regenerate: unexpected argument `{other}`; {USAGE}").into());
            }
        }
    }
    Ok(parsed)
}

/// `media:regenerate`.
async fn regenerate(ctx: AppContext, args: Vec<String>) -> Result<String, BoxError> {
    let args = parse_regenerate_args(&args)?;
    let media = ctx.get::<Media>().await?;
    if args.queue {
        return Ok(match media.queue_regenerate(&args.filter).await? {
            Some(batch) => format!(
                "Queued {} media {} in batch {}",
                batch.total_jobs,
                if batch.total_jobs == 1 { "item" } else { "items" },
                batch.id
            ),
            None => "Nothing to regenerate".to_owned(),
        });
    }
    let report = media.regenerate(&args.filter).await?;
    let mut out = format!(
        "Generated {} {} of {} media {}",
        report.conversions,
        if report.conversions == 1 { "conversion" } else { "conversions" },
        report.media,
        if report.media == 1 { "item" } else { "items" },
    );
    for (id, error) in &report.failed {
        out.push_str(&format!("\nfailed {id}: {error}"));
    }
    if !report.failed.is_empty() {
        return Err(out.into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn arguments() {
        let id = Uuid::now_v7();
        let parsed = parse_regenerate_args(&args(&[
            "--model",
            "Course",
            "--collection",
            "thumbnail",
            "--id",
            &id.to_string(),
            "--only",
            "card, hero",
            "--missing",
            "--queue",
        ]))
        .unwrap();
        assert_eq!(parsed.filter.model_type.as_deref(), Some("Course"));
        assert_eq!(parsed.filter.collection.as_deref(), Some("thumbnail"));
        assert_eq!(parsed.filter.ids, vec![id]);
        assert_eq!(parsed.filter.conversions, vec!["card", "hero"]);
        assert!(parsed.filter.missing_only);
        assert!(parsed.queue);
        assert_eq!(parse_regenerate_args(&[]).unwrap(), RegenerateArgs::default());
    }

    #[test]
    fn bad_arguments() {
        let error = parse_regenerate_args(&args(&["--id", "nope"])).unwrap_err();
        assert!(error.to_string().contains("not a UUID"), "{error}");
        let error = parse_regenerate_args(&args(&["--model"])).unwrap_err();
        assert!(error.to_string().contains("needs a value"), "{error}");
        let error = parse_regenerate_args(&args(&["thumbs"])).unwrap_err();
        assert!(error.to_string().contains("unexpected argument"), "{error}");
    }
}
