---
name: navree-validation
description: "Validates request input in Navree with `#[derive(Validate)]` DTOs and the `Valid` and `ValidAsync` extractors, including Navree's extra rules, required fields, nested data, database `unique`/`exists` rules, international input (E.164 `Phone`, sanitized `SafeHtml` rich text, names in any script, native digits with `Folded`, `Country`, `Locale`), custom rules and messages, manual `ValidationErrors` and the 400 response shape. Use when writing a request body or query DTO, adding or customizing validation rules or messages, accepting phone numbers, rich text, non-Latin names or digits, checking uniqueness against the database, or when a test expects a 400 with `errors`."
---

# Validation

Navree validates input before the handler runs. Describe the rules on a DTO
with `#[derive(Validate)]`, take `Valid<Json<T>>` (or `Valid<Query<T>>`, `Valid<Form<T>>`)
in the handler, and invalid input becomes a 400 with one message per broken
rule. The DTO lives in the module's `dto.rs`.

## Steps

1. Write the DTO: `#[derive(Debug, Deserialize, Validate)]`, and a
   `#[validate(..)]` attribute on **every** field (`#[validate(skip)]` when it has
   no rule; a field without one is a compile error).
2. Take it with `Valid(Json(input)): Valid<Json<CreateContact>>` as the
   handler's last argument (`Valid(Query(f)): Valid<Query<T>>` for query
   strings, any position; `Valid(Form(f)): Valid<Form<T>>` for forms, with
   `Form` from `navree::http`, as the last argument).
3. For database checks, implement `AsyncRules` and take `ValidAsync<..>`.
4. Test the failure with `.assert_validation_error("field")`.

The derive comes with `navree` (`navree::validation::Validate`, also in the
prelude); the app needs no other dependency for it, and every rule is
always available.

## Template

```rust
use navree::validation::{Validate, rules};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize, Validate)]
pub struct CreateContact {
    #[validate(length(min = 1, max = 100))]        // characters; `length(bytes, ..)` counts bytes
    pub first_name: String,
    #[validate(email)]
    pub email: String,
    #[validate(range(min = 16, max = 120))]        // rules on an Option run only when Some
    pub age: Option<u8>,
    #[validate(length(max = 10), each(length(min = 1, max = 30)))]
    pub tags: Vec<String>,
    #[validate(custom(rules::one_of(&["lead", "customer", "churned"])))]
    pub status: String,
    #[validate(skip)]
    pub stage: Stage,                               // an enum: serde rejects unknown values
    #[validate(nested)]
    pub address: Option<Address>,                   // nested: errors at `address.city`
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Lead,
    Won,
}

#[derive(Debug, Deserialize, Validate)]
pub struct Address {
    #[validate(length(min = 1))]
    pub city: String,
}

/// A PATCH body: every field optional, `required` where a value is mandatory.
#[derive(Debug, Deserialize, Validate)]
pub struct CreateDeal {
    #[validate(required, length(max = 255))]
    pub title: Option<String>,
    #[validate(required)]
    pub company_id: Option<Uuid>,
    #[validate(required, range(min = 0))]
    pub amount_cents: Option<i64>,
}
```

```rust,ignore
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
  "code": "validation_failed",
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

- `#[validate(..)]`: `length(min, max)` (characters of text, items of a
  collection; `length(bytes, ..)` counts bytes), `range`, `required`, `email`,
  `url`, `pattern(r"..")` (checked at compile time), `ascii`, `alphanumeric`,
  `contains("x")`, `starts_with("x")`, `ends_with("x")`, `ip`, `ipv4`, `ipv6`,
  `credit_card`, `same_as(other)`, `nested`, `each(..)` (rules on every item
  of a list, set or map), `custom(f)`, `skip`. Several rules run in the order
  written; rules on an `Option` pass on `None`.
- `#[validate(skip_unmarked)]` on the struct lets fields without
  `#[validate]` go unchecked. Paths follow serde's `rename` / `rename_all`;
  a `#[serde(flatten)]` field with `nested` reports at the parent's path;
  `#[validate(rename = "..")]` on a field sets its name in paths.
- Each rule once per field (only `custom` may repeat); `length` and `range`
  need `min` or `max`. `length(chars, ..)` is an error (`length` counts
  characters already) and there is no `equal`: write `length(min = n, max = n)`.
  `length(bytes, ..)` still words its message in "characters".
- The derive also takes enums and tuple structs (a newtype's field reports at
  the value's own path), and `#[validate(crate = "path")]` names the
  validation crate when neither `navree` nor `navree-validation` is a direct
  dependency.
- `navree::validation::rules` (use with `custom(..)`; `None` passes):
  `rules::one_of(&[..])`, `rules::uuid`, `rules::date_ymd`,
  `rules::unicode_name`, `rules::country`, `rules::locale`, `rules::phone`
  (see International input below).
- `navree::validation::confirmed`:
  `#[validate(length(min = 8), custom(confirmed(&self.password_confirmation)))]`
  with `#[validate(skip)] pub password_confirmation: String`.
- serde errors come first and one at a time (a missing required field:
  `title should not be empty`). To report every missing field at once, make
  the fields `Option` with `required`, as `CreateDeal` above.
- `#[serde(deny_unknown_fields)]` turns unknown keys into
  `property x should not exist`.

## International input

Names in Arabic or Devanagari, codes typed with Arabic-Indic digits, phone
numbers in any format, rich text from an editor: use these types and rules,
which normalize the value while the extractor deserializes it.

```rust
use navree::validation::{rules, Country, Folded, Locale, Phone, SafeHtml, Validate};
use serde::Deserialize;

#[derive(Debug, Deserialize, Validate)]
pub struct CreateContact {
    #[validate(length(min = 1, max = 100), custom(rules::unicode_name))]
    pub name: String,          // "हरप्रीत कौर", "مُحَمَّد" pass; "<b>", "😀", "١٢٣" fail
    #[validate(skip)]
    pub phone: Phone,          // "+٩٧١ ٥٠ ١٢٣ ٤٥٦٧" -> "+971501234567" (E.164)
    #[validate(skip)]
    pub country: Country,      // "gb" -> "GB" (ISO 3166-1 alpha-2)
    #[validate(skip)]
    pub locale: Option<Locale>, // "en-gb" -> "en-GB" (BCP 47)
    #[validate(length(min = 6, max = 6), pattern(r"^[0-9]+$"))]
    pub code: Folded<String>,  // "٦٥٤٣٢١" -> "654321" before the rules run
    #[validate(length(max = 20_000))]
    pub notes: SafeHtml,       // sanitized: no script, on* handler or javascript: link
}
```

- A number without a country code (`07400 123456`) needs a default region:
  `[validation.phone] default_region = "GB"`, or per request a
  `RegionSource` registered with `m.region_source::<T>()` (the tenant's or
  the user's country). Without either it is `phone must be a valid phone
  number`. Store `phone.as_str()` (E.164); `extension()` is not part of it.
- `Folded<u32>` (any `T: FromStr`) parses native digits and JSON numbers;
  `fold_digits(text)` folds any string.
- `SafeHtml` removes what the policy doesn't allow, and rules measure the
  sanitized HTML. It fails on a string only over a limit, a 400 on the field:
  `validation.html.max_input_bytes` (512 KiB, checked first), `max_nodes`
  (131,072: elements, text runs, attributes, every 32 bytes of attributes),
  `max_depth` (128, at most 256, elements only; browsers read past 512
  flat), and 256 attributes on a tag; a limit of 0 fails the boot. At the
  limit a parse takes at most about 0.2 s. The extractors read a body of
  16 KiB or more on the blocking pool, whatever fields it holds; the
  functions that sanitize work on the calling thread, so in async code call
  them from `spawn_blocking` when the text is long. `SafeHtml::new`, `with_policy` and
  `HtmlPolicy::clean` can't fail (over a limit: the input, escaped);
  `try_with_policy` and `try_clean` return `HtmlError`. The policy is
  `[validation.html]` (`tags`, `generic_attributes`, `tag_attributes`,
  `url_schemes`, `link_rel`); allowing `script`, `style`, `on*`, `srcdoc` or
  `http-equiv` fails the boot, and so does allowing an attribute the page acts
  on: `data-ui-*`, `data-genui*`, `form`, `formaction`, `formmethod`,
  `formenctype`, `formtarget`, `popover`, `popovertarget`,
  `popovertargetaction`, `command`, `commandfor`, `autofocus`, `accesskey` or
  `contenteditable` (the sanitizer drops them from rich text whatever the
  policy says; `id` and `name` may be allowed). A URL with a host and no scheme (`//x`) stays
  only when `http` and `https` are both allowed.
  The output is parsed again and returned only when a browser would build
  the tree that was checked, else only the text is kept; SVG animation
  elements are always removed.
- To check a `String` and keep it as sent: `custom(rules::phone)`,
  `custom(rules::country)`, `custom(rules::locale)`.
- `Phone` needs the facade feature `validation-phone`, `SafeHtml`
  `validation-html` (both in `full`).
- Message keys: `phone`, `country`, `locale`, `unicode_name`.
- Outside the extractors (a job), deserialize inside the app's
  `InputContext` (`ctx.get::<InputContext>().await?.scope(|| ..)`), or the
  defaults apply (no default region, the default HTML policy).
- In a `ToSchema` DTO, mark these fields `#[schema(value_type = String)]`.

## Custom rules

```rust
use navree::validation::{rule_error, Violation};

// A function of the value: `Result<(), E>` with `E: Into<Violation>`.
pub fn not_blank(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() {
        Err(":attribute must not be blank") // a text (`&str` or `String`) is the message
    } else {
        Ok(())
    }
}

pub fn even(value: &u32) -> Result<(), Violation> {
    if value.is_multiple_of(2) {
        Ok(())
    } else {
        Err(rule_error("even", &[("value", value)])) // message from the table, key `even`
    }
}
```

`Violation::rule("even").with("value", value)` is the same as `rule_error`;
`Violation::text(..)` wraps a text. To take what Navree's rules take
(`String`, `&str`, `Cow<str>`, `Folded` text, `Option`s; `None` passes), be
generic over `rules::RuleInput` and read `value.rule_str()`. A `custom`
expression can read `self`
(`custom(confirmed(&self.password_confirmation))`). There is no context
parameter: request data comes from the `InputContext` (default phone region,
HTML policy), and rules that need the database are async rules.

`value.validate()` returns `Result<(), Violations>` (each `Violation` has
`key()`, `params()` and `message()` with the built-in template;
`Violations` prints one message per line with the paths). For the app's
messages as `ValidationErrors`, use `validate_in(&value, Some(&ctx)).await`.

## Database rules

```rust,ignore
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

Async rules run after every `#[validate]` rule passes, inside the request's unit of
work (the app needs `DatabaseModule`). Table and column names must match
`[a-z_][a-z0-9_]*`; never build them from input. A rule compares one column
with one value: it can't add `deleted_at IS NULL`, so check soft-deleted
references in the service, or build the rule on the model with
`navree::orm::validation::{unique, exists}` (scopes, `where_(..)`,
soft-delete handling), or wrap your own queries in `AsyncRule::check(field,
rule, UowCheck)`. Outside a request: `rule.passes(&executor).await`
on a `Db`, `Transaction` or `Uow` (any navree-db `Executor`); a failed query
is `RuleError::Database` holding a boxed `DbError`.

## Validating in a service

```rust
use navree::http::Result;
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

```rust,ignore
app.acting_as(principal(Uuid::now_v7()))
    .post("/api/v1/contacts")
    .json(&json!({ "first_name": "", "email": "anu@", "tags": [], "status": "lead", "stage": "lead" }))
    .send()
    .await
    .assert_validation_error("first_name")
    .assert_validation_error("email")
    .assert_error(400, "email must be a valid email address");
```

## Common mistakes

- A field with no `#[validate(..)]` attribute (compile error): add `skip`.
- Old spellings (`dive`, `inner`, `prefix`, `suffix`, `matches`, `length(chars, ..)`,
  `allow_unvalidated`): the compile error names the new one (`nested`, `each`,
  `starts_with`, `ends_with`, `same_as`, `length(..)`, `skip_unmarked`).
- Storing rich text as a `String` and rendering it as HTML (stored XSS): use `SafeHtml`.
- A `String` phone field compared or deduplicated as typed: use `Phone` and store E.164.
- `pattern(r"^[0-9]+$")` on a code typed with Arabic-Indic digits: wrap the field in `Folded`.
- An ASCII-only name rule (`ascii`, `[A-Za-z]`): use `rules::unicode_name`.
- Taking `Json<T>` without `Valid`: nothing is checked.
- Putting `Valid<Json<T>>` before another extractor: the body extractor must be last.
- Expecting a 422: Navree answers every input error with 400.
- Checking uniqueness in the handler and racing: keep the `unique` index in
  the migration too; a violation at insert becomes a 409.

## Docs

- https://navree.com/docs/basics/validation/
- https://navree.com/docs/basics/errors/
