//! The configuration reference page: every section of an app's registry, its
//! keys, types, defaults, environment variables and documentation, written
//! from the sections themselves so it cannot drift from the code.

use std::fmt::Write as _;

use serde_json::Value as Json;

use crate::registry::ConfigRegistry;
use crate::schema::{FieldSchema, FieldType};

const INTRO: &str = r#"---
title: "Configuration reference"
description: "Every configuration section of the framework with its keys, types, defaults, environment variables and what each key does."
---

Every module of the framework reads one section of the configuration. This page lists them all: each key with its type, its default, the environment variable that sets it and what it does. It is written from the sections' own types and doc comments, so it cannot drift from the code.

A key is written under its section in `config/app.toml`, or in a file of its own, `config/<section>.toml`, without the section header. The environment variable of a key is `APP_`, the section, `__` and the key, in capitals; a dot becomes `__`, so `mail.smtp.port` is `APP_MAIL__SMTP__PORT`. A credential shows `secret` instead of a default: set it in `.env` or in the environment, not in a file that is committed. [How the sources combine](/docs/getting-started/configuration/) and [environment files](/docs/configuration/environments-and-files/) explain the rest.

`navree config:show --defaults` prints what an app uses, `navree config:publish <section>` writes a section's keys into a file of its own, and `navree config:schema` writes a JSON Schema for your editor.
"#;

/// The page for the sections of `registry`: front matter, an introduction,
/// and a table of keys for each section that describes itself, in key order.
/// The text is MDX: braces, angle brackets and pipes in a description are
/// escaped.
pub fn reference_markdown(registry: &ConfigRegistry) -> String {
    let mut out = String::from(INTRO);
    let defaults = registry.defaults_tree();
    for section in registry.sections() {
        let Some(schema) = &section.schema else {
            continue;
        };
        let _ = write!(out, "\n## `{}`\n\n", schema.key);
        if !schema.doc.is_empty() {
            let _ = writeln!(out, "{}\n", description(&schema.doc));
        }
        if let Some(docs) = &schema.docs {
            let _ = writeln!(out, "More: [{docs}]({docs}).\n");
        }
        out.push_str("| Key | Type | Default | Environment variable | Description |\n");
        out.push_str("|---|---|---|---|---|\n");
        let mut rows = Vec::new();
        collect(&schema.fields, "", &mut rows);
        for (relative, field) in rows {
            let full = format!("{}.{relative}", schema.key);
            let _ = writeln!(
                out,
                "| `{relative}` | {ty} | {default} | `{env}` | {doc} |",
                ty = escape(&field.ty.to_string()),
                default = default_text(field, &lookup(&defaults, &full)),
                env = env_name(&full),
                doc = field_description(field),
            );
        }
    }
    out
}

/// Every field of `fields` and of the tables in them, with its key below the
/// section (`smtp.port`).
fn collect<'a>(fields: &'a [FieldSchema], prefix: &str, rows: &mut Vec<(String, &'a FieldSchema)>) {
    for field in fields {
        let path = format!("{prefix}{}", field.name);
        rows.push((path.clone(), field));
        collect(&field.fields, &format!("{path}."), rows);
    }
}

/// The value at the dotted `path` in `tree`.
fn lookup(tree: &Json, path: &str) -> Option<Json> {
    let mut node = tree;
    for part in path.split('.') {
        node = node.get(part)?;
    }
    Some(node.clone())
}

/// `APP_MAIL__API_KEY` for `mail.api_key`.
fn env_name(key: &str) -> String {
    format!("APP_{}", key.replace('.', "__").to_ascii_uppercase())
}

fn default_text(field: &FieldSchema, default: &Option<Json>) -> String {
    if field.secret {
        return "secret".to_owned();
    }
    if field.ty == FieldType::Table {
        return String::new();
    }
    match default {
        None | Some(Json::Null) => String::new(),
        Some(json) => format!("`{}`", escape_code(&json.to_string())),
    }
}

fn field_description(field: &FieldSchema) -> String {
    let mut text = description(&field.doc);
    if field.required {
        text = format!("Required. {text}").trim_end().to_owned();
    }
    if let Some(variable) = &field.env_fallback {
        text = format!("{text} Falls back to `{variable}`.")
            .trim_start()
            .to_owned();
    }
    text
}

/// A description for a table cell or a paragraph: rustdoc links lose their
/// target (`[`Foo`](crate::Foo)` is `Foo`), and what MDX would read as markup
/// is escaped.
fn description(doc: &str) -> String {
    escape(&strip_links(doc))
}

/// Replaces `[text](target)` by `text` when the target is not a web address
/// or a site path (a rustdoc link).
fn strip_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(close) = rest.find("](") {
        let Some(open) = rest[..close].rfind('[') else {
            out.push_str(&rest[..close + 2]);
            rest = &rest[close + 2..];
            continue;
        };
        let after = &rest[close + 2..];
        let Some(end) = after.find(')') else {
            break;
        };
        let target = &after[..end];
        if target.starts_with("http") || target.starts_with('/') || target.starts_with('#') {
            out.push_str(&rest[..close + 2 + end + 1]);
        } else {
            out.push_str(&rest[..open]);
            out.push_str(&rest[open + 1..close]);
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Escapes text for an MDX table cell: outside code spans, braces and angle
/// brackets; everywhere, the pipe.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    for c in text.chars() {
        match c {
            '`' => {
                in_code = !in_code;
                out.push(c);
            }
            '|' => out.push_str("\\|"),
            '\n' => out.push(' '),
            '{' | '}' if !in_code => {
                out.push('\\');
                out.push(c);
            }
            '<' if !in_code => out.push_str("&lt;"),
            '>' if !in_code => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// The text of a code span: a pipe is escaped, a backtick would end the span.
fn escape_code(text: &str) -> String {
    text.replace('|', "\\|")
        .replace('`', "'")
        .replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::registry::SectionRegistration;
    use crate::{ConfigSection, SectionSchema};

    struct Fake;

    impl<'de> serde::Deserialize<'de> for Fake {
        fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
            Ok(Fake)
        }
    }

    impl ConfigSection for Fake {
        const KEY: &'static str = "mail";

        fn defaults() -> Option<Json> {
            Some(json!({ "from": "hello@example.test", "retries": 3, "smtp": { "port": 25 } }))
        }

        fn schema() -> Option<SectionSchema> {
            Some(
                SectionSchema::new(
                    "mail",
                    "Mail settings for `Vec<String>` and {braces}.",
                    vec![
                        FieldSchema::new(
                            "from",
                            FieldType::String,
                            "The sender | one of [`Foo`](crate::Foo) or [the guide](/docs/x/).",
                        )
                        .required(),
                        FieldSchema::new("retries", FieldType::Integer, "Attempts <n>."),
                        FieldSchema::new("api_key", FieldType::String, "Sends mail.").secret(),
                        FieldSchema::new("url", FieldType::String, "").env_fallback("MAIL_URL"),
                        FieldSchema::new("smtp", FieldType::Table, "The server.").with_fields(
                            vec![FieldSchema::new("port", FieldType::Integer, "The port.")],
                        ),
                    ],
                )
                .with_docs("/docs/digging-deeper/mail/"),
            )
        }
    }

    #[test]
    fn the_page_has_front_matter_and_a_table_per_section() {
        let registry = ConfigRegistry::new(&[SectionRegistration::of::<Fake>()]);
        let page = reference_markdown(&registry);
        assert!(page.starts_with("---\ntitle: \"Configuration reference\"\n"));
        assert!(page.contains("\n## `mail`\n"), "{page}");
        assert!(page.contains("More: [/docs/digging-deeper/mail/](/docs/digging-deeper/mail/)."));
        assert!(page.contains("| Key | Type | Default | Environment variable | Description |"));
        for row in [
            "| `from` | string | `\"hello@example.test\"` | `APP_MAIL__FROM` | Required. The sender \\| one of `Foo` or [the guide](/docs/x/). |",
            "| `retries` | integer | `3` | `APP_MAIL__RETRIES` | Attempts &lt;n&gt;. |",
            "| `api_key` | string | secret | `APP_MAIL__API_KEY` | Sends mail. |",
            "| `url` | string |  | `APP_MAIL__URL` | Falls back to `MAIL_URL`. |",
            "| `smtp` | table |  | `APP_MAIL__SMTP` | The server. |",
            "| `smtp.port` | integer | `25` | `APP_MAIL__SMTP__PORT` | The port. |",
        ] {
            assert!(page.contains(row), "missing {row}\n{page}");
        }
    }

    #[test]
    fn mdx_markup_in_a_description_is_escaped_but_code_is_not() {
        assert_eq!(
            description("Use `Vec<String>` or {id} and <b> | x"),
            "Use `Vec<String>` or \\{id\\} and &lt;b&gt; \\| x"
        );
        assert_eq!(
            strip_links("See [`Foo`](crate::Foo), [docs](https://x.test) and [a][b]."),
            "See `Foo`, [docs](https://x.test) and [a][b]."
        );
        assert_eq!(escape_code("a|b`c"), "a\\|b'c");
    }

    #[test]
    fn a_section_that_does_not_describe_itself_is_left_out() {
        struct ByHand;
        impl<'de> serde::Deserialize<'de> for ByHand {
            fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
                Ok(ByHand)
            }
        }
        impl ConfigSection for ByHand {
            // A key the framework reads, so the list of framework sections
            // stays true.
            const KEY: &'static str = "mail";
        }
        let registry = ConfigRegistry::new(&[SectionRegistration::of::<ByHand>()]);
        assert!(!reference_markdown(&registry).contains("## `mail`"));
    }
}
