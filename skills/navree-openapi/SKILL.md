---
name: navree-openapi
description: "Documents a Navree API with the OpenAPI 3.1 document generated at boot: `ApiModule`, `#[api(..)]` on routes, `ToSchema` resources, page schemas, `m.api_schema`, the schema boot errors, `openapi:export` and typed clients. Use when adding API documentation, describing request or response bodies, fixing a schema-conflict or missing-schema boot error, exporting the document, or generating a typed TypeScript client."
---

# OpenAPI

`ApiModule` (in a new app) builds an OpenAPI 3.1 document from every route
when the app boots: paths with prefix and version, path parameters,
security (bearer) and the access-related error responses come for free.
`#[api(..)]` adds summaries, tags and bodies. It is served at
`GET /api/openapi.json` with an API reference page at `GET /api/docs`.

## Steps

1. Derive `ToSchema` (`navree::openapi::ToSchema`, also in the prelude) on
   request DTOs and response resources; name components with
   `#[schema(as = Note)]`.
2. Put `#[api(..)]` (and `#[name("notes.store")]` for an `operationId`) on
   each route.
3. Check `/api/docs`, then export: `navree openapi:export web/openapi.json`.
4. Add a test that fetches the document and checks the route's schema.

## Template

```rust
use chrono::{DateTime, Utc};
use navree::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A note as the API returns it.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = Note)]
pub struct NoteResource {
    /// Always `"Note"` (the subject type front ends check permissions with).
    #[serde(rename = "__type")]
    pub resource_type: &'static str,
    pub id: Uuid,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

/// What `POST /api/v1/notes` takes.
#[derive(Debug, Deserialize, navree::validation::Validate, ToSchema)]
pub struct CreateNote {
    /// 1 to 2000 characters.
    #[validate(length(min = 1, max = 2000))]
    pub body: String,
}

#[injectable]
pub struct NotesController;

#[controller("/notes", version = "1")]
#[authenticated]
impl NotesController {
    #[post("/")]
    #[name("notes.store")]
    #[api(
        summary = "Create a note",
        tag = "Notes",
        request = CreateNote,
        response = NoteResource,
        status = 201,
        responses(400, 401)
    )]
    async fn store(&self, Valid(Json(input)): Valid<Json<CreateNote>>) -> Result<Created<NoteResource>> {
        let note = NoteResource {
            resource_type: "Note",
            id: Uuid::now_v7(),
            body: input.body,
            created_at: Utc::now(),
        };
        Ok(Created(format!("/api/v1/notes/{}", note.id), note))
    }
}
```

| `#[api]` key | Documents |
|---|---|
| `summary = ".."`, `description = ".."` | one-line summary, Markdown description |
| `tag = ".."` or `tags("A", "B")` | groups in the UI |
| `request = T` | the JSON request body |
| `response = T`, `status = 201` | the success body and status (default 200) |
| `responses(400, 404)` | extra error statuses, with the error schema `HttpError` |

Hand-written routes use `RouteDef::api(ApiMeta { .. })` with
`request`/`response` as `Some(schema_fn::<T>)` (`navree::http`).

## Pages

`Page<T>` has no `ToSchema`. Keep returning `Page<T>` from the handler, and
document a named page type:

```rust
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PageInfo {
    pub current_page: u64,
    pub per_page: u64,
    pub total: u64,
    pub last_page: u64,
}

/// A page of notes: `{data, meta}`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct NotePage {
    pub data: Vec<NoteResource>,
    #[schema(inline)]
    pub meta: PageInfo,
}
// #[api(response = NotePage, ..)] on the list route
```

## Boot errors

- Generic types are named after their arguments (`Page<Note>` is `Page_Note`).
  `skip_serializing` is `writeOnly`, `skip_deserializing` is `readOnly`, names
  that differ by direction are two properties, `transparent` is its field.
  `with`, `serialize_with`, `from`, `into` and `remote` need `value_type`,
  `schema_with` or `impl ToSchema` by hand: the derive refuses to guess.
- `ApiError::SchemaConflict`: two different types with one name; the error names both. Rename
  one with `#[schema(as = OtherName)]`.
- `ApiError::MissingSchemas`: a schema refers to a component nothing
  registered (typical with generic wrappers). Register it in the module's
  `wire`: `m.api_schema::<NoteResource>()`. Also use `m.api_schema::<T>()`
  for query DTOs so their fields appear among the schemas.

## Configuration

```toml
[api]
title = "My App API"
version = "1.0.0"                     # your API's version
servers = ["https://api.example.com"]
docs_ui = true                        # false removes /api/docs and its embedded stylesheet and script (/api/docs/assets/)
openapi_json = true                   # false removes /api/openapi.json; openapi:export still writes it. Both are public by default
```

The page's CSP allows styles and scripts from the app only, nothing inline,
and images from the app (`'self'`) and `data:` URIs only: a remote image in a
description shows its alt text, so serve it from the app or inline it as a
`data:` URI. `connect-src` adds the absolute `api.servers` URLs.

## Export and typed clients

```sh
navree openapi:client                        # both steps: web/src/lib/api/openapi.json and schema.d.ts
navree openapi:client --out web/app/lib/api  # another folder (a Nuxt app)

navree openapi:export web/openapi.json       # only the document: exactly what /api/openapi.json serves
navree openapi:client --input web/openapi.json --out web/lib/api   # only the schema, from a document you have
```

Re-export after changing routes or types; commit the files if the front end
builds from them. The schema's `paths` type goes to
`createNavreeClient<paths>()` from `@navree/client` (see the
`navree-frontend-next` and `navree-frontend-nuxt` skills and
`/docs/frontend/typed-client/`).

## Limits to know

- Query parameters aren't documented (describe them in `description`;
  register the filter DTO with `m.api_schema`).
- Bodies are JSON only (no multipart, no CSV schemas); path parameters are
  always strings.
- `/api/me/abilities`, `/api/access/*`, `/broadcasting/auth`, `/health/*`
  and `/metrics` have no schemas; `UserResource` extension fields aren't shown.

## Test

```rust
#[tokio::test]
async fn notes_are_documented() {
    let app = TestApp::builder()
        .module(ApiModule::from_config())
        .module(NotesModule)
        .build()
        .await;
    let document = app.client().get("/api/openapi.json").send().await.assert_status(200).json_value();
    let store = &document["paths"]["/api/v1/notes"]["post"];
    assert_eq!(store["operationId"], "notes.store");
    assert_eq!(
        store["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
        json!("#/components/schemas/Note")
    );
    app.shutdown().await.unwrap();
}
```

## Docs

- https://navree.com/docs/digging-deeper/openapi/
- https://navree.com/docs/basics/responses/
