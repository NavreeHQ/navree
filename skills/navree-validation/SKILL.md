---
name: navree-validation
description: "Validates request input in Navree with garde DTOs and the `Valid` and `ValidAsync` extractors, including Navree's extra rules, required fields, nested data, database `unique`/`exists` rules, custom rules and messages, manual `ValidationErrors` and the 400 response shape. Use when writing a request body or query DTO, adding or customizing validation rules or messages, checking uniqueness against the database, or when a test expects a 400 with `errors`."
---

# Validation

Navree validates input before the handler runs. Describe the rules on a DTO
with garde, take `Valid<Json<T>>` (or `Valid<Query<T>>`, `Valid<Form<T>>`)
in the handler, and invalid input becomes a 400 with one message per broken
rule. The DTO lives in the module's `dto.rs`.

## Steps

1. Write the DTO: `#[derive(Debug, Deserialize, Validate)]`, and a
   `#[garde(..)]` attribute on **every** field (`#[garde(skip)]` when it has
   no rule; a field without one is a compile error).
2. Take it with `Valid(Json(input)): Valid<Json<CreateContact>>` as the
   handler's last argument (`Valid(Query(f)): Valid<Query<T>>` for query
   strings, any position).
3. For database checks, implement `AsyncRules` and take `ValidAsync<..>`.
4. Test the failure with `.assert_validation_error("field")`.

The app depends on `garde` directly (`navree new` adds
`garde = { version = "0.23", features = ["derive", "email"] }`); add the
`url` or `regex` features for those rules.

## Template

```rust
use garde::Validate;
use navree::validation::rules;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize, Validate)]
pub struct CreateContact {
    #[garde(length(chars, min = 1, max = 100))]    // `chars` counts characters, not bytes
    pub first_name: String,
    #[garde(email)]
    pub email: String,
    #[garde(range(min = 16, max = 120))]           // rules on an Option run only when Some
    pub age: Option<u8>,
    #[garde(length(max = 10), inner(length(min = 1, max = 30)))]
    pub tags: Vec<String>,
    #[garde(custom(rules::one_of(&["lead", "customer", "churned"])))]
    pub status: String,
    #[garde(skip)]
    pub stage: Stage,                              // an enum: serde rejects unknown values
    #[garde(dive)]
    pub address: Option<Address>,                  // nested: errors at `address.city`
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Lead,
    Won,
}

#[derive(Debug, Deserialize, Validate)]
pub struct Address {
    #[garde(length(min = 1))]
    pub city: String,
}

/// A PATCH body: every field optional, `required` where a value is mandatory.
#[derive(Debug, Deserialize, Validate)]
pub struct CreateDeal {
    #[garde(required, length(chars, max = 255))]
    pub title: Option<String>,
    #[garde(required)]
    pub company_id: Option<Uuid>,
    #[garde(required, range(min = 0))]
    pub amount_cents: Option<i64>,
}
```

```rust
#[post("/")]
#[authenticated]
async fn store(&self, Valid(Json(input)): Valid<Json<CreateContact>>) -> Created<Value> { /* ... */ }
```

## The response

```json
{
  "statusCode": 400,
  "message": ["age must not be less than 16", "email must be a valid email address"],
  "error": "Bad Request",
  "errors": {
    "age": ["age must not be less than 16"],
    "email": ["email must be a valid email address"]
  }
}
```

`message` lists every message, sorted by field path; `errors` maps each
path (`address.city`, `tags.1`) to its messages. Malformed JSON is
`body must be a valid JSON string`; a wrong `Content-Type` is a 415.

## Rules

- garde: `length(min, max)` (bytes; add `chars`), `range`, `required`,
  `email`, `url`, `pattern(r"..")`, `ascii`, `alphanumeric`, `contains`,
  `prefix`, `suffix`, `ip`, `matches(other)`, `dive`, `inner(..)`,
  `custom(f)`, `skip`.
- `navree::validation::rules` (use with `custom(..)`; `None` passes):
  `rules::one_of(&[..])`, `rules::uuid`, `rules::date_ymd`.
- `navree::validation::confirmed`:
  `#[garde(length(min = 8), custom(confirmed(&self.password_confirmation)))]`
  with `#[garde(skip)] pub password_confirmation: String`.
- serde errors come first and one at a time (a missing required field:
  `title should not be empty`). To report every missing field at once, make
  the fields `Option` with `required`, as `CreateDeal` above.
- `#[serde(deny_unknown_fields)]` turns unknown keys into
  `property x should not exist`.

## Custom rules

```rust
use navree::validation::rule_error;

pub fn not_blank(value: &str, _ctx: &()) -> garde::Result {
    if value.trim().is_empty() {
        Err(garde::Error::new(":attribute must not be blank"))
    } else {
        Ok(())
    }
}

pub fn even(value: &u32, _ctx: &()) -> garde::Result {
    if value.is_multiple_of(2) {
        Ok(())
    } else {
        Err(rule_error("even", &[("value", value)])) // message from the table, key `even`
    }
}
```

The extractors need `Validate<Context = ()>`: garde's `#[garde(context(..))]`
isn't supported. Rules that need the database are async rules.

## Database rules

```rust
use navree::validation::{AsyncRule, AsyncRules};

impl AsyncRules for UpdateUser {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![
            AsyncRule::unique("email", "users", "email", self.email.clone(), None)
                .ignore_path_param("id"),                       // ignore the row being updated
            AsyncRule::exists("team_id", "teams", "id", self.team_id), // None passes
        ]
    }
}

#[patch("/{id}")]
#[authenticated]
async fn update(&self, Path(id): Path<Uuid>, ValidAsync(Json(input)): ValidAsync<Json<UpdateUser>>) -> Result<Json<Value>> { /* ... */ }
```

Async rules run after every garde rule passes, inside the request's unit of
work (the app needs `DatabaseModule`). Table and column names must match
`[a-z_][a-z0-9_]*`; never build them from input. A rule compares one column
with one value: it can't add `deleted_at IS NULL`, so check soft-deleted
references in the service.

## Validating in a service

```rust
use navree::validation::ValidationErrors;

pub fn check_dates(starts_on: &str, ends_on: &str) -> Result<()> {
    let mut errors = ValidationErrors::new();
    if ends_on < starts_on {
        errors.add("ends_on", "ends_on must be after starts_on");
    }
    if !errors.is_empty() {
        return Err(errors.into_app_error()); // the same 400 shape
    }
    Ok(())
}
```

`AppError::validation(map)` builds the same response from a
`BTreeMap<String, Vec<String>>`.

## Messages

Override the built-in messages (such as `:attribute should
not be empty`) per rule or per field, and name fields, in `config/app.toml`:

```toml
[validation.messages]
required = ":Attribute is required."
"email.unique" = "This e-mail is already registered."

[validation.attributes]
first_name = "first name"
```

`ValidationModule::from_config()` (in a new app) reads this section. The
full key table is `navree::validation::DEFAULT_MESSAGES`. There is one
message table per app, in English.

## Test

```rust
app.acting_as(principal(Uuid::now_v7()))
    .post("/api/v1/contacts")
    .json(&json!({ "first_name": "", "email": "ada@", "tags": [], "status": "lead", "stage": "lead" }))
    .send()
    .await
    .assert_validation_error("first_name")
    .assert_validation_error("email")
    .assert_error(400, "email must be a valid email address");
```

## Common mistakes

- A field with no `#[garde(..)]` attribute (compile error): add `skip`.
- `length(max = 255)` on text with accents counts bytes: use `length(chars, max = 255)`.
- Taking `Json<T>` without `Valid`: nothing is checked.
- Putting `Valid<Json<T>>` before another extractor: the body extractor must be last.
- Expecting a 422: Navree answers every input error with 400.
- Checking uniqueness in the handler and racing: keep the `unique` index in
  the migration too; a violation at insert becomes a 409.

## Docs

- https://navree.com/docs/basics/validation/
- https://navree.com/docs/basics/errors/
