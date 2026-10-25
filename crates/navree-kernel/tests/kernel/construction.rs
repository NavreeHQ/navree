//! Errors raised while constructing providers at boot.

use std::sync::Arc;

use navree_kernel::{BoxError, Factory, KernelError};

use crate::support::{boot_errors, module};

pub struct Mailer;
pub struct Unregistered;

module!(FailingFactory, |m| {
    m.factory(Factory::<Mailer>::new(|_| async {
        Err::<Arc<Mailer>, BoxError>("smtp host unreachable".into())
    }));
});

#[tokio::test]
async fn factory_error_is_reported_as_construction_naming_the_provider() {
    let errors = boot_errors(FailingFactory).await;
    let [KernelError::Construction { provider, source }] = errors.0.as_slice() else {
        panic!("expected one construction error, got {errors:?}");
    };
    assert!(provider.ends_with("Mailer"), "{provider}");
    assert_eq!(source.to_string(), "smtp host unreachable");
    assert!(
        errors.to_string().contains("failed to construct"),
        "{errors}"
    );
}

module!(UndeclaredMissing, |m| {
    m.factory(Factory::<Mailer>::new(|r| async move {
        r.get::<Unregistered>().await?;
        Ok::<_, BoxError>(Arc::new(Mailer))
    }));
});

#[tokio::test]
async fn resolution_errors_inside_factories_keep_their_kind() {
    let errors = boot_errors(UndeclaredMissing).await;
    assert!(
        matches!(
            errors.0.as_slice(),
            [KernelError::MissingProvider { module: "UndeclaredMissing", requested, required_by }]
                if requested.ends_with("Unregistered") && required_by.contains("Mailer")
        ),
        "{errors:?}"
    );
}
