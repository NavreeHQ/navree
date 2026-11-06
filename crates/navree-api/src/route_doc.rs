//! [`RouteDoc`]: what a route's `#[api]` metadata cannot say, added to the document by the module
//! that knows it.
//!
//! - **Query parameters**: the list query of a route that takes a
//!   [`List`](navree_http::list::List) ([`RouteDoc::list`]), or any other parameter
//!   ([`RouteDoc::query`]).
//! - **Responses that are not JSON** (a CSV export, Prometheus text, a WebSocket upgrade), more
//!   statuses, and another success status.
//! - **The security a route needs**, when it is not the app's default.
//!
//! A doc is registered by the module that owns the route
//! ([`ModuleBuilderApiExt::api_route`](crate::ModuleBuilderApiExt::api_route)) and names the route
//! by its name (`health.live`) or by its method and mounted path (`GET /api/v1/users`). A doc
//! that names no route stops the boot, so a renamed route cannot leave its description behind.

use navree_http::SchemaFn;
use navree_http::list::{Field, Kind, ListResource, ListSpec, MAX_CURSOR, MAX_PAGE, MAX_TEXT, Op, Search};
use navree_http::schema_fn;
use navree_openapi::{Parameter, Schema, ToSchema, Type};
use serde_json::json;

/// Which route a [`RouteDoc`] is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// The route's name (`health.live`).
    Name(String),
    /// The method (upper case) and the mounted path (`/api/v1/users`).
    Path { method: String, path: String },
}

impl Target {
    /// The route in the words an error uses.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Name(name) => format!("the route named `{name}`"),
            Self::Path { method, path } => format!("`{method} {path}`"),
        }
    }
}

/// The schema of a documented response body.
#[derive(Clone)]
pub(crate) enum Media {
    /// A type's schema, with the schemas it references (registered as components).
    Type(SchemaFn),
    /// A schema written here (boxed: it is large next to a function).
    Inline(Box<Schema>),
}

/// One response a doc adds.
#[derive(Clone)]
pub(crate) struct ResponseDoc {
    pub(crate) status: u16,
    pub(crate) description: Option<String>,
    /// The content type and schema, or `None` for a response without a body.
    pub(crate) content: Option<(String, Media)>,
}

/// What to add to one route's operation. See the [module](self) documentation.
///
/// ```
/// use navree_api::RouteDoc;
///
/// // `GET /api/v1/contacts/export` answers with a CSV file.
/// let doc = RouteDoc::get("/api/v1/contacts/export")
///     .text_response(200, "text/csv", "The contacts, one row each, with a header row.");
/// # let _ = doc;
/// ```
#[derive(Clone)]
pub struct RouteDoc {
    pub(crate) target: Target,
    pub(crate) optional: bool,
    pub(crate) query: Vec<Parameter>,
    pub(crate) success: Option<u16>,
    pub(crate) responses: Vec<ResponseDoc>,
    pub(crate) components: Vec<SchemaFn>,
    pub(crate) security: Option<Vec<String>>,
}

impl RouteDoc {
    fn new(target: Target) -> Self {
        Self {
            target,
            optional: false,
            query: Vec::new(),
            success: None,
            responses: Vec::new(),
            components: Vec::new(),
            security: None,
        }
    }

    /// The route named `name` (`RouteDef::name`).
    pub fn named(name: impl Into<String>) -> Self {
        Self::new(Target::Name(name.into()))
    }

    fn at(method: &str, path: impl Into<String>) -> Self {
        Self::new(Target::Path {
            method: method.to_owned(),
            path: path.into(),
        })
    }

    /// The `GET` route mounted at `path` (`/api/v1/users`, with `{id}` for captures).
    pub fn get(path: impl Into<String>) -> Self {
        Self::at("GET", path)
    }

    /// The `POST` route mounted at `path`.
    pub fn post(path: impl Into<String>) -> Self {
        Self::at("POST", path)
    }

    /// The `PUT` route mounted at `path`.
    pub fn put(path: impl Into<String>) -> Self {
        Self::at("PUT", path)
    }

    /// The `PATCH` route mounted at `path`.
    pub fn patch(path: impl Into<String>) -> Self {
        Self::at("PATCH", path)
    }

    /// The `DELETE` route mounted at `path`.
    pub fn delete(path: impl Into<String>) -> Self {
        Self::at("DELETE", path)
    }

    /// Not an error when the app has no such route. For a module that documents a route another
    /// module may or may not mount.
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Adds a query parameter.
    #[must_use]
    pub fn query(mut self, parameter: Parameter) -> Self {
        self.query.push(parameter);
        self
    }

    /// Documents the list query of a route that takes `List<R>`: `filter`, `sort`, `search` and the
    /// paging parameters `R`'s [`ListSpec`] allows.
    #[must_use]
    pub fn list<R: ListResource>(self) -> Self {
        self.list_spec(&R::SPEC)
    }

    /// [`list`](Self::list) for a spec held in a value.
    #[must_use]
    pub fn list_spec(mut self, spec: &ListSpec) -> Self {
        self.query.extend(list_parameters(spec));
        self
    }

    /// The `filter`, `sort` and `search` of `R`'s [`ListSpec`] and no paging: for a route that
    /// reads the list query to pick rows but answers with all of them (a CSV export).
    #[must_use]
    pub fn filters<R: ListResource>(mut self) -> Self {
        self.query.extend(
            list_parameters(&R::SPEC)
                .into_iter()
                .filter(|parameter| matches!(parameter.name.as_str(), "filter" | "sort" | "search")),
        );
        self
    }

    /// The status of the success response, when it is not the one `#[api]` says (a WebSocket
    /// upgrade answers 101).
    #[must_use]
    pub fn success_status(mut self, status: u16) -> Self {
        self.success = Some(status);
        self
    }

    /// Adds, or replaces, the `status` response with a JSON body of type `T`.
    #[must_use]
    pub fn json_response<T: ToSchema>(mut self, status: u16, description: &str) -> Self {
        self.responses.push(ResponseDoc {
            status,
            description: Some(description.to_owned()),
            content: Some(("application/json".to_owned(), Media::Type(schema_fn::<T>))),
        });
        self
    }

    /// Adds `T` to the document's component schemas, for a response schema written with
    /// [`json_schema_response`](Self::json_schema_response) that refers to it. Only when the route
    /// exists.
    #[must_use]
    pub fn with_schema<T: ToSchema>(mut self) -> Self {
        self.components.push(schema_fn::<T>);
        self
    }

    /// Adds, or replaces, the `status` response with a JSON body of the schema `schema`.
    #[must_use]
    pub fn json_schema_response(mut self, status: u16, description: &str, schema: Schema) -> Self {
        self.responses.push(ResponseDoc {
            status,
            description: Some(description.to_owned()),
            content: Some(("application/json".to_owned(), Media::Inline(Box::new(schema)))),
        });
        self
    }

    /// Adds, or replaces, the `status` response with a text body of `content_type` (`text/csv`).
    #[must_use]
    pub fn text_response(mut self, status: u16, content_type: &str, description: &str) -> Self {
        self.responses.push(ResponseDoc {
            status,
            description: Some(description.to_owned()),
            content: Some((content_type.to_owned(), Media::Inline(Box::new(Schema::string())))),
        });
        self
    }

    /// Adds, or replaces, the `status` response with no body.
    #[must_use]
    pub fn empty_response(mut self, status: u16, description: &str) -> Self {
        self.responses.push(ResponseDoc {
            status,
            description: Some(description.to_owned()),
            content: None,
        });
        self
    }

    /// The security schemes the route needs, any one of them (`["bearer"]`). Empty: none. Without
    /// this the route has the app's ([`api_security`](crate::ModuleBuilderApiExt::api_security)),
    /// or every scheme the document has, for a route that isn't public.
    #[must_use]
    pub fn security(mut self, schemes: &[&str]) -> Self {
        self.security = Some(schemes.iter().map(|scheme| (*scheme).to_owned()).collect());
        self
    }
}

/// A [`RouteDoc`] registered by a module.
#[derive(Clone)]
pub(crate) struct ApiRouteDoc(pub(crate) RouteDoc);

/// The security schemes an app uses, registered by a module.
#[derive(Clone)]
pub(crate) struct ApiSecurity(pub(crate) Vec<String>);

/// A length as the schema's `maxLength` takes it.
fn length(bytes: usize) -> u64 {
    u64::try_from(bytes).unwrap_or(u64::MAX)
}

/// A whole number between `minimum` and `maximum`.
fn count(minimum: u64, maximum: u64) -> Schema {
    Schema::of_type(Type::Integer)
        .minimum(json!(minimum))
        .maximum(json!(maximum))
}

/// The query parameters of a list: `filter` (an object written `filter[field]=value`), `sort`,
/// `search`, and either numbered or cursor paging.
pub(crate) fn list_parameters(spec: &ListSpec) -> Vec<Parameter> {
    let mut out = Vec::new();
    if !spec.fields.is_empty() {
        let mut filter = Schema::object().no_additional_properties();
        let mut names = Vec::new();
        for field in spec.fields {
            filter = filter.property(field.name, field_schema(field), false);
            names.push(format!("`{}`", field.name));
        }
        out.push(
            Parameter::query("filter", filter)
                .deep_object()
                .described(format!(
                    "Filters on {}: `filter[field]=value`, or `filter[field][operator]=value` with an operator the field takes (`ne`, `gt`, `gte`, `lt`, `lte`, `in`, `not_in`, `contains`, `starts_with`, `null`).",
                    names.join(", ")
                )),
        );
    }
    if !spec.sorts.is_empty() {
        let allowed: Vec<String> = spec.sorts.iter().map(|sort| format!("`{sort}`")).collect();
        let mut text = format!(
            "Sort by {}, comma-separated, with `-` in front for descending.",
            allowed.join(", ")
        );
        if !spec.default_sort.is_empty() {
            text.push_str(&format!(" Default: `{}`.", spec.default_sort.join(",")));
        }
        out.push(Parameter::query("sort", Schema::string()).described(text));
    }
    if !matches!(spec.search, Search::Off) {
        let mut schema = Schema::string();
        schema.max_length = Some(length(MAX_TEXT));
        out.push(
            Parameter::query("search", schema).described(match spec.search {
                Search::Columns(columns) => format!(
                    "Part of {}, ignoring case.",
                    columns
                        .iter()
                        .map(|column| format!("`{column}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                _ => "Text to search for.".to_owned(),
            }),
        );
    }
    let size = count(1, spec.max_per_page).default_value(json!(spec.default_per_page));
    out.push(
        Parameter::query("page", count(1, MAX_PAGE))
            .described("The page number, for numbered pages (default 1)."),
    );
    out.push(Parameter::query("per_page", size.clone()).described(format!(
        "The page size for numbered pages (default {}, at most {}).",
        spec.default_per_page, spec.max_per_page
    )));
    let mut cursor = Schema::string();
    cursor.max_length = Some(length(MAX_CURSOR));
    out.push(
        Parameter::query("cursor", cursor.clone())
            .described("The cursor of the page to read, for cursor pages: the `next` of the page before it."),
    );
    out.push(Parameter::query("limit", size).described(format!(
        "The page size for cursor pages (default {}, at most {}).",
        spec.default_per_page, spec.max_per_page
    )));
    if spec.two_way_cursors {
        out.push(
            Parameter::query("before", cursor)
                .described("The cursor of the page before the rows it was given for."),
        );
    }
    out
}

/// What one value of a filter field looks like.
fn value_schema(kind: Kind) -> Schema {
    match kind {
        Kind::Text => {
            let mut schema = Schema::string();
            schema.max_length = Some(length(MAX_TEXT));
            schema
        }
        Kind::Int => Schema::of_type(Type::Integer),
        Kind::Decimal => Schema::of_type(Type::Number),
        Kind::Bool => Schema::of_type(Type::Boolean),
        Kind::Date => Schema::string().format("date"),
        Kind::DateTime => Schema::string().format("date-time"),
        Kind::Uuid => Schema::string().format("uuid"),
        Kind::OneOf(words) => Schema::string_enum(words.iter().copied()),
    }
}

/// A filter field: its value, or an object of operator to value.
fn field_schema(field: &Field) -> Schema {
    let value = value_schema(field.kind);
    let operators: Vec<Op> = field
        .allowed_ops()
        .iter()
        .copied()
        .filter(|op| *op != Op::Eq)
        .collect();
    if operators.is_empty() {
        return value;
    }
    let mut with_operators = Schema::object().no_additional_properties();
    for op in operators {
        let schema = match op {
            Op::In | Op::NotIn => Schema::string().description("A comma-separated list of values."),
            Op::Null => Schema::of_type(Type::Boolean).description("`true`: is NULL. `false`: is not NULL."),
            _ => value.clone(),
        };
        with_operators = with_operators.property(op.name(), schema, false);
    }
    Schema::one_of(vec![value, with_operators])
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Deals;

    impl ListResource for Deals {
        const SPEC: ListSpec = ListSpec::new(&[
            Field::one_of("stage", &["lead", "won"]),
            Field::int("amount_cents"),
            Field::boolean("open"),
        ])
        .sortable(&["amount_cents", "created_at"])
        .default_sort(&["-created_at"])
        .searchable(&["title"])
        .page_size(20, 50);
    }

    fn by_name(parameters: &[Parameter], name: &str) -> serde_json::Value {
        let found = parameters.iter().find(|p| p.name == name);
        serde_json::to_value(found.unwrap_or_else(|| panic!("no `{name}` parameter"))).unwrap()
    }

    #[test]
    fn a_list_documents_filter_sort_search_and_paging() {
        let parameters = list_parameters(&Deals::SPEC);
        let names: Vec<&str> = parameters.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["filter", "sort", "search", "page", "per_page", "cursor", "limit"]);

        let filter = by_name(&parameters, "filter");
        assert_eq!(filter["in"], "query");
        assert_eq!(filter["style"], "deepObject");
        assert_eq!(filter["explode"], true);
        assert_eq!(filter["required"], false);
        let stage = &filter["schema"]["properties"]["stage"];
        assert_eq!(stage["oneOf"][0]["enum"], json!(["lead", "won"]));
        assert_eq!(stage["oneOf"][1]["properties"]["in"]["type"], "string");
        assert_eq!(stage["oneOf"][1]["properties"]["null"]["type"], "boolean");
        assert!(stage["oneOf"][1]["properties"].get("gte").is_none());
        let amount = &filter["schema"]["properties"]["amount_cents"];
        assert_eq!(amount["oneOf"][0]["type"], "integer");
        assert_eq!(amount["oneOf"][1]["properties"]["gte"]["type"], "integer");
        // A boolean takes eq, ne and null: `ne` is an operator, so it is an object too.
        assert_eq!(filter["schema"]["properties"]["open"]["oneOf"][0]["type"], "boolean");
        assert_eq!(filter["schema"]["additionalProperties"], false);

        let sort = by_name(&parameters, "sort");
        assert!(sort["description"].as_str().unwrap().contains("`amount_cents`"));
        assert!(sort["description"].as_str().unwrap().contains("Default: `-created_at`."));

        let per_page = by_name(&parameters, "per_page");
        assert_eq!(per_page["schema"]["default"], 20);
        assert_eq!(per_page["schema"]["maximum"], 50);
        assert_eq!(per_page["schema"]["minimum"], 1);
        assert_eq!(by_name(&parameters, "limit")["schema"]["maximum"], 50);
    }

    #[test]
    fn a_list_without_fields_sorts_or_search_leaves_them_out() {
        let spec = ListSpec::new(&[]).two_way_cursors();
        let parameters = list_parameters(&spec);
        let names: Vec<&str> = parameters.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["page", "per_page", "cursor", "limit", "before"]);
    }
}
