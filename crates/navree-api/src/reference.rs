//! The API reference page: the document as plain HTML, every operation and
//! schema on one page. Everything from the document is escaped. Without
//! JavaScript the page is complete to read; the script adds search and the
//! "Try it" form, which it builds from the `data-` attributes written here.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;

use navree_openapi::{Document, Operation, Schema, SchemaType, Subschema, Type};
use serde_json::{Value, json};

use crate::docs::callable_servers;
use crate::markdown;
use crate::markdown::escape_into;

/// How deep a schema's inline parts are written, and how many references an
/// example request body follows.
const MAX_SCHEMA_DEPTH: usize = 12;
const MAX_EXAMPLE_DEPTH: usize = 6;

/// The URLs the page links to.
pub(crate) struct Links<'a> {
    pub(crate) json: &'a str,
    pub(crate) stylesheet: &'a str,
    pub(crate) script: &'a str,
}

/// The whole page.
pub(crate) fn render(document: &Document, links: &Links<'_>) -> String {
    let mut ids = Anchors::default();
    let groups = operation_groups(document, &mut ids);
    let mut page = String::with_capacity(64 * 1024);
    let out = &mut page;

    let title = &document.info.title;
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<meta name=\"color-scheme\" content=\"light dark\">\n<title>");
    escape_into(title, out);
    out.push_str("</title>\n<link rel=\"stylesheet\" href=\"");
    escape_into(links.stylesheet, out);
    out.push_str("\">\n<script src=\"");
    escape_into(links.script, out);
    out.push_str("\" defer></script>\n</head>\n<body data-servers=\"");
    let urls: Vec<String> = document.servers.iter().map(|s| s.url.clone()).collect();
    let callable: Vec<&str> = callable_servers(&urls).map(String::as_str).collect();
    escape_into(&callable.join(" "), out);
    out.push_str("\">\n");
    out.push_str("<a class=\"skip\" href=\"#content\">Skip to the reference</a>\n");

    header(document, links, out);
    out.push_str("<div class=\"layout\">\n");
    navigation(document, &groups, out);
    out.push_str("<main id=\"content\" tabindex=\"-1\">\n");
    introduction(document, out);
    for group in &groups {
        out.push_str("<section class=\"group\" aria-labelledby=\"");
        escape_into(&group.id, out);
        out.push_str("\">\n<h2 id=\"");
        escape_into(&group.id, out);
        out.push_str("\">");
        escape_into(&group.name, out);
        out.push_str("</h2>\n");
        for operation in &group.operations {
            write_operation(document, operation, out);
        }
        out.push_str("</section>\n");
    }
    schemas_section(document, out);
    out.push_str("</main>\n</div>\n</body>\n</html>\n");
    page
}

fn header(document: &Document, links: &Links<'_>, out: &mut String) {
    out.push_str("<header class=\"bar\">\n<a class=\"title\" href=\"#content\">");
    escape_into(&document.info.title, out);
    out.push_str("</a>\n<span class=\"version\">");
    escape_into(&document.info.version, out);
    out.push_str("</span>\n<form class=\"search\" role=\"search\" hidden>");
    out.push_str("<label for=\"search\">Search</label>");
    out.push_str("<input id=\"search\" type=\"search\" autocomplete=\"off\" spellcheck=\"false\" placeholder=\"Search (press /)\" aria-describedby=\"search-count\">");
    out.push_str("<output id=\"search-count\" for=\"search\"></output></form>\n");
    out.push_str("<button id=\"menu\" class=\"menu\" type=\"button\" aria-expanded=\"false\" aria-controls=\"nav\" hidden>Contents</button>\n");
    out.push_str("<a class=\"raw\" href=\"");
    escape_into(links.json, out);
    out.push_str("\">openapi.json</a>\n</header>\n");
}

fn navigation(document: &Document, groups: &[Group<'_>], out: &mut String) {
    out.push_str("<nav id=\"nav\" aria-label=\"Contents\">\n");
    for group in groups {
        out.push_str("<details open><summary>");
        escape_into(&group.name, out);
        let _ = write!(
            out,
            " <span class=\"count\">{}</span></summary>\n<ul>\n",
            group.operations.len()
        );
        for operation in &group.operations {
            out.push_str("<li><a href=\"#");
            escape_into(&operation.id, out);
            out.push_str("\" data-target=\"");
            escape_into(&operation.id, out);
            out.push_str("\"><span class=\"method ");
            out.push_str(method_class(operation.method));
            out.push_str("\">");
            escape_into(&operation.method.to_ascii_uppercase(), out);
            out.push_str("</span><span class=\"label\">");
            escape_into(
                operation
                    .operation
                    .summary
                    .as_deref()
                    .unwrap_or(operation.path),
                out,
            );
            out.push_str("</span></a></li>\n");
        }
        out.push_str("</ul>\n</details>\n");
    }
    if !document.components.schemas.is_empty() {
        let _ = write!(
            out,
            "<details><summary>Schemas <span class=\"count\">{}</span></summary>\n<ul>\n",
            document.components.schemas.len()
        );
        for name in document.components.schemas.keys() {
            let id = schema_id(name);
            out.push_str("<li><a href=\"#");
            escape_into(&id, out);
            out.push_str("\" data-target=\"");
            escape_into(&id, out);
            out.push_str("\"><span class=\"label\">");
            escape_into(name, out);
            out.push_str("</span></a></li>\n");
        }
        out.push_str("</ul>\n</details>\n");
    }
    out.push_str("</nav>\n");
}

fn introduction(document: &Document, out: &mut String) {
    out.push_str("<section class=\"intro\" id=\"overview\">\n<h1>");
    escape_into(&document.info.title, out);
    out.push_str("</h1>\n");
    let servers: Vec<&str> = document.servers.iter().map(|s| s.url.as_str()).collect();
    if !servers.is_empty() {
        out.push_str("<h2>Servers</h2>\n<ul class=\"servers\">\n");
        for url in &servers {
            out.push_str("<li><code>");
            escape_into(url, out);
            out.push_str("</code></li>\n");
        }
        out.push_str("</ul>\n");
    }
    if !document.components.security_schemes.is_empty() {
        out.push_str("<h2>Authentication</h2>\n<dl class=\"schemes\">\n");
        for (name, scheme) in &document.components.security_schemes {
            out.push_str("<dt><code>");
            escape_into(name, out);
            out.push_str("</code></dt>\n<dd>");
            if let Some(description) = &scheme.description {
                out.push_str(&markdown::render(description));
            }
            out.push_str("</dd>\n");
        }
        out.push_str("</dl>\n");
    }
    out.push_str("</section>\n");
}

/// The tag's operations, or for the group of untagged ones the name `Other`.
struct Group<'a> {
    name: String,
    id: String,
    operations: Vec<PageOperation<'a>>,
}

struct PageOperation<'a> {
    id: String,
    method: &'a str,
    path: &'a str,
    operation: &'a Operation,
}

/// Every operation in one group: its first tag's, in the order of the
/// document's tags. Untagged operations come last.
fn operation_groups<'a>(document: &'a Document, ids: &mut Anchors) -> Vec<Group<'a>> {
    let mut groups: Vec<Group<'a>> = document
        .tags
        .iter()
        .map(|tag| Group {
            name: tag.name.clone(),
            id: ids.unique("tag", &tag.name),
            operations: Vec::new(),
        })
        .collect();
    let mut other = Vec::new();
    for (path, item) in &document.paths {
        for (method, operation) in item {
            let id = match &operation.operation_id {
                Some(name) => ids.unique("op", name),
                None => ids.unique("op", &format!("{method}-{path}")),
            };
            let page = PageOperation {
                id,
                method,
                path,
                operation,
            };
            let group = operation
                .tags
                .first()
                .and_then(|tag| groups.iter_mut().find(|group| group.name == *tag));
            match group {
                Some(group) => group.operations.push(page),
                None => other.push(page),
            }
        }
    }
    if !other.is_empty() {
        groups.push(Group {
            name: "Other".to_owned(),
            id: ids.unique("tag", "other"),
            operations: other,
        });
    }
    groups.retain(|group| !group.operations.is_empty());
    groups
}

fn write_operation(document: &Document, page: &PageOperation<'_>, out: &mut String) {
    let operation = page.operation;
    let summary = operation.summary.as_deref().unwrap_or("");
    let search = format!(
        "{} {} {} {} {}",
        page.method,
        page.path,
        summary,
        operation.operation_id.as_deref().unwrap_or(""),
        operation.tags.join(" "),
    )
    .to_lowercase();
    let params: Vec<&str> = operation
        .parameters
        .iter()
        .map(|p| p.name.as_str())
        .collect();

    out.push_str("<article class=\"operation\" id=\"");
    escape_into(&page.id, out);
    out.push_str("\" data-search=\"");
    escape_into(&search, out);
    out.push_str("\" data-method=\"");
    escape_into(&page.method.to_ascii_uppercase(), out);
    out.push_str("\" data-path=\"");
    escape_into(page.path, out);
    out.push_str("\" data-params=\"");
    escape_into(&json!(params).to_string(), out);
    if !operation.security.is_empty() {
        out.push_str("\" data-secured=\"true");
    }
    if let Some(body) = &operation.request_body
        && let Some((_, media)) = body.content.first()
    {
        let mut visiting = HashSet::new();
        let example = example_value(&media.schema, document, 0, &mut visiting);
        out.push_str("\" data-body=\"");
        escape_into(
            &serde_json::to_string_pretty(&example).unwrap_or_default(),
            out,
        );
    }
    out.push_str("\">\n<h3>");
    out.push_str("<a class=\"anchor\" href=\"#");
    escape_into(&page.id, out);
    out.push_str("\"><span class=\"method ");
    out.push_str(method_class(page.method));
    out.push_str("\">");
    escape_into(&page.method.to_ascii_uppercase(), out);
    out.push_str("</span> <code class=\"path\">");
    escape_into(page.path, out);
    out.push_str("</code></a></h3>\n");
    if !summary.is_empty() {
        out.push_str("<p class=\"summary\">");
        escape_into(summary, out);
        out.push_str("</p>\n");
    }
    if let Some(description) = &operation.description {
        out.push_str("<div class=\"prose\">");
        out.push_str(&markdown::render(description));
        out.push_str("</div>\n");
    }
    operation_facts(page, out);
    if !operation.parameters.is_empty() {
        out.push_str("<h4>Parameters</h4>\n<ul class=\"props\">\n");
        for parameter in &operation.parameters {
            out.push_str("<li><code class=\"name\">");
            escape_into(&parameter.name, out);
            out.push_str("</code> ");
            type_expression(&parameter.schema, out);
            out.push_str(" <span class=\"note\">in ");
            escape_into(&parameter.location, out);
            out.push_str("</span>");
            if parameter.required {
                out.push_str(" <span class=\"required\">required</span>");
            }
            out.push_str("</li>\n");
        }
        out.push_str("</ul>\n");
    }
    if let Some(body) = &operation.request_body {
        out.push_str("<h4>Request body");
        if body.required {
            out.push_str(" <span class=\"required\">required</span>");
        }
        out.push_str("</h4>\n");
        for (content_type, media) in &body.content {
            out.push_str("<p class=\"media\"><code>");
            escape_into(content_type, out);
            out.push_str("</code> ");
            type_expression(&media.schema, out);
            out.push_str("</p>\n");
            schema_details(&media.schema, 0, out);
        }
    }
    out.push_str("<h4>Responses</h4>\n<ul class=\"responses\">\n");
    for (status, response) in &operation.responses {
        out.push_str("<li><span class=\"status\">");
        escape_into(status, out);
        out.push_str("</span> ");
        escape_into(&response.description, out);
        for (content_type, media) in &response.content {
            out.push_str("<p class=\"media\"><code>");
            escape_into(content_type, out);
            out.push_str("</code> ");
            type_expression(&media.schema, out);
            out.push_str("</p>\n");
            schema_details(&media.schema, 0, out);
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n</article>\n");
}

fn operation_facts(page: &PageOperation<'_>, out: &mut String) {
    let operation = page.operation;
    out.push_str("<dl class=\"facts\">\n");
    if let Some(id) = &operation.operation_id {
        out.push_str("<dt>Operation</dt><dd><code>");
        escape_into(id, out);
        out.push_str("</code></dd>\n");
    }
    out.push_str("<dt>Access</dt><dd>");
    if operation.security.is_empty() {
        out.push_str("Public");
    } else {
        let schemes: Vec<&str> = operation
            .security
            .iter()
            .flat_map(|requirement| requirement.keys().map(String::as_str))
            .collect();
        out.push_str("Needs ");
        escape_into(&schemes.join(" or "), out);
    }
    out.push_str("</dd>\n");
    if operation.tags.len() > 1 {
        out.push_str("<dt>Tags</dt><dd>");
        escape_into(&operation.tags.join(", "), out);
        out.push_str("</dd>\n");
    }
    for (name, value) in &operation.extensions {
        out.push_str("<dt>");
        escape_into(name, out);
        out.push_str("</dt><dd><code>");
        escape_into(&value.to_string(), out);
        out.push_str("</code></dd>\n");
    }
    out.push_str("</dl>\n");
}

fn schemas_section(document: &Document, out: &mut String) {
    if document.components.schemas.is_empty() {
        return;
    }
    out.push_str(
        "<section class=\"group\" aria-labelledby=\"schemas\">\n<h2 id=\"schemas\">Schemas</h2>\n",
    );
    for (name, schema) in &document.components.schemas {
        let id = schema_id(name);
        out.push_str("<article class=\"schema\" id=\"");
        escape_into(&id, out);
        out.push_str("\" data-search=\"schema ");
        escape_into(&name.to_lowercase(), out);
        out.push_str("\">\n<h3><a class=\"anchor\" href=\"#");
        escape_into(&id, out);
        out.push_str("\"><code class=\"path\">");
        escape_into(name, out);
        out.push_str("</code></a></h3>\n<p class=\"media\">");
        type_expression(schema, out);
        out.push_str("</p>\n");
        if let Some(description) = &schema.description {
            out.push_str("<div class=\"prose\">");
            out.push_str(&markdown::render(description));
            out.push_str("</div>\n");
        }
        schema_details(schema, 0, out);
        out.push_str("</article>\n");
    }
    out.push_str("</section>\n");
}

/// A schema's inline form, with links to the components it names.
fn type_expression(schema: &Schema, out: &mut String) {
    out.push_str("<span class=\"type\">");
    write_type(schema, 0, out);
    out.push_str("</span>");
}

fn write_type(schema: &Schema, depth: usize, out: &mut String) {
    if depth > MAX_SCHEMA_DEPTH {
        out.push_str("...");
        return;
    }
    if let Some(reference) = &schema.reference {
        let name = reference
            .strip_prefix("#/components/schemas/")
            .unwrap_or(reference);
        out.push_str("<a href=\"#");
        escape_into(&schema_id(name), out);
        out.push_str("\">");
        escape_into(name, out);
        out.push_str("</a>");
        return;
    }
    if let [only] = schema.all_of.as_slice() {
        write_type(only, depth + 1, out);
        return;
    }
    if !schema.one_of.is_empty() {
        write_union(&schema.one_of, depth, out);
        return;
    }
    if !schema.any_of.is_empty() {
        write_union(&schema.any_of, depth, out);
        return;
    }
    if !schema.all_of.is_empty() {
        out.push_str("all of");
        return;
    }
    let write_one = |ty: Type, out: &mut String| match ty {
        Type::Array => match &schema.items {
            Some(Subschema::Schema(items)) => {
                write_type(items, depth + 1, out);
                out.push_str("[]");
            }
            _ => out.push_str("array"),
        },
        Type::Object => match (&schema.additional_properties, &schema.property_names) {
            (Some(Subschema::Schema(values)), Some(keys)) if schema.properties.is_empty() => {
                out.push_str("map&lt;");
                write_type(keys, depth + 1, out);
                out.push_str(", ");
                write_type(values, depth + 1, out);
                out.push_str("&gt;");
            }
            _ => out.push_str("object"),
        },
        ty => {
            out.push_str(ty.as_str());
            if let Some(format) = &schema.format {
                out.push_str(" (");
                escape_into(format, out);
                out.push(')');
            }
        }
    };
    match &schema.schema_type {
        Some(SchemaType::One(ty)) => write_one(*ty, out),
        Some(SchemaType::Many(types)) => {
            for (index, ty) in types.iter().enumerate() {
                if index > 0 {
                    out.push_str(" | ");
                }
                write_one(*ty, out);
            }
        }
        None => out.push_str("any"),
    }
}

/// `A | B | null`, when every member fits on a line, else `one of`.
fn write_union(items: &[Schema], depth: usize, out: &mut String) {
    let simple = items
        .iter()
        .all(|item| item.reference.is_some() || item.properties.is_empty());
    if !simple {
        out.push_str("one of");
        return;
    }
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(" | ");
        }
        write_type(item, depth + 1, out);
    }
}

/// What a schema says beyond its type: properties, members of a composition,
/// allowed values, limits, the default and examples.
fn schema_details(schema: &Schema, depth: usize, out: &mut String) {
    if depth > MAX_SCHEMA_DEPTH {
        return;
    }
    let mut facts = String::new();
    if !schema.enum_values.is_empty() {
        facts.push_str("<li>One of ");
        for (index, value) in schema.enum_values.iter().enumerate() {
            if index > 0 {
                facts.push_str(", ");
            }
            json_code(value, &mut facts);
        }
        facts.push_str("</li>");
    }
    let mut fact = |label: &str, value: &Value| {
        facts.push_str("<li>");
        facts.push_str(label);
        facts.push(' ');
        json_code(value, &mut facts);
        facts.push_str("</li>");
    };
    if let Some(value) = &schema.default {
        fact("Default", value);
    }
    if let Some(value) = &schema.minimum {
        fact("At least", value);
    }
    if let Some(value) = &schema.maximum {
        fact("At most", value);
    }
    if let Some(value) = &schema.const_value {
        fact("Always", value);
    }
    if let Some(value) = &schema.example {
        fact("Example", value);
    }
    for value in &schema.examples {
        fact("Example", value);
    }
    if !facts.is_empty() {
        out.push_str("<ul class=\"limits\">");
        out.push_str(&facts);
        out.push_str("</ul>\n");
    }

    if !schema.properties.is_empty() {
        out.push_str("<ul class=\"props\">\n");
        for (name, property) in &schema.properties {
            out.push_str("<li><code class=\"name\">");
            escape_into(name, out);
            out.push_str("</code> ");
            type_expression(property, out);
            if schema.required.contains(name) {
                out.push_str(" <span class=\"required\">required</span>");
            }
            let description = property.description.as_deref().or_else(|| {
                property
                    .one_of
                    .first()
                    .and_then(|first| first.description.as_deref())
            });
            if let Some(description) = description {
                out.push_str("<div class=\"prose\">");
                out.push_str(&markdown::render(description));
                out.push_str("</div>");
            }
            schema_details(property, depth + 1, out);
            out.push_str("</li>\n");
        }
        out.push_str("</ul>\n");
    }
    if matches!(schema.additional_properties, Some(Subschema::Bool(false))) {
        out.push_str("<p class=\"note\">No other properties.</p>\n");
    }
    if let Some(Subschema::Schema(items)) = &schema.items
        && items.reference.is_none()
        && has_details(items)
    {
        schema_details(items, depth + 1, out);
    }
    if let Some(Subschema::Schema(values)) = &schema.additional_properties
        && values.reference.is_none()
        && has_details(values)
    {
        schema_details(values, depth + 1, out);
    }
    for (label, members) in [
        ("One of", &schema.one_of),
        ("Any of", &schema.any_of),
        ("All of", &schema.all_of),
    ] {
        let parts: Vec<&Schema> = members.iter().filter(|m| has_details(m)).collect();
        if parts.is_empty() {
            continue;
        }
        let _ = write!(
            out,
            "<p class=\"note\">{label}:</p>\n<ol class=\"members\">\n"
        );
        for member in members {
            out.push_str("<li>");
            type_expression(member, out);
            if let Some(description) = &member.description {
                out.push_str("<div class=\"prose\">");
                out.push_str(&markdown::render(description));
                out.push_str("</div>");
            }
            schema_details(member, depth + 1, out);
            out.push_str("</li>\n");
        }
        out.push_str("</ol>\n");
    }
}

/// Whether [`schema_details`] writes something for `schema`.
fn has_details(schema: &Schema) -> bool {
    !schema.properties.is_empty()
        || !schema.one_of.is_empty()
        || !schema.any_of.is_empty()
        || !schema.all_of.is_empty()
        || !schema.enum_values.is_empty()
        || schema.default.is_some()
        || schema.example.is_some()
        || !schema.examples.is_empty()
        || schema.minimum.is_some()
        || schema.maximum.is_some()
}

fn json_code(value: &Value, out: &mut String) {
    out.push_str("<code>");
    escape_into(&value.to_string(), out);
    out.push_str("</code>");
}

/// A value of `schema` to start a request body from: its example, or the
/// required properties (every property when none is required) with values of
/// their types. A reference to a schema it's already inside is `{}`.
fn example_value(
    schema: &Schema,
    document: &Document,
    depth: usize,
    visiting: &mut HashSet<String>,
) -> Value {
    if let Some(example) = schema.example.as_ref().or(schema.examples.first()) {
        return example.clone();
    }
    if let Some(default) = &schema.default {
        return default.clone();
    }
    if depth > MAX_EXAMPLE_DEPTH {
        return json!({});
    }
    if let Some(reference) = &schema.reference {
        let name = reference
            .strip_prefix("#/components/schemas/")
            .unwrap_or(reference);
        let Some(target) = document.components.schemas.get(name) else {
            return json!({});
        };
        if !visiting.insert(name.to_owned()) {
            return json!({});
        }
        let value = example_value(target, document, depth + 1, visiting);
        visiting.remove(name);
        return value;
    }
    if let Some(value) = schema.enum_values.first() {
        return value.clone();
    }
    for member in schema.one_of.iter().chain(&schema.any_of) {
        if member.schema_type != Some(SchemaType::One(Type::Null)) {
            return example_value(member, document, depth + 1, visiting);
        }
    }
    if !schema.all_of.is_empty() {
        let mut merged = serde_json::Map::new();
        for member in &schema.all_of {
            if let Value::Object(object) = example_value(member, document, depth + 1, visiting) {
                merged.extend(object);
            }
        }
        return Value::Object(merged);
    }
    let ty = match &schema.schema_type {
        Some(SchemaType::One(ty)) => Some(*ty),
        Some(SchemaType::Many(types)) => types.iter().find(|ty| **ty != Type::Null).copied(),
        None => None,
    };
    match ty {
        Some(Type::Object) => {
            let mut object = serde_json::Map::new();
            for (name, property) in &schema.properties {
                if schema.required.is_empty() || schema.required.contains(name) {
                    object.insert(
                        name.clone(),
                        example_value(property, document, depth + 1, visiting),
                    );
                }
            }
            Value::Object(object)
        }
        Some(Type::Array) => match &schema.items {
            Some(Subschema::Schema(items)) => {
                json!([example_value(items, document, depth + 1, visiting)])
            }
            _ => json!([]),
        },
        Some(Type::String) => match schema.format.as_deref() {
            Some("uuid") => json!("00000000-0000-0000-0000-000000000000"),
            Some("date-time") => json!("2026-01-31T12:00:00Z"),
            Some("date") => json!("2026-01-31"),
            _ => json!("string"),
        },
        Some(Type::Integer | Type::Number) => schema.minimum.clone().unwrap_or(json!(0)),
        Some(Type::Boolean) => json!(false),
        Some(Type::Null) | None => Value::Null,
    }
}

fn method_class(method: &str) -> &'static str {
    match method {
        "get" => "get",
        "post" => "post",
        "put" => "put",
        "patch" => "patch",
        "delete" => "delete",
        _ => "other",
    }
}

fn schema_id(name: &str) -> String {
    format!("schema-{}", slug(name))
}

/// `text` with everything but letters, digits, `_`, `-` and `.` as `-`, so it
/// is a fragment without percent-encoding.
fn slug(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

/// Ids in use on the page.
#[derive(Default)]
struct Anchors(BTreeMap<String, usize>);

impl Anchors {
    fn unique(&mut self, kind: &str, name: &str) -> String {
        let base = format!("{kind}-{}", slug(name));
        let seen = self.0.entry(base.clone()).or_insert(0);
        *seen += 1;
        if *seen == 1 {
            base
        } else {
            format!("{base}-{seen}")
        }
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use navree_openapi::{
        Components, Info, MediaType, Parameter, RequestBody, Response, SecurityScheme, Tag,
    };

    use super::*;

    fn links() -> Links<'static> {
        Links {
            json: "/api/openapi.json",
            stylesheet: "/api/docs/assets/reference.css",
            script: "/api/docs/assets/reference.js",
        }
    }

    fn document() -> Document {
        let deal = Schema::object()
            .property("id", Schema::string().format("uuid"), true)
            .property("title", Schema::string().description("The *title*."), true)
            .property("stage", Schema::reference_to("Stage").nullable(), false)
            .description("A deal.");
        let stage = Schema::string_enum(["lead", "won"]);
        let mut show = Operation {
            tags: vec!["Deals".to_owned()],
            summary: Some("Show a deal".to_owned()),
            description: Some("Returns **one** deal.".to_owned()),
            operation_id: Some("deals.show".to_owned()),
            parameters: vec![Parameter {
                name: "id".to_owned(),
                location: "path".to_owned(),
                required: true,
                schema: Schema::string(),
            }],
            security: vec![BTreeMap::from([("bearer".to_owned(), Vec::new())])],
            ..Operation::default()
        };
        show.responses.insert(
            "200".to_owned(),
            Response {
                description: "OK".to_owned(),
                content: IndexMap::from([(
                    "application/json".to_owned(),
                    MediaType {
                        schema: Schema::reference_to("Deal"),
                    },
                )]),
            },
        );
        let mut create = Operation {
            summary: Some("Create".to_owned()),
            request_body: Some(RequestBody {
                required: true,
                content: IndexMap::from([(
                    "application/json".to_owned(),
                    MediaType {
                        schema: Schema::reference_to("Deal"),
                    },
                )]),
            }),
            ..Operation::default()
        };
        create.responses.insert(
            "201".to_owned(),
            Response {
                description: "Created".to_owned(),
                content: IndexMap::new(),
            },
        );
        Document {
            openapi: "3.1.0".to_owned(),
            info: Info {
                title: "CRM <API>".to_owned(),
                version: "1.0.0".to_owned(),
            },
            servers: Vec::new(),
            tags: vec![Tag {
                name: "Deals".to_owned(),
            }],
            paths: IndexMap::from([
                (
                    "/deals/{id}".to_owned(),
                    IndexMap::from([("get".to_owned(), show)]),
                ),
                (
                    "/deals".to_owned(),
                    IndexMap::from([("post".to_owned(), create)]),
                ),
            ]),
            components: Components {
                schemas: BTreeMap::from([("Deal".to_owned(), deal), ("Stage".to_owned(), stage)]),
                security_schemes: IndexMap::from([(
                    "bearer".to_owned(),
                    SecurityScheme {
                        kind: "http".to_owned(),
                        location: None,
                        name: None,
                        scheme: Some("bearer".to_owned()),
                        bearer_format: None,
                        description: Some("A token.".to_owned()),
                    },
                )]),
            },
        }
    }

    #[test]
    fn every_operation_and_schema_is_on_the_page() {
        let html = render(&document(), &links());
        assert!(html.contains("id=\"op-deals.show\""), "{html}");
        assert!(html.contains("data-path=\"/deals/{id}\""));
        assert!(html.contains("id=\"schema-Deal\""));
        assert!(html.contains("id=\"schema-Stage\""));
        assert!(html.contains("<a href=\"#schema-Deal\">Deal</a>"));
        assert!(html.contains("<a href=\"#schema-Stage\">Stage</a> | null"));
        assert!(html.contains("One of <code>&quot;lead&quot;</code>"));
    }

    #[test]
    fn an_untagged_operation_is_under_other() {
        let html = render(&document(), &links());
        assert!(html.contains(">Other</h2>"), "{html}");
        assert!(html.contains("id=\"op-post--deals\""), "{html}");
    }

    #[test]
    fn text_from_the_document_is_escaped() {
        let mut document = document();
        document.info.title = "<script>alert(1)</script>".to_owned();
        if let Some(operation) = document
            .paths
            .get_mut("/deals/{id}")
            .and_then(|item| item.get_mut("get"))
        {
            operation.summary = Some("\"><img src=x onerror=y>".to_owned());
            operation.description = Some("<style>*{}</style>".to_owned());
            operation.tags.push("<b>".to_owned());
        }
        let html = render(&document, &links());
        assert!(!html.contains("<script>alert"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(!html.contains("<style"), "{html}");
        assert!(!html.contains("<b>"), "{html}");
    }

    #[test]
    fn the_page_has_no_inline_script_style_or_third_party_url() {
        let html = render(&document(), &links());
        assert!(!html.contains(" style="), "{html}");
        assert!(!html.contains("onclick"), "{html}");
        assert!(
            !html.contains("http://") && !html.contains("https://"),
            "{html}"
        );
        assert_eq!(html.matches("<script").count(), 1);
        assert!(html.contains("<script src=\"/api/docs/assets/reference.js\" defer></script>"));
    }

    #[test]
    fn a_request_body_starts_from_its_required_properties() {
        let html = render(&document(), &links());
        let body = html
            .split("data-body=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_default()
            .replace("&quot;", "\"");
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            value,
            json!({"id": "00000000-0000-0000-0000-000000000000", "title": "string"})
        );
    }

    #[test]
    fn a_schema_containing_itself_ends() {
        let mut document = document();
        document.components.schemas.insert(
            "Tree".to_owned(),
            Schema::object().property("child", Schema::reference_to("Tree"), true),
        );
        let mut visiting = HashSet::new();
        let value = example_value(&Schema::reference_to("Tree"), &document, 0, &mut visiting);
        assert_eq!(value, json!({"child": {}}));
    }

    #[test]
    fn two_operations_with_the_same_id_get_two_anchors() {
        let mut ids = Anchors::default();
        assert_eq!(ids.unique("op", "a b"), "op-a-b");
        assert_eq!(ids.unique("op", "a-b"), "op-a-b-2");
    }
}
