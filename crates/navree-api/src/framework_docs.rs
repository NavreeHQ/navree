//! What the document says of the routes navree's own modules mount, which describe themselves
//! with a summary and nothing else: health, metrics, the real-time endpoints, the abilities
//! and the access module's role routes.
//! Each doc is optional, so an app without the module has no complaint.

use navree_openapi::Schema;

use crate::route_doc::RouteDoc;
use crate::shapes::{AbilityRule, AccessRole, AccessUserRoles};

/// The docs for the framework's routes, by route name.
pub(crate) fn framework_docs() -> Vec<RouteDoc> {
    vec![
        RouteDoc::named("health.live")
            .optional()
            .json_schema_response(
                200,
                "The process answers",
                Schema::object().property("status", Schema::string_enum(["up"]), true),
            ),
        RouteDoc::named("health.ready")
            .optional()
            .json_schema_response(200, "Every check is up", health_report())
            .json_schema_response(503, "At least one check is down", health_report()),
        RouteDoc::named("metrics").optional().text_response(
            200,
            "text/plain; version=0.0.4",
            "The metrics in the Prometheus text format.",
        ),
        RouteDoc::named("realtime.connect")
            .optional()
            .success_status(101)
            .empty_response(
                101,
                "Switching Protocols: the connection is now a Pusher protocol 7 WebSocket.",
            ),
        RouteDoc::named("realtime.auth")
            .optional()
            .json_schema_response(
                200,
                "The signature that lets the client subscribe",
                Schema::object()
                    .property(
                        "auth",
                        Schema::string().description("`<app key>:<signature>`."),
                        true,
                    )
                    .property(
                        "channel_data",
                        Schema::string()
                            .description("For a presence channel: the member, as JSON text."),
                        false,
                    ),
            ),
        RouteDoc::named("access.abilities")
            .optional()
            .with_schema::<AbilityRule>()
            .json_schema_response(
                200,
                "The rules, with an ETag",
                Schema::array(Schema::reference_to("AbilityRule")),
            ),
        RouteDoc::named("access.roles.index")
            .optional()
            .with_schema::<AccessRole>()
            .json_schema_response(
                200,
                "Every role, with its permissions",
                Schema::object().property(
                    "data",
                    Schema::array(Schema::reference_to("AccessRole")),
                    true,
                ),
            ),
        RouteDoc::named("access.roles.store")
            .optional()
            .json_response::<AccessRole>(201, "The role, created"),
        RouteDoc::named("access.roles.permissions")
            .optional()
            .json_response::<AccessRole>(200, "The role, with the permissions it grants now"),
        RouteDoc::named("access.users.roles")
            .optional()
            .json_response::<AccessUserRoles>(200, "The user's roles now"),
    ]
}

/// `{"status": "up"|"down", "checks": {name: {status, latency_ms, error?}}}`.
fn health_report() -> Schema {
    let state = || Schema::string_enum(["up", "down"]);
    let check = Schema::object()
        .property("status", state(), true)
        .property(
            "latency_ms",
            Schema::of_type(navree_openapi::Type::Integer).description("How long the check took."),
            true,
        )
        .property(
            "error",
            Schema::string().description("Why the check is down."),
            false,
        );
    Schema::object()
        .property(
            "status",
            state().description("`up` when every check is up."),
            true,
        )
        .property(
            "checks",
            Schema::map(Schema::string(), check).description("Each check's result, by its name."),
            true,
        )
}

#[cfg(test)]
mod tests {
    use navree_http::{AccessSpec, ApiMeta, RouteMeta};
    use serde_json::{Value, json};

    use super::*;
    use crate::ApiOptions;
    use crate::document::build;

    fn route(method: &str, path: &str, name: &str, success_status: u16) -> RouteMeta {
        RouteMeta {
            method: method.to_owned(),
            full_path: path.to_owned(),
            name: Some(name.to_owned()),
            access: AccessSpec {
                authenticated: true,
                ..AccessSpec::default()
            },
            api: Some(ApiMeta {
                summary: Some(name.to_owned()),
                success_status,
                ..ApiMeta::default()
            }),
            ..RouteMeta::default()
        }
    }

    #[test]
    fn the_access_role_routes_answer_with_their_shapes() {
        let routes = [
            route("GET", "/api/access/roles", "access.roles.index", 0),
            route("POST", "/api/access/roles", "access.roles.store", 201),
            route(
                "PUT",
                "/api/access/roles/{role}/permissions",
                "access.roles.permissions",
                0,
            ),
            route(
                "PUT",
                "/api/access/users/{user}/roles",
                "access.users.roles",
                0,
            ),
        ];
        let document = build(
            &routes,
            &[],
            &framework_docs(),
            &[],
            None,
            &ApiOptions::default(),
            "navree_session",
        )
        .expect("a document");
        let doc = serde_json::to_value(document).unwrap();
        let body = |path: &str, method: &str, status: &str| -> Value {
            doc["paths"][path][method]["responses"][status]["content"]["application/json"]["schema"]
                .clone()
        };
        let list = body("/api/access/roles", "get", "200");
        assert_eq!(list["required"], json!(["data"]), "{list}");
        assert!(
            list["properties"]["data"]
                .to_string()
                .contains("AccessRole")
        );
        for (path, method, status, shape) in [
            ("/api/access/roles", "post", "201", "AccessRole"),
            (
                "/api/access/roles/{role}/permissions",
                "put",
                "200",
                "AccessRole",
            ),
            (
                "/api/access/users/{user}/roles",
                "put",
                "200",
                "AccessUserRoles",
            ),
        ] {
            let schema = body(path, method, status);
            assert!(
                schema.to_string().contains(shape),
                "{method} {path}: {schema}"
            );
        }
        let role = &doc["components"]["schemas"]["AccessRole"];
        assert_eq!(role["properties"]["id"]["format"], "uuid", "{role}");
        assert_eq!(role["properties"]["created_at"]["format"], "date-time");
        assert_eq!(
            role["required"],
            json!(["id", "name", "permissions", "created_at"])
        );
        let user_roles = &doc["components"]["schemas"]["AccessUserRoles"];
        assert_eq!(user_roles["required"], json!(["user_id", "roles"]));
    }
}
