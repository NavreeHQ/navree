//! Runs the repository's disk conformance suite against `GodamModule` both at
//! the store root and under a configured folder prefix.

#[path = "../../../crates/navree-storage/tests/storage/support.rs"]
mod support;

#[path = "../../../crates/navree-storage/tests/storage/conformance.rs"]
#[allow(dead_code)]
mod conformance;

use navree::prelude::*;
use navree::storage::Disk;
use navree_plugin_godam::GodamModule;
use serde_json::json;

use crate::conformance::{Http, Subject};
use crate::support::TestApp;

fn uploads(disk: serde_json::Value) -> ConfigSources {
    ConfigSources::new().set(
        "storage",
        json!({ "default": "uploads", "disks": { "uploads": disk } }),
    )
}

#[tokio::test]
async fn godam_passes_the_disk_conformance_suite() {
    for disk_table in [
        json!({ "driver": "godam" }),
        json!({ "driver": "godam", "prefix": "ludhiana/" }),
    ] {
        let app = TestApp::boot(
            Navree::app()
                .module(ConfigModule::for_root(uploads(disk_table)))
                .module(StorageModule::from_config())
                .module(GodamModule),
        )
        .await;
        let disk = app.app.ctx().get::<Disk>().await.unwrap();
        assert_eq!((disk.name(), disk.driver()), ("uploads", "godam"));

        let subject = Subject {
            disk: (*disk).clone(),
            http: Http::app(app.router.clone()),
        };
        conformance::run(&subject).await;
    }
}
