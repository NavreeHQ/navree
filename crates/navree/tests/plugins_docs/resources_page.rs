//! `docs/plugins/resources.mdx`: embedded templates under a plugin's namespace, publishing
//! them into an app folder with `plugin:publish`, and disk files winning over embedded ones.

use std::fs;
use std::path::Path;
use std::process::Command;

use navree::prelude::*;
use navree::testing::TestApp;
use navree::view::{ViewModule, Views};
use serde_json::json;

const CHILD_ENV: &str = "NAVREE_PLUGINS_DOCS_RESOURCES_PUBLISH_ARGS";
const EMBEDDED_HTML: &str = "<p>Notices from the Ludhiana office</p>\n";

/// A community plugin standing in for its own crate (`module_path!()`, as an integration test
/// binary compiles as one crate).
mod notices_resources {
    use navree::plugin::PluginInfo;
    use navree::plugin::prelude::*;
    use navree::prelude::*;

    const NOTICES: PluginInfo =
        PluginInfo::new("navree-plugin-notices", module_path!(), "notices", "0.1.0")
            .title("Notice board");

    /// Embeds `tests/fixtures/plugins_docs_resources/views/list.html` as `notices/list.html`.
    pub struct NoticesResourcesModule;

    impl Module for NoticesResourcesModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(NOTICES).plugin_templates(navree::embed_templates!(
                "tests/fixtures/plugins_docs_resources/views"
            ));
        }
    }

    /// Attempts to register a template outside `notices/` via `m.templates`.
    pub struct OutsideNamespaceModule;

    impl Module for OutsideNamespaceModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(NOTICES)
                .templates(&[("mail/welcome.html", "<p>Welcome to Ludhiana</p>")]);
        }
    }
}

fn run_publish_in(dir: &Path, args: &[&str]) -> String {
    let encoded = serde_json::to_string(&args).expect("encode publish args");
    let output = Command::new(std::env::current_exe().expect("plugins_docs test binary"))
        .arg("--exact")
        .arg("resources_page::a_published_template_wins_over_the_embedded_one")
        .arg("--nocapture")
        .current_dir(dir)
        .env(CHILD_ENV, encoded)
        .output()
        .expect("spawn child publish process");
    assert!(
        output.status.success(),
        "command {args:?} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}

#[tokio::test]
async fn a_published_template_wins_over_the_embedded_one() {
    if let Ok(encoded) = std::env::var(CHILD_ENV) {
        let args: Vec<String> = serde_json::from_str(&encoded).expect("decode publish args");
        Navree::app()
            .module(ConfigModule::for_root(ConfigSources::new()))
            .module(notices_resources::NoticesResourcesModule)
            .run_with_args(args)
            .await
            .expect("plugin:publish runs cleanly");
        return;
    }

    // Without a template file on disk, rendering uses the embedded fixture.
    let app = TestApp::builder()
        .module(ViewModule::from_config())
        .module(notices_resources::NoticesResourcesModule)
        .build()
        .await;
    let views = app.get::<Views>().await;
    let rendered = views
        .render("notices/list", json!({}))
        .expect("render embedded notices/list");
    assert_eq!(rendered, EMBEDDED_HTML);
    app.shutdown().await.expect("clean shutdown");

    // Publish into a temporary app folder and check the file and output.
    let temp = tempfile::tempdir().expect("temp app dir");
    let root = temp.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"ludhiana-office\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("write Cargo.toml");

    let first = run_publish_in(root, &["plugin:publish", "notices", "--tag", "views"]);
    assert!(
        first.contains("resources/views/notices/list.html: written"),
        "{first}"
    );
    let published_file = root.join("resources/views/notices/list.html");
    assert_eq!(
        fs::read_to_string(&published_file).expect("read published list.html"),
        EMBEDDED_HTML
    );

    // Edit the published copy; when `view.dirs` points at the app's template folder, it wins.
    let edited = "<p>Our own notices</p>";
    fs::write(&published_file, edited).expect("write edited template");

    let view_dir = root.join("resources/views").to_string_lossy().into_owned();
    let overridden = TestApp::builder()
        .config("view.dirs", json!([view_dir]))
        .module(ViewModule::from_config())
        .module(notices_resources::NoticesResourcesModule)
        .build()
        .await;
    let views = overridden.get::<Views>().await;
    assert_eq!(
        views
            .render("notices/list", json!({}))
            .expect("render overridden notices/list"),
        edited
    );
    overridden.shutdown().await.expect("clean shutdown");

    // A second `plugin:publish` without `--force` keeps the edited file and says so.
    let second = run_publish_in(root, &["plugin:publish", "notices", "--tag", "views"]);
    assert!(
        second.contains(
            "resources/views/notices/list.html: exists, kept (use --force to replace it)"
        ),
        "{second}"
    );
    assert_eq!(
        fs::read_to_string(&published_file).expect("read kept list.html"),
        edited
    );

    // `--dry-run` (even alongside `--force`) writes nothing.
    let dry = run_publish_in(
        root,
        &[
            "plugin:publish",
            "notices",
            "--tag",
            "views",
            "--force",
            "--dry-run",
        ],
    );
    assert!(dry.contains("Nothing was written (--dry-run)."), "{dry}");
    assert_eq!(
        fs::read_to_string(&published_file).expect("read unchanged list.html after dry-run"),
        edited
    );

    // `--force` replaces the edited file with the plugin's embedded template.
    let forced = run_publish_in(
        root,
        &["plugin:publish", "notices", "--tag", "views", "--force"],
    );
    assert!(
        forced.contains("resources/views/notices/list.html: written"),
        "{forced}"
    );
    assert_eq!(
        fs::read_to_string(&published_file).expect("read forced list.html"),
        EMBEDDED_HTML
    );

    // Registering a template outside `notices/` through `m.templates` stops the boot.
    let err = TestApp::builder()
        .module(ViewModule::from_config())
        .module(notices_resources::OutsideNamespaceModule)
        .try_build()
        .await
        .expect_err("template outside plugin namespace must fail boot")
        .to_string();
    assert!(
        err.contains("navree-plugin-notices") && err.contains("mail/welcome.html"),
        "{err}"
    );
}
