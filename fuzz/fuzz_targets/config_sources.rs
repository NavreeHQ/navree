//! `navree_config`: the `.env` parser, TOML and JSON files, environment
//! variables and `set` overrides merged into one tree.
//!
//! Input: a depth-limit byte, then parts separated by NUL bytes: the `.env`
//! text, the TOML file, the JSON file, `NAME=value` lines of environment
//! variables, `key=value` lines of `set` overrides.
//!
//! Invariants: entries the `.env` parser returns, written back as
//! double-quoted lines, parse to the same entries; loading the same sources
//! twice gives the same redacted tree; a load that fails says so with an
//! error (never a panic) and stays within the depth and size limits.
#![no_main]

use std::path::PathBuf;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use navree_config::{Config, ConfigSources, parse_dotenv};

fn dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        tempfile::Builder::new()
            .prefix("navree-fuzz-config")
            .tempdir()
            .expect("a temporary directory")
            .keep()
    })
}

fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn dotenv(text: &str) {
    let Ok(entries) = parse_dotenv(text) else {
        return;
    };
    let written: String = entries
        .iter()
        .map(|(key, value)| format!("{key}={}\n", quote(value)))
        .collect();
    let again = parse_dotenv(&written).expect("written entries parse");
    assert_eq!(again, entries, "parse, write and parse differ");
}

fn pairs(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

fuzz_target!(|data: &[u8]| {
    let Some((&depth, rest)) = data.split_first() else {
        return;
    };
    let Ok(rest) = std::str::from_utf8(rest) else {
        return;
    };
    let mut parts = rest.split('\0');
    let mut next = || parts.next().unwrap_or("");
    let (env_file, toml, json) = (next(), next(), next());
    let (vars, sets) = (pairs(next()), pairs(next()));

    dotenv(env_file);

    let dir = dir();
    let (env_path, toml_path, json_path) =
        (dir.join(".env"), dir.join("app.toml"), dir.join("app.json"));
    std::fs::write(&env_path, env_file).expect("a file");
    std::fs::write(&toml_path, toml).expect("a file");
    std::fs::write(&json_path, json).expect("a file");
    let mut sources = ConfigSources::new()
        .dotenv(env_path)
        .file(toml_path)
        .file(json_path)
        .env_vars("APP_", vars)
        .max_depth(usize::from(depth));
    for (key, value) in &sets {
        sources = sources.set(key, value.as_str());
    }
    let Ok(config) = Config::load(&sources) else {
        return;
    };
    let tree = config.redacted();
    let again = Config::load(&sources).expect("a load that worked works again");
    assert_eq!(again.redacted(), tree, "loading is deterministic");
    for (key, _) in &sets {
        let _ = config.get_string(key);
        let _ = config.get_bool(key);
        let _ = config.section::<serde_json::Value>(key);
    }
    let _ = serde_json::to_string(&tree).expect("the tree prints");
});
