use std::sync::Arc;

use keel_kernel::{
    BoxFuture, Injectable, Keel, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
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
    let app = Keel::app()
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
