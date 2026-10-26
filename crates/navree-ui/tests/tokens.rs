//! The stylesheet's palette is the site's: every `--color-*` token of
//! `apps/website/src/app/globals.css` has the same value as `--ui-*` in
//! `navree-ui.css` (checked when the website sources are present).

use std::collections::BTreeMap;
use std::path::Path;

/// `--{prefix}name: value;` declarations of `css`.
fn tokens(css: &str, prefix: &str) -> BTreeMap<String, String> {
    css.lines()
        .filter_map(|line| line.trim().strip_prefix(prefix))
        .filter_map(|rest| rest.split_once(':'))
        .map(|(name, value)| {
            (
                name.trim().to_owned(),
                value.trim().trim_end_matches(';').trim().to_owned(),
            )
        })
        .collect()
}

#[test]
fn the_palette_matches_the_site() {
    let globals =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/website/src/app/globals.css");
    let Ok(site) = std::fs::read_to_string(&globals) else {
        eprintln!("{} not found; skipped", globals.display());
        return;
    };
    let kit = navree_ui::assets::get("navree-ui.css").unwrap();
    let kit = std::str::from_utf8(kit.bytes).unwrap();
    let site = tokens(&site, "--color-");
    let kit = tokens(kit, "--ui-");
    let palette: Vec<&String> = site
        .keys()
        .filter(|name| site[*name].starts_with('#'))
        .collect();
    assert!(palette.len() >= 15, "{palette:?}");
    for name in palette {
        assert_eq!(
            kit.get(name),
            Some(&site[name]),
            "--ui-{name} must equal the site's --color-{name}"
        );
    }
}
