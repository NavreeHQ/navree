//! The fonts the Typst engine uses: the bundled Noto fonts and the fonts
//! in the configured directories.

use std::path::{Path, PathBuf};

use typst::foundations::Bytes;
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;

/// The bundled fonts (SIL Open Font License 1.1, see `fonts/OFL.txt`):
/// Noto Sans (Latin, Greek and Cyrillic) in regular, bold, italic and bold
/// italic, Noto Sans Arabic and Noto Sans Devanagari in regular and bold.
pub(crate) const BUNDLED: [(&str, &[u8]); 8] = [
    (
        "NotoSans-Regular.ttf",
        include_bytes!("../../fonts/NotoSans-Regular.ttf"),
    ),
    (
        "NotoSans-Bold.ttf",
        include_bytes!("../../fonts/NotoSans-Bold.ttf"),
    ),
    (
        "NotoSans-Italic.ttf",
        include_bytes!("../../fonts/NotoSans-Italic.ttf"),
    ),
    (
        "NotoSans-BoldItalic.ttf",
        include_bytes!("../../fonts/NotoSans-BoldItalic.ttf"),
    ),
    (
        "NotoSansArabic-Regular.ttf",
        include_bytes!("../../fonts/NotoSansArabic-Regular.ttf"),
    ),
    (
        "NotoSansArabic-Bold.ttf",
        include_bytes!("../../fonts/NotoSansArabic-Bold.ttf"),
    ),
    (
        "NotoSansDevanagari-Regular.ttf",
        include_bytes!("../../fonts/NotoSansDevanagari-Regular.ttf"),
    ),
    (
        "NotoSansDevanagari-Bold.ttf",
        include_bytes!("../../fonts/NotoSansDevanagari-Bold.ttf"),
    ),
];

/// The families text falls back through by default, in order: Latin,
/// Greek and Cyrillic first, then Arabic, then Devanagari.
pub(crate) const DEFAULT_FAMILIES: [&str; 3] = ["Noto Sans", "Noto Sans Arabic", "Noto Sans Devanagari"];

/// The loaded fonts and their book.
pub(crate) struct FontSet {
    pub(crate) book: LazyHash<FontBook>,
    pub(crate) fonts: Vec<Font>,
}

impl FontSet {
    /// The bundled fonts (when `bundled`) and every font file (`.ttf`,
    /// `.otf`, `.ttc`, `.otc`) under `dirs`, recursively. A file that is
    /// not a font is skipped with a warning.
    pub(crate) fn load(bundled: bool, dirs: &[PathBuf]) -> FontSet {
        let mut fonts = Vec::new();
        if bundled {
            for (_, data) in BUNDLED {
                fonts.extend(Font::iter(Bytes::new(data)));
            }
        }
        for dir in dirs {
            load_dir(dir, &mut fonts, 0);
        }
        let book = FontBook::from_fonts(&fonts);
        FontSet {
            book: LazyHash::new(book),
            fonts,
        }
    }

    /// The family names, sorted, without duplicates.
    pub(crate) fn families(&self) -> Vec<String> {
        let mut families: Vec<String> = self
            .fonts
            .iter()
            .map(|font| font.info().family.clone())
            .collect();
        families.sort();
        families.dedup();
        families
    }
}

fn load_dir(dir: &Path, fonts: &mut Vec<Font>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            load_dir(&path, fonts, depth + 1);
            continue;
        }
        let is_font = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "ttf" | "otf" | "ttc" | "otc"
                )
            });
        if !is_font {
            continue;
        }
        match std::fs::read(&path) {
            Ok(data) => {
                let before = fonts.len();
                fonts.extend(Font::iter(Bytes::new(data)));
                if fonts.len() == before {
                    tracing::warn!(path = %path.display(), "documents: not a font file, skipped");
                }
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "documents: cannot read font file");
            }
        }
    }
}
