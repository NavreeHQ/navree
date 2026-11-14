//! `plugin:publish --tag config` and `config:publish --plugin`, end to end:
//! an app's command line, run in a folder of its own as the current directory.

use std::fs;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use navree_config::{ConfigModule, ConfigSection, ConfigSources, ModuleBuilderConfigExt};
use navree_kernel::{Module, ModuleBuilder, Navree, plugin_info};
use serde::Deserialize;

/// Held by each test while it runs in its folder (the current directory is the process's).
static LOCK: Mutex<()> = Mutex::new(());

/// On drop, even when the test fails, puts the current directory back to the package's
/// folder, so the temporary folder can be removed after it.
struct BackToThePackage;

impl Drop for BackToThePackage {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"));
    }
}

/// Runs `f` with a new empty folder as the current directory, then removes it.
fn in_app_folder(f: impl FnOnce(&Path)) {
    // A test that failed while holding the lock poisoned it; the next one takes it over.
    let _lock = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let folder = tempfile::tempdir().expect("a temporary folder");
    // Dropped before `folder`: the current directory leaves the folder first.
    let _back = BackToThePackage;
    std::env::set_current_dir(folder.path()).expect("the folder as the current directory");
    f(folder.path());
}

/// `run_with_args(args)` for an app of `ConfigModule`, `AnnouncementsModule` and `CrmModule`;
/// the error as text.
fn run(args: &[&str]) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let args = args.iter().map(|arg| (*arg).to_owned()).collect();
    runtime
        .block_on(
            Navree::app()
                .module(ConfigModule::for_root(ConfigSources::new()))
                .module(AnnouncementsModule)
                .module(CrmModule)
                .run_with_args(args),
        )
        .map_err(|error| error.to_string())
}

/// Every file and folder below `root`, `/`-separated, sorted.
fn files_under(root: &Path) -> Vec<String> {
    fn walk(folder: &Path, prefix: &str, out: &mut Vec<String>) {
        for entry in fs::read_dir(folder).expect("a folder") {
            let entry = entry.expect("a folder entry");
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                walk(&entry.path(), &format!("{name}/"), out);
            }
            out.push(name);
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out.sort();
    out
}

#[derive(Debug, Default, Deserialize, ConfigSection)]
#[config(key = "announcements")]
#[serde(default)]
struct AnnouncementsOptions {
    /// How long a notice stays up, in days.
    notice_days: u32,
}

#[derive(Debug, Default, Deserialize, ConfigSection)]
#[config(key = "announcements.feed")]
#[serde(default)]
struct FeedOptions {
    /// Notices in the feed.
    size: u32,
}

#[derive(Debug, Default, Deserialize, ConfigSection)]
#[config(key = "crm")]
#[serde(default)]
struct CrmOptions {
    /// The currency of deals, such as INR.
    currency: String,
}

/// A plugin with the namespace `announcements` and two sections, `announcements` and
/// `announcements.feed`.
struct AnnouncementsModule;

impl Module for AnnouncementsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(plugin_info!(namespace = "announcements"))
            .config_section::<AnnouncementsOptions>()
            .config_section::<FeedOptions>();
    }
}

/// A module of the app, not a plugin, with the section `crm`.
struct CrmModule;

impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config_section::<CrmOptions>();
    }
}

#[test]
fn plugin_publish_config_runs_config_publish_for_the_plugin() {
    in_app_folder(|root| {
        run(&["plugin:publish", "announcements", "--tag", "config"]).expect("published");
        assert_eq!(
            files_under(root),
            [
                "config",
                "config/announcements.feed.toml",
                "config/announcements.toml",
            ]
        );
        assert!(
            fs::read_to_string(root.join("config/announcements.toml"))
                .unwrap()
                .contains("notice_days")
        );

        fs::write(
            root.join("config/announcements.toml"),
            "# Simran Kaur's settings\n",
        )
        .unwrap();
        run(&["plugin:publish", "announcements", "--tag", "config"]).expect("kept");
        assert_eq!(
            fs::read_to_string(root.join("config/announcements.toml")).unwrap(),
            "# Simran Kaur's settings\n"
        );
        run(&["config:publish", "--plugin", "announcements", "--force"]).expect("replaced");
        assert!(
            fs::read_to_string(root.join("config/announcements.toml"))
                .unwrap()
                .contains("notice_days")
        );

        fs::remove_dir_all(root.join("config")).unwrap();
        run(&["plugin:publish", "announcements", "--dry-run"]).expect("dry run");
        assert!(
            !root.join("config").exists(),
            "a dry run runs no config:publish"
        );

        assert_eq!(
            run(&["config:publish", "--plugin", "weather"]).expect_err("unknown"),
            "config:publish: no plugin has the namespace `weather`"
        );
    });
}
