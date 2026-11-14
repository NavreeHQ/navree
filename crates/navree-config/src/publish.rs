//! `config:publish <section>`: a file for one section, with every key
//! commented out beside its default and its documentation, ready to edit.
//! `config:publish --plugin <namespace>`: such a file for every section a
//! plugin registers, written through the checked writer `plugin:publish`
//! uses.

use std::fmt::Write as _;
use std::path::Path;

use navree_kernel::__private::{Published, publish_files};
use serde_json::Value as Json;

use crate::registry::ConfigRegistry;
use crate::schema::{FieldSchema, FieldType, SectionSchema};

const USAGE: &str = "usage: config:publish <section> [--force] [--stdout] | config:publish --plugin <namespace> [--force]";

/// What `config:publish` was asked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PublishOptions {
    /// The section (`mail`, `database.mongodb`).
    pub section: String,
    /// Replace `config/<section>.toml` when it exists.
    pub force: bool,
    /// Print the file instead of writing it.
    pub stdout: bool,
    /// The namespace of the plugin whose sections to publish
    /// (`--plugin announcements`): every section its modules register.
    pub plugin: Option<String>,
}

impl PublishOptions {
    /// Reads the command line after `config:publish`: a section, or
    /// `--plugin <namespace>`.
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = PublishOptions::default();
        let mut words = args.iter();
        while let Some(arg) = words.next() {
            match arg.as_str() {
                "--force" => options.force = true,
                "--stdout" => options.stdout = true,
                "--plugin" => {
                    let Some(namespace) = words.next().filter(|word| !word.starts_with('-')) else {
                        return Err(format!("--plugin needs a namespace; {USAGE}"));
                    };
                    if options.plugin.is_some() {
                        return Err(format!("only one plugin can be published; {USAGE}"));
                    }
                    options.plugin = Some(namespace.clone());
                }
                option if option.starts_with('-') => {
                    return Err(format!("unknown option `{option}`; {USAGE}"));
                }
                section => {
                    if !options.section.is_empty() {
                        return Err(format!("only one section can be published; {USAGE}"));
                    }
                    options.section = section.to_owned();
                }
            }
        }
        if options.plugin.is_some() {
            if !options.section.is_empty() {
                return Err(format!("give a section or --plugin, not both; {USAGE}"));
            }
            if options.stdout {
                return Err(format!("--stdout prints one section; {USAGE}"));
            }
        } else if options.section.is_empty() {
            return Err(format!("name the section to publish; {USAGE}"));
        }
        Ok(options)
    }
}

/// Publishes the section `options.section` of `registry` into `dir`
/// (`dir/<section>.toml`), or returns the text when `options.stdout` is set.
/// The result is what the command prints. A file that exists is not
/// replaced without `force`.
///
/// With `options.plugin` set, `registry` holds the sections of that plugin
/// only, and each of them that describes its keys is published as
/// `<dir name>/<key>.toml` below `dir`'s parent, all through the checked
/// writer of `plugin:publish`: every path is checked before the first write,
/// an existing file is kept without `force` and the others are still written,
/// and each file is written beside its place and renamed over it. The result
/// has one line per file, in path order (`wrote config/<key>.toml`, or
/// `config/<key>.toml: exists, kept (use --force to replace it)`), then one
/// per section that does not describe its keys.
pub fn run(
    options: &PublishOptions,
    registry: &ConfigRegistry,
    dir: &Path,
) -> Result<String, String> {
    if let Some(namespace) = &options.plugin {
        return run_plugin(namespace, options.force, registry, dir);
    }
    let section = registry.section(&options.section).ok_or_else(|| {
        format!(
            "no section `{}` is registered; `config:show --defaults` lists the sections of the app",
            options.section
        )
    })?;
    let schema = section.schema.as_ref().ok_or_else(|| {
        format!(
            "section `{}` does not describe its keys, so there is nothing to publish",
            options.section
        )
    })?;
    let text = render(schema, section.defaults.as_ref());
    if options.stdout {
        return Ok(text);
    }
    let path = dir.join(format!("{}.toml", options.section));
    if path.exists() && !options.force {
        return Err(format!(
            "{} already exists; --force replaces it",
            path.display()
        ));
    }
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&path, &text))
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(format!("wrote {}\n", path.display()))
}

/// [`run`] with `--plugin <namespace>`: every section of `registry` (the
/// plugin's) that describes its keys, written through the checked writer.
fn run_plugin(
    namespace: &str,
    force: bool,
    registry: &ConfigRegistry,
    dir: &Path,
) -> Result<String, String> {
    if registry.sections().is_empty() {
        return Ok(format!(
            "config: plugin `{namespace}` has nothing to publish\n"
        ));
    }
    // The writer is given `<dir name>/<key>.toml` below `dir`'s parent, so
    // it checks the folder `dir` itself as well as the files in it.
    let Some(folder) = dir.file_name().and_then(|name| name.to_str()) else {
        return Err(format!(
            "config:publish: {} does not end in a folder name",
            dir.display()
        ));
    };
    let root = match dir.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let mut files: Vec<(String, String)> = Vec::new();
    let mut left_out = Vec::new();
    for section in registry.sections() {
        let path = format!("{folder}/{}.toml", section.key);
        match &section.schema {
            Some(schema) => files.push((path, render(schema, section.defaults.as_ref()))),
            None => left_out.push(format!(
                "{path}: section `{}` does not describe its keys, so it is not published",
                section.key
            )),
        }
    }
    if let Some(path) = shared_file(&files) {
        return Err(format!(
            "config:publish: two sections of plugin `{namespace}` would be written to one file, {path}; nothing was written"
        ));
    }
    let listed: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_bytes()))
        .collect();
    let published = publish_files(root, &listed, force, false)
        .map_err(|refused| format!("config:publish: {refused}"))?;
    let mut out = String::new();
    for (path, outcome) in &published {
        let line = match outcome {
            Published::Written => format!("wrote {path}"),
            other => other.line(path),
        };
        let _ = writeln!(out, "{line}");
    }
    for line in &left_out {
        let _ = writeln!(out, "{line}");
    }
    Ok(out)
}

/// The first path of `files` that names the same file as an earlier one on a
/// disk that ignores case: one key read by two types, or two keys that differ
/// only in case (their lower-case forms or their upper-case forms are equal).
/// The writer takes distinct paths.
fn shared_file(files: &[(String, String)]) -> Option<&str> {
    let mut seen: Vec<(String, String)> = Vec::new();
    files.iter().map(|(path, _)| path.as_str()).find(|path| {
        let (lower, upper) = (path.to_lowercase(), path.to_uppercase());
        let shared = seen
            .iter()
            .any(|(seen_lower, seen_upper)| *seen_lower == lower || *seen_upper == upper);
        seen.push((lower, upper));
        shared
    })
}

/// The text of the file of `schema`: every key commented out, with its
/// default from `defaults` (the section's defaults, when it publishes any).
/// A credential has no value: the file says which environment variable sets
/// it.
pub fn render(schema: &SectionSchema, defaults: Option<&Json>) -> String {
    let mut out = String::new();
    if schema.doc.is_empty() {
        let _ = writeln!(out, "# Settings of the `{}` section.", schema.key);
    } else {
        let _ = writeln!(out, "# {}", schema.doc);
    }
    if let Some(docs) = &schema.docs {
        let _ = writeln!(out, "# More: {docs}");
    }
    let _ = writeln!(
        out,
        "#\n# Every key is commented out beside its default. Remove the `#` to change one."
    );
    write_fields(&mut out, &schema.key, &schema.fields, defaults, "");
    out
}

/// Writes `fields`: plain keys, then each table under its own header.
/// `table` is the path of the table below the section (`smtp.tls`).
fn write_fields(
    out: &mut String,
    key: &str,
    fields: &[FieldSchema],
    defaults: Option<&Json>,
    table: &str,
) {
    for field in fields.iter().filter(|field| field.ty != FieldType::Table) {
        let default = defaults.and_then(|defaults| defaults.get(&field.name));
        let _ = writeln!(out);
        if !field.doc.is_empty() {
            let _ = writeln!(out, "# {}", field.doc);
        }
        if let Some(variable) = &field.env_fallback {
            let _ = writeln!(
                out,
                "# Without a value, `{variable}` in the environment is used."
            );
        }
        if field.secret {
            let _ = writeln!(
                out,
                "# A credential: set {} in the environment, not in a file.",
                env_name(&format!("{key}.{table}{}", field.name))
            );
            continue;
        }
        let required = if field.required { " (required)" } else { "" };
        let _ = writeln!(
            out,
            "# {name} = {value}{required}",
            name = field.name,
            value = toml_value(field, default)
        );
    }
    for field in fields.iter().filter(|field| field.ty == FieldType::Table) {
        let path = format!("{table}{}", field.name);
        let _ = writeln!(out);
        if !field.doc.is_empty() {
            let _ = writeln!(out, "# {}", field.doc);
        }
        let _ = writeln!(out, "# [{path}]");
        let nested = defaults.and_then(|defaults| defaults.get(&field.name));
        write_fields(out, key, &field.fields, nested, &format!("{path}."));
    }
}

/// `APP_MAIL__API_KEY` for `mail.api_key`.
fn env_name(key: &str) -> String {
    format!("APP_{}", key.replace('.', "__").to_ascii_uppercase())
}

/// The TOML for the default of `field`; a placeholder of the right type when
/// the section publishes none.
fn toml_value(field: &FieldSchema, default: Option<&Json>) -> String {
    match default {
        Some(Json::Null) | None => placeholder(&field.ty),
        Some(json) => json_to_toml(json),
    }
}

fn placeholder(ty: &FieldType) -> String {
    match ty {
        FieldType::Bool => "false".to_owned(),
        FieldType::Integer => "0".to_owned(),
        FieldType::Float => "0.0".to_owned(),
        FieldType::Choice(values) => {
            json_to_toml(&Json::String(values.first().cloned().unwrap_or_default()))
        }
        FieldType::List(_) => "[]".to_owned(),
        FieldType::Map(_) => "{}".to_owned(),
        FieldType::Duration => "\"30s\"".to_owned(),
        FieldType::String | FieldType::Any | FieldType::Table | FieldType::Other(_) => {
            "\"\"".to_owned()
        }
    }
}

fn json_to_toml(value: &Json) -> String {
    match value {
        Json::Null => "\"\"".to_owned(),
        Json::Bool(flag) => flag.to_string(),
        Json::Number(number) => number.to_string(),
        Json::String(text) => {
            let mut quoted = String::with_capacity(text.len() + 2);
            quoted.push('"');
            for c in text.chars() {
                match c {
                    '\\' => quoted.push_str("\\\\"),
                    '"' => quoted.push_str("\\\""),
                    '\n' => quoted.push_str("\\n"),
                    '\t' => quoted.push_str("\\t"),
                    c => quoted.push(c),
                }
            }
            quoted.push('"');
            quoted
        }
        Json::Array(items) => {
            let items: Vec<String> = items.iter().map(json_to_toml).collect();
            format!("[{}]", items.join(", "))
        }
        Json::Object(map) => {
            if map.is_empty() {
                return "{}".to_owned();
            }
            let pairs: Vec<String> = map
                .iter()
                .map(|(key, value)| format!("{key:?} = {}", json_to_toml(value)))
                .collect();
            format!("{{ {} }}", pairs.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn schema() -> SectionSchema {
        SectionSchema::new(
            "mail",
            "Mail settings.",
            vec![
                FieldSchema::new("from", FieldType::String, "The sender.").required(),
                FieldSchema::new(
                    "transport",
                    FieldType::choice(&["log", "smtp"]),
                    "How mail leaves.",
                ),
                FieldSchema::new("retries", FieldType::Integer, "Attempts."),
                FieldSchema::new("api_key", FieldType::String, "Sends mail.").secret(),
                FieldSchema::new("url", FieldType::String, "The server.").env_fallback("MAIL_URL"),
                FieldSchema::new("smtp", FieldType::Table, "The SMTP server.").with_fields(vec![
                    FieldSchema::new("port", FieldType::Integer, "The port."),
                    FieldSchema::new("password", FieldType::String, "").secret(),
                ]),
            ],
        )
        .with_docs("/docs/digging-deeper/mail/")
    }

    #[test]
    fn every_key_is_commented_with_its_default_and_doc() {
        let defaults = json!({
            "from": "hello@example.test",
            "transport": "log",
            "retries": 3,
            "api_key": null,
            "url": null,
            "smtp": { "port": 25, "password": null },
        });
        let text = render(&schema(), Some(&defaults));
        for expected in [
            "# Mail settings.\n",
            "# More: /docs/digging-deeper/mail/\n",
            "# The sender.\n# from = \"hello@example.test\" (required)\n",
            "# How mail leaves.\n# transport = \"log\"\n",
            "# Attempts.\n# retries = 3\n",
            "# A credential: set APP_MAIL__API_KEY in the environment, not in a file.\n",
            "# The server.\n# Without a value, `MAIL_URL` in the environment is used.\n# url = \"\"\n",
            "# The SMTP server.\n# [smtp]\n",
            "# The port.\n# port = 25\n",
            "# A credential: set APP_MAIL__SMTP__PASSWORD in the environment, not in a file.\n",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in\n{text}");
        }
        // Nothing is left uncommented: the file is valid TOML as it stands.
        assert!(
            text.lines()
                .all(|line| line.is_empty() || line.starts_with('#')),
            "{text}"
        );
        assert!(!text.contains("hunter"));
    }

    #[test]
    fn a_section_without_defaults_gets_placeholders_of_the_right_type() {
        let text = render(&schema(), None);
        assert!(text.contains("# from = \"\" (required)"), "{text}");
        assert!(text.contains("# transport = \"log\""), "{text}");
        assert!(text.contains("# retries = 0"), "{text}");
        let toml = json_to_toml(&json!({ "a": [1, "x\"y"], "b": true }));
        assert_eq!(toml, "{ \"a\" = [1, \"x\\\"y\"], \"b\" = true }");
    }

    #[test]
    fn the_command_line_is_read() {
        let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        let options = PublishOptions::parse(&args(&["mail", "--force"])).expect("valid");
        assert_eq!(options.section, "mail");
        assert!(options.force && !options.stdout);
        assert!(PublishOptions::parse(&args(&[])).is_err());
        assert!(PublishOptions::parse(&args(&["a", "b"])).is_err());
        assert!(PublishOptions::parse(&args(&["a", "--nope"])).is_err());
    }

    #[test]
    fn the_command_line_takes_a_plugin() {
        let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        let options =
            PublishOptions::parse(&args(&["--plugin", "announcements", "--force"])).expect("valid");
        assert_eq!(options.plugin.as_deref(), Some("announcements"));
        assert!(options.force && options.section.is_empty());
        assert!(
            PublishOptions::parse(&args(&["--plugin"]))
                .unwrap_err()
                .starts_with("--plugin needs a namespace; usage:")
        );
        assert!(
            PublishOptions::parse(&args(&["mail", "--plugin", "x"]))
                .unwrap_err()
                .starts_with("give a section or --plugin, not both; usage:")
        );
        assert!(
            PublishOptions::parse(&args(&["--plugin", "x", "--stdout"]))
                .unwrap_err()
                .starts_with("--stdout prints one section; usage:")
        );
    }
}
