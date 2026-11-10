//! The bundled Noto fonts (SIL Open Font License 1.1, see `fonts/OFL.txt`),
//! shared by both engines: the native engine loads them in process, Chrome
//! reaches them through [`FONTS_STYLESHEET`].

/// One bundled font file.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(feature = "chrome"), allow(dead_code))]
pub(crate) struct BundledFont {
    /// The file name (`NotoSans-Regular.ttf`).
    pub(crate) file: &'static str,
    /// The family (`Noto Sans`).
    pub(crate) family: &'static str,
    /// 400 or 700.
    pub(crate) weight: u16,
    /// Whether the face is italic.
    pub(crate) italic: bool,
    /// The TrueType data.
    pub(crate) data: &'static [u8],
}

/// Noto Sans and Noto Serif (Latin, Greek and Cyrillic) in regular, bold,
/// italic and bold italic, Noto Sans Mono, Noto Sans Arabic, Noto Sans
/// Hebrew, Noto Sans Devanagari and Noto Sans Gurmukhi in regular and bold.
pub(crate) const BUNDLED: [BundledFont; 18] = [
    font(
        "NotoSans-Regular.ttf",
        "Noto Sans",
        400,
        false,
        include_bytes!("../fonts/NotoSans-Regular.ttf"),
    ),
    font(
        "NotoSans-Bold.ttf",
        "Noto Sans",
        700,
        false,
        include_bytes!("../fonts/NotoSans-Bold.ttf"),
    ),
    font(
        "NotoSans-Italic.ttf",
        "Noto Sans",
        400,
        true,
        include_bytes!("../fonts/NotoSans-Italic.ttf"),
    ),
    font(
        "NotoSans-BoldItalic.ttf",
        "Noto Sans",
        700,
        true,
        include_bytes!("../fonts/NotoSans-BoldItalic.ttf"),
    ),
    font(
        "NotoSerif-Regular.ttf",
        "Noto Serif",
        400,
        false,
        include_bytes!("../fonts/NotoSerif-Regular.ttf"),
    ),
    font(
        "NotoSerif-Bold.ttf",
        "Noto Serif",
        700,
        false,
        include_bytes!("../fonts/NotoSerif-Bold.ttf"),
    ),
    font(
        "NotoSerif-Italic.ttf",
        "Noto Serif",
        400,
        true,
        include_bytes!("../fonts/NotoSerif-Italic.ttf"),
    ),
    font(
        "NotoSerif-BoldItalic.ttf",
        "Noto Serif",
        700,
        true,
        include_bytes!("../fonts/NotoSerif-BoldItalic.ttf"),
    ),
    font(
        "NotoSansMono-Regular.ttf",
        "Noto Sans Mono",
        400,
        false,
        include_bytes!("../fonts/NotoSansMono-Regular.ttf"),
    ),
    font(
        "NotoSansMono-Bold.ttf",
        "Noto Sans Mono",
        700,
        false,
        include_bytes!("../fonts/NotoSansMono-Bold.ttf"),
    ),
    font(
        "NotoSansArabic-Regular.ttf",
        "Noto Sans Arabic",
        400,
        false,
        include_bytes!("../fonts/NotoSansArabic-Regular.ttf"),
    ),
    font(
        "NotoSansArabic-Bold.ttf",
        "Noto Sans Arabic",
        700,
        false,
        include_bytes!("../fonts/NotoSansArabic-Bold.ttf"),
    ),
    font(
        "NotoSansHebrew-Regular.ttf",
        "Noto Sans Hebrew",
        400,
        false,
        include_bytes!("../fonts/NotoSansHebrew-Regular.ttf"),
    ),
    font(
        "NotoSansHebrew-Bold.ttf",
        "Noto Sans Hebrew",
        700,
        false,
        include_bytes!("../fonts/NotoSansHebrew-Bold.ttf"),
    ),
    font(
        "NotoSansDevanagari-Regular.ttf",
        "Noto Sans Devanagari",
        400,
        false,
        include_bytes!("../fonts/NotoSansDevanagari-Regular.ttf"),
    ),
    font(
        "NotoSansDevanagari-Bold.ttf",
        "Noto Sans Devanagari",
        700,
        false,
        include_bytes!("../fonts/NotoSansDevanagari-Bold.ttf"),
    ),
    font(
        "NotoSansGurmukhi-Regular.ttf",
        "Noto Sans Gurmukhi",
        400,
        false,
        include_bytes!("../fonts/NotoSansGurmukhi-Regular.ttf"),
    ),
    font(
        "NotoSansGurmukhi-Bold.ttf",
        "Noto Sans Gurmukhi",
        700,
        false,
        include_bytes!("../fonts/NotoSansGurmukhi-Bold.ttf"),
    ),
];

const fn font(
    file: &'static str,
    family: &'static str,
    weight: u16,
    italic: bool,
    data: &'static [u8],
) -> BundledFont {
    BundledFont {
        file,
        family,
        weight,
        italic,
        data,
    }
}

/// The stylesheet that gives HTML templates printed by Chrome the bundled
/// fonts: `<link rel="stylesheet" href="/_navree/fonts.css">`, then
/// `font-family: "Noto Sans", "Noto Sans Arabic", "Noto Sans Hebrew",
/// "Noto Sans Devanagari", "Noto Sans Gurmukhi"` (or `"Noto Serif"`, `"Noto Sans Mono"`). The
/// native engine has the fonts without it.
/// The `_navree/` prefix of the document origin is reserved for it.
pub const FONTS_STYLESHEET: &str = "/_navree/fonts.css";

/// The reserved prefix (without its leading `/`).
#[cfg(feature = "chrome")]
pub(crate) const RESERVED_PREFIX: &str = "_navree/";

/// The `@font-face` rules of [`FONTS_STYLESHEET`].
#[cfg(feature = "chrome")]
pub(crate) fn stylesheet() -> String {
    let mut css = String::from("/* The bundled Noto fonts (SIL Open Font License 1.1). */\n");
    for font in BUNDLED {
        css.push_str(&format!(
            "@font-face {{ font-family: \"{}\"; src: url(\"/_navree/fonts/{}\") format(\"truetype\"); \
             font-weight: {}; font-style: {}; font-display: block; }}\n",
            font.family,
            font.file,
            font.weight,
            if font.italic { "italic" } else { "normal" },
        ));
    }
    css
}

/// The reserved file at `path` (relative, after `_navree/`): the
/// stylesheet or a font, with its content type.
#[cfg(feature = "chrome")]
pub(crate) fn reserved_file(path: &str) -> Option<(bytes::Bytes, &'static str)> {
    if path == "fonts.css" {
        return Some((bytes::Bytes::from(stylesheet()), "text/css; charset=utf-8"));
    }
    let file = path.strip_prefix("fonts/")?;
    BUNDLED
        .iter()
        .find(|font| font.file == file)
        .map(|font| (bytes::Bytes::from_static(font.data), "font/ttf"))
}

#[cfg(all(test, feature = "chrome"))]
mod tests {
    use super::*;

    #[test]
    fn the_stylesheet_names_every_bundled_face() {
        let css = stylesheet();
        assert_eq!(css.matches("@font-face").count(), BUNDLED.len());
        assert!(css.contains(
            "font-family: \"Noto Sans Arabic\"; src: url(\"/_navree/fonts/NotoSansArabic-Bold.ttf\") \
             format(\"truetype\"); font-weight: 700; font-style: normal;"
        ));
        assert!(css.contains(
            "NotoSans-BoldItalic.ttf\") format(\"truetype\"); font-weight: 700; font-style: italic;"
        ));
    }

    #[test]
    fn reserved_files_are_the_stylesheet_and_the_fonts() {
        let (css, content_type) = reserved_file("fonts.css").unwrap();
        assert_eq!(content_type, "text/css; charset=utf-8");
        assert!(css.starts_with(b"/* The bundled"));
        let (font, content_type) = reserved_file("fonts/NotoSansDevanagari-Regular.ttf").unwrap();
        assert_eq!(content_type, "font/ttf");
        assert_eq!(font.len(), BUNDLED[14].data.len());
        assert!(reserved_file("fonts/NotoSerif-BoldItalic.ttf").is_some());
        assert!(reserved_file("fonts/other.ttf").is_none());
        assert!(reserved_file("secret").is_none());
    }
}
