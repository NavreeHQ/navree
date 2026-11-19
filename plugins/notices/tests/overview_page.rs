//! Checks that the `navree plugins` listing on `docs/plugins/overview.mdx` matches what
//! `NoticesModule::from_config()` prints at boot.

#![allow(missing_docs)]

use std::fs;
use std::path::Path;

use navree::orm::OrmModule;
use navree::prelude::*;
use navree::testing::TestApp;
use navree::ui::UiModule;
use navree::view::ViewModule;
use navree_plugin_notices::NoticesModule;

/// Extracts the first fenced `text` block inside the `### Listing plugins` section of `page`.
fn first_text_block_under_listing_plugins(page: &str) -> String {
    let mut in_section = false;
    let mut open_ticks: Option<(usize, usize, String)> = None;
    let mut lines: Vec<String> = Vec::new();

    for line in page.split('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let ticks = trimmed.chars().take_while(|&ch| ch == '`').count();

        if open_ticks.is_none() {
            if !in_section {
                if line.trim_end() == "### Listing plugins" {
                    in_section = true;
                }
                continue;
            }
            if line.starts_with("## ") || line.starts_with("### ") {
                break;
            }
            if ticks >= 3 {
                let info = trimmed[ticks..].trim().to_owned();
                open_ticks = Some((ticks, indent, info));
                lines.clear();
            }
        } else if let Some((opened, at, info)) = &open_ticks {
            if ticks >= *opened && trimmed.trim_end().len() == ticks {
                let lang = info
                    .split([',', ' '])
                    .find(|word| !word.is_empty())
                    .unwrap_or_default();
                if lang == "text" {
                    return lines.join("\n");
                }
                open_ticks = None;
                lines.clear();
            } else {
                let cut = line.len().min(*at);
                let strip = line[..cut].trim().is_empty();
                lines.push(if strip { &line[cut..] } else { line }.to_owned());
            }
        }
    }

    panic!("missing `text` block under `### Listing plugins` in docs/plugins/overview.mdx");
}

#[tokio::test]
async fn the_plugins_output_on_the_overview_page_is_what_the_example_prints() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(NoticesModule::from_config())
        .build()
        .await;

    let overview_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins/overview.mdx");
    let raw = fs::read_to_string(&overview_path).expect("read docs/plugins/overview.mdx");
    let page = raw.replace("\r\n", "\n");
    let documented = first_text_block_under_listing_plugins(&page);

    assert_eq!(app.ctx().plugins().to_string(), documented);

    app.shutdown().await.expect("clean shutdown");
}
