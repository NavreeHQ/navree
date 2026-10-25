//! A process whose global subscriber was installed by someone else:
//! `LoggerModule` boots and leaves it in place; levels cannot be changed.

use keel_kernel::Keel;
use keel_log::{LogControl, LogError, LogOptions, LoggerModule};

#[tokio::test]
async fn boot_keeps_a_foreign_global_subscriber() {
    tracing::subscriber::set_global_default(tracing_subscriber::fmt().finish())
        .expect("first global subscriber in this process");
    let app = Keel::app()
        .module(LoggerModule::for_root(LogOptions::default()))
        .boot()
        .await
        .expect("boot");
    let control = app.ctx().get::<LogControl>().await.expect("control");
    let error = control
        .set_level("keel", "debug")
        .expect_err("not Keel's subscriber");
    assert!(matches!(error, LogError::NotInstalled), "{error:?}");
    assert_eq!(control.directives(), "");
    control.flush().expect("nothing to flush");
    app.shutdown().await.expect("shutdown");
}
