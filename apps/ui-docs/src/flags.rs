//! The flags page's gallery: every flag the toolkit ships.
//!
//! Each cell is a `Flag`, drawn by the toolkit, with its code and English name.
//! A search matches the code, the name and what kind of flag it is.

use navree::ui::flags::{self, CodeKind};
use navree::ui::{Flag, FlagSize};

use crate::catalog::{Cells, Entry, Example, Kind};

/// One cell for every flag.
pub fn entries() -> Vec<Entry> {
    flags::ALL
        .iter()
        .map(|code| {
            let kind = match code.kind() {
                CodeKind::Country => "country",
                CodeKind::Region => "region",
                CodeKind::Organisation => "organisation body",
                _ => "",
            };
            Entry {
                caption: code.as_str().to_owned(),
                detail: Some(code.name().to_owned()),
                terms: format!(
                    "{} {} {kind}",
                    code.as_str().to_ascii_lowercase(),
                    code.name().to_lowercase()
                ),
                node: Flag::new(*code).size(FlagSize::Lg).decorative().into(),
            }
        })
        .collect()
}

/// The flags page's gallery.
pub fn all_flags() -> Example {
    let mut example = Example::new(
        "flags",
        "all",
        "Every flag",
        Kind::Search {
            label: "Search flags by country or code",
            noun: "flags",
            draw: entries,
        },
        "use navree::ui::{Flag, flag, flags};\n\n// A constant, completed by your editor:\nFlag::new(flags::AE)\n\n// The code as a person writes it, checked while your app compiles:\nflag!(\"gb-sct\")\n\n// A code that comes from data: any case, and an unknown code draws a placeholder.\nFlag::country(code)",
    )
    .fill()
    .height(640);
    example.cells = Cells::Flag;
    example
}

#[cfg(test)]
mod tests {
    use navree::ui::flags;

    #[test]
    fn every_flag_has_a_cell_you_can_search_by_code_and_name() {
        let entries = super::entries();
        assert_eq!(entries.len(), flags::ALL.len());
        let uae = entries.iter().find(|e| e.caption == "AE").expect("AE");
        assert!(uae.terms.contains("united arab emirates") && uae.terms.contains("country"));
        assert_eq!(uae.detail.as_deref(), Some("United Arab Emirates"));
    }
}
