//! The `i18n:*` commands.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use navree_kernel::__private::{Published, publish_files};
use navree_kernel::{AppContext, BoxError, CommandSpec, PublishTag, Publishable};

use crate::{I18n, shipped, tag};

/// The names of the module's commands (the app loads the Fluent files
/// leniently while running one, so they can report problems).
pub(crate) const NAMES: [&str; 3] = ["i18n:check", "i18n:missing", "i18n:publish"];

/// What `i18n:publish` answers when it is given no name.
const PUBLISH_USAGE: &str = "usage: i18n:publish validation|<plugin> [--force]";

/// Every command of the localization module.
pub(crate) fn commands() -> [CommandSpec; 3] {
    [
        CommandSpec::new(
            NAMES[0],
            "Parse every Fluent file and list the messages each locale is missing; exits 1 on a problem",
            |ctx, args| async move { run(ctx, args, check).await },
        ),
        CommandSpec::new(
            NAMES[1],
            "List the message ids a locale is missing: i18n:missing <locale>",
            |ctx, args| async move { run(ctx, args, missing).await },
        ),
        CommandSpec::new(
            NAMES[2],
            "Copy the shipped validation messages, or a plugin's translations, into i18n.dir: i18n:publish validation|<plugin> [--force]",
            publish_command,
        ),
    ]
}

/// Runs `i18n:publish` against the app's `I18n` and its plugins and prints
/// its output.
async fn publish_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let i18n = ctx.get::<I18n>().await?;
    println!("{}", publish(&i18n, &ctx, &args)?);
    Ok(())
}

/// Runs a command against the app's `I18n` and prints its output.
async fn run(
    ctx: AppContext,
    args: Vec<String>,
    command: fn(&I18n, &[String]) -> Result<String, BoxError>,
) -> Result<(), BoxError> {
    let i18n = ctx.get::<I18n>().await?;
    println!("{}", command(&i18n, &args)?);
    Ok(())
}

/// `i18n:check`: the report, and an error (exit status 1) when it has
/// problems.
pub(crate) fn check(i18n: &I18n, args: &[String]) -> Result<String, BoxError> {
    if let Some(arg) = args.first() {
        return Err(format!("i18n:check: unexpected argument `{arg}`").into());
    }
    let report = i18n.report();
    let text = report.lines.join("\n");
    if report.problems == 0 {
        Ok(text)
    } else {
        Err(text.into())
    }
}

/// `i18n:missing <locale>`: one id per line.
pub(crate) fn missing(i18n: &I18n, args: &[String]) -> Result<String, BoxError> {
    let [locale] = args else {
        return Err("usage: i18n:missing <locale>".into());
    };
    let ids = i18n.missing(locale)?;
    if ids.is_empty() {
        let tag = i18n.locale(locale);
        Ok(format!("No messages are missing from {tag}."))
    } else {
        Ok(ids.join("\n"))
    }
}

/// One thing `i18n:publish` has a line for, in the order of the supported
/// locales.
enum Step {
    /// A locale the source has nothing for: the line to print.
    Skipped(String),
    /// A file to write below `i18n.dir`: its path there and its bytes.
    File(String, &'static [u8]),
}

/// A plugin and the files it ships under the `locales` tag.
struct PluginLocales {
    /// The plugin's package name.
    name: &'static str,
    /// The plugin's namespace.
    namespace: &'static str,
    /// Each file's path (`{locale}/{rest}`) and bytes.
    files: Vec<(String, &'static [u8])>,
}

/// `i18n:publish validation|{plugin} [--force]`: writes the shipped
/// validation messages, or the translations a plugin ships, into `i18n.dir`
/// for each supported locale, keeping existing files unless `--force`.
///
/// - `validation` writes `{locale}/validation.ftl` for each supported locale
///   whose language navree-i18n ships messages for.
/// - A plugin (its namespace or its name) writes the files `plugin:publish
///   --tag locales` would copy, at `{locale}/{namespace}/{rest}`: for each
///   supported locale, the plugin's files of that locale, else of its first
///   locale of the same language.
///
/// Every file goes through the kernel's checked writer, so a path that is not
/// a safe relative path, two names that are one file on a disk that ignores
/// case, a file that is also another file's folder and a symbolic link in the
/// way are refused with nothing written.
pub(crate) fn publish(i18n: &I18n, ctx: &AppContext, args: &[String]) -> Result<String, BoxError> {
    let mut force = false;
    let mut package = None;
    for arg in args {
        match arg.as_str() {
            "--force" => force = true,
            other if package.is_none() && !other.starts_with('-') => package = Some(other),
            other => return Err(format!("i18n:publish: unexpected argument `{other}`").into()),
        }
    }
    let Some(package) = package else {
        return Err(PUBLISH_USAGE.into());
    };
    let steps = if package == "validation" {
        validation_steps(i18n)
    } else {
        let plugins = plugins_with_locales(ctx);
        let found = plugins
            .iter()
            .find(|plugin| plugin.namespace == package)
            .or_else(|| plugins.iter().find(|plugin| plugin.name == package));
        let Some(plugin) = found else {
            let mut available = vec!["validation"];
            available.extend(plugins.iter().map(|plugin| plugin.namespace));
            return Err(format!(
                "i18n:publish: nothing to publish called `{package}` (available: {})",
                available.join(", ")
            )
            .into());
        };
        plugin_steps(i18n, plugin)
    };
    let dir = &i18n.options().dir;
    let outcomes = write_files(dir, &steps, force)?;
    let mut lines = Vec::new();
    for step in steps {
        match step {
            Step::Skipped(line) => lines.push(line),
            Step::File(path, _) => {
                if let Some(outcome) = outcomes.get(&path) {
                    lines.push(outcome.line(&shown(&dir.join(&path))));
                }
            }
        }
    }
    Ok(lines.join("\n"))
}

/// `path` as the commands show it: `/` between folders.
fn shown(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The supported locales of `i18n`, in order, each once.
fn supported(i18n: &I18n) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut locales = i18n.locales();
    locales.retain(|locale| seen.insert(locale.clone()));
    locales
}

/// The shipped validation messages, for each supported locale whose language
/// has them.
fn validation_steps(i18n: &I18n) -> Vec<Step> {
    let mut steps = Vec::new();
    for locale in supported(i18n) {
        let Ok(langid) = tag::parse(&locale) else {
            continue;
        };
        steps.push(match shipped::validation_for(&langid) {
            Some(source) => Step::File(format!("{locale}/validation.ftl"), source.as_bytes()),
            None => Step::Skipped(format!(
                "{locale}: navree-i18n ships no validation messages in this language"
            )),
        });
    }
    steps
}

/// The plugins that ship files under the `locales` tag, in the order of the
/// `plugins` report.
fn plugins_with_locales(ctx: &AppContext) -> Vec<PluginLocales> {
    let locale_files: Vec<_> = ctx
        .contributions::<Publishable>()
        .into_iter()
        .filter(|contributed| contributed.value.tag() == PublishTag::Locales)
        .collect();
    let report = ctx.plugins();
    let mut found = Vec::new();
    for plugin in &report.plugins {
        let files: Vec<(String, &'static [u8])> = locale_files
            .iter()
            .filter(|contributed| {
                ctx.plugin(contributed.module.id)
                    .is_some_and(|info| info.name == plugin.info.name)
            })
            .flat_map(|contributed| {
                contributed
                    .value
                    .files()
                    .map(|(path, bytes)| (path.to_owned(), bytes))
            })
            .collect();
        if !files.is_empty() {
            found.push(PluginLocales {
                name: plugin.info.name,
                namespace: plugin.info.namespace,
                files,
            });
        }
    }
    found
}

/// `plugin`'s files for each supported locale, at `{locale}/{namespace}/{rest}`,
/// from the plugin's locale that the app's own fallback would reach first: the
/// same tag, else a less specific tag of it (`en` for `en-US`), else the first
/// other locale of the same language by tag (`en-AU` before `en-GB`). A locale
/// the plugin has none for gets a line saying so.
fn plugin_steps(i18n: &I18n, plugin: &PluginLocales) -> Vec<Step> {
    let mut files = plugin.files.clone();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    // The plugin's locales: the first segment of each path, with its canonical
    // tag, in tag order (path order would put `en-GB/` before `en/`).
    let mut locales: Vec<(String, &str)> = Vec::new();
    for (path, _) in &files {
        if let Some((locale, _)) = path.split_once('/')
            && let Ok(canonical) = tag::canonical(locale)
            && !locales.iter().any(|(_, known)| *known == locale)
        {
            locales.push((canonical, locale));
        }
    }
    locales.sort();
    let mut steps = Vec::new();
    for locale in supported(i18n) {
        let Ok(langid) = tag::parse(&locale) else {
            continue;
        };
        let parents = tag::parents(&langid);
        let chosen = locales
            .iter()
            .find(|(canonical, _)| *canonical == locale)
            .or_else(|| {
                parents
                    .iter()
                    .find_map(|parent| locales.iter().find(|(canonical, _)| canonical == parent))
            })
            .or_else(|| {
                locales.iter().find(|(canonical, _)| {
                    tag::parse(canonical).is_ok_and(|other| other.language == langid.language)
                })
            })
            .map(|(_, written)| *written);
        let Some(chosen) = chosen else {
            steps.push(Step::Skipped(format!(
                "{locale}: plugin `{}` ships no messages in this language",
                plugin.namespace
            )));
            continue;
        };
        for (path, bytes) in &files {
            if let Some((from, rest)) = path.split_once('/')
                && from == chosen
            {
                steps.push(Step::File(
                    format!("{locale}/{}/{rest}", plugin.namespace),
                    bytes,
                ));
            }
        }
    }
    steps
}

/// Writes the files of `steps` below `dir`, all through the kernel's checked
/// writer, and says what happened to each, by path. Every check of the writer
/// runs first, as a dry run, so a refusal writes nothing and leaves nothing
/// behind, not even a new `dir`; `dir` is made only when there is something to
/// write.
fn write_files(
    dir: &Path,
    steps: &[Step],
    force: bool,
) -> Result<HashMap<String, Published>, BoxError> {
    let files: Vec<(&str, &[u8])> = steps
        .iter()
        .filter_map(|step| match step {
            Step::File(path, bytes) => Some((path.as_str(), *bytes)),
            Step::Skipped(_) => None,
        })
        .collect();
    if files.is_empty() {
        return Ok(HashMap::new());
    }
    publish_files(dir, &files, force, true)
        .map_err(|refused| format!("i18n:publish: {refused}"))?;
    fs::create_dir_all(dir)
        .map_err(|error| format!("i18n:publish: cannot write {}: {error}", shown(dir)))?;
    let published = publish_files(dir, &files, force, false)
        .map_err(|refused| format!("i18n:publish: {refused}"))?;
    Ok(published.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::I18nOptions;

    /// What every test file holds.
    const NOTICES: &[u8] = b"announcements-title = Notices\n";

    /// Steps that write `NOTICES` at each of `paths`.
    fn files(paths: &[&str]) -> Vec<Step> {
        paths
            .iter()
            .map(|path| Step::File((*path).to_owned(), NOTICES))
            .collect()
    }

    /// What each step stands for: a file's path, or the line to print.
    fn describe(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .map(|step| match step {
                Step::Skipped(line) => line.clone(),
                Step::File(path, _) => path.clone(),
            })
            .collect()
    }

    /// The paths `i18n:publish` writes go through the kernel's writer, which refuses a path
    /// that is not a safe relative path, two names that are one file on a disk that ignores
    /// case and a file that is also a folder, before it writes anything: not even the files
    /// that were fine, and not even the folder `i18n.dir` that does not exist yet.
    #[test]
    fn every_path_goes_through_the_checked_writer_and_a_refusal_writes_nothing() {
        let temporary = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let dir = temporary.path().join("locales");
        let not_safe = [
            "../escape.ftl",
            "en/announcements/../../escape.ftl",
            "/etc/escape.ftl",
            "C:/escape.ftl",
            "en/announcements\\escape.ftl",
            "en/announcements/a:b.ftl",
            "en/announcements/con.ftl",
            "en/announcements/NUL.txt",
            "en/announcements/notes.",
            "en/announcements/notes ",
            "en//announcements/a.ftl",
            "",
        ];
        for path in not_safe {
            for force in [false, true] {
                let steps = files(&["en/announcements/ok.ftl", path]);
                let error = write_files(&dir, &steps, force).unwrap_err().to_string();
                assert_eq!(
                    error,
                    format!(
                        "i18n:publish: {path} is not a safe relative path; nothing was written"
                    )
                );
                assert!(!dir.exists(), "{path:?}");
            }
        }
        for (paths, refusal) in [
            (
                [
                    "en/announcements/notices.ftl",
                    "en/announcements/Notices.ftl",
                ],
                "en/announcements/notices.ftl is the same file as another one on a disk that ignores case; nothing was written",
            ),
            (
                ["en/announcements/a.ftl", "en/announcements/a.ftl"],
                "en/announcements/a.ftl is the same file as another one on a disk that ignores case; nothing was written",
            ),
            (
                ["en/announcements/admin", "en/announcements/admin/edit.ftl"],
                "en/announcements/admin would be both a file and the folder of en/announcements/admin/edit.ftl; nothing was written",
            ),
        ] {
            let error = write_files(&dir, &files(&paths), false)
                .unwrap_err()
                .to_string();
            assert_eq!(error, format!("i18n:publish: {refusal}"));
            assert!(!dir.exists(), "{paths:?}");
        }
        // The same files, fine, are written: the folder is made and the files are in it.
        let outcomes = write_files(&dir, &files(&["en/announcements/ok.ftl"]), false).unwrap();
        assert_eq!(
            outcomes.get("en/announcements/ok.ftl"),
            Some(&Published::Written)
        );
        assert_eq!(
            fs::read(dir.join("en/announcements/ok.ftl")).unwrap(),
            NOTICES
        );
    }

    /// A supported locale takes the plugin's files of its own tag, else of the first locale
    /// of its language, else gets a line saying the plugin has none; the lines follow the
    /// order of `i18n.locales`.
    #[test]
    fn a_plugins_files_follow_the_supported_locales_and_fall_back_to_the_same_language() {
        let i18n = I18n::new(I18nOptions {
            locales: vec!["en".into(), "pa-IN".into(), "ta".into()],
            dir: "tests/fixtures/locales".into(),
            ..I18nOptions::default()
        })
        .unwrap();
        let plugin = PluginLocales {
            name: "navree-plugin-announcements",
            namespace: "announcements",
            files: vec![
                ("pa/mail/welcome.ftl".to_owned(), NOTICES),
                ("hi/announcements.ftl".to_owned(), NOTICES),
                ("en/announcements.ftl".to_owned(), NOTICES),
                ("pa/announcements.ftl".to_owned(), NOTICES),
            ],
        };
        assert_eq!(
            describe(&plugin_steps(&i18n, &plugin)),
            [
                "en/announcements/announcements.ftl",
                "pa-IN/announcements/announcements.ftl",
                "pa-IN/announcements/mail/welcome.ftl",
                "ta: plugin `announcements` ships no messages in this language",
            ]
        );
    }

    /// A supported region takes the plugin's file of the language alone before one of a sibling
    /// region, as the app's own fallback would reach it, and among siblings the first by tag:
    /// `en-US` reads the plugin's `en`, not `en-GB` (which sorts before `en/` as a path), and
    /// `de-AT`, with no `de` to fall back to, takes `de-CH` before `de-DE`.
    #[test]
    fn a_region_takes_the_plugins_language_before_a_sibling_region() {
        const EN: &[u8] = b"announcements-title = Notices\n";
        const EN_GB: &[u8] = b"announcements-title = Notices, in British spelling\n";
        const DE_CH: &[u8] = b"announcements-title = Mitteilungen aus der Schweiz\n";
        const DE_DE: &[u8] = b"announcements-title = Mitteilungen aus Deutschland\n";
        let i18n = I18n::new(I18nOptions {
            locales: vec!["en".into(), "en-US".into(), "de-AT".into()],
            dir: "tests/fixtures/locales".into(),
            ..I18nOptions::default()
        })
        .unwrap();
        let plugin = PluginLocales {
            name: "navree-plugin-announcements",
            namespace: "announcements",
            files: vec![
                ("en-GB/announcements.ftl".to_owned(), EN_GB),
                ("en/announcements.ftl".to_owned(), EN),
                ("de-DE/announcements.ftl".to_owned(), DE_DE),
                ("de-CH/announcements.ftl".to_owned(), DE_CH),
            ],
        };
        let written: Vec<(String, &[u8])> = plugin_steps(&i18n, &plugin)
            .into_iter()
            .filter_map(|step| match step {
                Step::File(path, bytes) => Some((path, bytes)),
                Step::Skipped(_) => None,
            })
            .collect();
        assert_eq!(
            written,
            [
                ("en/announcements/announcements.ftl".to_owned(), EN),
                ("en-US/announcements/announcements.ftl".to_owned(), EN),
                ("de-AT/announcements/announcements.ftl".to_owned(), DE_CH),
            ]
        );
    }
}
