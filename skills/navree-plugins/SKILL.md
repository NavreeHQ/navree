---
name: navree-plugins
description: "Installs, writes, tests and releases Navree plugins (`navree plugin:add`, `plugin:remove`, `plugin:list`, `make:plugin`, `make:component`, `plugins`, `plugin:publish`), configures `plugin_info!`, namespaces and `[package.metadata.navree.plugin]`, embeds templates (`embed_templates!`, `m.plugin_templates`), translations (`embed_locales!`, `m.locales`) and namespaced files, registers drivers under a name (`m.cache_store`, `m.queue_store`, `m.session_store`, `m.backplane`, `m.mail_transport`, `m.disk_driver`, `m.notification_channel`, `m.sms_driver`, `m.whatsapp_driver`, `m.search_engine`, `m.ai_provider`), connects existing crates (`ClientModule`, `FallibleLayer`, `Mount`, `TonicService`), and adds sidebar links, Helm links and permissions. Use when installing or removing a first-party or community plugin, scaffolding or authoring a `navree-plugin-*` crate, registering a custom driver, mounting a Tower or tonic service, or publishing a plugin's templates, locales or config."
---

# Navree plugins

A Navree plugin is a Rust library crate whose modules call `m.plugin(navree::plugin_info!())` to declare a package name, version and namespace, and register routes, models, migrations, templates, translations, drivers or UI extensions through the `navree` facade.

## When to use this skill

| Goal | Section | Docs |
|---|---|---|
| Understand how plugins, namespaces and boot checks work | [What a plugin is](#what-a-plugin-is) | [Overview](https://navree.com/docs/plugins/overview/) |
| Add, inspect or remove a first-party or community plugin | [Install, list and remove](#install-list-and-remove) | [Installing, listing and removing](https://navree.com/docs/plugins/installing/) |
| Scaffold a plugin crate, options and `[package.metadata.navree.plugin]` | [Write one](#write-one) | [Writing a plugin](https://navree.com/docs/plugins/writing/) |
| Ship templates, Fluent locales, stylesheets or publishable files | [Templates, translations and files](#templates-translations-and-files) | [Templates, translations and files](https://navree.com/docs/plugins/resources/) |
| A map, a field or another component of your own | [`navree-ui` — Your own components](../navree-ui/SKILL.md#your-own-components) | [Components for Navree UI](https://navree.com/docs/plugins/ui-components/) |
| Register a cache, queue, session, backplane, mail, storage, notification, search or AI driver | [Drivers](#drivers) | [Drivers](https://navree.com/docs/plugins/drivers/) |
| Wrap a client crate, a fallible Tower layer, a mounted Tower service or a tonic server | [Connecting an existing crate](#connecting-an-existing-crate) | [Connecting an existing crate](https://navree.com/docs/plugins/existing-crates/) |
| Contribute sidebar links, Helm links, permissions or app-registered hooks | [Links, the admin panel and permissions](#links-the-admin-panel-and-permissions) | [Writing a plugin](https://navree.com/docs/plugins/writing/) |
| Boot a plugin in `TestApp`, verify a driver or test `plugin:publish` | [Testing](#testing) | [Testing a plugin](https://navree.com/docs/plugins/testing/) |
| Name, tag and version a community plugin crate | [Releasing](#releasing) | [Releasing a plugin](https://navree.com/docs/plugins/releasing/) |

## What a plugin is

- One line in a module's `register` names it a plugin: `m.plugin(navree::plugin_info!())` (or `navree::plugin_info!(namespace = "notice_board", title = "Notice board")`).
- By default, `plugin_info!()` derives the namespace from `CARGO_PKG_NAME` by stripping `navree-plugin-` (else `navree-`) and replacing `-` with `_` (`navree-plugin-notices` becomes `notices`).
- Navree's own add-on packages (`navree-flags`, `navree-social`, `navree-billing`, ...) name themselves plugins too. The kernel identifies first-party plugins from its built-in package list, matching crate version and crate module type; overriding a single Navree crate to another version makes it a community plugin and stops the boot on its reserved namespace.
- Every boot rule for a community plugin applies to every module that belongs to it: any module that calls `m.plugin(..)`, any other module defined in that crate, any generic module imported only by the plugin's modules, and any module of a second crate (outside the framework's own crates) imported only by the plugin's modules. Splitting a plugin into a `-core` helper crate does not exempt that second crate's modules: `navree plugins` lists them under the plugin and holds them to the same rules. Keep a plugin in one crate unless you want both crates governed together.
- Inspect plugins at the command line with `navree plugins` and `navree plugins --json`, or in code with `Navree::app().module(..).plugin_report()` (without booting) and `ctx.plugins()`.
- Boot rules keep an honest plugin inside its namespace and make its contributions visible; they are not a sandbox. A plugin compiles into the app binary and runs in the app's process with the app's database, network and configuration access, so read a community crate's source before building or running it.

## Install, list and remove

```sh
navree plugin:add billing                                # first-party opt-in package
navree plugin:add browser                                # first-party [dev-dependencies] feature
navree plugin:add navree-plugin-notices --version "^0.1" # community crate from Cargo
navree plugin:add --path plugins/notices                 # local plugin crate inside the app
navree plugin:list [--json]                              # inspect Cargo.toml + src/ without running code
navree plugin:remove navree-plugin-notices               # unwire src/app.rs and cargo remove
```

- `plugin:add` validates `[package.metadata.navree.plugin]` against a fixed grammar before changing any file, plans every edit in memory and rolls back `Cargo.toml`, `Cargo.lock` and app files byte for byte if any step fails.
- What `plugin:add` writes:
  - `Cargo.toml`: adds the dependency (for a community crate) and enables requested `navree` features (`[dev-dependencies]` for `browser`), preserving comments and formatting.
  - `src/app.rs`: inserts `.module(..)` directly above `// navree:modules` (unless `--no-wire` is passed or `// navree:modules` was deleted, in which case it prints a `next` line with the exact `.module(..)` call).
  - `.env.example`: appends `APP_<NS>__<KEY>=` lines marked with `# plugin:add <package>`.
  - `config/workspace.toml`: adds suggested role permissions (`member = ["notices.read"]`) marked with `# plugin:add <package>` when roles come from `config/workspace.toml`, or prints a `next` line otherwise.
  - `config/ui.policy.toml`: writes requested `img_src`, `media_src`, `connect_src`, `font_src` and `wasm = true` entries **commented out** so the app owner decides what to allow.
  - `config/<section>.toml`: written automatically for first-party packages, or when `--publish-config` is passed.
- Without `--publish-config`, `plugin:add` of a community crate runs only `cargo add` and `cargo metadata` — no build script, proc macro or plugin code runs on the machine — and finishes with `It is not an official Navree package: read its code before you deploy it.`
- `navree plugin:list` and plain `navree check` inspect `cargo metadata` and `src/` without building the app, warning when a plugin dependency is not wired under `src/` or when a non-Navree package sets `[lib] name` starting with `navree` that does not match its own package name.
- `navree plugin:remove <package>` removes the exact `.module(..)` line `plugin:add` wrote and (for a community crate) runs `cargo remove`, while keeping `config/<section>.toml`, marked grants, marked page-policy comments, `.env.example` keys, `navree` features, local `plugins/<name>/` directories and database tables.

## Write one

Scaffold a crate with `make:plugin`, and add Navree UI components or form controls with `make:component`:

```sh
navree make:plugin notices                          # writes plugins/notices/
navree make:plugin khata --wrap reqwest             # also writes a ClientModule in src/client.rs
navree make:component map --path plugins/geo --script --sheet
navree make:component pin_code --path plugins/geo --field
```

Import `navree::plugin::prelude::*;` alongside `navree::prelude::*;` to bring the plugin `ModuleBuilder` extension methods (`m.plugin_templates`, `m.locales`, `m.permissions`, `m.nav_item`, `m.helm_nav`, `m.mount`, `m.tonic_service` and the driver methods) without polluting an app's own prelude:

```rust
use navree::config::ConfigSection;
use navree::contracts::{Job, JobContext};
use navree::helm::HelmLink;
use navree::kernel::CommandSpec;
use navree::plugin::Permission;
use navree::plugin::prelude::*;
use navree::prelude::*;
use navree::ui::{AppSheet, ShellNavItem};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, ConfigSection)]
#[config(key = "notices")]
#[serde(default)]
pub struct NoticesOptions {
    pub keep_days: u32,
    pub digest_from: String,
}

impl Default for NoticesOptions {
    fn default() -> Self {
        Self {
            keep_days: 30,
            digest_from: "notices@ludhiana-society.example".to_owned(),
        }
    }
}

pub const READ: Permission = Permission::new("notices.read", "Read the society's notices");
pub const PUBLISH: Permission = Permission::new("notices.publish", "Post a notice to the society");

pub const BOARD_SHEET: AppSheet =
    AppSheet::new("board", b"[data-notice] { padding: 0.75rem; }", 4_000).namespace("notices");

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ExpireNotices;

impl Job for ExpireNotices {
    const NAME: &'static str = "notices.expire";

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        Ok(())
    }
}

#[injectable]
pub struct NoticesController;

#[controller("/notices")]
impl NoticesController {
    #[get("/")]
    #[permissions("notices.read")]
    async fn index(&self) -> Json<Vec<&'static str>> {
        Json(vec!["Water supply off on Sunday in Ludhiana Block C."])
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoticesModule;

impl NoticesModule {
    pub fn from_config() -> Self {
        Self
    }
}

impl Module for NoticesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(navree::plugin_info!(namespace = "notices", title = "Notice board"));
        m.config_section::<NoticesOptions>();
        m.permissions(&[READ, PUBLISH]);
        m.contribute(BOARD_SHEET);
        m.controller::<NoticesController>();
        m.nav_item(
            ShellNavItem::new("Notices", "/notices")
                .group("Society")
                .permission("notices.read"),
        );
        m.helm_nav(HelmLink::new("Notices", "/admin/notices").can("manage", "notices.Notice"));
        m.command(CommandSpec::new(
            "notices:expire",
            "Remove expired notices",
            |_ctx, _args| async { Ok(()) },
        ));
        m.job::<ExpireNotices>();
        m.schedule(|s| {
            s.job(ExpireNotices)
                .daily_at("02:30")
                .in_zone("Asia/Kolkata");
        });
        // navree:wire
    }
}
```

For a plugin that ships a browser-side Navree UI component (`ui-extension-files` feature on `navree`), declare its `UiExtension` and any external `PageSources` under the plugin's namespace (see https://navree.com/docs/ui/your-own-components/, https://navree.com/docs/ui/extension-contract/ and https://navree.com/docs/ui/page-policy/):

```rust,ignore
use navree::plugin::{Directive, PageSources, UiExtension};
use navree::ui::{extension_script, extension_sheet};

static MAP: UiExtension = UiExtension::new("geo", "map")
    .script(extension_script!("assets/map.js", 16 * 1024))
    .sheet(extension_sheet!("assets/map.css", "geo", 8 * 1024))
    .contract(1)
    .routes(&["/geo/"]);

// Inside Module::register:
// m.contribute(MAP);
// m.contribute(PageSources::new().ask(Directive::ImgSrc, "https://tile.openstreetmap.org", "map tiles"));
```

### The manifest grammar (`[package.metadata.navree.plugin]`)

```toml
[package.metadata.navree.plugin]
namespace = "notices"
title = "Notice board"
module = "NoticesModule::from_config()"
features = ["helm", "orm", "queue", "schedule", "ui", "view"]
config = ["notices"]
env = ["APP_NOTICES__DIGEST_FROM"]
grants = { member = ["notices.read"], admin = ["notices.read", "notices.publish"] }
policy = { img_src = ["https://tile.openstreetmap.org"] }
```

- **`namespace`** (optional string): `[a-z][a-z0-9_]{0,63}`, never one of the 87 framework-reserved words (`app`, `auth`, `cache`, `http`, `mail`, `navree`, `plugins`, `ui`, `views`, `workspace`, `all`, Windows device names such as `con` and `nul`, or first-party namespaces). Must match `plugin_info!(namespace = "..")` when set, and must not collide with permissions the app already grants.
- **`title`** (optional string): 1 to 64 characters, no control characters.
- **`module`** (required string, at most 200 characters): `path` of Rust identifiers separated by `::`, optional `::<TypePath, ..>` turbofish on segments, and optional trailing `()` with no arguments and no spaces (`NoticesModule`, `NoticesModule::from_config()`). Rejects Rust keywords (`crate`, `self`, `super`, `Self`), spaces, newlines, semicolons, arguments, nested generics (`::<Vec<u8>>`) and bidirectional control characters.
- **`features`** (optional string list, at most 32): only allow-listed `navree` module features (`access`, `ai`, `api`, `audit`, `auth`, `billing`, `cache`, `crypto`, `db`, `directory`, `documents-docx`, `documents-native`, `domains`, `events`, `flags`, `fleet`, `graphql`, `grpc`, `helm`, `http`, `http-client`, `i18n`, `integrations`, `lighthouse`, `log`, `logbook`, `mail`, `mcp`, `media`, `micro`, `notify`, `oauth`, `observe`, `orm`, `queue`, `realtime`, `redis`, `schedule`, `search`, `session`, `settings`, `sheets`, `social`, `sso`, `storage`, `telemetry`, `time`, `ui`, `validation`, `view`, `webhooks`, `workflows`, `workspace`). Refuses `default`, `full`, `documents` (headless Chrome is the owner's choice), `mongo`, `dock`, `harbor`, `navigator`, `browser`, `testing`, `otel`, database drivers (`postgres`, `mysql`, `sqlite`, `mongodb`), `-preview` features and sub-features (`storage-s3`, `ui-extension-files`, ...).
- **`config`** (optional string list, at most 16): `<ns>` or `<ns>.<word>` sections only.
- **`env`** (optional string list, at most 32): `APP_<NS>__[A-Z0-9_]{1,120}` only.
- **`grants`** (optional table, at most 8 roles): keys `admin`, `member`, `viewer` or a role configured in `workspace.roles` (never `owner` or `workspace.demo.guest_role`); each role lists at most 32 permissions of `<ns>.<words>` or `<ns>.*`, each at most 128 characters.
- **`policy`** (optional table): `img_src`, `media_src`, `connect_src`, `font_src` (origin lists) and `wasm` (`bool`).
- Any unknown key (such as `script`) or wrong value type refuses the install and leaves every file untouched.

## Templates, translations and files

- **Templates (`view` feature):** `m.plugin_templates(navree::embed_templates!("views"))` embeds every UTF-8 file under `views/` (relative to the plugin's `Cargo.toml`, no symlinks) beneath `<namespace>/`, so `views/list.html` renders as `"notices/list"`. Registering templates outside `<namespace>/` (unless replacing an imported module's exact template name) fails at boot. Any filter, test, function or global added via `m.view_environment` must start with `<namespace>_`.
- **Translations (no `i18n` feature needed):** `m.locales(navree::embed_locales!("locales"))` embeds Fluent files (`locales/en/notices.ftl`, `locales/pa-IN/notices.ftl`). Message IDs must start with `<namespace>-` and terms with `-<namespace>-`.
- **Static and publishable files:** build `AppSheet`, `AppFont`, `AppImage` and `IslandScript` with `.namespace("<namespace>")` (served as `<namespace>.<name>`; a font sheet named `face` takes no namespace). Ship extra copyable assets with `Publishable::new(PublishTag::Files).file("logo.svg", include_bytes!(..))` via `m.contribute(..)`.

```rust
use navree::plugin::{PublishTag, Publishable};
use navree::view::TemplateDir;

pub fn embedded_resources(m: &mut ModuleBuilder) {
    // In a plugin crate on disk, pass `navree::embed_templates!("views")`
    // and `navree::embed_locales!("locales")`.
    let views = TemplateDir::from(
        &[("list.html", "<p>Notices from the Ludhiana office</p>")] as &[(&str, &str)],
    );
    m.plugin_templates(views);
    m.contribute(Publishable::new(PublishTag::Files).file("print.css", b"@media print {}"));
}
```

- **Publishing and precedence:** `navree plugin:publish <plugin> [--tag views|locales|files|config] [--dry-run] [--force]` copies templates to `resources/views/<ns>/`, locales to `locales/<locale>/<ns>/`, files to `resources/<ns>/` and config sections via `config:publish --plugin <ns>` (or `i18n:publish <plugin>` when `i18n.dir` is customised). Existing files are kept unless `--force` is passed. App files in `resources/views/` and app Fluent messages always win over a plugin's embedded ones; a plugin cannot silently replace another module's template unless the app takes the name itself.

## Drivers

Register custom drivers on `ModuleBuilder` (`use navree::plugin::prelude::*;`). A driver name must match `[a-z][a-z0-9_]{0,63}`, must not equal a built-in driver name or a key of that config section, and may only be registered by one module; an unregistered selector fails both `config:check` and boot naming the setting and registration method.

| Kind | Registration method | Trait / return type | Settings read from |
|---|---|---|---|
| Cache store and locks | `m.cache_store(name, factory)` | `CacheBackend` (`CacheStore` + `LockStore`) | `cache.<name>` (`cache.store = "<name>"`) |
| Queue store | `m.queue_store(name, factory)` | `QueueStore` | `queue.<name>` (`queue.driver = "<name>"`) |
| Session store | `m.session_store(name, factory)` | `SessionStore` | `session.<name>` (`session.store = "<name>"`) |
| Realtime backplane | `m.backplane(name, factory)` | `Backplane` (requires `realtime.secret`) | `realtime.<name>` (`realtime.backplane = "<name>"`) |
| Mail transport | `m.mail_transport(name, factory)` | `MailTransport` | `mail.<name>` (`mail.transport = "<name>"` or failover list) |
| Storage disk | `m.disk_driver(name, factory)` | `DiskDriver` + `DiskUpload` | `storage.disks.<disk>` (`driver = "<name>"`) |
| Notification channel | `m.notification_channel::<C>()` | `ChannelDriver + Injectable` (`Channel::named(C::NAME)`) | `notify.<name>` |
| SMS / WhatsApp driver | `m.sms_driver` / `m.whatsapp_driver` | `SmsDriver` / `WhatsAppDriver` | `notify.<name>` (`notify.sms.driver` / `notify.whatsapp.driver`) |
| Search engine | `m.search_engine(name, factory)` | `SearchEngine` | `search.<name>` (`search.engine = "<name>"`) |
| AI provider | `m.ai_provider(name, factory)` | `AiDriver` | `ai.providers.<name>` (`driver = "<name>"`) |

Plugins can also emit custom telemetry events without a driver slot via `Telemetry::emit` and `CustomEvent::new("notices.published")` (at least two `.`-joined namespace words).

Keep credentials in `navree::contracts::Secret<String>`: factory, transport and disk errors are printed at boot or logged, so scrub any API key (plain, quoted and percent-encoded) and name bad URLs by their config key rather than printing the URL.

```rust
use navree::BoxFuture;
use navree::cache::{CacheBackend, MemoryLockStore, MemoryStore};
use navree::contracts::Secret;
use navree::mail::{MailError, MailTransport, Message};

#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct SandukCacheSettings {
    capacity: usize,
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct DakgharSettings {
    api_key: Secret<String>,
}

struct DakgharTransport {
    key: Secret<String>,
}

impl MailTransport for DakgharTransport {
    fn send<'a>(&'a self, _message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        let _ = self.key.expose();
        Box::pin(async { Ok(()) })
    }
}

pub struct DriversExampleModule;

impl Module for DriversExampleModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.cache_store("sanduk", |ctx| async move {
            let settings: SandukCacheSettings = ctx.settings()?;
            Ok(CacheBackend::new(
                Arc::new(MemoryStore::with_capacity(settings.capacity.max(64))),
                Arc::new(MemoryLockStore::new()),
            ))
        });
        m.mail_transport("dakghar", |ctx| async move {
            let settings: DakgharSettings = ctx.settings()?;
            if settings.api_key.expose().trim().is_empty() {
                return Err("mail.dakghar.api_key is required".into());
            }
            Ok(DakgharTransport {
                key: settings.api_key,
            })
        });
    }
}
```

AI provider drivers (`ai` feature on `navree`) bring their own HTTP client and register with `m.ai_provider`:

```rust,ignore
use navree::ai::{AiCapabilities, AiDriver, AiDriverAnswer, AiDriverRequest, AiDriverStream, AiError};

impl Module for VaaniModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.ai_provider("vaani", |ctx| async move {
            let _table = ctx.instance();
            Ok(VaaniDriver::from_settings(ctx.settings()?)?)
        });
    }
}
```

## Connecting an existing crate

- **`ClientModule`:** wraps an external client `T` in a module with typed `ConfigSection` options `O`, readiness check and shutdown hook. Call `.options::<O>()` **before** `.connect(..)` (calling `.options` resets an earlier `.connect`), put a `tokio::time::timeout` around network connects so an unreachable server cannot hang boot, never leak tokens in connect or health error strings, and wrap shared types (such as `reqwest::Client`) in a newtype of the plugin's crate so plain DI lookups stay unambiguous.
- **`FallibleLayer`:** adapts a Tower layer whose service error is not `Infallible` (such as a timeout layer) into a `GlobalLayer` or route layer whose errors become 500 `AppError`s (mappable via `m.map_error::<E>(..)` in every slot except `Edge`). A plugin ships the `GlobalLayer` constructor; a module of the app registers it with `m.http_layer(..)`.
- **`Mount`:** `m.mount(Mount::new("/khata", service).authenticated())` serves any Tower service under a prefix inside the request pipeline. Every mount requires an access declaration (`.public()`, `.authenticated()`, `.roles(..)`, `.permissions(..)` or `.can(..)`). The router rejects `.` / `..` segments, `%00`, backslashes and invalid UTF-8 with 400, answers empty segments (`//`, `%2F`) with 400 after access checks, answers `Upgrade` and `CONNECT` with 501, and forwards the undecoded subpath on `request.uri()`.

```rust
use std::time::Duration;

use navree::http::extract::Request;
use navree::http::middleware::{Next, from_fn};
use navree::http::response::Response;
use navree::http::{AppRouter, FallibleLayer, GlobalLayer, LayerSlot, Mount};
use navree::plugin::ClientModule;

#[derive(Clone, Default, serde::Deserialize, navree::config::ConfigSection)]
#[config(key = "khata")]
#[serde(default)]
pub struct KhataOptions {
    pub base_url: String,
}

pub struct KhataClient {
    pub base_url: String,
}

pub fn khata_client() -> ClientModule<KhataClient, KhataOptions> {
    ClientModule::<KhataClient>::new("khata")
        .options::<KhataOptions>()
        .connect(|options, _resolver| async move {
            Ok(KhataClient {
                base_url: options.base_url.clone(),
            })
        })
        .health(|client| async move {
            if client.base_url.starts_with("https://") {
                Ok(())
            } else {
                Err("khata.base_url must use https".into())
            }
        })
        .plugin(navree::plugin_info!(namespace = "khata"))
}

async fn pass_through(request: Request, next: Next) -> Response {
    next.run(request).await
}

/// Shipped by the plugin; registered by a module of the app with `m.http_layer(..)`.
pub fn khata_guard_layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::Context, "khata-guard", |_ctx, router| {
        Ok(router.layer(FallibleLayer::new(from_fn(pass_through))))
    })
}

async fn entries(request: Request) -> Result<String> {
    Ok(format!("ledger {}", request.uri().path()))
}

pub struct LedgerMountModule;

impl Module for LedgerMountModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mount(
            Mount::new("/khata", AppRouter::from_handler(entries))
                .authenticated()
                .throttle(60, Duration::from_secs(60)),
        );
    }
}
```

- **`TonicService` (`grpc` feature):** serves a pre-built tonic server behind Navree's auth, access checks, guards and unit of work. Method overrides use the generated Rust trait's `snake_case` spelling (`get_deal` for `GetDeal`), and message size caps are set on the tonic server before wrapping:

```rust,ignore
use navree::grpc::{GrpcMethod, TonicService};
use navree::plugin::prelude::*;

impl Module for KhataDealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.tonic_service(
            TonicService::new(DealsServer::new(KhataDeals).max_decoding_message_size(4 * 1024 * 1024))
                .authenticated()
                .method(GrpcMethod::new("get_deal").public()),
        );
    }
}
```

## Links, the admin panel and permissions

- `m.permissions(&[Permission::new("notices.read", "..")])` describes permissions so `AccessModule` and routes know them; describing a permission grants it to nobody. A workspace `owner` and the super-admin role hold every permission automatically, so never guard a staff-only operation solely with a permission that workspace owners receive.
- `m.nav_item(ShellNavItem::new("Notices", "/notices").permission("notices.read"))` contributes a sidebar link (drawn when the app's `Shell` calls `.contributed()`). Omit `.permission(..)` only when the link is meant for everyone who sees the shell.
- `m.helm_nav(HelmLink::new(..).can(..))`, `m.resource::<R>()`, `m.helm_page(..)` and `m.helm_widget(..)` extend the Helm admin panel (custom pages and widgets render HTML templates under the plugin's namespace).
- Registrations that stand for the whole app are refused when called from a plugin module. Ship a function or type from the plugin and show the app owner the line to put in an app module:

| To | The line in a module of your app |
|---|---|
| apply its policy | `.import(AccessModule::for_policy::<khata::InvoicePolicy>())` |
| register its subject | `.import(AccessModule::for_subject::<khata::Ledger>())` |
| configure a module it uses | `AccessModule::for_root(..)` in your app's root (the plugin imports `AccessModule`) |
| add its permission check | `m.multi(khata::permission_check())` |
| add its sign-in strategy | `m.strategy::<khata::LedgerStrategy>()` |
| add its middleware | `m.http_layer(khata::timeout())` |
| let it decide Helm's staff | `m.helm_staff_workspaces(khata::may_enter)` |
| use its channel authorizer | `m.channel("orders.{id}", notices::authorize)` |
| scope its model | `m.workspace_scoped::<notices::Notice>()` |
| apply its scope or hook to your model | `m.global_scope::<Deal>("open", khata::open)`, `m.on_model::<Deal, _>(ModelEvent::Saving, khata::stamp)` |

## Testing

- **Boot in `TestApp`:** wire the plugin module with the framework modules it uses, assert `app.ctx().plugins()` lists the namespace and title, and exercise its routes with `TestApp::acting_as`:

```rust
use navree::storage::Storage;
use navree::testing::TestApp;

#[tokio::test]
async fn plugin_and_disk_recipe_compile() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(StorageModule::from_config())
        .module(NoticesModule::from_config())
        .build()
        .await;

    assert!(
        app.ctx()
            .plugins()
            .plugins
            .iter()
            .any(|p| p.info.namespace == "notices")
    );

    let storage = app.ctx().get::<Storage>().await.unwrap();
    let disk = storage.disk("invoices").unwrap();
    disk.put("receipts/1101.txt", "Invoice 1101, ₹1,00,000").await.unwrap();
    assert_eq!(disk.get("receipts/1101.txt").await.unwrap(), b"Invoice 1101, \xe2\x82\xb91,00,000"[..]);
    assert!(disk.exists("receipts/1101.txt").await.unwrap());
    assert!(disk.size("receipts/1101.txt").await.unwrap() > 0);
    assert_eq!(disk.list("receipts").await.unwrap().len(), 1);
    disk.delete("receipts/1101.txt").await.unwrap();
    assert!(disk.get("receipts/1101.txt").await.is_err());

    app.shutdown().await.unwrap();
}
```

- **Test a driver** through its public facade service (for a `DiskDriver`, run the six `Disk` calls above — `put`, `get`, `exists`, `size`, `list`, `delete` — plus reading a missing file to verify `NotFound`, and verify prefix listing does not match sibling prefixes such as `docs2/` when listing `docs`).
- **Test `plugin:publish`** by running `plugin:publish <ns> --tag views` in a temporary directory holding a minimal `Cargo.toml`, checking the written file under `resources/views/<ns>/`, editing it, and running `plugin:publish` a second time to verify the edited file is preserved without `--force`.

## Releasing

- **Package name and description:** publish as `navree-plugin-<name>` so `plugin_info!()` derives `<name>` (with `-` turned into `_`), and end `description` in `Cargo.toml` with `"Not an official Navree package."`.
- **Keywords:** set `keywords = ["navree-plugin", "navree"]` in `Cargo.toml`.
- **README notice:** carry the exact sentence `It is not an official Navree package.` in `README.md` (and note any external origins your UI components request under `policy`).
- **Versions:** depend on `navree = "1.4"` with `default-features = false` and only the features the plugin uses. Adding an entry to `[package.metadata.navree.plugin].features` turns on a new feature in consumers' `Cargo.toml` on `plugin:add`, so treat adding a manifest feature as at least a minor version bump.

## Mistakes to avoid

- Running `navree plugin:add <crate> --publish-config` on an untrusted community crate before reading its code: `--publish-config` compiles and executes the crate's build scripts, macros and `register` hook.
- Splitting a plugin into a second helper crate expecting that crate's modules to bypass plugin checks: any non-framework crate imported only by the plugin's modules is attributed to the plugin and held to all of its rules.
- Setting `plugin_info!(namespace = "..")` without setting the same `namespace` in `[package.metadata.navree.plugin]` (or picking one of the framework's reserved namespaces).
- Calling `.connect(..)` before `.options::<O>()` on `ClientModule` (`.options` drops an earlier connect closure), or exporting a bare `reqwest::Client` from `ClientModule` instead of a plugin-owned wrapper struct.
- Echoing API keys, bearer tokens or connection URLs in `MailError`, `DiskDriverError`, `ClientModule` connect/health errors, `Debug` impls or log fields.
- Leaving `.permission(..)` off a `ShellNavItem` that points to a restricted page: a link without `.permission(..)` is shown to everyone.
- Listing `full`, `documents`, `sqlite`, `postgres`, `ui-extension-files` or other non-allow-listed features under `[package.metadata.navree.plugin].features`: `plugin:add` refuses the manifest.

A plugin's module adds no app-wide layer, configures no other crate's module, provides no permission check, authorizer, authenticator, strategy, workspace gate or shell, and hooks no other crate's model: ship a function and show the app's line.
