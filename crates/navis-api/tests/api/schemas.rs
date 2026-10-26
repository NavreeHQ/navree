//! Component schemas: hoisting, name conflicts and inlining.

use navis_api::{ApiOptions, ModuleBuilderApiExt};
use navis_http::{ApiMeta, schema_fn};
use serde_json::json;
use utoipa::ToSchema;

use crate::support::{app_registering, app_with, boot_error, document, documented};

/// A second, different schema named `Owner`.
mod crm {
    use utoipa::ToSchema;

    #[derive(ToSchema)]
    #[allow(dead_code)]
    pub struct Owner {
        pub email: String,
    }

    #[derive(ToSchema)]
    #[allow(dead_code)]
    pub struct Contact {
        pub owner: Owner,
    }
}

#[derive(ToSchema)]
#[allow(dead_code)]
struct Page<T> {
    data: Vec<T>,
    total: u64,
}

#[derive(ToSchema)]
#[allow(dead_code)]
struct Note {
    body: String,
}

#[derive(ToSchema)]
#[allow(dead_code)]
struct Address {
    city: String,
}

#[derive(ToSchema)]
#[allow(dead_code)]
struct Company {
    address: Address,
}

fn responding(response: navis_http::SchemaFn) -> ApiMeta {
    ApiMeta {
        response: Some(response),
        ..ApiMeta::default()
    }
}

#[tokio::test]
async fn conflicting_referenced_schema_names_fail_boot() {
    let builder = app_with(
        ApiOptions::default(),
        vec![documented(
            "/contact",
            responding(schema_fn::<crm::Contact>),
        )],
    );
    let error = boot_error(builder).await;
    assert!(error.contains("`Owner`"), "{error}");
    assert!(
        error.contains("GET /api/extra/contact"),
        "names the route: {error}"
    );
    assert!(
        error.contains("POST /api/v1/deals"),
        "and the route that registered it first: {error}"
    );
    assert!(error.contains("#[schema(as"), "says how to fix it: {error}");
}

#[tokio::test]
async fn same_named_body_schemas_are_inlined() {
    let app = app_registering(
        vec![
            documented(
                "/deals",
                responding(schema_fn::<Page<crate::support::Deal>>),
            ),
            documented("/notes", responding(schema_fn::<Page<Note>>)),
        ],
        |m| {
            m.api_schema::<Note>();
        },
    )
    .boot()
    .await
    .expect("same-named body schemas do not fail the boot");
    let doc = document(&app).await;
    let schema = |path: &str| {
        doc["paths"][path]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
            .clone()
    };

    assert_eq!(
        schema("/api/extra/deals"),
        json!({"$ref": "#/components/schemas/Page"})
    );
    let notes = schema("/api/extra/notes");
    assert!(notes.get("$ref").is_none(), "inlined: {notes}");
    assert_eq!(
        notes["properties"]["data"]["items"]["$ref"],
        "#/components/schemas/Note"
    );
    assert_eq!(
        doc["components"]["schemas"]["Page"]["properties"]["data"]["items"]["$ref"],
        "#/components/schemas/Deal"
    );
    assert!(doc["components"]["schemas"].get("Note").is_some());
}

#[tokio::test]
async fn unregistered_referenced_schemas_fail_boot() {
    // utoipa does not collect the schemas of generic type arguments, so
    // `Page<Note>` refers to a `Note` component nothing registered.
    let builder = app_with(
        ApiOptions::default(),
        vec![documented("/notes", responding(schema_fn::<Page<Note>>))],
    );
    let error = boot_error(builder).await;
    assert!(error.contains("`Note`"), "{error}");
    assert!(
        error.contains("GET /api/extra/notes"),
        "names the route: {error}"
    );
    assert!(
        error.contains("api_schema::<Note>()"),
        "says how to fix it: {error}"
    );
}

#[tokio::test]
async fn api_schema_registers_components() {
    let app = app_registering(vec![], |m| {
        m.api_schema::<Company>();
    })
    .boot()
    .await
    .unwrap();
    let doc = document(&app).await;
    let schemas = &doc["components"]["schemas"];
    assert_eq!(
        schemas["Company"]["properties"]["address"]["$ref"],
        "#/components/schemas/Address"
    );
    assert_eq!(schemas["Address"]["properties"]["city"]["type"], "string");
}
