//! A process whose global subscriber was installed by someone else:
//! `LoggerModule` boots and leaves it in place; levels cannot be changed.

use navis_kernel::Navis;
use navis_log::{LogControl, LogError, LogOptions, LoggerModule};

#[tokio::test]
async fn boot_keeps_a_foreign_global_subscriber() {
    tracing::subscriber::set_global_default(tracing_subscriber::fmt().finish())
        .expect("first global subscriber in this process");
    let app = Navis::app()
        .module(LoggerModule::for_root(LogOptions::default()))
        .boot()
        .await
        .expect("boot");
    let control = app.ctx().get::<LogControl>().await.expect("control");
    let error = control
        .set_level("navis", "debug")
        .expect_err("not Navis's subscriber");
    assert!(matches!(error, LogError::NotInstalled), "{error:?}");
    assert_eq!(control.directives(), "");
    control.flush().expect("nothing to flush");
    app.shutdown().await.expect("shutdown");
}
