//! `config/<section>.toml`: a file per section, found in `config/`.

use std::path::Path;

use navree_config::{Config, ConfigSources};
use tempfile::TempDir;

fn app() -> TempDir {
    let dir = tempfile::Builder::new()
        .prefix("navree-sections-")
        .tempdir()
        .expect("tempdir");
    std::fs::create_dir_all(dir.path().join("config")).expect("config dir");
    dir
}

fn write(dir: &Path, path: &str, contents: &str) {
    let target = dir.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).expect("directory");
    }
    std::fs::write(target, contents).expect("write");
}

fn load(dir: &TempDir) -> Config {
    Config::load(&ConfigSources::defaults_in(dir.path())).expect("load")
}

#[test]
fn the_file_stem_is_the_section_key() {
    let dir = app();
    write(
        dir.path(),
        "config/mail.toml",
        "from = \"hello@example.test\"\n[smtp]\nport = 2525\n",
    );
    let config = load(&dir);
    assert_eq!(
        config.get_string("mail.from").as_deref(),
        Some("hello@example.test")
    );
    assert_eq!(config.get_string("mail.smtp.port").as_deref(), Some("2525"));
    assert!(config.keys().iter().all(|key| key.starts_with("mail.")));
}

#[test]
fn a_dotted_stem_nests() {
    let dir = app();
    write(
        dir.path(),
        "config/database.mongodb.toml",
        "url = \"mongodb://localhost/app\"\n",
    );
    let config = load(&dir);
    assert_eq!(
        config.get_string("database.mongodb.url").as_deref(),
        Some("mongodb://localhost/app")
    );
}

#[test]
fn json_files_are_sections_too_and_other_files_are_not() {
    let dir = app();
    write(dir.path(), "config/cache.json", "{ \"prefix\": \"crm:\" }");
    write(dir.path(), "config/notes.txt", "prefix = \"no\"\n");
    write(dir.path(), "config/not a key.toml", "x = 1\n");
    write(dir.path(), "config/.hidden.toml", "x = 1\n");
    let config = load(&dir);
    assert_eq!(config.get_string("cache.prefix").as_deref(), Some("crm:"));
    assert_eq!(config.keys(), ["cache.prefix"]);
}

#[test]
fn app_toml_is_not_a_section_and_loads_first() {
    let dir = app();
    write(
        dir.path(),
        "config/app.toml",
        "[mail]\nfrom = \"base@example.test\"\ntransport = \"log\"\n",
    );
    write(
        dir.path(),
        "config/mail.toml",
        "from = \"section@example.test\"\n",
    );
    let config = load(&dir);
    assert_eq!(
        config.get_string("mail.from").as_deref(),
        Some("section@example.test"),
        "the section file ranks above config/app.toml"
    );
    assert_eq!(config.get_string("mail.transport").as_deref(), Some("log"));
    assert_eq!(config.get_string("app").as_deref(), None);
}

#[test]
fn a_key_set_in_app_toml_and_a_section_file_is_reported() {
    let dir = app();
    write(
        dir.path(),
        "config/app.toml",
        "[mail]\nfrom = \"a@example.test\"\n",
    );
    write(
        dir.path(),
        "config/mail.toml",
        "from = \"b@example.test\"\n",
    );
    let config = load(&dir);
    let shadowed = config.shadowed();
    assert_eq!(shadowed.len(), 1, "{shadowed:?}");
    assert_eq!(shadowed[0].key, "mail.from");
    let message = shadowed[0].message();
    assert!(message.contains("two config files"), "{message}");
    assert!(message.contains("mail.toml"), "{message}");
    assert!(message.contains("app.toml"), "{message}");
    assert!(
        !message.contains("example.test"),
        "never a value: {message}"
    );

    // The same value in both places conflicts with nothing.
    write(
        dir.path(),
        "config/mail.toml",
        "from = \"a@example.test\"\n",
    );
    assert!(load(&dir).shadowed().is_empty());
}

#[test]
fn section_files_load_in_name_order() {
    let dir = app();
    write(
        dir.path(),
        "config/mail.toml",
        "from = \"first\"\n[smtp]\nhost = \"from-mail\"\n",
    );
    write(
        dir.path(),
        "config/mail.smtp.toml",
        "host = \"from-smtp\"\n",
    );
    let config = load(&dir);
    assert_eq!(config.get_string("mail.from").as_deref(), Some("first"));
    // `mail.smtp.toml` comes after `mail.toml` and wins; the two are reported.
    assert_eq!(
        config.get_string("mail.smtp.host").as_deref(),
        Some("from-smtp")
    );
    assert_eq!(config.shadowed().len(), 1);
}

#[test]
fn dotenv_and_the_environment_outrank_section_files() {
    let dir = app();
    write(dir.path(), "config/mail.toml", "from = \"file\"\n");
    write(dir.path(), ".env", "APP_MAIL__FROM=dotenv\n");
    assert_eq!(
        load(&dir).get_string("mail.from").as_deref(),
        Some("dotenv")
    );
}

#[test]
fn a_section_file_can_be_added_by_hand() {
    let dir = app();
    write(dir.path(), "mail.toml", "from = \"hand\"\n");
    let sources = ConfigSources::new().section_file("mail", dir.path().join("mail.toml"));
    let config = Config::load(&sources).expect("load");
    assert_eq!(config.get_string("mail.from").as_deref(), Some("hand"));
    // A missing file is skipped.
    let sources = ConfigSources::new().section_file("mail", dir.path().join("none.toml"));
    assert!(Config::load(&sources).expect("load").keys().is_empty());
}

#[test]
fn a_malformed_section_file_names_the_file() {
    let dir = app();
    write(dir.path(), "config/mail.toml", "from = \n");
    let error = Config::load(&ConfigSources::defaults_in(dir.path())).expect_err("malformed");
    assert!(error.to_string().contains("mail.toml"), "{error}");
}
