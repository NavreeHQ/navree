---
name: navree-i18n
description: "Localization with navree-i18n: the opt-in `i18n` facade feature, `I18nModule` and the `i18n` config section (`default_locale`, `locales`, `dir`, `negotiation`, `query_param`, `cookie`, `missing`, `reload`, `bidi_isolation`, `routes`), Fluent files in `locales/<locale>/*.ftl` (plurals, variables, terms, attributes, `NUMBER()`, `DATETIME()`), `embed_locales!`, `t!` and `args!`, `Money` arguments, the `Locale` extractor and `Locale::scope`/`Locale::current`, the `I18n` service (`t`, `has`, `locales`, saved user and workspace locales), negotiation from the query, the user, the workspace, a cookie and `Accept-Language`, `PUT /api/me/locale` and `GET /api/i18n/locales`, translated validation messages (`validation-<rule>`, `field-<path>`, seven shipped languages), `t()`/`locale()` and locale-aware `money`/`date` in templates, notifications in the recipient's locale, and `i18n:check`, `i18n:missing`, `i18n:publish validation`. Use when translating an app or a message, adding a locale, formatting numbers, money or dates for a locale, choosing how a request's locale is picked, translating validation errors or mail, testing translated output, or debugging a message that shows its id, the wrong language, a boot failure naming an .ftl file, or a `t!` panic."
---

# Localization

`navree::i18n` (crate `navree-i18n`, facade feature `i18n`, **opt-in**: not
part of `full`, because it compiles in ICU4X's locale data) translates an app
with [Fluent](https://projectfluent.org) files. Everything below is in the
code: when in doubt read `crates/navree-i18n/src` and its tests in
`crates/navree-i18n/tests/` (`i18n/` for the module, `docs_examples.rs` for
the docs page's examples, fixtures in `tests/fixtures/locales`).

## Steps: localize an app

1. Turn the feature on:

```toml
[dependencies]
navree = { git = "https://github.com/NavreeHQ/navree", features = ["i18n"] }
```

   It brings `navree::i18n` (with `t!`, `args!`, `embed_locales!`) and puts
   `I18nModule` and `Locale` in the prelude.

2. Add the module to the app root, before `ValidationModule`, `ViewModule`
   and `MailModule`:

```rust,ignore
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(HttpModule::from_config())
    .module(I18nModule::from_config()) // section `i18n`; or I18nModule::for_root(I18nOptions { .. })
    .module(ValidationModule::from_config())
    .module(ViewModule::from_config())
    .module(MailModule::from_config())
```

   With `DatabaseModule` it saves user and workspace locales in
   `locale_preferences` (migration set `i18n`; run `navree migrate`): columns
   `scope_type` (`user` or `workspace`), `scope_id` (`uuid`; `BINARY(16)` on
   MySQL), `locale`, `updated_at`, key `(scope_type, scope_id)`. Write it
   through `I18n`; `navree::i18n::migration::LocalePreferenceRow` is its
   `navree::db::Record` for reads.

3. Configure the locales:

```toml
[i18n]
default_locale = "en"
locales = ["en", "fr", "de", "pt-BR"]   # env: APP_I18N__LOCALES=en,fr,de,pt-BR
dir = "locales"
negotiation = ["query", "user", "workspace", "cookie", "header"]  # default: user, workspace, header
missing = "warn"                        # warn | error | id
```

4. Write `locales/<locale>/*.ftl` (any file names, subdirectories allowed;
   one namespace per locale):

```text
deal-created = { $count ->
    [one] One deal created
   *[other] { $count } deals created
}
deal-won = { $owner } won { $title }.
deal-amount = { $amount }
deal-closes = Closes { DATETIME($date, dateStyle: "long") }
login = Log in
    .title = Sign in to your account
```

5. Translate:

```rust,ignore
use navree::i18n::{I18n, Locale, Money, args, t};

let text = t!("deal-created", count = 3);                     // current locale
let amount = t!("deal-amount", amount = Money::new(4_200_000, "EUR"));
let title = t!("login.title");                                // an attribute
let german = Locale::scope("de", async { t!("deal-won", owner = "Ada", title = "X") }).await;
let explicit = i18n.t("pt-BR", "deal-created", args!(count = 2))?; // Arc<I18n>
```

6. Run `navree i18n:check` (CI) and `navree i18n:missing fr` (for a
   translator).

## The current locale

- In a request: negotiated by the layer (slot `Context`, inside the unit of
  work) from `i18n.negotiation`, first supported match wins; handlers take
  `locale: Locale`; responses get `Content-Language`.
- Sources: `query` (`?locale=`, `i18n.query_param`), `user` (saved, needs a
  database and a signed-in user), `workspace` (the saved locale of the
  workspace the request names through the workspace resolvers, only for
  members), `cookie` (`i18n.cookie`), `header` (`Accept-Language` by `q`).
- Matching: exact tag, then a parent (`fr-CA` → `fr`), then the first
  supported locale of the language (`pt` → `pt-BR`). Unsupported values are
  skipped; nothing matches → `default_locale`.
- Outside a request (commands, jobs, tests) `Locale::current()` is the
  `default_locale` of the app booted last in the process. `Locale::scope(x,
  fut)` takes a `Locale` or a tag. `I18n::scope(tag, fut)` and
  `I18n::locale(tag)` bind to one app explicitly.

## Formatting

```rust,ignore
locale.format_money(4_200_000, "EUR");   // en "€42,000.00", fr "42 000,00 €" (U+202F, U+00A0)
locale.format_date(chrono::Utc::now());  // en "Oct 26, 2026", fr "26 oct. 2026"
locale.format_date_with(day, DateStyle::Long);
locale.format_number(1234.5);            // en "1,234.5", de "1.234,5"
```

In Fluent: numbers print with the locale's separators; `NUMBER($n, style:
"percent")`, `NUMBER($n, style: "currency", currency: "EUR")` (named
arguments are literals), `NUMBER($year, useGrouping: "false")`;
`DATETIME($date, dateStyle: "short|medium|long|full", timeStyle:
"short|medium")`. Arguments: integers, floats, strings, `bool`, `Money`,
`NaiveDate`, `NaiveDateTime`, `DateTime<Tz>`, `Option<_>`.

## Saved locales

```rust,ignore
i18n.set_user_locale(&uow, user_id, "fr").await?;          // stores the matched supported tag
i18n.set_workspace_locale(&uow, workspace_id, "de").await?;
let locale = i18n.user_locale(user_id).await?;              // saved, else default
```

Routes (when `i18n.routes`): `GET /api/i18n/locales` (public, `{data:
[{tag, name, direction, default}]}`) and `PUT /api/me/locale {"locale":
"fr"}` (204; 400 `errors.locale` when unsupported; mounted only with
`DatabaseModule` and an authenticator).

## Validation, templates, mail, notifications

- Validation: `validation-<rule with dots as hyphens>` (`validation-min-string`)
  with `$attribute` and the rule's variables; `field-<path>` names a field
  (`field-first_name`, `field-address-city`). Order: the app's message for
  the locale, then (in the default locale's language) `validation.messages`,
  then the shipped message (en, fr, de, es, it, nl, pt-BR; every key of
  `DEFAULT_MESSAGES`, including `phone`, `country`, `locale` and
  `unicode_name`), then
  `validation.messages`, then the default locale's, then the English
  template. `navree i18n:publish validation [--force]` copies the shipped
  files into `locales/`.
- Templates: `{{ t("deal-won", owner=o.name, title=d.title) }}`,
  `<html lang="{{ locale() }}">`, `{{ cents | money("EUR") }}`,
  `{{ value | date }}` (locale medium) or `| date("%d/%m/%Y")`.
- Mail: rendered when sent or queued, in the current locale; build the
  subject with `t!` there.
- Notifications to users render in the user's saved locale (else default),
  including queued mail at delivery.
- Your own jobs run in `default_locale`: put `Locale::current().tag()` in the
  job and `Locale::scope(self.locale, ..)` in `handle`.

## Testing templates

```rust,ignore
let app = TestApp::builder()
    .config("i18n.locales", json!(["en", "fr"]))
    .config("i18n.missing", "error")          // a missing message fails (t! panics)
    .module(I18nModule::from_config())
    .module(ValidationModule::from_config())
    .module(ContactsModule)
    .build()
    .await;
app.client()
    .post("/api/v1/contacts")
    .header("Accept-Language", "fr")
    .json(&json!({ "last_name": "Lovelace" }))
    .send()
    .await
    .assert_json_path("errors.first_name.0", json!("prénom ne doit pas être vide"));

let i18n = app.get::<I18n>().await;
let text = i18n.scope("fr", async { t!("deal-created", count = 3) }).await;
```

Unit tests without an app: `I18n::new(I18nOptions { locales, dir, .. })?`
then `Locale::scope(i18n.locale("fr"), async { .. })`.

## Checks before you finish

- `navree i18n:check` exits 0 (syntax errors, duplicates and messages missing
  from non-regional locales fail it).
- Tests run with `i18n.missing = "error"`.
- Every new message exists in `default_locale`.
- Handlers that show text take `Locale` or call `t!` inside the request.

## Gotchas

- The feature is opt-in: `features = ["i18n"]`, or `navree::i18n` does not
  exist.
- A syntax error or a message defined twice fails the boot naming the file
  and line; `navree i18n:check` loads leniently and lists them all.
- `i18n.dir` is relative to the working directory; tests run in the crate
  directory. Use `embed_locales!("locales")` (relative to `Cargo.toml`) for
  a self-contained binary; `i18n.reload` does not work with embedded files.
- A message shows its id: missing everywhere in the chain (`warn` logs it
  once). `t!` panics with `missing = "error"`.
- Fluent named arguments must be literals: `currency: $currency` is a syntax
  error; pass `Money` instead.
- Numbers are grouped (`2,026`); use `NUMBER($year, useGrouping: "false")`
  or pass a string.
- Bidi isolation marks (U+2068/U+2069) wrap variables only in right-to-left
  locales by default (`i18n.bidi_isolation`).
- French ICU output uses U+202F (narrow no-break space) for grouping and
  U+00A0 before `€`: compare with those characters in tests.
- Several apps in one test process share "the app booted last" for tags
  given to `Locale::scope`; use `i18n.locale(tag)` or `i18n.scope` there.
- The workspace source reads the workspace the request names only for its
  members and never enters the workspace; it costs a lookup per request
  that names one.
- Two types are called `Locale`: `navree::i18n::Locale` (the prelude's;
  the request's negotiated locale) and `navree::validation::Locale` (a
  BCP 47 tag in input DTOs). Name the validation one by path in a DTO.
  It converts: `Locale::scope(&input.locale, fut)` matches it with the
  supported locales like a tag.
- `Locale::scope(tag, fut)` matches the tag when it is called, not when
  the future runs: inside `i18n.scope(..)` wrap it in `async { .. }` so the
  tag is matched with that app's locales.
- When a new rule key is added to navree-validation's `DEFAULT_MESSAGES`,
  add `validation-<key>` to all seven `crates/navree-i18n/resources/*/
  validation.ftl` files; `shipped::tests` fails until you do.
