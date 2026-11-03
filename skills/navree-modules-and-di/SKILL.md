---
name: navree-modules-and-di
description: "Structures Navree code into modules wired by dependency injection: `#[module]` keys, `#[injectable]` providers and scopes, values, trait objects, factories, named and lazy providers, configurable modules (`for_root`, `from_config`, `for_feature`) and the boot-time graph errors. Use when adding or wiring a service, splitting a feature into a module, sharing a service between modules, registering something in `wire`, or fixing errors such as \"cannot resolve\", \"private to module\", \"configured more than once\" or \"provider dependency cycle\"."
---

# Modules and dependency injection

Everything above the kernel is a module, the framework included. A module
groups one feature: its providers (services), controllers and anything else
it contributes (migrations, jobs, listeners, commands, templates). It says
which providers others may use (`exports`) and which modules it needs
(`imports`). The kernel checks the whole graph at boot, before building
anything, and refuses to start with an error naming the module and type.

## Steps to add a service to a feature

1. Generate it: `navree make:service posts pricing` writes
   `src/modules/posts/services/pricing.rs` (`PricingService`) and registers
   it at the module's `// navree:wire` marker. Or add it to `providers`.
2. Give it dependencies as `Arc<T>` fields; take request data (the `Uow`,
   the user) as method parameters, never as fields.
3. Inject it where it is used: a controller field `pricing: Arc<PricingService>`.
4. To use it from another module: add it to this module's `exports`, and
   that module's struct to the other module's `imports`.
5. Run `navree check` (or `cargo test`) to validate the graph.

## Define a module

```rust,ignore
use navree::prelude::*;

use crate::modules::companies::{CompaniesModule, CompanyService};

#[injectable]
pub struct DealService {
    companies: Arc<CompanyService>, // exported by CompaniesModule
}

#[module(
    imports = [CompaniesModule],
    providers = [DealService],
    controllers = [DealsController],
    exports = [DealService],
    register = Self::wire,
)]
pub struct DealsModule;

impl DealsModule {
    /// Everything `#[module]` has no key for.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(migrations::migrations());
        // navree:wire
    }
}
```

| Key | Takes |
|---|---|
| `imports` | module expressions: `[CompaniesModule, AccessModule::for_feature::<Deal>()]` |
| `providers` | `#[injectable]` types, one provider each |
| `controllers` | controller types (also registered as singleton providers) |
| `exports` | types visible to importers, trait objects included (`dyn Notifier`) |
| `global` | a flag: exports visible to every module without an import |
| `register` | `fn(&self, &mut ModuleBuilder)` or a closure, run last |

Every key is optional; an unknown key, a repeated key, `global = true` or a
list without brackets is a compile error. Add the module to the root in
`src/app.rs` (`.module(posts::PostsModule)` above `// navree:modules`).
`#[module]` only writes `impl Module`; a hand-written `impl Module { fn
register(&self, m: &mut ModuleBuilder) { m.import(..).provider::<T>()...
} }` is equally supported.

## Visibility

A module resolves only: its own providers, the exports of modules it
imports **directly**, and the exports of global modules. Visibility is not
transitive; a module can re-export a provider it imports. Framework modules
(`HttpModule`, `DatabaseModule`, `CacheModule`, `QueueModule`,
`MailModule`, `AuthModule`, `AccessModule`, ...) are global, so
`Arc<Db>`, `Arc<AuthService>`, `Arc<Queue>` inject anywhere once the root
adds the module. Keep your own modules non-global unless they are app-wide
infrastructure.

## Providers

```rust,ignore
#[injectable]
pub struct ReportService {
    repo: Arc<ReportRepository>,                 // dependency: the provider of the type
    clock: Arc<dyn Clock>,                       // a trait object the app registered
    #[inject(name = "billing")]
    billing: Arc<ApiClient>,                     // a named provider
    notifier: Lazy<dyn Notifier>,                // resolved on first `.get().await`; breaks cycles
    #[inject(default)]
    hits: AtomicU64,                             // Default::default(), not injected
}

#[injectable(scope = "request")]                 // "singleton" (default), "request", "transient"
pub struct RequestCache {
    #[inject(default)]
    names: Mutex<HashMap<Uuid, String>>,
}
```

- Singletons are built once at boot in dependency order and shared.
  Request-scoped providers are built once per request scope; read them in a
  handler with the `Scoped<T>` extractor. A singleton may not depend on a
  request-scoped provider (boot error "scope inversion").
- Register other kinds in `wire`:

```rust,ignore
fn wire(&self, m: &mut ModuleBuilder) {
    m.value(DefaultCurrency("EUR"))                              // an existing value
        .value_arc::<dyn Clock>(Arc::new(SystemClock))           // a trait object
        .factory(Factory::<ApiClient>::new(|_r: Resolver| async {
            Ok(Arc::new(ApiClient::new("https://billing.example.com")))
        }).named("billing"))                                     // async construction, named
        .exports::<dyn Clock>();
}
```

  `Factory::new(..)` closures return `Result<Arc<T>, BoxError>`; declare
  what they resolve with `.deps(vec![ProviderKey::of::<T>()])` so the graph
  check sees it; `.scope(Scope::Request)` changes the scope; `m.multi(..)`
  adds one entry of a multi-binding (collect with `ctx.get_all::<T>()`).
- Outside providers (commands, hooks, jobs, tests) resolve through the
  context: `ctx.get::<T>().await?`, `ctx.try_get::<T>().await`,
  `ctx.get_named::<T>("x").await?`. It ignores visibility; keep it out of
  per-request code.

## Configurable modules

Framework modules come as `X::from_config()` (reads its section),
`X::for_root(options)` (options in code) or plain `X`. A configured
instance has the same identity as `X` and wins everywhere, so **configure
each module once, in the root, and import it by type elsewhere**. Writing
your own:

```rust,ignore
use navree::ModuleId;
use navree::config::ConfigSection;
use navree::prelude::*;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct BillingOptions {
    pub currency: String,
    pub trial_days: u32,
}

impl Default for BillingOptions {
    fn default() -> Self {
        Self { currency: "EUR".to_owned(), trial_days: 14 }
    }
}

impl ConfigSection for BillingOptions {
    const KEY: &'static str = "billing";
}

/// Plain `BillingModule` reads `[billing]`; `BillingModule::for_root(..)` takes the options.
#[derive(Clone, Copy, Debug, Default)]
pub struct BillingModule;

impl BillingModule {
    pub fn for_root(options: BillingOptions) -> impl Module {
        ConfiguredBillingModule { options }
    }
}

impl Module for BillingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<BillingOptions>();
        register(m);
    }
}

struct ConfiguredBillingModule {
    options: BillingOptions,
}

impl Module for ConfiguredBillingModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<BillingModule>() // the same module as plain BillingModule
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        m.value(self.options.clone());
        register(m);
    }
}

fn register(m: &mut ModuleBuilder) {
    m.provider::<InvoiceService>().exports::<InvoiceService>();
}
```

`X::for_feature::<T>()` is the other half: a feature module imports it to
register something with module `X`, as in
`imports = [AccessModule::for_feature::<Post>()]`.

## Boot errors and their fixes

| Error says | Fix |
|---|---|
| `cannot use T: it is private to module M` | add `T` to `M`'s `exports` |
| `it is exported by module M, which X does not import` | add `M` to `X`'s `imports` |
| `cannot resolve T ... no provider is registered` | register `T` (`providers`, `m.value`, a factory) or add the module that provides it |
| `sees T from several modules` | export it from one module only, or use a named provider |
| `module M is configured more than once` | keep one `for_root`/`from_config` in the root; import plain `M` elsewhere |
| `provider dependency cycle: A -> B -> A` | make one field `Lazy<T>` and resolve it in a method |
| `scope inversion` | pass request values as parameters or extractors, not singleton fields |
| `module import cycle` | move the shared part into a third module both import |

## Test

```rust,ignore
#[test]
fn the_graph_is_valid() {
    let report = Navree::app().module(DealsModule).validate().expect("valid graph");
    assert!(report.modules.iter().any(|m| m.name == "DealsModule"));
}
```

Boot a module for real with `TestApp::builder().module(DealsModule).build().await`
(see `navree-testing`), and swap a provider with
`.override_value::<dyn Clock>(Arc::new(FixedClock::new(now)))`.
`navree check --graph` prints the tree with every provider and dependency.

## Common mistakes

- Constructing services with `new()` inside other services instead of
  injecting them: the graph check and test overrides then can't see them.
- Storing an `AppContext` in a singleton (a reference cycle): store
  `ctx.downgrade()` or a `Lazy<T>`.
- Registering the same listener with `m.listen` twice in one module, or the
  same provider in `providers` and in `wire`: duplicate provider error.
- Putting a module in `providers`: modules go in `imports` or the root.

## Docs

- https://navree.com/docs/architecture/modules/
- https://navree.com/docs/architecture/providers/
- https://navree.com/docs/architecture/lifecycle-hooks/
