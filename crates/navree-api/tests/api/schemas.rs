//! Component schemas: hoisting, name conflicts and inlining.

use navree_api::{ApiOptions, ModuleBuilderApiExt};
use navree_http::{ApiMeta, schema_fn};
use navree_openapi::ToSchema;
use serde_json::json;

use crate::support::{app_registering, app_with, boot_error, document, documented};

/// A second, different schema named `Owner`.
mod crm {
    use navree_openapi::ToSchema;

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

fn responding(response: navree_http::SchemaFn) -> ApiMeta {
    ApiMeta {
        response: Some(response),
        ..ApiMeta::default()
    }
}

/// Two types of one name with the same schema by text, whose `child` differs.
mod alike {
    use navree_openapi::ToSchema;

    pub mod first {
        use super::ToSchema;

        #[derive(ToSchema)]
        #[allow(dead_code)]
        pub struct Child {
            pub in_first: i32,
        }

        #[derive(ToSchema)]
        #[allow(dead_code)]
        pub struct Item {
            pub child: Child,
        }
    }

    pub mod second {
        use super::ToSchema;

        #[derive(ToSchema)]
        #[allow(dead_code)]
        pub struct Child {
            pub in_second: String,
        }

        #[derive(ToSchema)]
        #[allow(dead_code)]
        pub struct Item {
            pub child: Child,
        }
    }

    #[derive(ToSchema)]
    #[allow(dead_code)]
    pub struct Both {
        pub first: first::Item,
        pub second: second::Item,
    }
}

#[tokio::test]
async fn a_conflict_below_two_types_that_look_the_same_fails_boot() {
    let builder = app_with(
        ApiOptions::default(),
        vec![documented("/both", responding(schema_fn::<alike::Both>))],
    );
    let error = boot_error(builder).await;
    assert!(error.contains("`Child`"), "{error}");
    assert!(error.contains("first::Child"), "names one type: {error}");
    assert!(error.contains("second::Child"), "and the other: {error}");
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

mod first {
    use navree_openapi::ToSchema;

    #[derive(ToSchema)]
    #[allow(dead_code)]
    pub struct Item {
        pub in_first: String,
    }
}

mod second {
    use navree_openapi::ToSchema;

    #[derive(ToSchema)]
    #[allow(dead_code)]
    pub struct Item {
        pub in_second: String,
    }
}

#[derive(ToSchema)]
#[allow(dead_code)]
struct Both {
    a: first::Item,
    b: second::Item,
}

#[tokio::test]
async fn two_types_of_one_name_in_one_schema_refuse_the_boot_and_are_named() {
    let builder = app_with(
        ApiOptions::default(),
        vec![documented("/both", responding(schema_fn::<Both>))],
    );
    let error = boot_error(builder).await;
    assert!(error.contains("first::Item"), "{error}");
    assert!(error.contains("second::Item"), "{error}");
    assert!(error.contains("GET /api/extra/both"), "{error}");
    assert!(error.contains("#[schema(as"), "{error}");
}

#[tokio::test]
async fn two_body_types_of_one_name_refuse_the_boot_and_are_named() {
    let builder = app_with(
        ApiOptions::default(),
        vec![
            documented("/first", responding(schema_fn::<first::Item>)),
            documented("/second", responding(schema_fn::<second::Item>)),
        ],
    );
    let error = boot_error(builder).await;
    assert!(error.contains("first::Item"), "{error}");
    assert!(error.contains("second::Item"), "{error}");
}

#[derive(ToSchema)]
#[allow(dead_code)]
struct Pages {
    notes: Page<Note>,
    deals: Page<crate::support::Deal>,
}

#[tokio::test]
async fn pages_of_two_types_in_one_schema_are_two_components() {
    let app = app_registering(
        vec![documented("/pages", responding(schema_fn::<Pages>))],
        |m| {
            m.api_schema::<Note>();
        },
    )
    .boot()
    .await
    .unwrap();
    let doc = document(&app).await;
    let schemas = &doc["components"]["schemas"];
    assert_eq!(
        schemas["Pages"]["properties"]["notes"]["$ref"],
        "#/components/schemas/Page_Note"
    );
    assert_eq!(
        schemas["Page_Note"]["properties"]["data"]["items"]["$ref"],
        "#/components/schemas/Note"
    );
    assert_eq!(
        schemas["Page_Deal"]["properties"]["data"]["items"]["$ref"],
        "#/components/schemas/Deal"
    );
}

#[tokio::test]
async fn generic_bodies_are_a_component_each() {
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
    .expect("pages of two types do not share a name");
    let doc = document(&app).await;
    let schema = |path: &str| {
        doc["paths"][path]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
            .clone()
    };

    assert_eq!(
        schema("/api/extra/deals"),
        json!({"$ref": "#/components/schemas/Page_Deal"})
    );
    assert_eq!(
        schema("/api/extra/notes"),
        json!({"$ref": "#/components/schemas/Page_Note"})
    );
    assert_eq!(
        doc["components"]["schemas"]["Page_Note"]["properties"]["data"]["items"]["$ref"],
        "#/components/schemas/Note"
    );
    assert_eq!(
        doc["components"]["schemas"]["Page_Deal"]["properties"]["data"]["items"]["$ref"],
        "#/components/schemas/Deal"
    );
    assert!(doc["components"]["schemas"].get("Note").is_some());
}

#[tokio::test]
async fn unregistered_referenced_schemas_fail_boot() {
    // The derive does not collect the schemas of generic type arguments, so
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
