//! Boots `navree-plugin-godam` in [`TestApp`] and serves a `godam` disk.

use navree::prelude::*;
use navree::storage::Disk;
use navree::testing::TestApp;
use navree_plugin_godam::GodamModule;
use serde_json::json;

#[tokio::test]
async fn the_plugin_boots_and_is_listed() {
    let app = TestApp::builder().module(GodamModule).build().await;
    let report = app.ctx().plugins();
    let plugin = report
        .plugins
        .iter()
        .find(|plugin| plugin.info.name == "navree-plugin-godam")
        .expect("navree-plugin-godam is listed in plugins");
    assert_eq!(plugin.info.namespace, "godam");
    assert_eq!(plugin.info.title, Some("Godam disks"));
    assert!(!plugin.first_party);
    app.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn a_disk_naming_godam_is_served_by_it() {
    let app = TestApp::builder()
        .config("storage.default", "invoices")
        .config(
            "storage.disks.invoices",
            json!({ "driver": "godam", "prefix": "ludhiana/" }),
        )
        .module(StorageModule::from_config())
        .module(GodamModule)
        .build()
        .await;

    let disk = app.get::<Disk>().await;
    assert_eq!(disk.name(), "invoices");
    assert_eq!(disk.driver(), "godam");

    disk.put("2026/inv-1101.txt", "Invoice 1101, ₹1,00,000")
        .await
        .expect("put succeeds");
    let contents = disk.get("2026/inv-1101.txt").await.expect("get succeeds");
    assert_eq!(contents, "Invoice 1101, ₹1,00,000");

    app.shutdown().await.expect("clean shutdown");
}
