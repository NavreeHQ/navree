//! Language tags: parsing, parents, direction and native names.

use unic_langid::{CharacterDirection, LanguageIdentifier};

/// `tag` parsed and printed in its canonical form (`pt-br` → `pt-BR`,
/// `en_US` → `en-US`); an error for anything that is not a BCP 47 language
/// tag.
pub(crate) fn canonical(tag: &str) -> Result<String, String> {
    parse(tag).map(|langid| langid.to_string())
}

/// `tag` parsed (`_` is accepted for `-`). The empty tag and `und` are not
/// languages, so they are errors.
pub(crate) fn parse(tag: &str) -> Result<LanguageIdentifier, String> {
    let trimmed = tag.trim();
    let langid: LanguageIdentifier = trimmed
        .parse()
        .map_err(|_| format!("`{tag}` is not a valid BCP 47 language tag"))?;
    if trimmed.is_empty() || langid.language.is_empty() {
        return Err(format!("`{tag}` is not a valid BCP 47 language tag"));
    }
    Ok(langid)
}

/// The less specific tags `tag` falls back to, most specific first:
/// `zh-Hant-TW` → `zh-Hant`, `zh`; `fr-CA` → `fr`; `en` → none.
pub(crate) fn parents(langid: &LanguageIdentifier) -> Vec<String> {
    let mut parents = Vec::new();
    let mut current = langid.clone();
    if current.variants().len() > 0 {
        current.clear_variants();
        parents.push(current.to_string());
    }
    if current.region.is_some() {
        current.region = None;
        parents.push(current.to_string());
    }
    if current.script.is_some() {
        current.script = None;
        parents.push(current.to_string());
    }
    parents
}

/// Whether the locale writes right to left (Arabic, Hebrew, Persian, ...).
pub(crate) fn is_rtl(langid: &LanguageIdentifier) -> bool {
    langid.character_direction() == CharacterDirection::RTL
}

/// Whether the tag is regional: it has a region, script or variant, so a
/// less specific locale stands behind it (`pt-BR` behind `pt`).
pub(crate) fn is_regional(langid: &LanguageIdentifier) -> bool {
    !parents(langid).is_empty()
}

/// The name of the locale in its own language (`fr` → `Français`,
/// `pt-BR` → `Português (Brasil)`), from a built-in table; the tag itself
/// for a language the table does not know.
pub(crate) fn native_name(langid: &LanguageIdentifier) -> String {
    let tag = langid.to_string();
    if let Some((_, name)) = REGIONAL_NAMES.iter().find(|(known, _)| *known == tag) {
        return (*name).to_owned();
    }
    let language = langid.language.as_str();
    match LANGUAGE_NAMES.iter().find(|(code, _)| *code == language) {
        Some((_, name)) if langid.region.is_some() || langid.script.is_some() => {
            let qualifier: Vec<&str> = langid
                .script
                .iter()
                .map(|script| script.as_str())
                .chain(langid.region.iter().map(|region| region.as_str()))
                .collect();
            format!("{name} ({})", qualifier.join("-"))
        }
        Some((_, name)) => (*name).to_owned(),
        None => tag,
    }
}

/// Native names of languages, by ISO 639 code.
const LANGUAGE_NAMES: &[(&str, &str)] = &[
    ("af", "Afrikaans"),
    ("am", "አማርኛ"),
    ("ar", "العربية"),
    ("az", "Azərbaycan"),
    ("be", "Беларуская"),
    ("bg", "Български"),
    ("bn", "বাংলা"),
    ("bs", "Bosanski"),
    ("ca", "Català"),
    ("cs", "Čeština"),
    ("cy", "Cymraeg"),
    ("da", "Dansk"),
    ("de", "Deutsch"),
    ("el", "Ελληνικά"),
    ("en", "English"),
    ("eo", "Esperanto"),
    ("es", "Español"),
    ("et", "Eesti"),
    ("eu", "Euskara"),
    ("fa", "فارسی"),
    ("fi", "Suomi"),
    ("fil", "Filipino"),
    ("fr", "Français"),
    ("ga", "Gaeilge"),
    ("gl", "Galego"),
    ("gu", "ગુજરાતી"),
    ("he", "עברית"),
    ("hi", "हिन्दी"),
    ("hr", "Hrvatski"),
    ("hu", "Magyar"),
    ("hy", "Հայերեն"),
    ("id", "Bahasa Indonesia"),
    ("is", "Íslenska"),
    ("it", "Italiano"),
    ("ja", "日本語"),
    ("ka", "ქართული"),
    ("kk", "Қазақ"),
    ("km", "ខ្មែរ"),
    ("kn", "ಕನ್ನಡ"),
    ("ko", "한국어"),
    ("lt", "Lietuvių"),
    ("lv", "Latviešu"),
    ("mk", "Македонски"),
    ("ml", "മലയാളം"),
    ("mn", "Монгол"),
    ("mr", "मराठी"),
    ("ms", "Bahasa Melayu"),
    ("mt", "Malti"),
    ("my", "မြန်မာ"),
    ("nb", "Norsk bokmål"),
    ("ne", "नेपाली"),
    ("nl", "Nederlands"),
    ("nn", "Norsk nynorsk"),
    ("no", "Norsk"),
    ("pa", "ਪੰਜਾਬੀ"),
    ("pl", "Polski"),
    ("ps", "پښتو"),
    ("pt", "Português"),
    ("ro", "Română"),
    ("ru", "Русский"),
    ("si", "සිංහල"),
    ("sk", "Slovenčina"),
    ("sl", "Slovenščina"),
    ("sq", "Shqip"),
    ("sr", "Српски"),
    ("sv", "Svenska"),
    ("sw", "Kiswahili"),
    ("ta", "தமிழ்"),
    ("te", "తెలుగు"),
    ("th", "ไทย"),
    ("tr", "Türkçe"),
    ("uk", "Українська"),
    ("ur", "اردو"),
    ("uz", "Oʻzbek"),
    ("vi", "Tiếng Việt"),
    ("zh", "中文"),
    ("zu", "isiZulu"),
];

/// Native names of common regional locales.
const REGIONAL_NAMES: &[(&str, &str)] = &[
    ("de-AT", "Deutsch (Österreich)"),
    ("de-CH", "Deutsch (Schweiz)"),
    ("de-DE", "Deutsch (Deutschland)"),
    ("en-AU", "English (Australia)"),
    ("en-CA", "English (Canada)"),
    ("en-GB", "English (United Kingdom)"),
    ("en-IN", "English (India)"),
    ("en-US", "English (United States)"),
    ("es-419", "Español (Latinoamérica)"),
    ("es-AR", "Español (Argentina)"),
    ("es-ES", "Español (España)"),
    ("es-MX", "Español (México)"),
    ("fr-BE", "Français (Belgique)"),
    ("fr-CA", "Français (Canada)"),
    ("fr-CH", "Français (Suisse)"),
    ("fr-FR", "Français (France)"),
    ("it-CH", "Italiano (Svizzera)"),
    ("nl-BE", "Nederlands (België)"),
    ("pt-BR", "Português (Brasil)"),
    ("pt-PT", "Português (Portugal)"),
    ("zh-CN", "中文（中国）"),
    ("zh-HK", "中文（香港）"),
    ("zh-Hans", "简体中文"),
    ("zh-Hant", "繁體中文"),
    ("zh-TW", "中文（台灣）"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn langid(tag: &str) -> LanguageIdentifier {
        parse(tag).unwrap()
    }

    #[test]
    fn canonical_forms() {
        assert_eq!(canonical("pt-br").unwrap(), "pt-BR");
        assert_eq!(canonical("en_US").unwrap(), "en-US");
        assert_eq!(canonical(" fr ").unwrap(), "fr");
        assert_eq!(canonical("zh-hant-tw").unwrap(), "zh-Hant-TW");
        assert!(canonical("").is_err());
        assert!(canonical("und").is_err());
        assert!(canonical("not a tag").is_err());
    }

    #[test]
    fn parents_drop_variants_region_then_script() {
        assert_eq!(parents(&langid("zh-Hant-TW")), ["zh-Hant", "zh"]);
        assert_eq!(parents(&langid("fr-CA")), ["fr"]);
        assert!(parents(&langid("en")).is_empty());
        assert!(is_regional(&langid("pt-BR")));
        assert!(!is_regional(&langid("pt")));
    }

    #[test]
    fn direction() {
        assert!(is_rtl(&langid("ar")));
        assert!(is_rtl(&langid("he")));
        assert!(is_rtl(&langid("fa-IR")));
        assert!(!is_rtl(&langid("fr")));
    }

    #[test]
    fn native_names() {
        assert_eq!(native_name(&langid("fr")), "Français");
        assert_eq!(native_name(&langid("de")), "Deutsch");
        assert_eq!(native_name(&langid("pt-BR")), "Português (Brasil)");
        assert_eq!(native_name(&langid("fr-LU")), "Français (LU)");
        assert_eq!(native_name(&langid("tlh")), "tlh");
    }

    #[test]
    fn tables_are_sorted_and_unique() {
        assert!(LANGUAGE_NAMES.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(REGIONAL_NAMES.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for (tag, _) in REGIONAL_NAMES {
            assert_eq!(canonical(tag).unwrap(), *tag);
        }
    }
}
