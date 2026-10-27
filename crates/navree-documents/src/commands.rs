//! The `documents:*` commands.

use std::path::PathBuf;

use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::{Documents, EngineKind, Render};

/// Every command of the documents module.
pub(crate) fn commands() -> [CommandSpec; 2] {
    [
        CommandSpec::new(
            "documents:render",
            "Render a document template to a PDF file: documents:render <template> \
             [--data <file.json>] [--locale <tag>] [--engine typst|chrome] [--out <file.pdf>]",
            |ctx, args| async move { print(render(ctx, args).await) },
        ),
        CommandSpec::new(
            "documents:fonts",
            "List the font families Typst templates can use",
            |ctx, args| async move { print(fonts(ctx, args).await) },
        ),
    ]
}

/// Prints a command's output.
fn print(output: Result<String, BoxError>) -> Result<(), BoxError> {
    println!("{}", output?);
    Ok(())
}

/// The arguments of `documents:render`.
#[derive(Debug, PartialEq)]
pub(crate) struct RenderArgs {
    pub(crate) template: String,
    pub(crate) data: Option<PathBuf>,
    pub(crate) locale: Option<String>,
    pub(crate) engine: Option<EngineKind>,
    pub(crate) out: PathBuf,
}

pub(crate) fn parse_render_args(args: &[String]) -> Result<RenderArgs, BoxError> {
    const USAGE: &str = "usage: documents:render <template> [--data <file.json>] \
                         [--locale <tag>] [--engine typst|chrome] [--out <file.pdf>]";
    let mut template = None;
    let mut data = None;
    let mut locale = None;
    let mut engine = None;
    let mut out = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .cloned()
                .ok_or_else(|| format!("documents:render: {name} needs a value; {USAGE}"))
        };
        match arg.as_str() {
            "--data" => data = Some(PathBuf::from(value("--data")?)),
            "--locale" => locale = Some(value("--locale")?),
            "--engine" => {
                engine = Some(
                    value("--engine")?
                        .parse::<EngineKind>()
                        .map_err(|error| format!("documents:render: {error}"))?,
                );
            }
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            flag if flag.starts_with("--") => {
                return Err(format!("documents:render: unknown option `{flag}`; {USAGE}").into());
            }
            name if template.is_none() => template = Some(name.to_owned()),
            extra => {
                return Err(
                    format!("documents:render: unexpected argument `{extra}`; {USAGE}").into(),
                );
            }
        }
    }
    let template = template.ok_or_else(|| format!("documents:render: name a template; {USAGE}"))?;
    let out = out.unwrap_or_else(|| {
        let base = template.rsplit('/').next().unwrap_or(&template);
        let base = base
            .strip_suffix(".typ")
            .or_else(|| base.strip_suffix(".html"))
            .unwrap_or(base);
        PathBuf::from(format!("{base}.pdf"))
    });
    Ok(RenderArgs {
        template,
        data,
        locale,
        engine,
        out,
    })
}

/// `documents:render`.
async fn render(ctx: AppContext, args: Vec<String>) -> Result<String, BoxError> {
    let args = parse_render_args(&args)?;
    let documents = ctx.get::<Documents>().await?;
    let mut render = Render::template(&args.template);
    if let Some(path) = &args.data {
        let text = tokio::fs::read_to_string(path).await.map_err(|error| {
            format!("documents:render: cannot read {}: {error}", path.display())
        })?;
        let data: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
            format!("documents:render: {} is not JSON: {error}", path.display())
        })?;
        render = render.data(data);
    }
    if let Some(locale) = &args.locale {
        render = render.locale(locale);
    }
    if let Some(engine) = args.engine {
        render = render.engine(engine);
    }
    let engine = documents.engine_for(&render)?;
    let pdf = documents.render(&render).await;
    documents.shutdown().await;
    let pdf = pdf?;
    let pages = pdf.page_count()?;
    let bytes = pdf.len();
    tokio::fs::write(&args.out, pdf.bytes())
        .await
        .map_err(|error| {
            format!(
                "documents:render: cannot write {}: {error}",
                args.out.display()
            )
        })?;
    Ok(format!(
        "Rendered {} with {engine} to {} ({pages} {}, {bytes} bytes)",
        args.template,
        args.out.display(),
        if pages == 1 { "page" } else { "pages" },
    ))
}

/// `documents:fonts`.
async fn fonts(ctx: AppContext, args: Vec<String>) -> Result<String, BoxError> {
    if let Some(arg) = args.first() {
        return Err(format!("documents:fonts: unexpected argument `{arg}`").into());
    }
    let documents = ctx.get::<Documents>().await?;
    let families = documents.font_families().await?;
    if families.is_empty() {
        return Ok(
            "No fonts: documents.typst.bundled_fonts is off and no font directory has fonts".into(),
        );
    }
    Ok(families.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn render_arguments() {
        let parsed = parse_render_args(&args(&[
            "certificates/classic",
            "--data",
            "sample.json",
            "--locale",
            "ar",
            "--engine",
            "chrome",
            "--out",
            "out/c.pdf",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            RenderArgs {
                template: "certificates/classic".into(),
                data: Some("sample.json".into()),
                locale: Some("ar".into()),
                engine: Some(EngineKind::Chrome),
                out: "out/c.pdf".into(),
            }
        );
        let parsed = parse_render_args(&args(&["reports/audit.typ"])).unwrap();
        assert_eq!(parsed.out, PathBuf::from("audit.pdf"));
        assert!(parse_render_args(&args(&[])).is_err());
        assert!(parse_render_args(&args(&["a", "b"])).is_err());
        assert!(parse_render_args(&args(&["a", "--engine", "unknown"])).is_err());
        assert!(parse_render_args(&args(&["a", "--data"])).is_err());
        assert!(parse_render_args(&args(&["a", "--bogus"])).is_err());
    }
}
