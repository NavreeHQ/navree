use std::sync::Arc;

use navree_kernel::{
    BoxFuture, Injectable, Navree, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};

use crate::support::{Counter, injectable};

/// Constructed once per app; counts its constructions in an injected counter.
pub struct Repo;

impl Injectable for Repo {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Counter>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            r.get::<Counter>().await?.hit();
            Ok(Repo)
        })
    }
}

injectable!(ServiceA { repo: Repo });
injectable!(ServiceB { repo: Repo });

struct AppModule {
    counter: Arc<Counter>,
}

impl Module for AppModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc(self.counter.clone())
            .provider::<Repo>()
            .provider::<ServiceA>()
            .provider::<ServiceB>();
    }
}

#[tokio::test]
async fn boots_singleton_once_and_shares_arc() {
    let counter = Arc::new(Counter::default());
    let app = Navree::app()
        .module(AppModule {
            counter: counter.clone(),
        })
        .boot()
        .await
        .expect("boot");

    let a = app.ctx().get::<ServiceA>().await.expect("ServiceA");
    let b = app.ctx().get::<ServiceB>().await.expect("ServiceB");
    let repo = app.ctx().get::<Repo>().await.expect("Repo");

    assert!(
        Arc::ptr_eq(&a.repo, &b.repo),
        "both services share one Repo"
    );
    assert!(
        Arc::ptr_eq(&a.repo, &repo),
        "the container hands out the same Repo"
    );
    assert!(Arc::ptr_eq(
        &a,
        &app.ctx().get::<ServiceA>().await.expect("ServiceA again")
    ));
    assert_eq!(counter.get(), 1, "Repo is constructed exactly once");
}

#[tokio::test]
async fn weak_app_context_does_not_keep_the_app_alive() {
    let app = Navree::app()
        .module(AppModule {
            counter: Arc::new(Counter::default()),
        })
        .boot()
        .await
        .expect("boot");
    let weak = app.ctx().downgrade();

    let upgraded = weak.upgrade().expect("the app is alive");
    upgraded
        .get::<ServiceA>()
        .await
        .expect("resolves through the upgraded handle");
    drop(upgraded);
    let copy = weak.clone();

    drop(app);

    assert!(weak.upgrade().is_none(), "the app was dropped");
    assert!(copy.upgrade().is_none());
}
