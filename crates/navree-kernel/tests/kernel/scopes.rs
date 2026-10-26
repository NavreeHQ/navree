//! Runtime scopes: request scope memoization, transient providers, lazy handles,
//! and scope rules enforced while resolving.

use std::sync::Arc;

use navree_kernel::{
    BoxError, BoxFuture, Factory, Injectable, KernelError, Lazy, Module, ModuleBuilder, ModuleId,
    Navree, ProviderKey, RequestScope, Resolver, Scope,
};

use crate::support::{Counter, module};

/// Counts constructions of every scoped provider below.
pub struct Constructions {
    pub cart: Counter,
    pub token: Counter,
}

pub struct Catalog;

/// Request-scoped; depends on a singleton.
pub struct Cart {
    pub catalog: Arc<Catalog>,
}

impl Injectable for Cart {
    fn scope() -> Scope {
        Scope::Request
    }

    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Catalog>(),
            ProviderKey::of::<Constructions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            r.get::<Constructions>().await?.cart.hit();
            Ok(Cart {
                catalog: r.get::<Catalog>().await?,
            })
        })
    }
}

/// Request-scoped; shares the scope's `Cart`.
pub struct Checkout {
    pub cart: Arc<Cart>,
}

impl Injectable for Checkout {
    fn scope() -> Scope {
        Scope::Request
    }

    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Cart>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Checkout {
                cart: r.get::<Cart>().await?,
            })
        })
    }
}

/// Transient: built on every resolution.
pub struct Token;

impl Injectable for Token {
    fn scope() -> Scope {
        Scope::Transient
    }

    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Constructions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            r.get::<Constructions>().await?.token.hit();
            Ok(Token)
        })
    }
}

struct ShopModule(Arc<Constructions>);

impl Module for ShopModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc(self.0.clone())
            .value(Catalog)
            .provider::<Cart>()
            .provider::<Checkout>()
            .provider::<Token>();
    }
}

async fn shop() -> (navree_kernel::App, Arc<Constructions>) {
    let constructions = Arc::new(Constructions {
        cart: Counter::default(),
        token: Counter::default(),
    });
    let app = Navree::app()
        .module(ShopModule(constructions.clone()))
        .boot()
        .await
        .expect("boot");
    (app, constructions)
}

#[tokio::test]
async fn request_scope_memoizes_per_scope() {
    let (app, constructions) = shop().await;
    assert_eq!(
        constructions.cart.get(),
        0,
        "request-scoped providers are not built at boot"
    );

    let module = ModuleId::of::<ShopModule>();
    let first = app.ctx().request_resolver(module, RequestScope::default());
    let cart = first.get::<Cart>().await.expect("cart");
    let checkout = first.get::<Checkout>().await.expect("checkout");
    assert!(Arc::ptr_eq(&cart, &checkout.cart), "one Cart per scope");
    assert!(Arc::ptr_eq(
        &cart,
        &first.get::<Cart>().await.expect("cart again")
    ));

    let scope = RequestScope::default();
    let second = app.ctx().request_resolver(module, scope.clone());
    let other_cart = second.get::<Cart>().await.expect("cart in second scope");
    assert!(
        !Arc::ptr_eq(&cart, &other_cart),
        "a new scope builds a new Cart"
    );
    let same_scope = app.ctx().request_resolver(module, scope);
    assert!(
        Arc::ptr_eq(&other_cart, &same_scope.get::<Cart>().await.expect("cart")),
        "clones of a RequestScope share its cache"
    );

    assert!(
        Arc::ptr_eq(&cart.catalog, &other_cart.catalog),
        "singletons are shared across scopes"
    );
    assert_eq!(constructions.cart.get(), 2);
}

#[tokio::test]
async fn transient_builds_every_time() {
    let (app, constructions) = shop().await;
    assert_eq!(
        constructions.token.get(),
        0,
        "transient providers are not built at boot"
    );
    let a = app.ctx().get::<Token>().await.expect("token");
    let b = app.ctx().get::<Token>().await.expect("token");
    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(constructions.token.get(), 2);
}

#[tokio::test]
async fn request_scoped_provider_outside_a_request_scope_is_an_error() {
    let (app, _) = shop().await;
    let error = app
        .ctx()
        .get::<Cart>()
        .await
        .err()
        .expect("no request scope");
    assert!(
        matches!(&error, KernelError::MissingRequestScope { provider } if provider.ends_with("Cart")),
        "{error:?}"
    );
}

/// A singleton built by a factory that (without declaring it) asks for a
/// request-scoped provider while being built lazily inside a request.
pub struct Leaky;

module!(LeakyModule, |m| {
    m.value(Catalog)
        .provider::<Cart>()
        .value(Constructions {
            cart: Counter::default(),
            token: Counter::default(),
        })
        .factory(
            Factory::<Leaky>::new(|r| async move {
                r.get::<Cart>().await?;
                Ok::<_, BoxError>(Arc::new(Leaky))
            })
            .scope(Scope::Transient),
        )
        .exports::<Leaky>();
});

#[tokio::test]
async fn singleton_cannot_capture_request_scoped_provider_at_runtime() {
    // A singleton resolves a transient that, without declaring it, resolves a
    // request-scoped provider. Static checks cannot see it; resolution must.
    struct Holder;
    impl Injectable for Holder {
        fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
            Box::pin(async move {
                r.get::<Leaky>().await?;
                Ok(Holder)
            })
        }
    }
    struct HolderModule;
    impl Module for HolderModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.import(LeakyModule).provider::<Holder>();
        }
    }
    let error = Navree::app()
        .module(HolderModule)
        .boot()
        .await
        .expect_err("inversion");
    assert!(
        matches!(
            error.0.as_slice(),
            [KernelError::ScopeInversion { module: "HolderModule", singleton, request_scoped }]
                if singleton.ends_with("Holder") && request_scoped.ends_with("Cart")
        ),
        "{error:?}"
    );
}

/// `Parent` holds a lazy handle to `Child`, and `Child` depends on `Parent`:
/// no cycle, because a lazy handle is not a graph edge.
pub struct Parent {
    pub child: Lazy<Child>,
}

impl Injectable for Parent {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Parent {
                child: r.lazy::<Child>(),
            })
        })
    }
}

pub struct Child {
    pub parent: Arc<Parent>,
}

impl Injectable for Child {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Parent>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Child {
                parent: r.get::<Parent>().await?,
            })
        })
    }
}

module!(FamilyModule, |m| {
    m.provider::<Parent>().provider::<Child>();
});

#[tokio::test]
async fn lazy_resolves_after_boot_without_graph_edge() {
    let app = Navree::app()
        .module(FamilyModule)
        .boot()
        .await
        .expect("boot without a cycle");
    let parent = app.ctx().get::<Parent>().await.expect("parent");
    let child = parent.child.get().await.expect("lazy child");
    assert!(Arc::ptr_eq(&child.parent, &parent));
    assert!(Arc::ptr_eq(
        &child,
        &parent.child.get().await.expect("lazy child again")
    ));
    assert!(Arc::ptr_eq(
        &child,
        &app.ctx().get::<Child>().await.expect("child")
    ));
}

fn assert_send_sync<T: Send + Sync>() {}

fn assert_send<T: Send>(_: &T) {}

#[tokio::test]
async fn handles_and_resolution_futures_are_send() {
    assert_send_sync::<navree_kernel::AppContext>();
    assert_send_sync::<navree_kernel::App>();
    assert_send_sync::<Resolver>();
    assert_send_sync::<RequestScope>();
    assert_send_sync::<Lazy<dyn std::any::Any + Send + Sync>>();
    assert_send_sync::<KernelError>();
    assert_send_sync::<navree_kernel::BootErrors>();

    // Handlers and spawned tasks await these, so their futures must be `Send`.
    let (app, _) = shop().await;
    let ctx = app.ctx().clone();
    assert_send(&ctx.get::<Catalog>());
    assert_send(&ctx.try_get::<Catalog>());
    assert_send(&ctx.get_named::<Catalog>("x"));
    assert_send(&ctx.get_all::<Catalog>());
    let resolver = ctx.request_resolver(ModuleId::of::<ShopModule>(), RequestScope::new());
    assert_send(&resolver.get::<Cart>());
    assert_send(&resolver.get_named::<Cart>("x"));
    assert_send(&resolver.get_all::<Cart>());
    let lazy = resolver.lazy::<Catalog>();
    assert_send(&lazy.get());
    assert!(Arc::ptr_eq(
        &lazy.get().await.expect("lazy catalog"),
        &ctx.get::<Catalog>().await.expect("catalog")
    ));
}
