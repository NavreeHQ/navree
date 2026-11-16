//! The system app's exact module set passes the checks the kernel runs on plugins before
//! anything is built, and holds no plugin.

use navree::config::ConfigSources;

#[test]
fn the_system_app_has_no_plugin_and_passes_the_plugin_checks() {
    let report = system_app::app(ConfigSources::new())
        .plugin_report()
        .expect("the system app's modules pass the plugin checks");
    assert!(report.plugins.is_empty(), "{report}");
}
