//! `#[injectable]`: constructor injection from struct fields.

use std::sync::Arc;

use keel_kernel::{Injectable, Keel, Module, ModuleBuilder, ProviderKey};
use keel_macros::injectable;

/// A dependency registered as a trait object.
trait Greeter: Send + Sync {
    fn greet(&self) -> String;
}

struct English;

impl Greeter for English {
    fn greet(&self) -> String {
        "hello".to_owned()
    }
}

#[injectable]
struct Repo;

#[injectable]
struct Service {
    repo: Arc<Repo>,
    greeter: std::sync::Arc<dyn Greeter>,
}

struct ServiceModule;

impl Module for ServiceModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.provider::<Repo>()
            .provider::<Service>()
            .value_arc::<dyn Greeter>(Arc::new(English));
    }
}

#[tokio::test]
async fn injectable_resolves_fields() {
    let app = Keel::app()
        .module(ServiceModule)
        .boot()
        .await
        .expect("boot");
    let service = app.ctx().get::<Service>().await.expect("Service");
    let repo = app.ctx().get::<Repo>().await.expect("Repo");
    assert!(Arc::ptr_eq(&service.repo, &repo));
    assert_eq!(service.greeter.greet(), "hello");
    assert_eq!(
        Service::dependencies(),
        vec![ProviderKey::of::<Repo>(), ProviderKey::of::<dyn Greeter>()]
    );
    assert!(Repo::dependencies().is_empty());
}

mod named_and_lazy {
    use super::*;
    use keel_kernel::{Factory, Lazy};

    pub struct Db {
        pub label: &'static str,
    }

    #[injectable]
    pub struct Reports {
        #[inject(name = "analytics")]
        pub analytics: Arc<Db>,
        pub primary: Arc<Db>,
        /// `Mailer` depends on `Reports`: the lazy handle breaks the cycle.
        pub mailer: Lazy<Mailer>,
    }

    #[injectable]
    pub struct Mailer {
        pub reports: Arc<Reports>,
    }

    pub struct ReportsModule;

    impl Module for ReportsModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.value(Db { label: "primary" })
                .factory(
                    Factory::<Db>::new(|_| async { Ok(Arc::new(Db { label: "analytics" })) })
                        .named("analytics"),
                )
                .provider::<Reports>()
                .provider::<Mailer>();
        }
    }
}

#[tokio::test]
async fn injectable_named_and_lazy() {
    use named_and_lazy::{Db, Reports, ReportsModule};

    let app = Keel::app()
        .module(ReportsModule)
        .boot()
        .await
        .expect("boot");
    let reports = app.ctx().get::<Reports>().await.expect("Reports");
    assert_eq!(reports.analytics.label, "analytics");
    assert_eq!(reports.primary.label, "primary");

    let mailer = reports.mailer.get().await.expect("lazy Mailer");
    assert!(Arc::ptr_eq(&mailer.reports, &reports));

    assert_eq!(
        Reports::dependencies(),
        vec![
            ProviderKey::named::<Db>("analytics"),
            ProviderKey::of::<Db>()
        ],
        "lazy fields are not dependency edges"
    );
}

mod defaults {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[injectable]
    pub struct Stats {
        pub repo: Arc<Repo>,
        #[inject(default)]
        pub hits: AtomicUsize,
        #[inject(default)]
        pub label: String,
    }

    pub struct StatsModule;

    impl Module for StatsModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.provider::<Repo>().provider::<Stats>();
        }
    }
}

#[tokio::test]
async fn injectable_default_fields_use_default() {
    use defaults::{Stats, StatsModule};
    use std::sync::atomic::Ordering;

    let app = Keel::app().module(StatsModule).boot().await.expect("boot");
    let stats = app.ctx().get::<Stats>().await.expect("Stats");
    let repo = app.ctx().get::<Repo>().await.expect("Repo");
    assert!(Arc::ptr_eq(&stats.repo, &repo));
    assert_eq!(stats.hits.load(Ordering::SeqCst), 0);
    assert_eq!(stats.label, "");
    assert_eq!(Stats::dependencies(), vec![ProviderKey::of::<Repo>()]);
}

mod scopes {
    use super::*;

    #[injectable(scope = "transient")]
    pub struct Fresh;

    #[injectable(scope = "request")]
    pub struct PerRequest {
        pub fresh: Arc<Fresh>,
    }

    #[injectable(scope = "singleton")]
    pub struct Shared;

    pub struct ScopesModule;

    impl Module for ScopesModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.provider::<Fresh>()
                .provider::<PerRequest>()
                .provider::<Shared>();
        }
    }
}

#[tokio::test]
async fn injectable_scope_argument_sets_the_scope() {
    use keel_kernel::{ModuleId, RequestScope, Scope};
    use scopes::{Fresh, PerRequest, ScopesModule, Shared};

    assert_eq!(Fresh::scope(), Scope::Transient);
    assert_eq!(PerRequest::scope(), Scope::Request);
    assert_eq!(Shared::scope(), Scope::Singleton);
    assert_eq!(Repo::scope(), Scope::Singleton, "singleton by default");

    let app = Keel::app().module(ScopesModule).boot().await.expect("boot");
    let ctx = app.ctx();
    let a = ctx.get::<Fresh>().await.expect("Fresh");
    let b = ctx.get::<Fresh>().await.expect("Fresh");
    assert!(!Arc::ptr_eq(&a, &b), "transient builds on every resolve");

    let module = ModuleId::of::<ScopesModule>();
    let first = ctx.request_resolver(module, RequestScope::new());
    let x = first.get::<PerRequest>().await.expect("PerRequest");
    let y = first.get::<PerRequest>().await.expect("PerRequest");
    assert!(Arc::ptr_eq(&x, &y), "one instance per request scope");
    let other = ctx.request_resolver(module, RequestScope::new());
    let z = other.get::<PerRequest>().await.expect("PerRequest");
    assert!(
        !Arc::ptr_eq(&x, &z),
        "a new request scope builds a new instance"
    );
    assert!(!Arc::ptr_eq(&x.fresh, &z.fresh));
}

mod generic {
    use super::*;

    /// Generic providers keep their parameters and bounds.
    #[injectable]
    pub struct Holder<T: Send + Sync + 'static> {
        pub inner: Arc<T>,
    }

    /// An empty braced struct behaves like a unit struct.
    #[injectable]
    pub struct Empty {}

    /// A field named like the generated resolver variable.
    #[injectable]
    pub struct Shadow {
        pub resolver: Arc<Repo>,
    }

    pub struct GenericModule;

    impl Module for GenericModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.provider::<Repo>()
                .provider::<Holder<Repo>>()
                .provider::<Empty>()
                .provider::<Shadow>();
        }
    }
}

#[tokio::test]
async fn injectable_supports_generics_and_empty_structs() {
    use generic::{Empty, GenericModule, Holder, Shadow};

    assert_eq!(
        Holder::<Repo>::dependencies(),
        vec![ProviderKey::of::<Repo>()]
    );
    let app = Keel::app()
        .module(GenericModule)
        .boot()
        .await
        .expect("boot");
    let holder = app.ctx().get::<Holder<Repo>>().await.expect("Holder");
    let shadow = app.ctx().get::<Shadow>().await.expect("Shadow");
    assert!(Arc::ptr_eq(&holder.inner, &shadow.resolver));
    app.ctx().get::<Empty>().await.expect("Empty");
}
