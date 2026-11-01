//! Shared fixtures: the template directory, golden files and helpers.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use navree_documents::Templates;

/// `tests/fixtures/templates`.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/templates")
}

/// `data` compressed as small as Flate can: what a bomb is made of.
pub fn smallest_deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// The fixture templates.
pub fn fixture_templates() -> Templates {
    Templates::new(vec![fixtures_dir()], Vec::<(String, Vec<u8>)>::new())
}

/// Compares `actual` with `tests/golden/<name>`; with `UPDATE_GOLDEN=1`
/// in the environment, (re)writes the file instead.
pub fn assert_golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    let actual = normalize(actual);
    if std::env::var_os("UPDATE_GOLDEN").is_some() || !path.exists() {
        std::fs::write(&path, &actual).unwrap();
        if std::env::var_os("UPDATE_GOLDEN").is_none() {
            panic!(
                "wrote the missing golden file {}; check it and run again",
                path.display()
            );
        }
        return;
    }
    let expected = normalize(&std::fs::read_to_string(&path).unwrap());
    assert_eq!(
        actual,
        expected,
        "{} differs (UPDATE_GOLDEN=1 rewrites it)",
        path.display()
    );
}

/// Line endings and trailing whitespace normalized.
fn normalize(text: &str) -> String {
    let mut out: String = text
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    out.push('\n');
    out
}

/// The Chrome executable from `NAVREE_BROWSER_CHROME`, or `None` after
/// printing why the test is skipped (a failure when
/// `NAVREE_TEST_REQUIRE_SERVICES=1`).
pub fn chrome_or_skip(test: &str) -> Option<PathBuf> {
    match std::env::var_os("NAVREE_BROWSER_CHROME") {
        Some(path) if !path.is_empty() => Some(PathBuf::from(path)),
        _ => {
            if std::env::var_os("NAVREE_TEST_REQUIRE_SERVICES").is_some_and(|v| v == "1") {
                panic!("{test}: set NAVREE_BROWSER_CHROME (NAVREE_TEST_REQUIRE_SERVICES=1)");
            }
            println!("SKIPPED {test}: set NAVREE_BROWSER_CHROME");
            None
        }
    }
}
