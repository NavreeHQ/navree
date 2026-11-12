//! The association files of the site's native apps (`auth.apps`):
//! `/.well-known/apple-app-site-association` and
//! `/.well-known/assetlinks.json`.

use navree_auth::{ANDROID_ASSOCIATION_PATH, APPLE_ASSOCIATION_PATH};
use serde_json::json;

use crate::support;

const FINGERPRINT: &str = "14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5";

#[tokio::test]
async fn the_association_files_are_served_as_json_without_redirect() {
    let app = support::builder()
        .config("auth.apps.paths", json!(["/orders/*"]))
        .config("auth.apps.exclude", json!(["/orders/*/print"]))
        .config("auth.apps.apple.orders.team_id", "ABCDE12345")
        .config("auth.apps.apple.orders.bundle_id", "com.example.orders")
        .config("auth.apps.android.orders.package", "com.example.orders")
        .config(
            "auth.apps.android.orders.fingerprints",
            json!([FINGERPRINT]),
        )
        .build()
        .await;
    let client = app.client();

    let apple = client
        .get(APPLE_ASSOCIATION_PATH)
        .send()
        .await
        .assert_status(200);
    assert_eq!(apple.header("content-type"), Some("application/json"));
    assert_eq!(apple.header("location"), None);
    let apple = apple.json_value();
    assert_eq!(
        apple["applinks"]["details"][0]["appIDs"],
        json!(["ABCDE12345.com.example.orders"])
    );
    assert_eq!(
        apple["applinks"]["details"][0]["components"],
        json!([{ "/": "/orders/*/print", "exclude": true }, { "/": "/orders/*" }])
    );
    assert_eq!(
        apple["webcredentials"]["apps"],
        json!(["ABCDE12345.com.example.orders"])
    );

    let android = client
        .get(ANDROID_ASSOCIATION_PATH)
        .send()
        .await
        .assert_status(200);
    assert_eq!(android.header("content-type"), Some("application/json"));
    let android = android.json_value();
    assert_eq!(android[0]["target"]["package_name"], "com.example.orders");
    assert_eq!(
        android[0]["target"]["sha256_cert_fingerprints"],
        json!([FINGERPRINT])
    );
    assert_eq!(
        android[0]["relation"],
        json!([
            "delegate_permission/common.handle_all_urls",
            "delegate_permission/common.get_login_creds"
        ])
    );

    // Exactly these paths: no slash added, no redirect, nothing under the
    // API prefix.
    for path in [
        format!("{APPLE_ASSOCIATION_PATH}/"),
        format!("{APPLE_ASSOCIATION_PATH}.json"),
        format!("/api{ANDROID_ASSOCIATION_PATH}"),
    ] {
        let response = client.get(&path).send().await;
        assert_eq!(response.status().as_u16(), 404, "{path}");
    }
}

#[tokio::test]
async fn no_association_files_without_configuration() {
    let app = support::app().await;
    let client = app.client();
    for path in [APPLE_ASSOCIATION_PATH, ANDROID_ASSOCIATION_PATH] {
        let response = client.get(path).send().await;
        assert_eq!(response.status().as_u16(), 404, "{path}");
    }
    // An Apple app with credentials only: the Apple file, and still no
    // Android file.
    let app = support::builder()
        .config("auth.apps.apple.orders.team_id", "ABCDE12345")
        .config("auth.apps.apple.orders.bundle_id", "com.example.orders")
        .build()
        .await;
    let client = app.client();
    let apple = client
        .get(APPLE_ASSOCIATION_PATH)
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        apple.json_value(),
        json!({ "webcredentials": { "apps": ["ABCDE12345.com.example.orders"] } })
    );
    let response = client.get(ANDROID_ASSOCIATION_PATH).send().await;
    assert_eq!(response.status().as_u16(), 404);
}
