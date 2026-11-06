//! What the document says of the routes navree's own modules mount, which describe themselves
//! with a summary and nothing else: health, metrics, the real-time endpoints and the abilities.
//! Each doc is optional, so an app without the module has no complaint.

use navree_openapi::Schema;

use crate::route_doc::RouteDoc;
use crate::shapes::AbilityRule;

/// The docs for the framework's routes, by route name.
pub(crate) fn framework_docs() -> Vec<RouteDoc> {
    vec![
        RouteDoc::named("health.live").optional().json_schema_response(
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
                        Schema::string().description("For a presence channel: the member, as JSON text."),
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
        .property("error", Schema::string().description("Why the check is down."), false);
    Schema::object()
        .property("status", state().description("`up` when every check is up."), true)
        .property(
            "checks",
            Schema::map(Schema::string(), check).description("Each check's result, by its name."),
            true,
        )
}
