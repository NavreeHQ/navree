//! `docs/plugins/overview.mdx`: a module that names itself a plugin is listed by `plugins`,
//! and the overview page carries the two paragraphs stating what a community plugin cannot do
//! and what is not enforced.

use std::fs;
use std::path::Path;

use navree::testing::TestApp;
use serde_json::Value;

/// A community plugin of its own crate. Because an integration test binary compiles as a single
/// crate, this nested module stands in for the plugin's crate (`module_path!()`), where a standalone
/// plugin crate calls `navree::plugin_info!(title = "Notice board")`.
mod notice_board {
    use navree::plugin::PluginInfo;
    use navree::prelude::*;

    pub struct NoticeBoardModule;

    impl Module for NoticeBoardModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(
                PluginInfo::new("navree-plugin-notices", module_path!(), "notices", "0.1.0")
                    .title("Notice board"),
            );
        }
    }
}

const WHAT_A_PLUGIN_CANNOT_DO: &str = "\
A plugin is code you compile into your app, so it is not a sandbox. But the framework's \
registration APIs give a plugin that is not one of Navree's own packages no way to change who \
may do what without a line in your app. A module of such a plugin, or a module that acts for it \
(a module of its crate, or a generic module that no module of your app imports), stops the boot \
when it registers a policy with rules beyond the plugin's own subjects or on `all`; registers a \
subject named outside the plugin's namespace; configures a module of another crate; provides a \
permission check, the route authorizer, the authenticator, a sign-in strategy, the workspace \
gate, the options of access, workspaces, authentication or the admin panel, or your app's \
shell; adds a layer that runs on every request; decides who works as staff in the admin panel; \
authorizes a realtime channel outside its namespace; scopes a subject that is not its own to \
workspaces; or registers a global scope or a model hook on a model of another crate. To use any \
of these, register the plugin's piece from a module of your app.";

const WHAT_IS_NOT_ENFORCED: &str = "\
A plugin's rules on its own subjects are its decision (your abilities function can override \
them). Code a plugin runs (its handlers, jobs and migrations) can call `AccessService` like any \
code, and its own routes, tools and MCP items may be public. A module of a third crate that \
names no plugin counts as yours wherever it is imported, with what it registers; a generic \
module, such as the one `AccessModule::for_policy` makes, is yours only when a module of your \
app imports it.";

fn normalise_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn section_under<'a>(markdown: &'a str, heading: &str) -> Option<&'a str> {
    let mut start_offset = None;
    let mut offset = 0;
    for line in markdown.split('\n') {
        let next_offset = offset + line.len() + 1;
        if start_offset.is_none() {
            if line.trim_end() == heading {
                start_offset = Some(next_offset.min(markdown.len()));
            }
        } else if line.starts_with("## ") {
            let from = start_offset.unwrap();
            return Some(&markdown[from..offset]);
        }
        offset = next_offset;
    }
    start_offset.map(|from| &markdown[from..])
}

#[tokio::test]
async fn a_module_that_names_itself_is_listed_by_plugins() {
    let app = TestApp::builder()
        .module(notice_board::NoticeBoardModule)
        .build()
        .await;

    let report = app.ctx().plugins();
    assert_eq!(report.plugins.len(), 1);
    let plugin = &report.plugins[0];
    assert_eq!(plugin.info.name, "navree-plugin-notices");
    assert_eq!(plugin.info.namespace, "notices");
    assert_eq!(plugin.info.version, "0.1.0");
    assert_eq!(plugin.info.title, Some("Notice board"));
    assert!(!plugin.first_party);
    assert_eq!(plugin.modules.len(), 1);
    assert_eq!(plugin.modules[0].id.name(), "NoticeBoardModule");

    let raw_json = report.to_json();
    let parsed: Value = serde_json::from_str(&raw_json).expect("valid plugins json");
    assert_eq!(parsed["plugins"][0]["name"], "navree-plugin-notices");
    assert!(
        raw_json.contains("\"name\":\"navree-plugin-notices\""),
        "{raw_json}"
    );

    app.shutdown().await.expect("clean shutdown");
}

#[test]
fn the_overview_page_carries_the_two_paragraphs_the_framework_enforces() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins/overview.mdx");
    let raw = fs::read_to_string(&path).expect("read docs/plugins/overview.mdx");
    let page = raw.replace("\r\n", "\n");

    let cannot_do = section_under(&page, "## What a plugin cannot do")
        .expect("## What a plugin cannot do section in docs/plugins/overview.mdx");
    assert_eq!(
        normalise_whitespace(cannot_do),
        normalise_whitespace(WHAT_A_PLUGIN_CANNOT_DO)
    );

    let not_enforced = section_under(&page, "## What is not enforced")
        .expect("## What is not enforced section in docs/plugins/overview.mdx");
    assert_eq!(
        normalise_whitespace(not_enforced),
        normalise_whitespace(WHAT_IS_NOT_ENFORCED)
    );
}
