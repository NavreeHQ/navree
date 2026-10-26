//! Boot-time checks: provider cycles, module import cycles, scope inversion,
//! duplicate providers.

use std::sync::Arc;
use std::time::Duration;

use navis_kernel::{BoxError, Factory, KernelError, Navis};

use crate::support::{boot_errors, injectable, module};

injectable!(Left { right: Right });
injectable!(Right { left: Left });

module!(CyclicProviders, |m| {
    m.provider::<Left>().provider::<Right>();
});

#[tokio::test]
async fn detects_provider_cycle() {
    let errors = Navis::app()
        .module(CyclicProviders)
        .validate()
        .expect_err("cycle");
    let [KernelError::ProviderCycle { path }] = errors.0.as_slice() else {
        panic!("expected one provider cycle, got {errors:?}");
    };
    assert_eq!(path.len(), 3, "{path:?}");
    assert_eq!(path.first(), path.last(), "{path:?}");
    assert!(path.iter().any(|p| p.ends_with("Left")) && path.iter().any(|p| p.ends_with("Right")));
    assert!(errors.to_string().contains(" -> "), "{errors}");
}

/// Factories without declared dependencies: the cycle is only visible at runtime.
pub struct Ping;
pub struct Pong;

module!(UndeclaredCycle, |m| {
    m.factory(Factory::<Ping>::new(|r| async move {
        r.get::<Pong>().await?;
        Ok::<_, BoxError>(Arc::new(Ping))
    }))
    .factory(Factory::<Pong>::new(|r| async move {
        r.get::<Ping>().await?;
        Ok::<_, BoxError>(Arc::new(Pong))
    }));
});

#[tokio::test]
async fn detects_provider_cycle_at_runtime_without_deadlock() {
    let boot = Navis::app().module(UndeclaredCycle).boot();
    let errors = tokio::time::timeout(Duration::from_secs(5), boot)
        .await
        .expect("cycle detection must not deadlock")
        .expect_err("cycle");
    let [KernelError::ProviderCycle { path }] = errors.0.as_slice() else {
        panic!("expected one provider cycle, got {errors:?}");
    };
    assert_eq!(path.len(), 3, "{path:?}");
    assert_eq!(path.first(), path.last(), "{path:?}");
}

module!(AModule, |m| {
    m.import(BModule);
});

module!(BModule, |m| {
    m.import(CModule);
});

module!(CModule, |m| {
    m.import(AModule);
});

#[tokio::test]
async fn detects_module_import_cycle() {
    let errors = boot_errors(AModule).await;
    assert!(
        matches!(
            errors.0.as_slice(),
            [KernelError::ModuleCycle { path }] if path == &["AModule", "BModule", "CModule", "AModule"]
        ),
        "{errors:?}"
    );
}

injectable!(CurrentUser [Request] {});
injectable!(Session [Transient] { user: CurrentUser });
injectable!(Mailer { user: CurrentUser });
injectable!(Reports { session: Session });

module!(DirectInversion, |m| {
    m.provider::<CurrentUser>().provider::<Mailer>();
});

module!(TransitiveInversion, |m| {
    m.provider::<CurrentUser>()
        .provider::<Session>()
        .provider::<Reports>();
});

#[tokio::test]
async fn rejects_singleton_depending_on_request_scoped() {
    let errors = Navis::app()
        .module(DirectInversion)
        .validate()
        .expect_err("inversion");
    assert!(
        matches!(
            errors.0.as_slice(),
            [KernelError::ScopeInversion { module: "DirectInversion", singleton, request_scoped }]
                if singleton.ends_with("Mailer") && request_scoped.ends_with("CurrentUser")
        ),
        "{errors:?}"
    );
    let text = errors.to_string();
    assert!(text.contains("DirectInversion"), "names the module: {text}");
    assert!(
        text.contains(std::any::type_name::<Mailer>()),
        "names the singleton: {text}"
    );
    assert!(
        text.contains(std::any::type_name::<CurrentUser>()),
        "names the request-scoped type: {text}"
    );
}

#[tokio::test]
async fn rejects_singleton_reaching_request_scoped_through_transient() {
    let errors = Navis::app()
        .module(TransitiveInversion)
        .validate()
        .expect_err("inversion");
    assert!(
        matches!(
            errors.0.as_slice(),
            [KernelError::ScopeInversion { module: "TransitiveInversion", singleton, request_scoped }]
                if singleton.ends_with("Reports") && request_scoped.ends_with("CurrentUser")
        ),
        "{errors:?}"
    );
}

module!(DuplicateKeys, |m| {
    m.value(1_u8).value(2_u8);
});

#[tokio::test]
async fn rejects_duplicate_provider_in_one_module() {
    let errors = boot_errors(DuplicateKeys).await;
    assert!(
        matches!(
            errors.0.as_slice(),
            [KernelError::DuplicateProvider { module: "DuplicateKeys", key }] if key == "u8"
        ),
        "{errors:?}"
    );
}
