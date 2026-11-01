//! From an OpenAPI document to the declarations of `schema.d.ts`: `paths`,
//! `webhooks`, `components`, `$defs` and `operations`.

use super::discriminators::{self, Discriminators, resolve_keys, value_at};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use super::json::{Object, Value, js_number};
use super::refs::{create_ref, escape_pointer, oapi_ref, parse_ref};
use super::ts::{
    BOOLEAN, Key, Member, NEVER, NUMBER, STRING, Statement, Ty, UNKNOWN, doc_comment, intersection,
    is_primitive, js_trim, literal, literal_str, nullable, property_key, record, union,
};

type Res<T> = Result<T, String>;

/// The most one document may ask the generator to write, counted in bytes of
/// TypeScript or so. A schema that lists several types is written once for
/// each, so what a document asks for can grow much faster than the document.
const MAX_WORK: usize = 32 * 1024 * 1024;

fn too_much_work() -> String {
    format!(
        "the document asks for more than {} MiB of TypeScript",
        MAX_WORK / (1024 * 1024)
    )
}

/// The methods of a path item, in the order they are written.
const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// What the declarations of a document are, and what was worth a warning.
pub(crate) struct Declarations {
    pub(crate) statements: Vec<Statement>,
    pub(crate) warnings: Vec<String>,
}

/// A declaration added after `components` as the document is read.
enum Footer {
    /// `export interface operations`, with the operations so far.
    Operations(Vec<Member>),
    /// The `WithRequired` helper type.
    WithRequired,
}

const WITH_REQUIRED: &str =
    "type WithRequired<T, K extends keyof T> = T & {\n    [P in K]-?: T[P];\n};";

/// The declarations for `doc`.
pub(crate) fn declarations(mut doc: Value) -> Res<Declarations> {
    let mut warnings = Vec::new();
    let discriminators = discriminators::scan(&mut doc, &mut warnings);
    let mut generator = Generator {
        doc: &doc,
        discriminators,
        footer: Vec::new(),
        warnings,
        work: Cell::new(0),
    };
    let mut statements = Vec::new();
    let no_sections = Value::object([]);
    for root in ["paths", "webhooks", "components", "$defs"] {
        let empty = Statement::Alias(root.to_owned(), record(STRING, NEVER));
        // `components` is always there: a document without it, or with it
        // empty of everything, still names every group.
        let section = match doc.get(root) {
            Some(section @ Value::Obj(_)) => Some(section),
            None | Some(Value::Null | Value::Arr(_)) if root == "components" => Some(&no_sections),
            _ => None,
        };
        let Some(section) = section else {
            statements.push(empty);
            continue;
        };
        let ty = match root {
            "paths" => generator.paths(section)?,
            "webhooks" => generator.webhooks(section)?,
            "components" => generator.components(section)?,
            _ => generator.schema(section, "#/$defs")?,
        };
        match ty {
            Ty::Literal(members) if !members.is_empty() => {
                statements.push(Statement::Interface(root.to_owned(), members));
            }
            _ => statements.push(empty),
        }
    }
    if generator.work.get() > MAX_WORK {
        return Err(too_much_work());
    }
    let mut has_operations = false;
    for item in generator.footer {
        match item {
            Footer::Operations(members) => {
                has_operations = true;
                statements.push(Statement::Interface("operations".to_owned(), members));
            }
            Footer::WithRequired => statements.push(Statement::Helper(WITH_REQUIRED)),
        }
    }
    if !has_operations {
        statements.push(Statement::Alias(
            "operations".to_owned(),
            record(STRING, NEVER),
        ));
    }
    Ok(Declarations {
        statements,
        warnings: generator.warnings,
    })
}

struct Generator<'d> {
    doc: &'d Value,
    discriminators: Discriminators,
    footer: Vec<Footer>,
    warnings: Vec<String>,
    /// What the declarations have cost so far, in bytes of text or so.
    work: Cell<usize>,
}

/// The entries of an object as JavaScript lists them (none for anything
/// else).
fn entries(value: Option<&Value>) -> Vec<(&str, &Value)> {
    value
        .and_then(Value::as_object)
        .map(Object::js_entries)
        .unwrap_or_default()
}

/// A string member, or an error naming what it should have been.
fn text<'a>(value: Option<&'a Value>, what: &str) -> Res<&'a str> {
    value
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{what} must be a string"))
}

fn truthy(value: Option<&Value>) -> bool {
    value.is_some_and(Value::truthy)
}

/// Refuses what is not an object, where the document must have one.
fn require_object(value: &Value, path: &str, name: &str) -> Res<()> {
    if matches!(value, Value::Obj(_)) {
        Ok(())
    } else {
        Err(format!("{name} must be an object at {path}"))
    }
}

/// A `parameters` list: absent or null is empty, anything but an array is
/// refused.
fn parameter_list(value: Option<&Value>, path: &str) -> Res<Vec<Value>> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Arr(items)) => Ok(items.to_vec()),
        Some(_) => Err(format!("parameters must be an array at {path}")),
    }
}

/// `String(value)` as a template literal prints it.
fn js_text(value: Option<&Value>) -> String {
    value.map_or_else(|| "undefined".to_owned(), Value::js_string)
}

impl Generator<'_> {
    // ----- references -------------------------------------------------

    /// What a reference leads to.
    fn resolve(&mut self, reference: &str) -> Option<Value> {
        let keys = resolve_keys(self.doc, reference, &mut Vec::new(), &mut self.warnings)?;
        value_at(self.doc, &keys).cloned()
    }

    // ----- comments ---------------------------------------------------

    /// The JSDoc comment for what `node` says about itself.
    fn comment(&self, node: &Value) -> Option<String> {
        let object = node.as_object()?;
        let mut lines: Vec<String> = Vec::new();
        for key in ["title", "summary"] {
            if let Some(Value::Str(text)) = object.get(key).filter(|v| v.truthy()) {
                lines.push(break_lines(js_trim(text)));
            }
        }
        if let Some(format) = object.get("format").filter(|v| v.truthy()) {
            lines.push(format!("Format: {}", format.js_string()));
        }
        if truthy(object.get("deprecated")) {
            lines.push("@deprecated".to_owned());
        }
        for field in ["description", "default", "example"] {
            let Some(value) = object.get(field) else {
                continue;
            };
            if field == "description" && value.as_str() == Some("") {
                continue;
            }
            lines.push(format!(
                "@{field} {}",
                break_lines(js_trim(&value.js_string()))
            ));
        }
        if let Some(examples) = object.get("examples").and_then(Value::as_array) {
            for example in examples {
                lines.push(format!(
                    "@example {}",
                    break_lines(js_trim(&example.js_string()))
                ));
            }
        }
        if object.contains("const") {
            lines.push("@constant".to_owned());
        }
        if truthy(object.get("enum")) {
            let kind = match object.get("type") {
                Some(Value::Arr(types)) => types
                    .iter()
                    .map(|t| {
                        if matches!(t, Value::Null) {
                            String::new()
                        } else {
                            t.js_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("|"),
                Some(Value::Str(kind)) => kind.to_string(),
                _ => "unknown".to_owned(),
            };
            let null = if truthy(object.get("nullable")) {
                "|null"
            } else {
                ""
            };
            lines.push(format!("@enum {{{kind}{null}}}"));
        }
        let comment = doc_comment(&lines);
        self.spend(comment.as_ref().map_or(0, String::len));
        comment
    }

    /// How a property is named, counted against the work.
    fn key(&self, name: &str) -> Key {
        self.spend(name.len() + 16);
        property_key(name)
    }

    /// Counts `units` against the work one document may ask for.
    fn spend(&self, units: usize) {
        self.work.set(self.work.get().saturating_add(units));
    }

    fn member(&self, key: &str, ty: Ty, node: &Value) -> Member {
        let mut member = Member::new(self.key(key), ty);
        member.comment = self.comment(node);
        member
    }

    // ----- paths ------------------------------------------------------

    fn paths(&mut self, paths: &Value) -> Res<Ty> {
        let mut members = Vec::new();
        for (url, item) in entries(Some(paths)) {
            if !matches!(item, Value::Obj(_) | Value::Arr(_)) {
                continue;
            }
            if item.has("$ref") {
                let ty = oapi_ref(text(item.get("$ref"), "$ref")?, None)?;
                members.push(self.member(url, ty, item));
            } else {
                let ty = self.path_item(item, &create_ref(&["paths", url]))?;
                members.push(Member::new(self.key(url), ty));
            }
        }
        Ok(Ty::Literal(members))
    }

    fn webhooks(&mut self, webhooks: &Value) -> Res<Ty> {
        let mut members = Vec::new();
        for (name, item) in entries(Some(webhooks)) {
            let ty = self.path_item(item, &create_ref(&["webhooks", name]))?;
            members.push(Member::new(self.key(name), ty));
        }
        Ok(Ty::Literal(members))
    }

    fn path_item(&mut self, item: &Value, path: &str) -> Res<Ty> {
        let shared = parameter_list(item.get("parameters"), path)?;
        let mut members = self.parameters(&shared, &create_ref(&[path, "parameters"]))?;
        for method in METHODS {
            let Some(operation) = item.get(method).filter(|op| op.truthy()) else {
                members.push(Member::new(Key::Ident(method.to_owned()), NEVER).optional(true));
                continue;
            };
            if !matches!(operation, Value::Obj(_)) {
                return Err(format!("{path}/{method} must be an object"));
            }
            let ty = if operation.has("$ref") {
                oapi_ref(text(operation.get("$ref"), "$ref")?, None)?
            } else {
                let operation_path = create_ref(&[path, method]);
                let folded = self.fold_parameters(&shared, operation, &operation_path)?;
                match operation.get("operationId").filter(|id| id.truthy()) {
                    Some(id) => {
                        let id = text(Some(id), "operationId")?.replace('#', "/");
                        let ty = oapi_ref(&create_ref(&["operations", &id]), None)?;
                        self.inject_operation(&id, operation, &folded, &operation_path)?;
                        ty
                    }
                    None => Ty::Literal(self.operation(operation, &folded, &operation_path)?),
                }
            };
            let mut member = Member::new(Key::Ident(method.to_owned()), ty);
            member.comment = self.comment(operation);
            members.push(member);
        }
        Ok(Ty::Literal(members))
    }

    /// The path item's parameters and the operation's, the operation's
    /// replacing a path item parameter with the same `in` and `name`.
    fn fold_parameters(
        &mut self,
        shared: &[Value],
        operation: &Value,
        path: &str,
    ) -> Res<Vec<Value>> {
        let own = parameter_list(operation.get("parameters"), path)?;
        let mut keyed: Vec<(String, Value)> = Vec::new();
        let mut places: HashMap<String, usize> = HashMap::new();
        for parameter in shared.iter().chain(&own) {
            let described = if parameter.has("$ref") {
                parameter
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|reference| self.resolve(reference))
            } else {
                Some(parameter.clone())
            };
            let key = match &described {
                Some(parameter) => {
                    format!(
                        "{}-{}",
                        js_text(parameter.get("in")),
                        js_text(parameter.get("name"))
                    )
                }
                None => "undefined-undefined".to_owned(),
            };
            match places.get(&key).and_then(|place| keyed.get_mut(*place)) {
                Some(entry) => entry.1 = parameter.clone(),
                None => {
                    places.insert(key.clone(), keyed.len());
                    keyed.push((key, parameter.clone()));
                }
            }
        }
        Ok(keyed.into_iter().map(|(_, parameter)| parameter).collect())
    }

    /// Adds an operation to the `operations` interface.
    fn inject_operation(
        &mut self,
        id: &str,
        operation: &Value,
        parameters: &[Value],
        path: &str,
    ) -> Res<()> {
        let place = match self
            .footer
            .iter()
            .position(|item| matches!(item, Footer::Operations(_)))
        {
            Some(place) => place,
            None => {
                self.footer.push(Footer::Operations(Vec::new()));
                self.footer.len() - 1
            }
        };
        let members = self.operation(operation, parameters, path)?;
        if let Some(Footer::Operations(operations)) = self.footer.get_mut(place) {
            operations.push(Member::new(property_key(id), Ty::Literal(members)));
        }
        Ok(())
    }

    fn operation(
        &mut self,
        operation: &Value,
        parameters: &[Value],
        path: &str,
    ) -> Res<Vec<Member>> {
        let mut members = self.parameters(parameters, path)?;
        match operation.get("requestBody").filter(|body| body.truthy()) {
            Some(body) => {
                require_object(body, path, "requestBody")?;
                let (ty, required) = if body.has("$ref") {
                    let reference = text(body.get("$ref"), "$ref")?;
                    let required = self
                        .resolve(reference)
                        .is_some_and(|resolved| truthy(resolved.get("required")));
                    (oapi_ref(reference, None)?, required)
                } else {
                    (
                        self.request_body(body, &create_ref(&[path, "requestBody"]))?,
                        truthy(body.get("required")),
                    )
                };
                let mut member =
                    Member::new(Key::Ident("requestBody".to_owned()), ty).optional(!required);
                member.comment = self.comment(body);
                members.push(member);
            }
            None => members
                .push(Member::new(Key::Ident("requestBody".to_owned()), NEVER).optional(true)),
        }
        let responses = self.responses(operation.get("responses"), path)?;
        members.push(Member::new(Key::Ident("responses".to_owned()), responses));
        Ok(members)
    }

    // ----- parameters, bodies, responses --------------------------------

    /// The `parameters` member of an operation or path item.
    fn parameters(&mut self, parameters: &[Value], path: &str) -> Res<Vec<Member>> {
        let mut groups = Vec::new();
        for place in ["query", "header", "path", "cookie"] {
            let mut located = Vec::new();
            for original in parameters {
                let reference = original
                    .has("$ref")
                    .then(|| text(original.get("$ref"), "$ref"))
                    .transpose()?;
                let resolved = match reference {
                    Some(reference) => self.resolve(reference),
                    None => Some(original.clone()),
                };
                let Some(resolved) = resolved else {
                    continue;
                };
                if resolved.get("in").and_then(Value::as_str) != Some(place) {
                    continue;
                }
                let name = js_text(resolved.get("name"));
                let key = match resolved.get("name") {
                    Some(Value::Num(number)) if *number >= 0.0 => Key::Number(js_number(*number)),
                    Some(Value::Str(name)) => self.key(name),
                    _ => Key::Quoted(name.clone()),
                };
                let optional = place != "path" && !truthy(resolved.get("required"));
                let ty = match reference {
                    Some(reference) => oapi_ref(reference, Some(&resolved))?,
                    None => {
                        self.parameter(&resolved, &create_ref(&[path, "parameters", place, &name]))?
                    }
                };
                let mut member = Member::new(key, ty).optional(optional);
                member.comment = self.comment(&resolved);
                located.push(member);
            }
            let all_optional = located.iter().all(|member| member.optional);
            let ty = if located.is_empty() {
                NEVER
            } else {
                Ty::Literal(located)
            };
            groups.push(Member::new(Key::Ident(place.to_owned()), ty).optional(all_optional));
        }
        Ok(vec![Member::new(
            Key::Ident("parameters".to_owned()),
            Ty::Literal(groups),
        )])
    }

    fn parameter(&mut self, parameter: &Value, path: &str) -> Res<Ty> {
        match parameter.get("schema").filter(|schema| schema.truthy()) {
            Some(schema) => self.schema(schema, path),
            None => Ok(STRING),
        }
    }

    fn request_body(&mut self, body: &Value, path: &str) -> Res<Ty> {
        let mut members = Vec::new();
        for (content_type, media) in entries(body.get("content")) {
            require_object(media, path, content_type)?;
            let next = create_ref(&[path, "content", content_type]);
            let ty = if media.has("$ref") {
                self.schema(media, &next)?
            } else {
                self.media_type(media, &next)?
            };
            members.push(self.member(content_type, ty, media));
        }
        if members.is_empty() {
            members.push(Member::new(property_key("*/*"), NEVER).optional(true));
        }
        Ok(Ty::Literal(vec![Member::new(
            Key::Ident("content".to_owned()),
            Ty::Literal(members),
        )]))
    }

    fn media_type(&mut self, media: &Value, path: &str) -> Res<Ty> {
        match media.get("schema").filter(|schema| schema.truthy()) {
            Some(schema) => self.schema(schema, path),
            None => Ok(UNKNOWN),
        }
    }

    fn responses(&mut self, responses: Option<&Value>, path: &str) -> Res<Ty> {
        let mut members = Vec::new();
        for (code, response) in entries(responses) {
            require_object(response, path, code)?;
            let ty = if response.has("$ref") {
                oapi_ref(text(response.get("$ref"), "$ref")?, None)?
            } else {
                self.response(response, &create_ref(&[path, "responses", code]))?
            };
            members.push(self.member(code, ty, response));
        }
        Ok(if members.is_empty() {
            NEVER
        } else {
            Ty::Literal(members)
        })
    }

    fn response(&mut self, response: &Value, path: &str) -> Res<Ty> {
        let mut headers = Vec::new();
        if truthy(response.get("headers")) {
            for (name, header) in entries(response.get("headers")) {
                require_object(header, path, name)?;
                let optional = !(header.has("$ref") || truthy(header.get("required")));
                let ty = if header.has("$ref") {
                    oapi_ref(text(header.get("$ref"), "$ref")?, None)?
                } else {
                    self.header(header, &create_ref(&[path, "headers", name]))?
                };
                let mut member = self.member(name, ty, header);
                member.optional = optional;
                headers.push(member);
            }
        }
        headers.push(Member::new(Key::Index("name"), UNKNOWN));
        let mut members = vec![Member::new(
            Key::Ident("headers".to_owned()),
            Ty::Literal(headers),
        )];
        let mut content = Vec::new();
        for (content_type, media) in entries(response.get("content")) {
            let ty = self.media_type(media, &create_ref(&[path, "content", content_type]))?;
            content.push(self.member(content_type, ty, media));
        }
        members.push(if content.is_empty() {
            Member::new(Key::Ident("content".to_owned()), NEVER).optional(true)
        } else {
            Member::new(Key::Ident("content".to_owned()), Ty::Literal(content))
        });
        Ok(Ty::Literal(members))
    }

    fn header(&mut self, header: &Value, path: &str) -> Res<Ty> {
        if let Some(schema) = header.get("schema").filter(|schema| schema.truthy()) {
            return self.schema(schema, path);
        }
        if truthy(header.get("content")) {
            let mut members = Vec::new();
            for (content_type, media) in entries(header.get("content")) {
                let next = format!("{path}/{}", escape_pointer(content_type));
                let ty = if media.has("$ref") {
                    self.schema(media, &next)?
                } else {
                    self.media_type(media, &next)?
                };
                members.push(self.member(content_type, ty, media));
            }
            return Ok(Ty::Literal(members));
        }
        Ok(UNKNOWN)
    }

    // ----- components -------------------------------------------------

    fn components(&mut self, components: &Value) -> Res<Ty> {
        let mut sections = Vec::new();
        for key in [
            "schemas",
            "responses",
            "parameters",
            "requestBodies",
            "headers",
            "pathItems",
        ] {
            let mut items = Vec::new();
            for (name, item) in entries(components.get(key)) {
                let path = create_ref(&["components", key, name]);
                let ty = match key {
                    "schemas" => self.schema(item, &path)?,
                    "responses" => self.response(item, &path)?,
                    "parameters" => self.parameter(item, &path)?,
                    "requestBodies" => self.request_body(item, &path)?,
                    "headers" => self.header(item, &path)?,
                    _ => self.path_item(item, &path)?,
                };
                items.push(self.member(name, ty, item));
            }
            let ty = if items.is_empty() {
                NEVER
            } else {
                Ty::Literal(items)
            };
            sections.push(Member::new(Key::Ident(key.to_owned()), ty));
        }
        Ok(Ty::Literal(sections))
    }

    // ----- schemas ----------------------------------------------------

    /// The type of a schema.
    fn schema(&mut self, node: &Value, path: &str) -> Res<Ty> {
        // Every schema below this one is given a path that starts with this
        // one's, copied for each, so the length of the path is work too.
        self.spend(path.len().saturating_add(64));
        if self.work.get() > MAX_WORK {
            return Err(too_much_work());
        }
        if !node.truthy() {
            return Ok(NEVER);
        }
        let object = match node {
            Value::Bool(_) => return Ok(UNKNOWN),
            Value::Obj(object) => object,
            Value::Arr(_) => {
                return Err(format!("Expected SchemaObject, received Array at {path}"));
            }
            _ => {
                let kind = if matches!(node, Value::Str(_)) {
                    "string"
                } else {
                    "number"
                };
                return Err(format!("Expected SchemaObject, received {kind} at {path}"));
            }
        };
        if let Some(reference) = object.get("$ref") {
            return oapi_ref(text(Some(reference), "$ref")?, None);
        }
        if let Some(value) = object
            .get("const")
            .filter(|value| !matches!(value, Value::Null))
        {
            return Ok(literal(value));
        }
        if let Some(Value::Arr(values)) = object.get("enum") {
            let is_object_type = object.get("type").and_then(Value::as_str) == Some("object");
            if !is_object_type
                && !object.contains("properties")
                && !object.contains("additionalProperties")
            {
                let mut members: Vec<Ty> = values.iter().map(literal).collect();
                let null_type = object
                    .get("type")
                    .and_then(Value::as_array)
                    .is_some_and(|types| types.iter().any(|t| t.as_str() == Some("null")));
                if null_type || truthy(object.get("nullable")) {
                    members.push(Ty::Null);
                }
                return Ok(union(members));
            }
        }

        let core = self.core(node, object, path)?;
        let all_of = self.all_of(object, path)?;
        let mut result: Option<Ty> = None;
        if core.is_some() || !all_of.is_empty() {
            let all_of = (!all_of.is_empty()).then(|| intersection(all_of));
            result = Some(intersection(core.into_iter().chain(all_of).collect()));
        }
        let any_of = self.compositions(object.get("anyOf"), "anyOf", path)?;
        if !any_of.is_empty() {
            result = Some(union(result.into_iter().chain(any_of).collect()));
        }
        let one_of_source = if truthy(object.get("oneOf")) {
            object.get("oneOf")
        } else if object.get("type").and_then(Value::as_str) == Some("object") {
            object.get("enum").filter(|value| value.truthy())
        } else {
            None
        };
        let one_of = self.compositions(one_of_source, "oneOf", path)?;
        if !one_of.is_empty() {
            result = Some(if one_of.iter().all(is_primitive) {
                union(result.into_iter().chain(one_of).collect())
            } else {
                intersection(result.into_iter().chain([union(one_of)]).collect())
            });
        }
        let mut result = result.unwrap_or_else(|| {
            if object.contains("type") {
                record(STRING, NEVER)
            } else {
                UNKNOWN
            }
        });
        if result != UNKNOWN && truthy(object.get("nullable")) {
            result = nullable(result);
        }
        Ok(result)
    }

    /// The members of a `oneOf` or `anyOf`.
    fn compositions(&mut self, items: Option<&Value>, key: &str, path: &str) -> Res<Vec<Ty>> {
        let Some(items) = items else {
            return Ok(Vec::new());
        };
        let Some(items) = items.as_array() else {
            return Err(format!("{key} must be an array at {path}"));
        };
        let mut types = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let next = create_ref(&[path, key, &index.to_string()]);
            types.push(self.schema(item, &next)?);
        }
        Ok(types)
    }

    /// The members of an `allOf`.
    fn all_of(&mut self, object: &Object, path: &str) -> Res<Vec<Ty>> {
        let Some(items) = object.get("allOf") else {
            return Ok(Vec::new());
        };
        let Some(items) = items.as_array() else {
            return Err(format!("allOf must be an array at {path}"));
        };
        let required = object.get("required");
        let mut output = Vec::with_capacity(items.len());
        for item in items {
            let Some(member) = item.as_object() else {
                return Err(format!(
                    "allOf holds something that is not a schema at {path}"
                ));
            };
            // The member is read with the required names of the parent and
            // of its own, copied for each member: members times names.
            let names =
                |value: Option<&Value>| value.and_then(Value::as_array).map_or(0, <[Value]>::len);
            self.spend(
                names(required)
                    .saturating_add(names(member.get("required")))
                    .saturating_mul(16),
            );
            let mut ty;
            if member.contains("$ref") {
                let reference = text(member.get("$ref"), "$ref")?;
                ty = self.schema(item, path)?;
                let resolved = self.resolve(reference);
                if let Some(resolved) = resolved.filter(|resolved| resolved.has("properties"))
                    && !self.discriminators.refs_handled.contains(reference)
                {
                    let properties = resolved.get("properties");
                    let valid: Vec<&str> = required
                        .and_then(Value::as_array)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|key| {
                            properties
                                .and_then(|p| p.get(key))
                                .is_some_and(Value::truthy)
                        })
                        .collect();
                    if !valid.is_empty() {
                        ty = self.with_required(ty, &valid);
                    }
                }
            } else {
                let mut combined: Vec<Value> = required
                    .and_then(Value::as_array)
                    .map(<[Value]>::to_vec)
                    .unwrap_or_default();
                if let Some(own) = member.get("required").and_then(Value::as_array) {
                    combined.extend(own.iter().cloned());
                }
                let mut copy = member.clone();
                copy.set("required", Value::Arr(std::rc::Rc::new(combined)));
                ty = self.schema(&Value::Obj(std::rc::Rc::new(copy)), path)?;
            }
            let discriminator = member
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|reference| self.discriminators.objects.get(reference).cloned())
                .or_else(|| member.get("discriminator").filter(|d| d.truthy()).cloned());
            if let Some(discriminator) = discriminator {
                let key = match discriminator.get("propertyName") {
                    Some(name) => literal(name),
                    None => UNKNOWN,
                };
                ty = Ty::Reference("Omit".to_owned(), vec![ty, Ty::Union(vec![key])]);
            }
            output.push(ty);
        }
        Ok(output)
    }

    fn with_required(&mut self, ty: Ty, keys: &[&str]) -> Ty {
        if !self
            .footer
            .iter()
            .any(|item| matches!(item, Footer::WithRequired))
        {
            self.footer.push(Footer::WithRequired);
        }
        let keys = union(keys.iter().map(|key| literal_str(key)).collect());
        Ty::Reference("WithRequired".to_owned(), vec![ty, keys])
    }

    /// A schema without its compositions: the primitive, array or object it
    /// says it is.
    fn core(&mut self, node: &Value, object: &Object, path: &str) -> Res<Option<Ty>> {
        if let Some(kind) = object.get("type").filter(|kind| kind.truthy()) {
            match kind {
                Value::Str(name) => match &**name {
                    "null" => return Ok(Some(Ty::Null)),
                    "string" => return Ok(Some(STRING)),
                    "number" | "integer" => return Ok(Some(NUMBER)),
                    "boolean" => return Ok(Some(BOOLEAN)),
                    "array" => return self.array(object, path).map(Some),
                    _ => {}
                },
                Value::Arr(types) => return self.polymorphic(node, object, types, path).map(Some),
                _ => {}
            }
        }
        self.object_type(object, path)
    }

    fn array(&mut self, object: &Object, path: &str) -> Res<Ty> {
        let mut item = UNKNOWN;
        let items = object.get("items");
        let prefix = object
            .get("prefixItems")
            .filter(|prefix| prefix.truthy())
            .or_else(|| items.filter(|items| items.as_array().is_some()));
        if let Some(prefix) = prefix {
            let Some(prefix) = prefix.as_array() else {
                return Err(format!("prefixItems must be an array at {path}"));
            };
            let mut elements = Vec::with_capacity(prefix.len());
            for element in prefix {
                elements.push(self.schema(element, path)?);
            }
            item = Ty::Tuple(elements);
        } else if let Some(items) = items.filter(|items| items.truthy()) {
            let inner = self.schema(items, path)?;
            let nested = items.get("type").and_then(Value::as_str) == Some("array");
            item = if nested {
                Ty::Array(Box::new(inner))
            } else {
                inner
            };
        }
        Ok(if matches!(item, Ty::Tuple(_) | Ty::Array(_)) {
            item
        } else {
            Ty::Array(Box::new(item))
        })
    }

    /// `type: [...]`: the union of the schema read as each type.
    fn polymorphic(
        &mut self,
        node: &Value,
        object: &Object,
        types: &[Value],
        path: &str,
    ) -> Res<Ty> {
        let one_of = object.get("oneOf").and_then(Value::as_array);
        let one_of_types: HashSet<&str> = one_of
            .unwrap_or_default()
            .iter()
            .filter_map(|member| member.get("type").and_then(Value::as_str))
            .collect();
        let mut unique = Vec::with_capacity(types.len());
        for kind in types {
            let name = kind.as_str();
            if let Some(name) = name
                && one_of.is_some()
                && matches!(name, "boolean" | "string" | "number" | "integer" | "null")
                && one_of_types.contains(name)
            {
                continue;
            }
            if name == Some("null") || matches!(kind, Value::Null) {
                unique.push(Ty::Null);
                continue;
            }
            // Each type reads a copy of the whole schema: types times keys.
            self.spend(object.len().saturating_mul(16));
            let mut copy = node.clone();
            if let Some(copy) = copy.object_mut() {
                copy.set("type", kind.clone());
                if one_of.is_some() {
                    copy.remove("oneOf");
                }
            }
            unique.push(self.schema(&copy, path)?);
        }
        Ok(union(unique))
    }

    /// The object part of a schema: properties, `$defs`, index signatures.
    fn object_type(&mut self, object: &Object, path: &str) -> Res<Option<Ty>> {
        let mut members: Vec<Member> = Vec::new();

        if ["allOf", "anyOf"].iter().any(|key| truthy(object.get(key)))
            && !truthy(object.get("discriminator"))
            && !self.discriminators.refs_handled.contains(path)
            && let Some(discriminator) = self.discriminators.objects.get(path)
        {
            members.insert(0, discriminator_property(discriminator, path));
        }

        let property_count = object
            .get("properties")
            .and_then(Value::as_object)
            .map_or(0, Object::len);
        let additional = object.get("additionalProperties");
        let pattern = object.get("patternProperties");
        if !(property_count > 0
            || truthy(additional)
            || truthy(pattern)
            || truthy(object.get("$defs")))
        {
            return Ok((!members.is_empty()).then_some(Ty::Literal(members)));
        }

        if property_count > 0 {
            for (name, property) in entries(object.get("properties")) {
                if !matches!(property, Value::Obj(_) | Value::Bool(_)) {
                    return Err(format!(
                        "{path}: invalid property {name}. Expected Schema Object or boolean, got {}",
                        match property {
                            Value::Arr(_) => "Array",
                            Value::Str(_) => "string",
                            Value::Num(_) => "number",
                            _ => "object",
                        }
                    ));
                }
                let reference = property.get("$ref").filter(|r| r.truthy());
                let read_only = truthy(property.get("readOnly"));
                let has_default = property.has("default");
                let required = object
                    .get("required")
                    .and_then(Value::as_array)
                    .is_some_and(|names| names.iter().any(|n| n.as_str() == Some(name)));
                let required = required
                    || (has_default
                        && !path.contains("parameters")
                        && !path.contains("requestBody")
                        && !path.contains("requestBodies"));
                let ty = match reference {
                    Some(reference) => oapi_ref(text(Some(reference), "$ref")?, None)?,
                    None => self.schema(property, &create_ref(&[path, name]))?,
                };
                let mut member = Member::new(self.key(name), ty).optional(!required);
                member.readonly = read_only;
                member.comment = self.comment(property);
                members.push(member);
            }
        }

        if truthy(object.get("$defs"))
            && let Some(defs) = object.get("$defs")
        {
            let defs = entries(Some(defs));
            if !defs.is_empty() {
                let mut definitions = Vec::new();
                for (name, definition) in defs {
                    let ty = self.schema(definition, &create_ref(&[path, "$defs", name]))?;
                    let mut member = Member::new(self.key(name), ty);
                    member.readonly = truthy(definition.get("readOnly"));
                    member.comment = self.comment(definition);
                    definitions.push(member);
                }
                members.push(Member::new(
                    Key::Ident("$defs".to_owned()),
                    Ty::Literal(definitions),
                ));
            }
        }

        // `typeof x === "object"` and `Object.keys(x).length`.
        let count = |value: Option<&Value>| match value {
            Some(Value::Obj(object)) => Some(object.len()),
            Some(Value::Arr(items)) => Some(items.len()),
            _ => None,
        };
        let explicit = count(additional).is_some_and(|count| count > 0);
        let implicit =
            matches!(additional, Some(Value::Bool(true))) || count(additional) == Some(0);
        let mut index_types = Vec::new();
        if explicit && let Some(additional) = additional {
            index_types.push(self.schema(additional, path)?);
        }
        if implicit {
            index_types.push(UNKNOWN);
        }
        if count(pattern).is_some_and(|count| count > 0) {
            if pattern.and_then(Value::as_object).is_none() {
                return Err(format!("patternProperties must be an object at {path}"));
            }
            for (_, value) in entries(pattern) {
                index_types.push(self.schema(value, path)?);
            }
        }
        if index_types.is_empty() {
            return Ok((!members.is_empty()).then_some(Ty::Literal(members)));
        }
        let index = Ty::Literal(vec![Member::new(Key::Index("key"), union(index_types))]);
        let parts: Vec<Ty> = (!members.is_empty())
            .then_some(Ty::Literal(members))
            .into_iter()
            .chain([index])
            .collect();
        Ok(Some(intersection(parts)))
    }
}

/// `name: "Value"`, the property a mapped schema gets: the last part of the
/// schema's pointer, or the mapped value that points at it.
fn discriminator_property(discriminator: &Value, path: &str) -> Member {
    let mut value = parse_ref(path).pop();
    if let Some(mapping) = discriminator.get("mapping").and_then(Value::as_object) {
        for (mapped, target) in mapping.js_entries() {
            let Some(target) = target.as_str() else {
                continue;
            };
            let found = if target.starts_with('#') {
                parse_ref(target).pop() == value
            } else {
                Some(target.to_owned()) == value
            };
            if found {
                value = Some(mapped.to_owned());
                break;
            }
        }
    }
    let name = discriminator
        .get("propertyName")
        .map(Value::js_string)
        .unwrap_or_default();
    let ty = value.map_or(UNKNOWN, |value| literal_str(&value));
    Member::new(property_key(&name), ty)
}

/// `\r\n` and `\n` inside one line of a comment become a new line of the
/// comment, indented under the tag.
fn break_lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\n *     ")
}
