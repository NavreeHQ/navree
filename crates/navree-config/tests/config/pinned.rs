//! Behaviour pinned before the loader was rewritten: what loads, in which
//! order, how environment variables map, how layers merge, how typed reads
//! convert values and what the errors say. These tests describe the loader as
//! it was when it wrapped the `config` crate, and they pass unchanged on the
//! loader that replaced it.

use std::collections::HashMap;
use std::path::PathBuf;

use navree_config::{Config, ConfigError, ConfigSection, ConfigSources};
use serde::Deserialize;
use serde_json::{Value, json};
use tempfile::TempDir;

fn write(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create dir");
    }
    std::fs::write(&path, contents).expect("write fixture");
    path
}

/// A message with the temporary directory replaced by `<dir>` and `/` as the
/// separator, so it reads the same on every platform.
fn scrub(dir: &TempDir, text: impl ToString) -> String {
    text.to_string()
        .replace(&dir.path().display().to_string(), "<dir>")
        .replace('\\', "/")
}

fn env(pairs: &[(&str, &str)]) -> Config {
    Config::load(&ConfigSources::new().env_vars("APP_", pairs.iter().copied())).expect("load")
}

fn tree(config: &Config) -> Value {
    config.section::<Value>("").expect("the whole tree")
}

fn error_of<T: serde::de::DeserializeOwned + std::fmt::Debug>(
    config: &Config,
    key: &str,
) -> String {
    config
        .section::<T>(key)
        .expect_err("expected the section to be rejected")
        .to_string()
}

// ---------------------------------------------------------------------------
// Sources and their order
// ---------------------------------------------------------------------------

#[test]
fn dotenv_does_not_override_app_toml_but_the_environment_overrides_both() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    write(
        &dir,
        ".env",
        "APP_HTTP__ADDR=0.0.0.0:1\nAPP_HTTP__PORT=1\nAPP_KEY=from-dotenv\n",
    );
    write(&dir, "config/app.toml", "[http]\naddr = \"127.0.0.1:2\"\n");
    let sources = ConfigSources::new()
        .dotenv(dir.path().join(".env"))
        .file(dir.path().join("config/app.toml"))
        .env_vars("APP_", [("APP_HTTP__PORT", "3")]);
    let config = Config::load(&sources).expect("load");
    // The TOML file is read after `.env`, so its key wins.
    assert_eq!(
        config.get_string("http.addr").as_deref(),
        Some("127.0.0.1:2")
    );
    // A key only `.env` has is kept; the environment is read last.
    assert_eq!(config.get_string("key").as_deref(), Some("from-dotenv"));
    assert_eq!(config.get_string("http.port").as_deref(), Some("3"));
}

#[test]
fn an_environment_file_after_the_base_file_overrides_it_and_a_missing_one_is_skipped() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    write(&dir, ".env", "APP_NAME=dotenv\nAPP_ONLY__DOTENV=1\n");
    write(
        &dir,
        "config/app.toml",
        "name = \"base\"\n[server]\nport = 80\nhost = \"base\"\n",
    );
    write(&dir, "config/production.toml", "[server]\nport = 443\n");
    let build = |environment: &str| {
        let sources = ConfigSources::new()
            .dotenv(dir.path().join(".env"))
            .file(dir.path().join("config/app.toml"))
            .file(dir.path().join(format!("config/{environment}.toml")))
            .env_vars("APP_", [("APP_SERVER__HOST", "from-env")]);
        tree(&Config::load(&sources).expect("load"))
    };
    assert_eq!(
        build("production"),
        json!({
            "name": "base",
            "only": { "dotenv": "1" },
            "server": { "port": 443, "host": "from-env" },
        })
    );
    assert_eq!(
        build("staging"),
        json!({
            "name": "base",
            "only": { "dotenv": "1" },
            "server": { "port": 80, "host": "from-env" },
        })
    );
}

#[test]
fn no_sources_is_an_empty_tree() {
    let config = Config::load(&ConfigSources::new()).expect("load");
    assert_eq!(tree(&config), json!({}));
}

#[test]
fn the_same_file_listed_twice_loads_twice_and_changes_nothing() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let toml = write(&dir, "app.toml", "a = 1\n[b]\nc = [1, 2]\n");
    let config = Config::load(&ConfigSources::new().file(&toml).file(&toml)).expect("load");
    assert_eq!(tree(&config), json!({ "a": 1, "b": { "c": [1, 2] } }));
}

#[test]
fn set_applies_in_call_order_above_every_source() {
    let sources = ConfigSources::new()
        .set("a", json!({ "x": 1, "y": { "z": 2 } }))
        .env_vars("APP_", [("APP_A__X", "from-env"), ("APP_A__W", "w")])
        .set("a", json!({ "y": { "q": 3 } }))
        .set("a.x", "last");
    let config = Config::load(&sources).expect("load");
    assert_eq!(
        tree(&config),
        json!({ "a": { "x": "last", "w": "w", "y": { "z": 2, "q": 3 } } })
    );
}

#[test]
fn set_with_an_empty_key_merges_an_object_at_the_root_and_ignores_other_values() {
    let config = Config::load(
        &ConfigSources::new()
            .set("", json!({ "x": 1 }))
            .set("", json!(5))
            .set("", json!("text")),
    )
    .expect("load");
    assert_eq!(tree(&config), json!({ "x": 1 }));
}

#[test]
fn set_keys_split_on_dots_and_skip_empty_segments() {
    let config = Config::load(&ConfigSources::new().set("a..b.", 5).set(".c", true)).expect("load");
    assert_eq!(tree(&config), json!({ "a": { "b": 5 }, "c": true }));
}

#[test]
fn set_keeps_json_number_kinds_and_null() {
    let config = Config::load(
        &ConfigSources::new()
            .set("int", -3)
            .set("float", 1.5)
            .set("whole_float", 2.0)
            .set("big", u64::MAX)
            .set("null", Value::Null),
    )
    .expect("load");
    assert_eq!(
        tree(&config),
        json!({ "int": -3, "float": 1.5, "whole_float": 2.0, "big": u64::MAX, "null": null })
    );
}

// ---------------------------------------------------------------------------
// Environment variables
// ---------------------------------------------------------------------------

#[test]
fn environment_names_map_by_prefix_double_underscore_and_lower_case() {
    let config = env(&[
        ("APP_KEY", "k"),
        ("APP_DATABASE__URL", "u"),
        ("APP_HTTP__PER_IP_RATE_LIMIT", "600"),
        ("APP_LOG__LEVELS__NAVREE_QUEUE", "debug"),
        ("app_lower__case", "prefix in any case"),
        ("APP_MIXED__Key__Name", "segments are lower-cased"),
        ("APP_DOTS.IN.NAME", "dots stay in the name"),
    ]);
    assert_eq!(
        tree(&config),
        json!({
            "key": "k",
            "database": { "url": "u" },
            "http": { "per_ip_rate_limit": "600" },
            "log": { "levels": { "navree_queue": "debug" } },
            "lower": { "case": "prefix in any case" },
            "mixed": { "key": { "name": "segments are lower-cased" } },
            "dots.in.name": "dots stay in the name",
        })
    );
}

#[test]
fn environment_names_outside_the_prefix_or_with_an_empty_segment_are_ignored() {
    let config = env(&[
        ("OTHER", "x"),
        ("APPLICATION", "x"),
        ("PATH", "x"),
        ("APP_", "x"),
        ("APP_TRAILING__", "x"),
        ("APP_A____B", "x"),
        ("APP_OK", "yes"),
    ]);
    assert_eq!(tree(&config), json!({ "ok": "yes" }));
}

#[test]
fn the_prefix_takes_one_underscore_so_a_second_one_belongs_to_the_name() {
    // `APP_` is the prefix; what is left, `_LEADING`, has no `__` to split on.
    // With three underscores the rest starts with `__`: an empty first segment.
    let config = env(&[("APP__LEADING", "x"), ("APP___THREE", "y")]);
    assert_eq!(tree(&config), json!({ "_leading": "x" }));
}

#[test]
fn environment_values_stay_strings_in_the_tree() {
    let config = env(&[
        ("APP_PORT", "8080"),
        ("APP_FLAG", "true"),
        ("APP_RATIO", "0.5"),
        ("APP_LIST", "a,b"),
        ("APP_EMPTY", ""),
        ("APP_PADDED", "  x  "),
    ]);
    assert_eq!(
        tree(&config),
        json!({
            "port": "8080",
            "flag": "true",
            "ratio": "0.5",
            "list": "a,b",
            "empty": "",
            "padded": "  x  ",
        })
    );
}

#[test]
fn a_longer_name_beats_its_prefix_within_one_source() {
    let forward = env(&[("APP_A", "scalar"), ("APP_A__B", "child")]);
    let backward = env(&[("APP_A__B", "child"), ("APP_A", "scalar")]);
    assert_eq!(tree(&forward), json!({ "a": { "b": "child" } }));
    assert_eq!(tree(&backward), tree(&forward));
}

#[test]
fn a_prefix_with_or_without_its_underscore_is_the_same_prefix() {
    for prefix in ["MY", "MY_"] {
        let config = Config::load(&ConfigSources::new().env_vars(
            prefix,
            [
                ("MY_NAME", "x"),
                ("MYNAME", "ignored"),
                ("APP_NAME", "ignored"),
            ],
        ))
        .expect("load");
        assert_eq!(tree(&config), json!({ "name": "x" }), "prefix {prefix}");
    }
}

#[test]
fn an_empty_prefix_takes_every_name() {
    let config = Config::load(&ConfigSources::new().env_vars("", [("PATH", "p"), ("A__B", "c")]))
        .expect("load");
    assert_eq!(tree(&config), json!({ "path": "p", "a": { "b": "c" } }));
}

#[test]
fn dotenv_entries_map_like_environment_variables_with_the_app_prefix_only() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let dotenv = write(
        &dir,
        ".env",
        "APP_A__B=1\nOTHER=2\nMYAPP_C=3\nexport APP_D=4\n",
    );
    let config = Config::load(&ConfigSources::new().dotenv(&dotenv)).expect("load");
    assert_eq!(tree(&config), json!({ "a": { "b": "1" }, "d": "4" }));
}

#[test]
fn the_process_environment_source_reads_only_the_configured_prefix() {
    // `cargo test` exports `CARGO_PKG_NAME` and friends.
    let config = Config::load(&ConfigSources::new().env_prefixed("CARGO_PKG_NAME")).expect("load");
    // `CARGO_PKG_NAME_` is the prefix: nothing starts with it.
    assert_eq!(tree(&config), json!({}));
    let config = Config::load(&ConfigSources::new().env_prefixed("CARGO_PKG")).expect("load");
    assert_eq!(
        config.get_string("name").as_deref(),
        Some("navree-config"),
        "CARGO_PKG_NAME"
    );
    assert_eq!(
        config.get_string("path"),
        None,
        "PATH is not under the prefix"
    );
}

// ---------------------------------------------------------------------------
// Merging
// ---------------------------------------------------------------------------

fn merged(layers: &[Value]) -> Value {
    let mut sources = ConfigSources::new();
    for layer in layers {
        sources = sources.set("", layer.clone());
    }
    tree(&Config::load(&sources).expect("load"))
}

#[test]
fn tables_merge_key_by_key_at_every_depth() {
    assert_eq!(
        merged(&[
            json!({ "a": { "x": 1, "y": { "p": 1, "q": 2 } }, "keep": true }),
            json!({ "a": { "y": { "q": 20, "r": 30 }, "z": 3 } }),
        ]),
        json!({ "a": { "x": 1, "y": { "p": 1, "q": 20, "r": 30 }, "z": 3 }, "keep": true })
    );
}

#[test]
fn arrays_are_replaced_whole() {
    assert_eq!(
        merged(&[
            json!({ "a": [1, 2, 3], "b": [{ "x": 1 }] }),
            json!({ "a": [9], "b": [] })
        ]),
        json!({ "a": [9], "b": [] })
    );
}

#[test]
fn a_scalar_replaces_a_table_and_a_table_replaces_a_scalar() {
    assert_eq!(
        merged(&[
            json!({ "a": { "x": 1 }, "b": 5 }),
            json!({ "a": 2, "b": { "y": 1 } })
        ]),
        json!({ "a": 2, "b": { "y": 1 } })
    );
}

#[test]
fn an_array_replaces_a_table_and_the_reverse() {
    assert_eq!(
        merged(&[
            json!({ "a": { "x": 1 }, "b": [1] }),
            json!({ "a": [1], "b": { "y": 1 } })
        ]),
        json!({ "a": [1], "b": { "y": 1 } })
    );
}

#[test]
fn null_replaces_whatever_was_there() {
    assert_eq!(
        merged(&[
            json!({ "a": { "x": 1 }, "b": [1], "c": "s" }),
            json!({ "a": null, "b": null, "c": null })
        ]),
        json!({ "a": null, "b": null, "c": null })
    );
    // And a later value replaces a null.
    assert_eq!(
        merged(&[json!({ "a": null }), json!({ "a": { "x": 1 } })]),
        json!({ "a": { "x": 1 } })
    );
}

#[test]
fn a_layer_never_removes_keys_it_does_not_mention() {
    assert_eq!(
        merged(&[
            json!({ "a": 1, "b": { "c": 2 } }),
            json!({}),
            json!({ "b": {} })
        ]),
        json!({ "a": 1, "b": { "c": 2 } })
    );
}

#[test]
fn toml_files_merge_with_environment_overlays_on_top() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let base = write(
        &dir,
        "base.toml",
        "[db]\nurl = \"a\"\nreplicas = [\"r1\", \"r2\"]\n[db.pool]\nmin = 1\nmax = 5\n",
    );
    let local = write(
        &dir,
        "local.toml",
        "[db]\nreplicas = [\"r3\"]\n[db.pool]\nmax = 9\n",
    );
    let config = Config::load(
        &ConfigSources::new()
            .file(&base)
            .file(&local)
            .env_vars("APP_", [("APP_DB__POOL__MIN", "2"), ("APP_DB__URL", "b")]),
    )
    .expect("load");
    assert_eq!(
        tree(&config),
        json!({
            "db": { "url": "b", "replicas": ["r3"], "pool": { "min": "2", "max": 9 } }
        })
    );
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

#[test]
fn toml_keys_keep_their_case_and_values_keep_their_kind() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let toml = write(
        &dir,
        "app.toml",
        "Mixed_Case = 1\nint = 5\nfloat = 2.0\ntruth = true\ndate = 1979-05-27T07:32:00Z\n\
         time = 07:32:00\n\"quoted.key\" = \"x\"\nlist = [1, \"two\", 3.5]\n",
    );
    let config = Config::load(&ConfigSources::new().file(&toml)).expect("load");
    assert_eq!(
        tree(&config),
        json!({
            "Mixed_Case": 1,
            "int": 5,
            "float": 2.0,
            "truth": true,
            "date": "1979-05-27T07:32:00Z",
            "time": "07:32:00",
            "quoted.key": "x",
            "list": [1, "two", 3.5],
        })
    );
    // A key written with different case in another source is another key.
    let mixed = Config::load(
        &ConfigSources::new()
            .file(&toml)
            .env_vars("APP_", [("APP_MIXED_CASE", "env")]),
    )
    .expect("load");
    assert_eq!(mixed.get_string("Mixed_Case").as_deref(), Some("1"));
    assert_eq!(mixed.get_string("mixed_case").as_deref(), Some("env"));
}

#[test]
fn a_byte_order_mark_and_crlf_line_endings_are_accepted() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let toml = write(&dir, "app.toml", "\u{feff}a = 1\r\n[b]\r\nc = \"x\"\r\n");
    let config = Config::load(&ConfigSources::new().file(&toml)).expect("load");
    assert_eq!(tree(&config), json!({ "a": 1, "b": { "c": "x" } }));
}

#[test]
fn an_empty_file_is_an_empty_layer() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let toml = write(&dir, "app.toml", "");
    let dotenv = write(&dir, ".env", "# only a comment\n");
    let config = Config::load(&ConfigSources::new().file(&toml).dotenv(&dotenv)).expect("load");
    assert_eq!(tree(&config), json!({}));
}

#[test]
fn toml_syntax_errors_name_the_file_the_line_and_the_column() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let cases = [
        ("a = 1\na = 2\n", "line 2, column 1"),
        ("[a\n", "line 1, column 3"),
        ("a = \"x\nb = 1\n", "line 1, column 7"),
        ("a = 1\r\nb = \r\n", "line 2, column 5"),
        ("é = \"ü\"\nb = \n", "line 1, column 1"),
    ];
    for (src, position) in cases {
        let toml = write(&dir, "app.toml", src);
        let error = Config::load(&ConfigSources::new().file(&toml)).expect_err("invalid TOML");
        assert!(matches!(error, ConfigError::Toml { .. }), "{error:?}");
        let text = scrub(&dir, &error);
        assert!(
            text.starts_with(&format!("invalid TOML in `<dir>/app.toml` at {position}: ")),
            "{src:?}: {text}"
        );
    }
}

#[test]
fn toml_errors_never_contain_the_text_of_the_file() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    for src in [
        "password = hunter2\n",
        "password = \"hunter2\n",
        "[database]\nurl = \"postgres://u:hunter2@h/d\nmore = 1\n",
        "password = \"hunter2\"\npassword = \"hunter3\"\n",
        "password hunter2\n",
        "hunter2 = =\n",
    ] {
        let toml = write(&dir, "app.toml", src);
        let error = Config::load(&ConfigSources::new().file(&toml)).expect_err("invalid TOML");
        let text = error.to_string();
        assert!(!text.contains("hunter"), "{src:?}: {text}");
    }
}

#[test]
fn a_file_that_is_not_utf8_is_an_io_error_naming_the_file() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let path = dir.path().join("app.toml");
    std::fs::write(&path, b"a = \"\xff\"").expect("write");
    let error = Config::load(&ConfigSources::new().file(&path)).expect_err("not UTF-8");
    assert!(matches!(error, ConfigError::Io { .. }), "{error:?}");
    assert_eq!(
        scrub(&dir, &error),
        "cannot read configuration file `<dir>/app.toml`: stream did not contain valid UTF-8"
    );
}

#[test]
fn dotenv_syntax_errors_name_the_file_and_the_line() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let dotenv = write(&dir, ".env", "APP_A=1\nnot a pair\n");
    let error = Config::load(&ConfigSources::new().dotenv(&dotenv)).expect_err("invalid .env");
    assert!(
        matches!(error, ConfigError::Dotenv { line: 2, .. }),
        "{error:?}"
    );
    assert_eq!(
        scrub(&dir, &error),
        "invalid .env syntax in `<dir>/.env` on line 2: expected `KEY=value`"
    );
}

#[test]
fn a_malformed_file_fails_the_load_even_after_valid_sources() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let good = write(&dir, "good.toml", "a = 1\n");
    let bad = write(&dir, "bad.toml", "a = \n");
    assert!(Config::load(&ConfigSources::new().file(&good).file(&bad)).is_err());
    assert!(Config::load(&ConfigSources::new().file(&bad).file(&good)).is_err());
}

// ---------------------------------------------------------------------------
// Typed reads
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, PartialEq)]
struct Port {
    port: u16,
}

#[test]
fn integers_convert_from_toml_numbers_and_from_environment_strings() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let toml = write(&dir, "app.toml", "port = 8080\n");
    let from_toml = Config::load(&ConfigSources::new().file(&toml)).expect("load");
    assert_eq!(
        from_toml.section::<Port>("").expect("toml"),
        Port { port: 8080 }
    );
    assert_eq!(
        env(&[("APP_PORT", "8080")])
            .section::<Port>("")
            .expect("env"),
        Port { port: 8080 }
    );
    // Rust's integer syntax: a sign is accepted, spaces and decimals are not.
    assert_eq!(
        env(&[("APP_PORT", "+80")])
            .section::<Port>("")
            .expect("sign"),
        Port { port: 80 }
    );
    for bad in [" 80", "80 ", "8.0", "-1", "", "0x50", "eighty", "70000"] {
        assert!(
            env(&[("APP_PORT", bad)]).section::<Port>("").is_err(),
            "{bad:?}"
        );
    }
}

#[test]
fn a_whole_float_fills_an_integer_and_numbers_fill_strings() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Mixed {
        count: u32,
        label: String,
        flag_label: String,
    }
    let config = Config::load(
        &ConfigSources::new()
            .set("count", 2.0)
            .set("label", 7)
            .set("flag_label", true),
    )
    .expect("load");
    assert_eq!(
        config.section::<Mixed>("").expect("section"),
        Mixed {
            count: 2,
            label: "7".to_owned(),
            flag_label: "true".to_owned()
        }
    );
}

#[test]
fn unsigned_fields_reject_negative_numbers_and_range_overflow() {
    #[derive(Debug, Deserialize)]
    struct Small {
        #[allow(dead_code)]
        n: u8,
    }
    let negative = Config::load(&ConfigSources::new().set("n", -5)).expect("load");
    assert!(
        error_of::<Small>(&negative, "")
            .contains("expected an unsigned 64 bit or less integer for key `n`")
    );
    let big = Config::load(&ConfigSources::new().set("n", 300)).expect("load");
    assert!(error_of::<Small>(&big, "").contains("expected an unsigned 8 bit integer for key `n`"));
    // The environment strings follow the same rules.
    assert!(env(&[("APP_N", "300")]).section::<Small>("").is_err());
    assert_eq!(
        env(&[("APP_N", "255")])
            .section::<Small>("")
            .expect("fits")
            .n,
        255
    );
}

#[test]
fn booleans_accept_the_documented_spellings_in_any_case() {
    #[derive(Debug, Deserialize)]
    struct Flag {
        b: bool,
    }
    for yes in ["true", "TRUE", "True", "yes", "YES", "on", "On", "1"] {
        assert!(
            env(&[("APP_B", yes)]).section::<Flag>("").expect(yes).b,
            "{yes}"
        );
        assert_eq!(env(&[("APP_B", yes)]).get_bool("b"), Some(true), "{yes}");
    }
    for no in ["false", "FALSE", "no", "No", "off", "OFF", "0"] {
        assert!(
            !env(&[("APP_B", no)]).section::<Flag>("").expect(no).b,
            "{no}"
        );
        assert_eq!(env(&[("APP_B", no)]).get_bool("b"), Some(false), "{no}");
    }
    for other in ["", "2", "y", "n", "enabled", " true", "tru"] {
        assert!(
            env(&[("APP_B", other)]).section::<Flag>("").is_err(),
            "{other:?}"
        );
        assert_eq!(env(&[("APP_B", other)]).get_bool("b"), None, "{other:?}");
    }
}

#[test]
fn booleans_also_come_from_numbers() {
    #[derive(Debug, Deserialize)]
    struct Flag {
        b: bool,
    }
    for (value, expected) in [
        (json!(1), true),
        (json!(0), false),
        (json!(-3), true),
        (json!(0.0), false),
    ] {
        let config = Config::load(&ConfigSources::new().set("b", value.clone())).expect("load");
        assert_eq!(
            config.section::<Flag>("").expect("bool").b,
            expected,
            "{value}"
        );
        assert_eq!(config.get_bool("b"), Some(expected), "{value}");
    }
}

#[test]
fn floats_convert_from_strings_and_integers() {
    #[derive(Debug, Deserialize)]
    struct Ratio {
        r: f64,
    }
    assert_eq!(
        env(&[("APP_R", "0.25")])
            .section::<Ratio>("")
            .expect("string")
            .r,
        0.25
    );
    assert_eq!(
        env(&[("APP_R", "1e3")])
            .section::<Ratio>("")
            .expect("exponent")
            .r,
        1000.0
    );
    assert_eq!(
        Config::load(&ConfigSources::new().set("r", 3))
            .expect("load")
            .section::<Ratio>("")
            .expect("int")
            .r,
        3.0
    );
    assert!(env(&[("APP_R", "fast")]).section::<Ratio>("").is_err());
}

#[test]
fn lists_do_not_come_from_strings() {
    #[derive(Debug, Deserialize)]
    struct Proxies {
        #[allow(dead_code)]
        proxies: Vec<String>,
    }
    let config = env(&[("APP_PROXIES", "10.0.0.1,10.0.0.2")]);
    assert_eq!(
        error_of::<Proxies>(&config, ""),
        "configuration section `` is invalid: invalid type: string [redacted], expected a sequence for key `proxies`"
    );
    // The string stays in the tree: modules that accept a comma-separated
    // value read it with their own deserializer.
    assert_eq!(
        config.get_string("proxies").as_deref(),
        Some("10.0.0.1,10.0.0.2")
    );
}

#[test]
fn lists_of_numbers_and_strings_convert_elementwise() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Lists {
        ports: Vec<u16>,
        names: Vec<String>,
        pairs: (u8, String),
    }
    let config = Config::load(
        &ConfigSources::new()
            .set("ports", json!(["80", 443]))
            .set("names", json!([1, "b", true]))
            .set("pairs", json!(["7", 8])),
    )
    .expect("load");
    assert_eq!(
        config.section::<Lists>("").expect("lists"),
        Lists {
            ports: vec![80, 443],
            names: vec!["1".to_owned(), "b".to_owned(), "true".to_owned()],
            pairs: (7, "8".to_owned()),
        }
    );
}

#[test]
fn options_maps_nested_structs_and_defaults() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Db {
        url: String,
        #[serde(default)]
        pool: Option<u32>,
        #[serde(default = "five")]
        retries: u32,
        #[serde(rename = "log-queries", default)]
        log_queries: bool,
    }
    fn five() -> u32 {
        5
    }
    #[derive(Debug, Deserialize, PartialEq)]
    struct Root {
        db: Db,
        limits: HashMap<String, u32>,
        servers: Vec<Port>,
    }
    let config = Config::load(
        &ConfigSources::new()
            .set("db", json!({ "url": "u", "log-queries": "yes" }))
            .set("limits", json!({ "a": 1, "b": "2" }))
            .set("servers", json!([{ "port": 1 }, { "port": "2" }])),
    )
    .expect("load");
    assert_eq!(
        config.section::<Root>("").expect("root"),
        Root {
            db: Db {
                url: "u".to_owned(),
                pool: None,
                retries: 5,
                log_queries: true
            },
            limits: HashMap::from([("a".to_owned(), 1), ("b".to_owned(), 2)]),
            servers: vec![Port { port: 1 }, Port { port: 2 }],
        }
    );
    // A null is `None`; an empty string is not.
    #[derive(Debug, Deserialize)]
    struct Maybe {
        v: Option<u16>,
    }
    let nulled = Config::load(&ConfigSources::new().set("v", Value::Null)).expect("load");
    assert_eq!(nulled.section::<Maybe>("").expect("null").v, None);
    assert!(env(&[("APP_V", "")]).section::<Maybe>("").is_err());
}

#[test]
fn enums_match_exactly_by_name() {
    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    enum Level {
        Debug,
        Info,
    }
    #[derive(Debug, Deserialize, PartialEq)]
    struct Log {
        level: Level,
    }
    assert_eq!(
        env(&[("APP_LEVEL", "info")])
            .section::<Log>("")
            .expect("info")
            .level,
        Level::Info
    );
    assert!(env(&[("APP_LEVEL", "INFO")]).section::<Log>("").is_err());
    assert!(env(&[("APP_LEVEL", "trace")]).section::<Log>("").is_err());

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    enum Shape {
        Circle(u32),
        Rect { w: u32, h: u32 },
        Dot,
    }
    #[derive(Debug, Deserialize, PartialEq)]
    struct Drawing {
        shape: Shape,
    }
    let dot = env(&[("APP_SHAPE", "dot")]);
    assert_eq!(dot.section::<Drawing>("").expect("unit").shape, Shape::Dot);
    let circle = env(&[("APP_SHAPE__CIRCLE", "3")]);
    assert_eq!(
        circle.section::<Drawing>("").expect("newtype").shape,
        Shape::Circle(3)
    );
    let rect = env(&[("APP_SHAPE__RECT__W", "1"), ("APP_SHAPE__RECT__H", "2")]);
    assert_eq!(
        rect.section::<Drawing>("").expect("struct").shape,
        Shape::Rect { w: 1, h: 2 }
    );
    // More than one key cannot name a variant.
    let two = env(&[("APP_SHAPE__CIRCLE", "3"), ("APP_SHAPE__DOT", "x")]);
    assert!(error_of::<Drawing>(&two, "").contains(
        "value of enum Shape should be represented by either string or table with exactly one key"
    ));
}

#[test]
fn untagged_enums_and_flattened_maps_see_the_native_kinds() {
    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(untagged)]
    enum Number {
        Int(i64),
        Text(String),
    }
    #[derive(Debug, Deserialize, PartialEq)]
    struct Holder {
        n: Number,
        #[serde(flatten)]
        rest: HashMap<String, Value>,
    }
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let toml = write(&dir, "app.toml", "n = 5\nextra = 1.5\nname = \"x\"\n");
    let from_toml = Config::load(&ConfigSources::new().file(&toml)).expect("load");
    let holder = from_toml.section::<Holder>("").expect("toml");
    assert_eq!(holder.n, Number::Int(5));
    assert_eq!(
        holder.rest,
        HashMap::from([
            ("extra".to_owned(), json!(1.5)),
            ("name".to_owned(), json!("x"))
        ])
    );
    // From the environment the same field is text.
    let from_env = env(&[("APP_N", "5")]);
    assert_eq!(
        from_env.section::<Holder>("").expect("env").n,
        Number::Text("5".to_owned())
    );
}

#[test]
fn unknown_fields_are_ignored_unless_the_section_denies_them() {
    #[derive(Debug, Deserialize)]
    struct Lenient {
        a: u32,
    }
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Strict {
        a: u32,
    }
    let config = Config::load(&ConfigSources::new().set("a", 1).set("b", 2)).expect("load");
    assert_eq!(config.section::<Lenient>("").expect("lenient").a, 1);
    assert_eq!(
        error_of::<Strict>(&config, ""),
        "configuration section `` is invalid: unknown field `b`, expected `a`"
    );
}

#[test]
fn a_single_character_string_fills_a_char() {
    #[derive(Debug, Deserialize)]
    struct Sep {
        c: char,
    }
    assert_eq!(
        env(&[("APP_C", ";")]).section::<Sep>("").expect("char").c,
        ';'
    );
    assert!(env(&[("APP_C", ";;")]).section::<Sep>("").is_err());
}

#[test]
fn section_reads_a_subtree_by_dotted_path() {
    let config = Config::load(
        &ConfigSources::new().set("mail", json!({ "smtp": { "port": 25 }, "from": "a" })),
    )
    .expect("load");
    assert_eq!(
        config.section::<Port>("mail.smtp").expect("nested"),
        Port { port: 25 }
    );
    assert_eq!(
        config.section::<Value>("mail.smtp").expect("value"),
        json!({ "port": 25 })
    );
    // Empty segments are skipped, so these name the same section.
    assert_eq!(
        config.section::<Port>(".mail..smtp.").expect("dots"),
        Port { port: 25 }
    );
    assert!(matches!(
        config
            .section::<Port>("mail.smtp.port")
            .expect_err("a scalar"),
        ConfigError::Section { .. }
    ));
    assert!(matches!(
        config.section::<Port>("mail.nope.port").expect_err("absent"),
        ConfigError::MissingSection { key } if key == "mail.nope.port"
    ));
}

#[test]
fn get_string_and_get_bool_read_scalars_only() {
    let config = Config::load(
        &ConfigSources::new()
            .set("s", "text")
            .set("i", 5)
            .set("f", 1.5)
            .set("b", true)
            .set("n", Value::Null)
            .set("t", json!({ "x": 1 }))
            .set("a", json!([1])),
    )
    .expect("load");
    assert_eq!(config.get_string("s").as_deref(), Some("text"));
    assert_eq!(config.get_string("i").as_deref(), Some("5"));
    assert_eq!(config.get_string("f").as_deref(), Some("1.5"));
    assert_eq!(config.get_string("b").as_deref(), Some("true"));
    for absent in ["n", "t", "a", "", "missing", "s.deeper", "t.x.y"] {
        assert_eq!(config.get_string(absent), None, "{absent}");
    }
    assert_eq!(config.get_string("t.x").as_deref(), Some("1"));
    assert_eq!(config.get_bool("s"), None);
    assert_eq!(config.get_bool("b"), Some(true));
    assert_eq!(config.get_bool("i"), Some(true));
    assert_eq!(config.get_bool("t"), None);
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
struct Billing {
    currency: String,
    trial_days: u32,
}

impl ConfigSection for Billing {
    const KEY: &'static str = "billing";

    fn validate(&self) -> Result<(), String> {
        if self.currency.len() == 3 || self.currency.is_empty() {
            Ok(())
        } else {
            Err("currency must be a 3-letter ISO code".to_owned())
        }
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Required {
    url: String,
}

impl ConfigSection for Required {
    const KEY: &'static str = "required.section";
}

#[test]
fn typed_reads_the_section_key_and_applies_serde_defaults_when_it_is_absent() {
    let empty = Config::load(&ConfigSources::new()).expect("load");
    assert_eq!(
        empty.typed::<Billing>().expect("defaults"),
        Billing::default()
    );

    let config = env(&[
        ("APP_BILLING__CURRENCY", "EUR"),
        ("APP_BILLING__TRIAL_DAYS", "14"),
    ]);
    assert_eq!(
        config.typed::<Billing>().expect("typed"),
        Billing {
            currency: "EUR".to_owned(),
            trial_days: 14
        }
    );
}

#[test]
fn typed_validates_and_names_the_section() {
    let config = env(&[("APP_BILLING__CURRENCY", "EURO")]);
    let error = config.typed::<Billing>().expect_err("rejected");
    assert!(
        matches!(&error, ConfigError::Validation { key, .. } if key == "billing"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "configuration section `billing` failed validation: currency must be a 3-letter ISO code"
    );
}

#[test]
fn typed_without_the_section_fails_on_a_required_field_and_names_it() {
    let error = Config::load(&ConfigSources::new())
        .expect("load")
        .typed::<Required>()
        .expect_err("url is required");
    assert!(
        matches!(&error, ConfigError::Section { key, .. } if key == "required.section"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "configuration section `required.section` is invalid: missing configuration field \"url\""
    );
}

#[test]
fn section_or_default_defaults_only_an_absent_section() {
    let config = env(&[("APP_BILLING__TRIAL_DAYS", "many")]);
    assert_eq!(
        Config::load(&ConfigSources::new())
            .expect("load")
            .section_or_default::<Billing>("billing")
            .expect("absent"),
        Billing::default()
    );
    // A present but invalid section is an error, not the default.
    assert!(config.section_or_default::<Billing>("billing").is_err());
}

// ---------------------------------------------------------------------------
// Error messages
// ---------------------------------------------------------------------------

#[test]
fn a_missing_section_says_which() {
    let config = env(&[]);
    assert_eq!(
        config
            .section::<Port>("server")
            .expect_err("absent")
            .to_string(),
        "configuration section `server` is missing"
    );
}

#[test]
fn a_missing_field_names_its_path_below_the_section() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Zzz {
        zzz: String,
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct B {
        b: Zzz,
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct A {
        a: B,
    }
    let config = Config::load(
        &ConfigSources::new()
            .set("a.b.other", 1)
            .set("top.a.b.other", 1),
    )
    .expect("load");
    assert_eq!(
        error_of::<A>(&config, ""),
        "configuration section `` is invalid: missing configuration field \"a.b.zzz\""
    );
    assert_eq!(
        error_of::<A>(&config, "top"),
        "configuration section `top` is invalid: missing configuration field \"a.b.zzz\""
    );
    assert_eq!(
        error_of::<Zzz>(&config, "a.b"),
        "configuration section `a.b` is invalid: missing configuration field \"zzz\""
    );
}

#[test]
fn a_wrong_type_names_the_key_and_the_source_it_came_from() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let toml = write(&dir, "app.toml", "[server]\nport = \"hunter2\"\n");
    let dotenv = write(&dir, ".env", "APP_SERVER__PORT=hunter2\n");
    let section = |sources: &ConfigSources| {
        scrub(
            &dir,
            error_of::<Port>(&Config::load(sources).expect("load"), "server"),
        )
    };
    let prefix = "configuration section `server` is invalid: invalid type: string [redacted], expected an integer for key `port` in";
    assert_eq!(
        section(&ConfigSources::new().file(&toml)),
        format!("{prefix} <dir>/app.toml")
    );
    assert_eq!(
        section(&ConfigSources::new().dotenv(&dotenv)),
        format!("{prefix} <dir>/.env")
    );
    assert_eq!(
        section(&ConfigSources::new().env_vars("APP_", [("APP_SERVER__PORT", "hunter2")])),
        format!("{prefix} environment")
    );
    assert_eq!(
        section(&ConfigSources::new().set("server.port", "hunter2")),
        format!("{prefix} ConfigSources::set")
    );
}

#[test]
fn the_source_named_is_the_one_whose_value_won() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let base = write(&dir, "base.toml", "[server]\nport = \"base\"\n");
    let local = write(&dir, "local.toml", "[server]\nport = \"local\"\n");
    let config = Config::load(&ConfigSources::new().file(&base).file(&local)).expect("load");
    assert!(
        scrub(&dir, error_of::<Port>(&config, "server")).ends_with("in <dir>/local.toml"),
        "{}",
        error_of::<Port>(&config, "server")
    );
}

#[test]
fn nested_and_listed_keys_are_dotted_paths_with_indexes() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Inner {
        port: u16,
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Outer {
        inner: Inner,
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Ports {
        ports: Vec<u16>,
    }
    let config = Config::load(
        &ConfigSources::new()
            .set("outer.inner.port", "x")
            .set("ports", json!([1, "y", 3])),
    )
    .expect("load");
    assert_eq!(
        error_of::<Outer>(&config, "outer"),
        "configuration section `outer` is invalid: invalid type: string [redacted], expected an integer for key `inner.port` in ConfigSources::set"
    );
    assert_eq!(
        error_of::<Ports>(&config, ""),
        "configuration section `` is invalid: invalid type: string [redacted], expected an integer for key `ports[1]` in ConfigSources::set"
    );
}

#[test]
fn a_section_of_the_wrong_shape_says_what_was_expected() {
    let config = Config::load(&ConfigSources::new().set("n", 5).set("s", "text")).expect("load");
    assert_eq!(
        error_of::<Port>(&config, "n"),
        "configuration section `n` is invalid: invalid type: integer `5`, expected struct Port"
    );
    assert_eq!(
        error_of::<Port>(&config, "s"),
        "configuration section `s` is invalid: invalid type: string [redacted], expected struct Port"
    );
}

#[test]
fn errors_never_quote_string_values() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Wants {
        flag: bool,
        n: u32,
        f: f64,
        v: Vec<String>,
        c: char,
    }
    for field in ["flag", "n", "f", "v", "c"] {
        let config =
            Config::load(&ConfigSources::new().set(field, "hunter2-secret")).expect("load");
        let text = error_of::<Wants>(&config, "");
        assert!(!text.contains("hunter2"), "{field}: {text}");
    }
}

#[test]
fn a_user_deserializer_error_keeps_its_text_but_its_quoted_strings_are_masked() {
    #[derive(Debug)]
    struct Https;
    impl<'de> Deserialize<'de> for Https {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            use serde::de::{Error, Unexpected};
            let url = String::deserialize(d)?;
            if url.starts_with("https://") {
                Ok(Https)
            } else {
                Err(D::Error::invalid_value(
                    Unexpected::Str(&url),
                    &"an https URL",
                ))
            }
        }
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Hook {
        url: Https,
    }
    let config =
        Config::load(&ConfigSources::new().set("hook.url", "http://u:hunter2@h")).expect("load");
    assert_eq!(
        error_of::<Hook>(&config, "hook"),
        "configuration section `hook` is invalid: invalid value: string [redacted], expected an https URL for key `url`"
    );
}

#[test]
fn sources_and_config_clone_and_debug_without_values() {
    let sources = ConfigSources::new()
        .env_vars("APP_", [("APP_DATABASE__PASSWORD", "hunter2")])
        .set("mail.password", "hunter2");
    let debug = format!("{:?}", sources.clone());
    assert!(!debug.contains("hunter2"), "{debug}");
    let config = Config::load(&sources).expect("load");
    let copy = config.clone();
    assert_eq!(tree(&copy), tree(&config));
    assert!(!format!("{config:?}").contains("hunter2"));
}
