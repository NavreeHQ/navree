//! `plugin:publish --tag config` and `config:publish --plugin`, end to end:
//! an app's command line, run in a folder of its own as the current directory,
//! and the plugin mode of `publish_section` on a folder of its own.

use std::fs;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use navree_config::{
    ConfigModule, ConfigRegistry, ConfigSection, ConfigSources, FieldSchema, FieldType,
    ModuleBuilderConfigExt, PublishOptions, SectionRegistration, SectionSchema, publish_section,
};
use navree_kernel::{Module, ModuleBuilder, Navree, PublishTag, Publishable, plugin_info};
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

/// A plugin with the namespace `announcements`, two sections, `announcements` and
/// `announcements.feed`, and one views file, `list.html`.
struct AnnouncementsModule;

impl Module for AnnouncementsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(plugin_info!(namespace = "announcements"))
            .config_section::<AnnouncementsOptions>()
            .config_section::<FeedOptions>()
            .contribute(Publishable::new(PublishTag::Views).file(
                "list.html",
                "<ul><li>Fee for Ludhiana: ₹1,00,000</li></ul>".as_bytes(),
            ));
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

#[test]
fn plugin_publish_writes_no_file_when_config_publish_refuses() {
    in_app_folder(|root| {
        // `config` is a file, so `config:publish --plugin` refuses before it writes; the
        // plugin's views file was checked before it and is not written after it.
        fs::write(root.join("config"), "").unwrap();
        assert_eq!(
            run(&["plugin:publish", "announcements"]).expect_err("refused"),
            "config:publish: config is a file where a folder is needed; nothing was written"
        );
        assert_eq!(files_under(root), ["config"]);

        fs::remove_file(root.join("config")).unwrap();
        run(&["plugin:publish", "announcements"]).expect("published");
        assert_eq!(
            files_under(root),
            [
                "config",
                "config/announcements.feed.toml",
                "config/announcements.toml",
                "resources",
                "resources/views",
                "resources/views/announcements",
                "resources/views/announcements/list.html",
            ]
        );
    });
}

/// A plugin's section that describes its keys.
#[derive(Deserialize)]
struct Notices;

impl ConfigSection for Notices {
    const KEY: &'static str = "announcements";

    fn schema() -> Option<SectionSchema> {
        Some(SectionSchema::new(
            "announcements",
            "Notices for every member of a workspace.",
            vec![FieldSchema::new(
                "notice_days",
                FieldType::Integer,
                "How long a notice stays up, in days.",
            )],
        ))
    }
}

/// A plugin's section written by hand: it does not describe its keys.
#[derive(Deserialize)]
struct Manual;

impl ConfigSection for Manual {
    const KEY: &'static str = "announcements.manual";
}

/// A section whose key differs from `Notices`' only in case.
#[derive(Deserialize)]
struct Shouting;

impl ConfigSection for Shouting {
    const KEY: &'static str = "Announcements";

    fn schema() -> Option<SectionSchema> {
        Some(SectionSchema::new("Announcements", "", Vec::new()))
    }
}

#[test]
fn a_plugins_sections_are_written_through_the_checked_writer() {
    let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
    let dir = tempfile::tempdir().expect("a temporary folder");
    let config = dir.path().join("config");
    let plugin = PublishOptions::parse(&args(&["--plugin", "announcements"])).expect("valid");
    let registry = ConfigRegistry::new(&[
        SectionRegistration::of::<Notices>(),
        SectionRegistration::of::<Manual>(),
    ]);
    let manual = "config/announcements.manual.toml: section `announcements.manual` does not describe its keys, so it is not published\n";
    assert_eq!(
        publish_section(&plugin, &registry, &config).expect("written"),
        format!("wrote config/announcements.toml\n{manual}")
    );
    let text = fs::read_to_string(config.join("announcements.toml")).expect("the file");
    assert!(text.contains("# notice_days = 0"), "{text}");
    fs::write(
        config.join("announcements.toml"),
        "# Gurpreet Singh's settings\n",
    )
    .unwrap();
    assert_eq!(
        publish_section(&plugin, &registry, &config).expect("kept"),
        format!("config/announcements.toml: exists, kept (use --force to replace it)\n{manual}")
    );
    assert_eq!(
        fs::read_to_string(config.join("announcements.toml")).unwrap(),
        "# Gurpreet Singh's settings\n"
    );
    assert!(!config.join("announcements.manual.toml").exists());

    assert_eq!(
        publish_section(&plugin, &ConfigRegistry::new(&[]), &config).expect("nothing"),
        "config: plugin `announcements` has nothing to publish\n"
    );
    let shouting = ConfigRegistry::new(&[
        SectionRegistration::of::<Notices>(),
        SectionRegistration::of::<Shouting>(),
    ]);
    assert_eq!(
        publish_section(&plugin, &shouting, &config).expect_err("one file"),
        "config:publish: config/announcements.toml is the same file as another one on a disk that ignores case; nothing was written"
    );
    assert_eq!(
        fs::read_dir(&config).unwrap().count(),
        1,
        "nothing was written"
    );
}
