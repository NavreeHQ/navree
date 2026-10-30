//! The [`Catalog`]: an app's Fluent files loaded into one bundle per
//! locale, with the fallback chains, lenient tag matching, and the
//! `i18n:check` report.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use fluent_syntax::ast;
use unic_langid::LanguageIdentifier;

use crate::args::{datetime, format_value};
use crate::options::{BidiIsolation, I18nOptions};
use crate::{shipped, tag};

/// One locale's Fluent bundle.
pub(crate) type Bundle = FluentBundle<Arc<FluentResource>>;

/// A Fluent file: its locale directory's name, the path shown in messages
/// (`locales/fr/deals.ftl`) and its source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceFile {
    pub(crate) locale: String,
    pub(crate) path: String,
    pub(crate) source: String,
}

/// Where an app message is defined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Definition {
    pub(crate) path: String,
    pub(crate) line: usize,
}

/// One locale: its bundle (the app's files, then the shipped validation
/// messages) and the app's message ids.
pub(crate) struct LocaleBundle {
    pub(crate) bundle: Bundle,
    /// The app's messages and terms (not the shipped ones), by id.
    pub(crate) messages: BTreeMap<String, Definition>,
}

/// The result of a lookup.
pub(crate) enum Lookup {
    /// The formatted message, and what went wrong while formatting it.
    Found { text: String, errors: Vec<String> },
    /// No locale in the chain defines it.
    Missing,
}

/// Every locale's bundle.
pub(crate) struct Catalog {
    /// `i18n.default_locale`, canonical.
    pub(crate) default: String,
    /// `i18n.locales`, canonical, in order.
    pub(crate) supported: Vec<String>,
    bundles: HashMap<String, LocaleBundle>,
    /// Problems found while loading (syntax errors, duplicates); loading
    /// fails on them unless it is lenient.
    pub(crate) problems: Vec<String>,
}

impl Catalog {
    /// Loads `files` for the locales of `options`. Any problem fails the
    /// load unless `lenient` (then broken entries are skipped and the
    /// problems kept in [`problems`](Catalog::problems)).
    pub(crate) fn build(
        options: &I18nOptions,
        files: Vec<SourceFile>,
        lenient: bool,
    ) -> Result<Catalog, Vec<String>> {
        let supported = options
            .locales
            .iter()
            .map(|locale| tag::canonical(locale))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| vec![format!("i18n.locales: {error}")])?;
        let default = tag::canonical(&options.default_locale)
            .map_err(|error| vec![format!("i18n.default_locale: {error}")])?;
        let mut problems = Vec::new();

        // Files by the canonical tag of their directory.
        let mut by_locale: BTreeMap<String, Vec<SourceFile>> = BTreeMap::new();
        for file in files {
            match tag::canonical(&file.locale) {
                Ok(canonical) => by_locale.entry(canonical).or_default().push(file),
                Err(_) => problems.push(format!(
                    "{}: `{}` is not a locale directory (a BCP 47 tag such as `fr` or `pt-BR`)",
                    file.path, file.locale
                )),
            }
        }

        // The supported locales, and their parents that have files.
        let mut needed: Vec<LanguageIdentifier> = Vec::new();
        for locale in supported.iter().chain(std::iter::once(&default)) {
            let langid = tag::parse(locale).map_err(|error| vec![error])?;
            for parent in tag::parents(&langid) {
                if by_locale.contains_key(&parent) {
                    needed.push(tag::parse(&parent).map_err(|error| vec![error])?);
                }
            }
            needed.push(langid);
        }
        let mut bundles = HashMap::new();
        for langid in needed {
            let key = langid.to_string();
            if bundles.contains_key(&key) {
                continue;
            }
            let files = by_locale.remove(&key).unwrap_or_default();
            let bundle = locale_bundle(options, langid, &files, &mut problems);
            bundles.insert(key, bundle);
        }
        if !problems.is_empty() && !lenient {
            return Err(problems);
        }
        Ok(Catalog {
            default,
            supported,
            bundles,
            problems,
        })
    }

    /// The supported locale `requested` stands for: the same tag, else its
    /// nearest supported parent (`fr-CA` → `fr`), else the first supported
    /// locale of the same language (`pt` → `pt-BR`, `fr-CA` → `fr-FR`).
    pub(crate) fn match_tag(&self, requested: &str) -> Option<&str> {
        let langid = tag::parse(requested).ok()?;
        let canonical = langid.to_string();
        if let Some(found) = self.supported.iter().find(|tag| **tag == canonical) {
            return Some(found);
        }
        for parent in tag::parents(&langid) {
            if let Some(found) = self.supported.iter().find(|tag| **tag == parent) {
                return Some(found);
            }
        }
        self.supported
            .iter()
            .find(|tag| {
                tag::parse(tag).is_ok_and(|supported| supported.language == langid.language)
            })
            .map(String::as_str)
    }

    /// The bundles a lookup in `locale` tries, in order: the locale, its
    /// parents, then the default locale and its parents.
    pub(crate) fn chain(&self, locale: &str) -> Vec<&LocaleBundle> {
        let mut chain: Vec<&LocaleBundle> = Vec::new();
        let mut push = |tag: &str| {
            if let Some(bundle) = self.bundles.get(tag)
                && !chain.iter().any(|seen| std::ptr::eq(*seen, bundle))
            {
                chain.push(bundle);
            }
        };
        for tag in own_chain(locale)
            .into_iter()
            .chain(own_chain(&self.default))
        {
            push(&tag);
        }
        chain
    }

    /// `locale`'s chain split in two: the locale and its parents, then the
    /// default locale's bundles not already in the first part.
    pub(crate) fn split_chain(&self, locale: &str) -> (Vec<&LocaleBundle>, Vec<&LocaleBundle>) {
        let own: Vec<&LocaleBundle> = own_chain(locale)
            .iter()
            .filter_map(|tag| self.bundles.get(tag))
            .collect();
        let fallback = own_chain(&self.default)
            .iter()
            .filter_map(|tag| self.bundles.get(tag))
            .filter(|bundle| !own.iter().any(|seen| std::ptr::eq(*seen, *bundle)))
            .collect();
        (own, fallback)
    }

    /// `id` (or `id.attribute`) formatted in the first bundle of `locale`'s
    /// chain that defines it.
    pub(crate) fn format(&self, locale: &str, id: &str, args: Option<&FluentArgs<'_>>) -> Lookup {
        for bundle in self.chain(locale) {
            if let Some(found) = format_in(bundle, id, args) {
                return found;
            }
        }
        Lookup::Missing
    }

    /// Whether `locale` or a parent of it (not the default locale, unless
    /// it is one of them) defines `id`.
    pub(crate) fn has(&self, locale: &str, id: &str) -> bool {
        let (message, attribute) = split_id(id);
        own_chain(locale)
            .iter()
            .filter_map(|tag| self.bundles.get(tag))
            .any(|bundle| {
                bundle
                    .bundle
                    .get_message(message)
                    .is_some_and(|found| match attribute {
                        Some(attribute) => found.get_attribute(attribute).is_some(),
                        None => found.value().is_some(),
                    })
            })
    }

    /// The message ids of the default locale's files that `locale` and its
    /// parents do not define, sorted, with where the default defines them.
    pub(crate) fn missing(&self, locale: &str) -> Vec<(String, Definition)> {
        let Some(default) = self.bundles.get(&self.default) else {
            return Vec::new();
        };
        if own_chain(&self.default).iter().any(|tag| tag == locale) {
            return Vec::new();
        }
        let own: Vec<&LocaleBundle> = own_chain(locale)
            .iter()
            .filter(|tag| **tag != self.default)
            .filter_map(|tag| self.bundles.get(tag))
            .collect();
        default
            .messages
            .iter()
            .filter(|(id, _)| !id.starts_with('-'))
            .filter(|(id, _)| !own.iter().any(|bundle| bundle.messages.contains_key(*id)))
            .map(|(id, definition)| (id.clone(), definition.clone()))
            .collect()
    }

    /// The number of messages (not terms) the default locale's files define.
    pub(crate) fn message_count(&self) -> usize {
        self.bundles.get(&self.default).map_or(0, |bundle| {
            bundle
                .messages
                .keys()
                .filter(|id| !id.starts_with('-'))
                .count()
        })
    }

    /// The `i18n:check` report: every loading problem, the messages missing
    /// from each non-regional locale, a summary for regional ones, and a
    /// total. `dir` is the directory as shown (`locales`).
    pub(crate) fn report(&self, dir: &str) -> Report {
        let mut lines: Vec<String> = self.problems.clone();
        let mut problems = self.problems.len();
        let total = self.message_count();
        for locale in &self.supported {
            if *locale == self.default {
                continue;
            }
            let missing = self.missing(locale);
            if missing.is_empty() {
                continue;
            }
            let langid = tag::parse(locale).ok();
            if langid.as_ref().is_some_and(tag::is_regional) {
                let fallback = own_chain(locale)
                    .into_iter()
                    .skip(1)
                    .find(|tag| self.bundles.contains_key(tag))
                    .unwrap_or_else(|| self.default.clone());
                lines.push(format!(
                    "{dir}/{locale}  {} of {total} messages missing (falls back to {fallback})",
                    missing.len()
                ));
            } else {
                for (id, definition) in missing {
                    lines.push(format!(
                        "{dir}/{locale}  {id}: missing (defined in {}:{})",
                        definition.path, definition.line
                    ));
                    problems += 1;
                }
            }
        }
        let locales = self.supported.len();
        lines.push(if problems == 0 {
            format!("No problems in {locales} locale(s)")
        } else {
            format!("{problems} problem(s) in {locales} locale(s)")
        });
        Report { lines, problems }
    }
}

/// What `i18n:check` prints.
#[derive(Debug)]
pub(crate) struct Report {
    pub(crate) lines: Vec<String>,
    pub(crate) problems: usize,
}

/// `locale` and its parents, most specific first.
fn own_chain(locale: &str) -> Vec<String> {
    match tag::parse(locale) {
        Ok(langid) => {
            let mut chain = vec![langid.to_string()];
            chain.extend(tag::parents(&langid));
            chain
        }
        Err(_) => vec![locale.to_owned()],
    }
}

/// `id` split at its first `.` into a message and an attribute.
fn split_id(id: &str) -> (&str, Option<&str>) {
    match id.split_once('.') {
        Some((message, attribute)) => (message, Some(attribute)),
        None => (id, None),
    }
}

/// `id` formatted with `bundle`, if the bundle has it.
pub(crate) fn format_in(
    bundle: &LocaleBundle,
    id: &str,
    args: Option<&FluentArgs<'_>>,
) -> Option<Lookup> {
    let (message, attribute) = split_id(id);
    let found = bundle.bundle.get_message(message)?;
    let pattern = match attribute {
        Some(attribute) => found.get_attribute(attribute)?.value(),
        None => found.value()?,
    };
    let mut errors = Vec::new();
    let text = bundle
        .bundle
        .format_pattern(pattern, args, &mut errors)
        .into_owned();
    Some(Lookup::Found {
        text,
        errors: errors.iter().map(ToString::to_string).collect(),
    })
}

/// The bundle of `langid`: the app's `files`, then the shipped validation
/// messages for its language. Problems are added to `problems`.
fn locale_bundle(
    options: &I18nOptions,
    langid: LanguageIdentifier,
    files: &[SourceFile],
    problems: &mut Vec<String>,
) -> LocaleBundle {
    let mut bundle: Bundle = FluentBundle::new_concurrent(vec![langid.clone()]);
    bundle.set_use_isolating(match options.bidi_isolation {
        BidiIsolation::Auto => tag::is_rtl(&langid),
        BidiIsolation::Always => true,
        BidiIsolation::Never => false,
    });
    bundle.set_formatter(Some(format_value));
    // Neither can clash in a new bundle.
    let _ = bundle.add_builtins();
    let _ = bundle.add_function("DATETIME", datetime);

    let mut messages: BTreeMap<String, Definition> = BTreeMap::new();
    for file in files {
        let resource = match FluentResource::try_new(file.source.clone()) {
            Ok(resource) => resource,
            Err((resource, errors)) => {
                for error in errors {
                    let at = error
                        .slice
                        .as_ref()
                        .map_or(error.pos.start, |slice| slice.start);
                    problems.push(format!(
                        "{}:{}: syntax error: {}",
                        file.path,
                        line_of(&file.source, at),
                        error.kind
                    ));
                }
                resource
            }
        };
        let lines = entry_lines(&file.source);
        for id in entry_ids(&resource) {
            let definition = Definition {
                path: file.path.clone(),
                line: lines.get(&id).copied().unwrap_or(1),
            };
            match messages.get(&id) {
                Some(first) => problems.push(format!(
                    "{}:{}: `{id}` is already defined in {}:{}",
                    definition.path, definition.line, first.path, first.line
                )),
                None => {
                    messages.insert(id, definition);
                }
            }
        }
        // Duplicates are reported above; the first definition is kept.
        let _ = bundle.add_resource(Arc::new(resource));
    }
    if let Some(source) = shipped::validation_for(&langid)
        && let Ok(resource) = FluentResource::try_new(source.to_owned())
    {
        // The app's own messages win over the shipped ones.
        let _ = bundle.add_resource(Arc::new(resource));
    }
    LocaleBundle { bundle, messages }
}

/// The ids of the messages (`id`) and terms (`-id`) of `resource`.
fn entry_ids(resource: &FluentResource) -> Vec<String> {
    resource
        .entries()
        .filter_map(|entry| match entry {
            ast::Entry::Message(message) => Some(message.id.name.to_owned()),
            ast::Entry::Term(term) => Some(format!("-{}", term.id.name)),
            _ => None,
        })
        .collect()
}

/// The 1-based line of byte offset `at` in `source`.
fn line_of(source: &str, at: usize) -> usize {
    source[..at.min(source.len())].matches('\n').count() + 1
}

/// The line where each entry of `source` starts: an identifier at the
/// start of a line, followed by `=`.
fn entry_lines(source: &str) -> HashMap<String, usize> {
    let mut lines = HashMap::new();
    for (index, line) in source.lines().enumerate() {
        let id_len = line
            .char_indices()
            .take_while(|(position, c)| {
                c.is_ascii_alphabetic()
                    || (*position > 0 && (c.is_ascii_digit() || *c == '_' || *c == '-'))
                    || (*position == 0 && *c == '-')
            })
            .count();
        if id_len == 0 {
            continue;
        }
        let (id, rest) = line.split_at(id_len);
        if rest.trim_start().starts_with('=') {
            lines.entry(id.to_owned()).or_insert(index + 1);
        }
    }
    lines
}

/// The Fluent files under `dir`: every `.ftl` file in each locale
/// directory, at any depth, sorted (hidden entries skipped). A missing
/// directory has no files.
pub(crate) fn read_dir(dir: &Path) -> Result<Vec<SourceFile>, Vec<String>> {
    let shown = dir.to_string_lossy().replace('\\', "/");
    let shown = shown.trim_end_matches('/');
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(vec![format!("i18n.dir `{shown}`: {error}")]),
    };
    let mut locales: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            (!name.starts_with('.')).then(|| (name, entry.path()))
        })
        .collect();
    locales.sort();
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for (locale, path) in locales {
        let mut found = Vec::new();
        ftl_files(&path, &mut found);
        found.sort();
        for file in found {
            let relative = file
                .strip_prefix(&path)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            let shown_path = format!("{shown}/{locale}/{relative}");
            match fs::read_to_string(&file) {
                Ok(source) => files.push(SourceFile {
                    locale: locale.clone(),
                    path: shown_path,
                    source,
                }),
                Err(error) => errors.push(format!("{shown_path}: cannot read the file: {error}")),
            }
        }
    }
    if errors.is_empty() {
        Ok(files)
    } else {
        Err(errors)
    }
}

/// Every `.ftl` file under `dir`, recursively.
fn ftl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            ftl_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "ftl") {
            out.push(path);
        }
    }
}

/// The files of `options`: the embedded ones, else those under
/// `options.dir`.
pub(crate) fn load_files(options: &I18nOptions) -> Result<Vec<SourceFile>, Vec<String>> {
    match options.files.embedded() {
        Some(embedded) => Ok(embedded
            .iter()
            .map(|(locale, path, source)| SourceFile {
                locale: (*locale).to_owned(),
                path: (*path).to_owned(),
                source: (*source).to_owned(),
            })
            .collect()),
        None => read_dir(&options.dir),
    }
}

/// A fingerprint of the files under `dir` (paths, sizes and modification
/// times), to notice changes.
pub(crate) fn fingerprint(dir: &Path) -> Vec<(PathBuf, u64, Option<std::time::SystemTime>)> {
    let mut found = Vec::new();
    ftl_files(dir, &mut found);
    found.sort();
    found
        .into_iter()
        .map(|path| {
            let metadata = fs::metadata(&path).ok();
            let size = metadata.as_ref().map_or(0, fs::Metadata::len);
            let modified = metadata.and_then(|metadata| metadata.modified().ok());
            (path, size, modified)
        })
        .collect()
}

/// The ids `files` define, per locale directory (for tests and tools).
#[cfg(test)]
pub(crate) fn ids(files: &[SourceFile]) -> std::collections::BTreeSet<String> {
    files
        .iter()
        .filter_map(|file| FluentResource::try_new(file.source.clone()).ok())
        .flat_map(|resource| entry_ids(&resource))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(locale: &str, name: &str, source: &str) -> SourceFile {
        SourceFile {
            locale: locale.to_owned(),
            path: format!("locales/{locale}/{name}"),
            source: source.to_owned(),
        }
    }

    fn options(locales: &[&str], default: &str) -> I18nOptions {
        I18nOptions {
            locales: locales.iter().map(|tag| (*tag).to_owned()).collect(),
            default_locale: default.to_owned(),
            ..I18nOptions::default()
        }
    }

    fn text(lookup: Lookup) -> String {
        match lookup {
            Lookup::Found { text, errors } => {
                assert!(errors.is_empty(), "{errors:?}");
                text
            }
            Lookup::Missing => panic!("missing"),
        }
    }

    fn catalog() -> Catalog {
        Catalog::build(
            &options(&["en", "fr", "fr-CA", "pt-BR"], "en"),
            vec![
                file("en", "main.ftl", "hello = Hello, { $name }!\nonly-en = English only\nlogin = Log in\n    .title = Sign in to your account\n"),
                file("fr", "main.ftl", "hello = Bonjour, { $name } !\n"),
                file("fr-CA", "main.ftl", "hello = Allô, { $name } !\n"),
                file("pt-BR", "main.ftl", "hello = Olá, { $name }!\n"),
            ],
            false,
        )
        .unwrap()
    }

    fn args(name: &str) -> FluentArgs<'static> {
        let mut args = FluentArgs::new();
        args.set("name", name.to_owned());
        args
    }

    #[test]
    fn lookups_fall_back_through_the_chain() {
        let catalog = catalog();
        assert_eq!(
            text(catalog.format("fr", "hello", Some(&args("Ada")))),
            "Bonjour, Ada !"
        );
        assert_eq!(
            text(catalog.format("fr-CA", "hello", Some(&args("Ada")))),
            "Allô, Ada !"
        );
        assert_eq!(
            text(catalog.format("fr-CA", "only-en", None)),
            "English only"
        );
        assert_eq!(
            text(catalog.format("fr", "login.title", None)),
            "Sign in to your account"
        );
        assert!(matches!(
            catalog.format("fr", "nope", None),
            Lookup::Missing
        ));
        assert!(matches!(
            catalog.format("en", "login.nope", None),
            Lookup::Missing
        ));
    }

    #[test]
    fn has_ignores_the_default_fallback() {
        let catalog = catalog();
        assert!(catalog.has("fr", "hello"));
        assert!(catalog.has("fr-CA", "hello"));
        assert!(!catalog.has("fr", "only-en"));
        assert!(catalog.has("en", "only-en"));
        assert!(catalog.has("en", "login.title"));
        // The shipped validation messages count.
        assert!(catalog.has("fr", "validation-required"));
    }

    #[test]
    fn tags_match_leniently() {
        let catalog = catalog();
        assert_eq!(catalog.match_tag("fr"), Some("fr"));
        assert_eq!(catalog.match_tag("FR-ca"), Some("fr-CA"));
        assert_eq!(catalog.match_tag("fr-BE"), Some("fr"));
        assert_eq!(catalog.match_tag("pt"), Some("pt-BR"));
        assert_eq!(catalog.match_tag("pt-PT"), Some("pt-BR"));
        assert_eq!(catalog.match_tag("en-GB"), Some("en"));
        assert_eq!(catalog.match_tag("de"), None);
        assert_eq!(catalog.match_tag("not a tag"), None);
    }

    #[test]
    fn plurals_follow_the_language() {
        let source = "files = { $count ->\n    [one] One file\n    [few] A few files\n   *[other] { $count } files\n}\n";
        let polish = "files = { $count ->\n    [one] Jeden plik\n    [few] { $count } pliki\n    [many] { $count } plików\n   *[other] { $count } pliku\n}\n";
        let catalog = Catalog::build(
            &options(&["en", "pl"], "en"),
            vec![
                file("en", "main.ftl", source),
                file("pl", "main.ftl", polish),
            ],
            false,
        )
        .unwrap();
        let count = |n: i64| {
            let mut args = FluentArgs::new();
            args.set("count", n);
            args
        };
        assert_eq!(
            text(catalog.format("en", "files", Some(&count(1)))),
            "One file"
        );
        assert_eq!(
            text(catalog.format("en", "files", Some(&count(3)))),
            "3 files"
        );
        assert_eq!(
            text(catalog.format("en", "files", Some(&count(1000)))),
            "1,000 files"
        );
        assert_eq!(
            text(catalog.format("pl", "files", Some(&count(3)))),
            "3 pliki"
        );
        assert_eq!(
            text(catalog.format("pl", "files", Some(&count(5)))),
            "5 plików"
        );
    }

    #[test]
    fn syntax_errors_and_duplicates_name_the_file_and_line() {
        let errors = Catalog::build(
            &options(&["en"], "en"),
            vec![
                file("en", "a.ftl", "ok = Fine\n\nbroken = { $count\n"),
                file("en", "b.ftl", "\nok = Again\n"),
            ],
            false,
        )
        .err()
        .unwrap();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(
            errors[0].starts_with("locales/en/a.ftl:3: syntax error"),
            "{errors:?}"
        );
        assert_eq!(
            errors[1],
            "locales/en/b.ftl:2: `ok` is already defined in locales/en/a.ftl:1"
        );
    }

    #[test]
    fn lenient_loading_keeps_the_good_entries() {
        let catalog = Catalog::build(
            &options(&["en"], "en"),
            vec![file("en", "a.ftl", "ok = Fine\n\nbroken = { $count\n")],
            true,
        )
        .unwrap();
        assert_eq!(catalog.problems.len(), 1);
        assert_eq!(text(catalog.format("en", "ok", None)), "Fine");
    }

    #[test]
    fn the_report_lists_missing_messages() {
        let catalog = Catalog::build(
            &options(&["en", "fr", "pt", "pt-BR"], "en"),
            vec![
                file(
                    "en",
                    "deals.ftl",
                    "deal-won = Won\n-brand = Acme\ndeal-lost = Lost\n",
                ),
                file("fr", "deals.ftl", "deal-won = Gagné\n"),
                file("pt", "deals.ftl", "deal-won = Ganho\ndeal-lost = Perdido\n"),
                file("pt-BR", "deals.ftl", "deal-won = Ganhou\n"),
            ],
            false,
        )
        .unwrap();
        assert_eq!(catalog.missing("fr").len(), 1);
        assert!(catalog.missing("pt-BR").is_empty(), "pt covers pt-BR");
        let report = catalog.report("locales");
        assert_eq!(
            report.lines,
            [
                "locales/fr  deal-lost: missing (defined in locales/en/deals.ftl:3)",
                "1 problem(s) in 4 locale(s)",
            ]
        );
        assert_eq!(report.problems, 1);
    }

    #[test]
    fn regional_locales_only_get_a_summary() {
        let catalog = Catalog::build(
            &options(&["en", "pt-BR"], "en"),
            vec![
                file("en", "deals.ftl", "a = A\nb = B\n"),
                file("pt-BR", "deals.ftl", "a = A\n"),
            ],
            false,
        )
        .unwrap();
        let report = catalog.report("locales");
        assert_eq!(
            report.lines,
            [
                "locales/pt-BR  1 of 2 messages missing (falls back to en)",
                "No problems in 2 locale(s)",
            ]
        );
    }

    #[test]
    fn bidi_isolation_is_automatic_for_right_to_left_locales() {
        let files = || {
            vec![
                file("en", "main.ftl", "hello = Hello { $name }\n"),
                file("ar", "main.ftl", "hello = مرحبا { $name }\n"),
            ]
        };
        let catalog = Catalog::build(&options(&["en", "ar"], "en"), files(), false).unwrap();
        assert_eq!(
            text(catalog.format("en", "hello", Some(&args("Ada")))),
            "Hello Ada"
        );
        assert_eq!(
            text(catalog.format("ar", "hello", Some(&args("Ada")))),
            "مرحبا \u{2068}Ada\u{2069}"
        );
        let always = I18nOptions {
            bidi_isolation: BidiIsolation::Always,
            ..options(&["en", "ar"], "en")
        };
        let catalog = Catalog::build(&always, files(), false).unwrap();
        assert_eq!(
            text(catalog.format("en", "hello", Some(&args("Ada")))),
            "Hello \u{2068}Ada\u{2069}"
        );
    }

    #[test]
    fn the_directory_is_read_recursively() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let write = |relative: &str, source: &str| {
            let path = dir.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        };
        write("en/main.ftl", "a = A\n");
        write("en/mail/welcome.ftl", "b = B\n");
        write("en/notes.txt", "not fluent");
        write("fr/main.ftl", "a = A\n");
        let files = read_dir(dir.path()).unwrap();
        let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths.len(), 3);
        assert!(paths[0].ends_with("/en/mail/welcome.ftl"), "{paths:?}");
        assert_eq!(
            ids(&files),
            std::collections::BTreeSet::from(["a".to_owned(), "b".to_owned()])
        );
        assert!(read_dir(&dir.path().join("missing")).unwrap().is_empty());
        assert_eq!(fingerprint(dir.path()).len(), 3);
    }
}
