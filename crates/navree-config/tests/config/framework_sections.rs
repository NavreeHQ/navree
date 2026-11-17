//! `FRAMEWORK_SECTIONS` names every section the framework's crates read, so
//! `config:check` does not report one as unknown when the app does not use
//! the module that reads it. This test keeps the list equal to the
//! `ConfigSection` impls and `#[config(key = "..")]` derives under
//! `crates/*/src`.

use std::path::{Path, PathBuf};

use navree_config::FRAMEWORK_SECTIONS;

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The key of every section in `src`: derived (`#[config(key = ".."`) and
/// written by hand.
fn section_keys(src: &str) -> Vec<String> {
    let mut keys = derived_keys(src);
    keys.extend(hand_written_keys(src));
    keys
}

/// The key of every `#[derive(ConfigSection)]` in `src`: the `key = ".."` of
/// a `#[config(..)]` attribute (it may span lines).
fn derived_keys(src: &str) -> Vec<String> {
    const ATTRIBUTE: &str = "#[config(";
    const KEY: &str = "key = \"";
    let mut keys = Vec::new();
    let mut rest = src;
    while let Some(at) = rest.find(ATTRIBUTE) {
        let after = &rest[at + ATTRIBUTE.len()..];
        let attribute = &after[..after.find(")]").unwrap_or(after.len())];
        if let Some(key_at) = attribute.find(KEY) {
            let value = &attribute[key_at + KEY.len()..];
            if let Some(end) = value.find('"') {
                keys.push(value[..end].to_owned());
            }
        }
        rest = after;
    }
    keys
}

/// The `KEY` of every `impl ConfigSection for ...` in `src`.
fn hand_written_keys(src: &str) -> Vec<String> {
    const IMPL: &str = "impl ConfigSection for ";
    const KEY: &str = "const KEY: &'static str = \"";
    let mut keys = Vec::new();
    let mut rest = src;
    while let Some(at) = rest.find(IMPL) {
        let after = &rest[at + IMPL.len()..];
        // The impl's own body starts at the next `{`; its first `KEY` follows.
        if let Some(body) = after.find('{') {
            let body = &after[body..];
            let window = &body[..body.len().min(400)];
            if let Some(key) = window.find(KEY) {
                let value = &window[key + KEY.len()..];
                if let Some(end) = value.find('"') {
                    keys.push(value[..end].to_owned());
                }
            }
        }
        rest = after;
    }
    keys
}

/// Where a file's inline test module starts: `#[cfg(test)]` right before
/// `mod .. {`. A test module in a file of its own (`mod tests;`) and an item
/// that is only compiled for tests are not one.
fn inline_tests(src: &str) -> Option<usize> {
    const ATTRIBUTE: &str = "#[cfg(test)]";
    src.match_indices(ATTRIBUTE).map(|(at, _)| at).find(|at| {
        let after = src[at + ATTRIBUTE.len()..].trim_start();
        after.starts_with("mod ")
            && after
                .find(['{', ';'])
                .is_some_and(|end| after[end..].starts_with('{'))
    })
}

#[test]
fn an_inline_test_module_is_where_a_files_own_tests_start() {
    let src = "impl ConfigSection for A {}\n#[cfg(test)]\nuse x::Y;\n#[cfg(test)]\nmod tests;\n\n#[cfg(test)]\nmod tests {\n}\n";
    let at = inline_tests(src).expect("an inline test module");
    assert!(src[at..].starts_with("#[cfg(test)]\nmod tests {"));
    assert_eq!(inline_tests("#[cfg(test)]\nmod tests;\n"), None);
    assert_eq!(inline_tests("fn main() {}\n"), None);
}

#[test]
fn the_list_covers_every_config_section_of_the_framework() {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates_dir()).expect("crates").flatten() {
        // The macros' own tests expand made-up sections.
        if entry.file_name() == "navree-macros" {
            continue;
        }
        let src = entry.path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    assert!(
        files.len() > 500,
        "the sources were not found: {}",
        files.len()
    );

    let mut missing = Vec::new();
    let mut found = 0;
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        // A file's own unit tests make up sections that no app reads.
        let text = &text[..inline_tests(&text).unwrap_or(text.len())];
        // Skip the commented examples of rustdoc.
        let code: String = text
            .lines()
            .filter(|line| {
                let line = line.trim_start();
                !(line.starts_with("//"))
            })
            .collect::<Vec<_>>()
            .join("\n");
        for key in section_keys(&code) {
            found += 1;
            let top = key.split('.').next().unwrap_or(&key);
            if !FRAMEWORK_SECTIONS.contains(&key.as_str()) || !FRAMEWORK_SECTIONS.contains(&top) {
                missing.push(format!("{key} ({})", file.display()));
            }
        }
    }
    assert!(found >= 40, "found only {found} sections");
    assert!(
        missing.is_empty(),
        "add these to FRAMEWORK_SECTIONS: {missing:?}"
    );
}

#[test]
fn the_list_is_sorted_and_has_no_duplicates() {
    let mut sorted = FRAMEWORK_SECTIONS.to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, FRAMEWORK_SECTIONS, "keep the list sorted");
    sorted.dedup();
    assert_eq!(sorted.len(), FRAMEWORK_SECTIONS.len());
}

#[test]
fn sections_read_without_a_config_section_are_listed() {
    // Read with `section_or_default` or `get_string`, so the scan above does
    // not see them.
    for key in ["hashing", "ui", "validation"] {
        assert!(FRAMEWORK_SECTIONS.contains(&key), "{key}");
    }
}
