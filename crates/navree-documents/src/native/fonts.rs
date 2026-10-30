//! The fonts of the native engine: the bundled Noto families and the font
//! files of the configured directories, and the choice of a face for each
//! character.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use skrifa::raw::types::NameId;

use crate::fonts::BUNDLED;

/// The families text falls back through after its own, in order.
pub(crate) const FALLBACK: [&str; 6] = [
    "Noto Sans",
    "Noto Sans Arabic",
    "Noto Sans Hebrew",
    "Noto Sans Devanagari",
    "Noto Serif",
    "Noto Sans Mono",
];

/// A face's index in its [`FontBook`].
pub(crate) type FaceId = usize;

/// Font file bytes shared by the shaper and the PDF writer.
type FontData = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// One font face.
pub(crate) struct Face {
    pub(crate) family: String,
    /// 100 to 900.
    pub(crate) weight: u16,
    pub(crate) italic: bool,
    /// The font for shaping and metrics.
    pub(crate) font: skrifa::Font,
    /// The font for the PDF writer (subset and embedded on use).
    pub(crate) pdf: krilla::text::Font,
    /// Ascender and descender (positive, below the baseline), in ems.
    pub(crate) ascent: f32,
    pub(crate) descent: f32,
    /// Underline and strike-through: position of the stroke's top above
    /// the baseline, and thickness, in ems.
    pub(crate) underline: (f32, f32),
    pub(crate) strike: (f32, f32),
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Face")
            .field("family", &self.family)
            .field("weight", &self.weight)
            .field("italic", &self.italic)
            .finish_non_exhaustive()
    }
}

impl Face {
    /// The face in `data` at `index`, named `family` (else from its
    /// `name` table), or `None` when the data is not a font.
    fn load(data: FontData, index: u32, named: Option<(&str, u16, bool)>) -> Option<Face> {
        let font = skrifa::Font::new(data.clone(), index)?;
        let pdf = krilla::text::Font::new(krilla::Data::from(data), index)?;
        let (family, weight, italic) = match named {
            Some((family, weight, italic)) => (family.to_owned(), weight, italic),
            None => {
                let names = font.names();
                let family = names
                    .get(NameId::TYPOGRAPHIC_FAMILY_NAME)
                    .or_else(|| names.get(NameId::FAMILY_NAME))
                    .and_then(|name| name.english_or_first())
                    .map(|name| name.name().chars().collect::<String>())?;
                let attributes = font.default_attributes();
                let weight = attributes.weight.value().round().clamp(1.0, 1000.0) as u16;
                let italic = !matches!(attributes.style, skrifa::font::attribute::Style::Normal);
                (family, weight, italic)
            }
        };
        let metrics = font.metrics();
        let upem = f32::from(metrics.units_per_em.max(1));
        let line = metrics.h_line().unwrap_or_default();
        let mut ascent = line.ascender.to_f32() / upem;
        let mut descent = -line.descender.to_f32() / upem;
        if ascent <= 0.0 && descent <= 0.0 {
            ascent = 0.8;
            descent = 0.2;
        }
        let style = font.style_metrics();
        let decoration = |d: Option<skrifa::font::metrics::Decoration>, default: (f32, f32)| {
            d.map(|d| (d.position.to_f32() / upem, d.thickness.to_f32() / upem))
                .filter(|(_, thickness)| *thickness > 0.0)
                .unwrap_or(default)
        };
        let underline = decoration(style.underline, (-0.1, 0.05));
        let strike = decoration(style.strike_through, (0.3, 0.05));
        Some(Face {
            family,
            weight,
            italic,
            font,
            pdf,
            ascent,
            descent,
            underline,
            strike,
        })
    }

    /// Whether the face has a glyph for `c`.
    pub(crate) fn covers(&self, c: char) -> bool {
        self.font.charmap().map_unicode(c).is_some()
    }

    /// Units per em.
    pub(crate) fn units_per_em(&self) -> f32 {
        f32::from(self.font.metrics().units_per_em.max(1))
    }
}

/// Every face the engine can print with.
#[derive(Debug, Default)]
pub(crate) struct FontBook {
    faces: Vec<Face>,
    /// Lower-case family name → faces.
    families: BTreeMap<String, Vec<FaceId>>,
}

impl FontBook {
    /// The bundled fonts (when `bundled`) and every font file (`.ttf`,
    /// `.otf`, `.ttc`, `.otc`) under `dirs`, recursively. A file that is
    /// not a font is skipped with a warning.
    pub(crate) fn load(bundled: bool, dirs: &[PathBuf]) -> FontBook {
        let mut book = FontBook::default();
        if bundled {
            for font in BUNDLED {
                let data: FontData = Arc::new(font.data);
                if let Some(face) =
                    Face::load(data, 0, Some((font.family, font.weight, font.italic)))
                {
                    book.push(face);
                }
            }
        }
        for dir in dirs {
            book.load_dir(dir, 0);
        }
        book
    }

    /// The bundled fonts, loaded once per process.
    pub(crate) fn bundled() -> Arc<FontBook> {
        static BOOK: OnceLock<Arc<FontBook>> = OnceLock::new();
        BOOK.get_or_init(|| Arc::new(FontBook::load(true, &[])))
            .clone()
    }

    fn push(&mut self, face: Face) {
        let id = self.faces.len();
        self.families
            .entry(face.family.to_lowercase())
            .or_default()
            .push(id);
        self.faces.push(face);
    }

    fn load_dir(&mut self, dir: &Path, depth: usize) {
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
                self.load_dir(&path, depth + 1);
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
                Ok(bytes) => {
                    let data: FontData = Arc::new(bytes);
                    let before = self.faces.len();
                    // A collection holds several faces; a plain file one.
                    for index in 0..64 {
                        match Face::load(data.clone(), index, None) {
                            Some(face) => self.push(face),
                            None => break,
                        }
                    }
                    if self.faces.len() == before {
                        tracing::warn!(path = %path.display(), "documents: not a font file, skipped");
                    }
                }
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "documents: cannot read font file");
                }
            }
        }
    }

    /// The face `id`.
    pub(crate) fn face(&self, id: FaceId) -> &Face {
        &self.faces[id]
    }

    /// Whether there are no faces at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// The family names, sorted, without duplicates.
    pub(crate) fn families(&self) -> Vec<String> {
        let mut families: Vec<String> = self.faces.iter().map(|f| f.family.clone()).collect();
        families.sort();
        families.dedup();
        families
    }

    /// The face of `family` closest to `bold` and `italic`.
    pub(crate) fn face_of(&self, family: &str, bold: bool, italic: bool) -> Option<FaceId> {
        let wanted = if bold { 700 } else { 400 };
        let faces = self.families.get(&generic(family).to_lowercase())?;
        faces.iter().copied().min_by_key(|&id| {
            let face = &self.faces[id];
            let style = if face.italic == italic { 0 } else { 10_000 };
            style + u32::from(face.weight.abs_diff(wanted))
        })
    }

    /// The face that prints `c` in a run asking for `families`: the first
    /// of them that has it, then the fallback families, then any face
    /// with it, then the run's first family (its `.notdef` box).
    pub(crate) fn select(
        &self,
        families: &[String],
        bold: bool,
        italic: bool,
        c: char,
    ) -> Option<FaceId> {
        let wanted = families
            .iter()
            .map(String::as_str)
            .chain(FALLBACK)
            .filter_map(|family| self.face_of(family, bold, italic));
        let mut first = None;
        for id in wanted {
            first.get_or_insert(id);
            if self.faces[id].covers(c) {
                return Some(id);
            }
        }
        if let Some(id) = (0..self.faces.len()).find(|&id| self.faces[id].covers(c)) {
            return Some(id);
        }
        first.or(if self.faces.is_empty() { None } else { Some(0) })
    }

    /// The face a run's empty lines and metrics use: its first family
    /// that exists, else the first fallback, else any.
    pub(crate) fn primary(&self, families: &[String], bold: bool, italic: bool) -> Option<FaceId> {
        families
            .iter()
            .map(String::as_str)
            .chain(FALLBACK)
            .find_map(|family| self.face_of(family, bold, italic))
            .or(if self.faces.is_empty() { None } else { Some(0) })
    }
}

/// A CSS generic family as the bundled family that stands for it.
fn generic(family: &str) -> &str {
    match family.to_ascii_lowercase().as_str() {
        "sans-serif" | "system-ui" | "ui-sans-serif" | "cursive" | "fantasy" | "arial"
        | "helvetica" => "Noto Sans",
        "serif" | "ui-serif" | "times" | "times new roman" | "georgia" => "Noto Serif",
        "monospace" | "ui-monospace" | "courier" | "courier new" => "Noto Sans Mono",
        _ => family,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_families_load() {
        let book = FontBook::bundled();
        let families = book.families();
        for family in FALLBACK {
            assert!(families.iter().any(|f| f == family), "{families:?}");
        }
        assert_eq!(families.len(), FALLBACK.len());
        let bold = book.face_of("Noto Sans", true, false).unwrap();
        assert_eq!(book.face(bold).weight, 700);
        assert!(!book.face(bold).italic);
        let italic = book.face_of("noto serif", false, true).unwrap();
        assert!(book.face(italic).italic);
        // No italic Arabic: the upright face.
        let arabic = book.face_of("Noto Sans Arabic", false, true).unwrap();
        assert!(!book.face(arabic).italic);
        assert_eq!(
            book.face(book.face_of("monospace", false, false).unwrap())
                .family,
            "Noto Sans Mono"
        );
        let face = book.face(book.primary(&[], false, false).unwrap());
        assert!(face.ascent > 0.5 && face.descent > 0.1, "{face:?}");
    }

    #[test]
    fn characters_fall_back_to_a_family_that_has_them() {
        let book = FontBook::bundled();
        let serif = vec!["Noto Serif".to_owned()];
        let family = |c: char| {
            book.face(book.select(&serif, false, false, c).unwrap())
                .family
                .clone()
        };
        assert_eq!(family('A'), "Noto Serif");
        assert_eq!(family('م'), "Noto Sans Arabic");
        assert_eq!(family('ש'), "Noto Sans Hebrew");
        assert_eq!(family('क'), "Noto Sans Devanagari");
        // Nobody has it: the run's own family (a .notdef box).
        assert_eq!(family('\u{10FFFD}'), "Noto Serif");
        let unknown = vec!["Not A Font".to_owned()];
        assert_eq!(
            book.face(book.select(&unknown, false, false, 'x').unwrap())
                .family,
            "Noto Sans"
        );
    }

    #[test]
    fn font_directories_add_families_and_skip_other_files() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        std::fs::create_dir_all(dir.path().join("nested")).unwrap();
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/fonts/NotoSansHebrew-Bold.ttf"),
            dir.path().join("nested/Hebrew.ttf"),
        )
        .unwrap();
        std::fs::write(dir.path().join("broken.ttf"), b"not a font").unwrap();
        std::fs::write(dir.path().join("readme.txt"), b"fonts").unwrap();
        let book = FontBook::load(false, &[dir.path().to_path_buf()]);
        assert_eq!(book.families(), vec!["Noto Sans Hebrew".to_owned()]);
        let face = book.face(book.face_of("Noto Sans Hebrew", true, false).unwrap());
        assert_eq!(face.weight, 700);
        assert!(FontBook::load(false, &[]).is_empty());
    }
}
