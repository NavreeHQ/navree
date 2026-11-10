//! The `@navree/theme` package and the toolkit's tokens agree: the colours
//! the package declares are the colours of the shared set, to the digit.
//! (The website checks its own stylesheet against the same package with a
//! check of its own.)

use std::collections::BTreeMap;

use navree_ui::tokens;

/// `--color-<name>: #rrggbb` declarations in the first `@theme` block.
fn declared(css: &str) -> BTreeMap<String, String> {
    let block = css.split("@theme inline").next().unwrap_or(css);
    let block = block.split("@theme {").nth(1).unwrap_or_default();
    let block = block.split('}').next().unwrap_or_default();
    block
        .lines()
        .filter_map(|line| {
            let line = line.trim().strip_prefix("--color-")?;
            let (name, value) = line.split_once(':')?;
            Some((
                name.trim().to_owned(),
                value.trim().trim_end_matches(';').to_owned(),
            ))
        })
        .collect()
}

#[test]
fn the_themes_colours_are_the_shared_tokens() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/js/theme/css/theme.css");
    let css = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let theme = declared(&css);
    assert!(!theme.is_empty(), "no colours found in {}", path.display());
    let ours: BTreeMap<String, String> = tokens::SITE
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                tokens::dark(name).unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        theme, ours,
        "the theme package and the token table disagree; change the table, then write the generated files"
    );
}

#[test]
fn every_shared_token_exists_in_both_themes() {
    for name in tokens::SITE {
        let token = tokens::TOKENS
            .iter()
            .find(|token| token.name == *name)
            .unwrap_or_else(|| panic!("{name} is not a token"));
        assert!(
            token.dark.starts_with('#') && token.light.starts_with('#'),
            "{name}"
        );
    }
}
