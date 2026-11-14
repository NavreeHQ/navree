//! `config:schema`, `config:publish` and `config:show --fallbacks`.

use navree_config::{
    ConfigModule, ConfigRegistry, ConfigSection, ConfigSources, ModuleBuilderConfigExt,
    PublishOptions, SectionRegistration, SectionSchema, publish_section, show,
};
use navree_contracts::Secret;
use navree_kernel::{Module, ModuleBuilder, Navree};
use serde::Deserialize;
use serde_json::{Value, json};

/// Cache settings.
#[derive(Debug, Default, Deserialize, ConfigSection)]
#[config(key = "toolcache", docs = "/docs/digging-deeper/cache/")]
#[serde(default)]
struct ToolCache {
    /// Prepended to every key.
    prefix: String,
    /// How long an entry lives, in seconds.
    ttl_secs: u64,
    /// The Redis address.
    #[config(env_fallback = "NAVREE_TOOL_TEST_REDIS_URL")]
    url: Option<String>,
    /// The Redis password.
    password: Option<Secret<String>>,
}

/// A section written by hand.
#[derive(Deserialize)]
struct ByHand {
    #[allow(dead_code)]
    x: u8,
}

impl ConfigSection for ByHand {
    const KEY: &'static str = "toolhand";
}

struct ToolModule;

impl Module for ToolModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<ToolCache>().config_section::<ByHand>();
    }
}

fn registry() -> ConfigRegistry {
    ConfigRegistry::new(&[
        SectionRegistration::of::<ToolCache>(),
        SectionRegistration::of::<ByHand>(),
    ])
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

#[test]
fn publish_writes_a_commented_file_and_will_not_overwrite_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = PublishOptions::parse(&args(&["toolcache"])).expect("valid");
    let message = publish_section(&options, &registry(), dir.path()).expect("written");
    assert!(message.starts_with("wrote "), "{message}");
    let text = std::fs::read_to_string(dir.path().join("toolcache.toml")).expect("file");
    assert!(text.contains("# Cache settings."), "{text}");
    assert!(text.contains("# prefix = \"\""), "{text}");
    assert!(text.contains("# ttl_secs = 0"), "{text}");
    assert!(
        text.contains("# A credential: set APP_TOOLCACHE__PASSWORD in the environment"),
        "{text}"
    );
    assert!(text.lines().all(|l| l.is_empty() || l.starts_with('#')));

    let error = publish_section(&options, &registry(), dir.path()).expect_err("exists");
    assert!(error.contains("--force"), "{error}");
    let forced = PublishOptions::parse(&args(&["toolcache", "--force"])).expect("valid");
    publish_section(&forced, &registry(), dir.path()).expect("replaced");
}

#[test]
fn publish_can_print_and_refuses_what_it_cannot_describe() {
    let dir = tempfile::tempdir().expect("tempdir");
    let print = PublishOptions::parse(&args(&["toolcache", "--stdout"])).expect("valid");
    let text = publish_section(&print, &registry(), dir.path()).expect("text");
    assert!(text.contains("# ttl_secs = 0"), "{text}");
    assert!(
        !dir.path().join("toolcache.toml").exists(),
        "--stdout writes nothing"
    );

    let unknown = PublishOptions::parse(&args(&["nothing"])).expect("valid");
    let error = publish_section(&unknown, &registry(), dir.path()).expect_err("unknown");
    assert!(error.contains("no section `nothing`"), "{error}");
    let by_hand = PublishOptions::parse(&args(&["toolhand"])).expect("valid");
    let error = publish_section(&by_hand, &registry(), dir.path()).expect_err("no schema");
    assert!(error.contains("does not describe its keys"), "{error}");
}

/// A section written by hand whose key leaves the `config` folder.
#[derive(Deserialize)]
struct Escaping;

impl ConfigSection for Escaping {
    const KEY: &'static str = "../escape";

    fn schema() -> Option<SectionSchema> {
        Some(SectionSchema::new("../escape", "", Vec::new()))
    }
}

#[test]
fn publish_checks_the_file_name_and_never_writes_through_a_link() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = dir.path().join("config");
    let escaping = ConfigRegistry::new(&[SectionRegistration::of::<Escaping>()]);
    let forced = PublishOptions::parse(&args(&["../escape", "--force"])).expect("valid");
    assert_eq!(
        publish_section(&forced, &escaping, &config).expect_err("outside the folder"),
        "config:publish: ../escape.toml is not a safe relative path; nothing was written"
    );
    assert!(!dir.path().join("escape.toml").exists());

    // A link in the file's place is refused, also with --force: what it points at keeps
    // its text.
    #[cfg(unix)]
    {
        let outside = dir.path().join("outside.toml");
        std::fs::write(&outside, "# Simran Kaur's notes\n").unwrap();
        std::os::unix::fs::symlink(&outside, config.join("toolcache.toml")).unwrap();
        let forced = PublishOptions::parse(&args(&["toolcache", "--force"])).expect("valid");
        assert_eq!(
            publish_section(&forced, &registry(), &config).expect_err("a link"),
            "config:publish: toolcache.toml is a symbolic link or inside one; nothing was written"
        );
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "# Simran Kaur's notes\n"
        );
    }
}

#[test]
fn the_fallbacks_are_listed_with_whether_they_are_set() {
    let registry = registry();
    let text = show::render_fallbacks(&registry, false, |name| {
        name == "NAVREE_TOOL_TEST_REDIS_URL"
    });
    assert_eq!(text, "toolcache.url  NAVREE_TOOL_TEST_REDIS_URL  set\n");
    let json: Value =
        serde_json::from_str(&show::render_fallbacks(&registry, true, |_| false)).expect("json");
    assert_eq!(
        json,
        json!({ "toolcache.url": { "variable": "NAVREE_TOOL_TEST_REDIS_URL", "set": false } })
    );
    let none = ConfigRegistry::new(&[SectionRegistration::of::<ByHand>()]);
    assert_eq!(
        show::render_fallbacks(&none, false, |_| false),
        "no key falls back to an environment variable\n"
    );
    let options = show::ShowOptions::parse(&args(&["--fallbacks", "--json"])).expect("valid");
    assert!(options.fallbacks && options.json);
}

#[tokio::test]
async fn the_commands_are_registered_and_schema_writes_a_file() {
    let report = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(ToolModule)
        .validate()
        .expect("valid graph");
    let commands: Vec<&str> = report
        .modules
        .iter()
        .flat_map(|module| module.commands.iter().map(|command| command.name))
        .collect();
    for name in [
        "config:show",
        "config:check",
        "config:schema",
        "config:publish",
    ] {
        assert!(commands.contains(&name), "{name} in {commands:?}");
    }

    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(ToolModule)
        .boot()
        .await
        .expect("boots");
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("schema.json");
    app.run_command(
        "config:schema",
        vec!["--out".to_owned(), path.display().to_string()],
    )
    .await
    .expect("config:schema runs");
    let schema: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("file")).expect("json");
    assert_eq!(
        schema["properties"]["toolcache"]["properties"]["ttl_secs"]["type"],
        "integer"
    );
    assert_eq!(
        schema["properties"]["toolcache"]["properties"]["password"]["x-secret"],
        true
    );
    assert!(schema["properties"].get("toolhand").is_none());
    app.shutdown().await.expect("shutdown");
}
