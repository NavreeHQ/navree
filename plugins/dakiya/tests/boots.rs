//! Boots `navree-plugin-dakiya` in [`TestApp`] and checks its registration and settings.

use std::sync::Arc;

use navree::http_client::{HttpClient, HttpClientModule};
use navree::mail::{MailModule, Mailer};
use navree::testing::TestApp;
use navree::view::ViewModule;
use navree_plugin_dakiya::DakiyaModule;

#[tokio::test]
async fn the_plugin_boots_and_is_listed() {
    let app = TestApp::builder().module(DakiyaModule).build().await;
    let report = app.ctx().plugins();
    let plugin = report
        .plugins
        .iter()
        .find(|plugin| plugin.info.name == "navree-plugin-dakiya")
        .expect("navree-plugin-dakiya is listed in plugins");
    assert_eq!(plugin.info.namespace, "dakiya");
    assert_eq!(plugin.info.title, Some("Dakiya mail"));
    assert!(!plugin.first_party);
    app.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn a_transport_named_dakiya_is_selected_by_the_configuration() {
    let (client, _server) = HttpClient::fake();
    let app = TestApp::builder()
        .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
        .config("mail.transport", "dakiya")
        .config("mail.dakiya.url", "https://api.dakiya.example/v1/send")
        .config("mail.dakiya.api_key", "test-only-key-ludhiana")
        .config("mail.dakiya.sender_name", "Ludhiana Desk")
        .module(HttpClientModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(DakiyaModule)
        .override_value(Arc::new(client))
        .build()
        .await;

    let mailer = app.get::<Mailer>().await;
    assert_eq!(mailer.transport().name(), "dakiya");

    app.run_command("config:check", &[])
        .await
        .expect("config:check reports no unknown key of mail.dakiya");

    app.shutdown().await.expect("clean shutdown");
}
