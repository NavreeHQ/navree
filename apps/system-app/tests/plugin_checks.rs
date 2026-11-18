//! The system app's exact module set passes the checks the kernel runs on plugins before
//! anything is built, and every plugin it names is one of the framework's own.

use navree::config::ConfigSources;

#[test]
fn the_system_app_passes_the_plugin_checks_and_names_only_first_party_plugins() {
    let report = system_app::app(ConfigSources::new())
        .plugin_report()
        .expect("the system app's modules pass the plugin checks");
    assert!(!report.plugins.is_empty(), "{report}");
    assert!(
        report.plugins.iter().all(|plugin| plugin.first_party),
        "{report}"
    );
}
