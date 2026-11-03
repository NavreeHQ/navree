//! The icons page's gallery: every icon the toolkit ships, with the words people
//! search for it by.
//!
//! The words are Lucide's own tags, generated with the icons into
//! `data/lucide-tags.json`. The icons are drawn by the toolkit's `components::icon`,
//! so the page shows the markup an app would emit.

use std::collections::BTreeMap;

use navree::ui::{components, icons};
use serde::Deserialize;

use crate::catalog::{Cells, Entry, Example, Kind};

/// Lucide's tags by icon name.
#[derive(Deserialize)]
#[serde(transparent)]
struct Tags(BTreeMap<String, Vec<String>>);

fn tags() -> Tags {
    serde_json::from_str(&crate::read_repo("apps/ui-docs/data/lucide-tags.json"))
        .unwrap_or_else(|e| panic!("data/lucide-tags.json: {e}"))
}

/// One cell for every icon: its name, and the words a search matches.
pub fn entries() -> Vec<Entry> {
    let tags = tags();
    icons::ALL
        .iter()
        .map(|(name, icon)| {
            let mut terms = format!("{name} {}", name.replace('-', "_"));
            for tag in tags.0.get(*name).into_iter().flatten() {
                terms.push(' ');
                terms.push_str(tag);
            }
            Entry {
                caption: (*name).to_owned(),
                detail: None,
                terms,
                node: components::icon(*icon),
            }
        })
        .collect()
}

/// The icons page's gallery.
pub fn all_icons() -> Example {
    let mut example = Example::new(
        "icons",
        "all",
        "Every icon",
        Kind::Search {
            label: "Search icons by name or tag",
            noun: "icons",
            draw: entries,
        },
        "use navree::ui::{icons, lucide};\n\n// A constant, completed by your editor:\nButton::primary(\"New deal\").icon(icons::MAIL_OPEN)\n\n// The name as lucide.dev writes it, checked while your app compiles:\nButton::primary(\"New deal\").icon(lucide!(\"mail-open\"))",
    )
    .fill()
    .height(640);
    example.cells = Cells::Icon;
    example
}

#[cfg(test)]
mod tests {
    use navree::ui::{components, icons, render};

    #[test]
    fn every_icon_has_a_cell_and_a_searchable_name() {
        let entries = super::entries();
        assert_eq!(entries.len(), icons::ALL.len());
        let mail = entries
            .iter()
            .find(|entry| entry.caption == "mail-open")
            .expect("mail-open is an icon");
        assert!(mail.terms.contains("mail_open"));
        assert!(mail.terms.contains("email"), "{}", mail.terms);
        assert_eq!(
            render(&mail.node),
            render(&components::icon(icons::MAIL_OPEN))
        );
    }

    #[test]
    fn the_tags_cover_every_icon() {
        let tags = super::tags();
        for (name, _) in icons::ALL {
            assert!(tags.0.contains_key(*name), "{name} has no tags entry");
        }
    }
}
