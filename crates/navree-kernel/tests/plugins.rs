//! Plugins: what a module says about itself, the boot checks, the report and the
//! `plugins` built-in. This file's crate is `plugins`, in the package `navree-kernel`.

use navree_kernel::{PluginInfo, plugin_info};

#[test]
fn plugin_info_fills_every_field_from_the_calling_crate() {
    let info = plugin_info!();
    assert_eq!(info.name, "navree-kernel");
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.namespace, "kernel");
    assert_eq!(info.title, None);
    assert_eq!(info.description, Some(env!("CARGO_PKG_DESCRIPTION")));
    assert_eq!(info.homepage, Some("https://navree.com"));
    assert_eq!(info.repository, Some("https://github.com/NavreeHQ/navree"));
    assert_eq!(info.license, Some("MIT OR Apache-2.0"));
}

#[test]
fn plugin_info_records_the_rust_crate_name_even_when_lib_name_differs() {
    // The package is `navree-kernel`; the crate this test is compiled in is `plugins`.
    let info = plugin_info!();
    assert_eq!(info.crate_name, "plugins");
    assert_eq!(info.name, "navree-kernel");
}

#[test]
fn plugin_info_takes_a_namespace_and_a_title() {
    let both = plugin_info!(namespace = "notice_board", title = "Notice board");
    assert_eq!(
        (both.namespace, both.title),
        ("notice_board", Some("Notice board"))
    );
    assert_eq!(
        plugin_info!(title = "Notice board", namespace = "notice_board",),
        both
    );
    assert_eq!(plugin_info!(title = "Notices").namespace, "kernel");
    assert_eq!(plugin_info!(namespace = "notices").title, None);
    assert_eq!(plugin_info!(title = "").title, None);
    const AT_COMPILE_TIME: PluginInfo = plugin_info!(namespace = "notices");
    assert_eq!(AT_COMPILE_TIME.namespace, "notices");

    let by_hand = PluginInfo::new(
        "navree-plugin-announcements",
        "navree_plugin_announcements",
        "announcements",
        "1.0.0",
    )
    .title("Announcements")
    .description("")
    .repository("https://code.ludhiana.example/navree-plugin-announcements");
    assert_eq!(by_hand.description, None);
    assert_eq!(by_hand.homepage, None);
    assert_eq!(
        by_hand.repository,
        Some("https://code.ludhiana.example/navree-plugin-announcements")
    );
}
