//! Shared helpers: the two settings Navree renders with, and fixtures.

use navree_template::{AutoEscape, Environment, Error, ErrorKind, UndefinedBehavior};
use serde_json::{Value, json};

/// Views' settings: escaping by file name, undefined values fail when used.
pub fn view_env() -> Environment {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::SemiStrict);
    env
}

/// The settings of generated files and scripts: blocks trim their line, the
/// trailing newline stays, nothing is escaped, undefined values always fail.
pub fn plain_env() -> Environment {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.set_keep_trailing_newline(true);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env
}

/// The context of the language fixtures.
pub fn ctx() -> Value {
    json!({
        "s": "<b>&\"'/x</b>",
        "n": 42,
        "neg": -7,
        "f": 1.5,
        "big": 12345678901234567890_u64,
        "whole": 3.0,
        "t": true,
        "no": false,
        "nil": null,
        "empty": "",
        "list": [1, "two", null, 3.5, [4, "five"], {"k": "v"}],
        "strs": ["b", "a", "<c>", "A"],
        "nums": [3, 1, 2],
        "map": {"b": 2, "a": 1, "z<": "y>"},
        "nested": {"a": {"b": {"c": [10, 20, {"d": "deep"}]}}},
        "rows": [{"name": "x", "v": 1}, {"name": "y", "v": 2}],
        "pairs": [["a", 1], ["b", 2]],
        "text": "Hello World foo  bar",
        "uni": "héllo wörld ñ 日本語",
    })
}

/// An error as the fixtures write it: kind, template and line.
pub fn outcome(result: Result<String, Error>) -> Value {
    match result {
        Ok(output) => json!({ "o": output }),
        Err(error) => json!({ "e": describe(&error) }),
    }
}

pub fn describe(error: &Error) -> String {
    let kind = match error.kind() {
        ErrorKind::InvalidOperation => "InvalidOperation",
        ErrorKind::SyntaxError => "SyntaxError",
        ErrorKind::TemplateNotFound => "TemplateNotFound",
        ErrorKind::TooManyArguments => "TooManyArguments",
        ErrorKind::MissingArgument => "MissingArgument",
        ErrorKind::UnknownFilter => "UnknownFilter",
        ErrorKind::UnknownTest => "UnknownTest",
        ErrorKind::UnknownFunction => "UnknownFunction",
        ErrorKind::UnknownMethod => "UnknownMethod",
        ErrorKind::BadEscape => "BadEscape",
        ErrorKind::UndefinedError => "UndefinedError",
        ErrorKind::BadSerialization => "BadSerialization",
        ErrorKind::BadInclude => "BadInclude",
        ErrorKind::CannotUnpack => "CannotUnpack",
        ErrorKind::UnknownBlock => "UnknownBlock",
        _ => "Other",
    };
    format!(
        "{kind}|{}|{}",
        error.name().unwrap_or("-"),
        error.line().unwrap_or(0)
    )
}

/// The lines of a fixture file, parsed.
pub fn fixture(text: &str) -> Vec<Value> {
    text.lines()
        .map(|line| serde_json::from_str(line).expect("a fixture line"))
        .collect()
}

/// FNV-1a over `text`: how the escaping fixtures hold their outputs.
pub fn fnv(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}
