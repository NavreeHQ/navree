//! Names given on the command line: validation and the case conversions the
//! generators need.

use heck::{ToKebabCase, ToSnakeCase, ToUpperCamelCase};

/// A rejected name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    /// Not `snake_case`.
    #[error(
        "invalid {what} name `{given}`: use snake_case (lower-case letters, digits and single underscores, starting with a letter), e.g. `{suggestion}`"
    )]
    NotSnakeCase {
        /// What was being named (`module`, `job`, ...).
        what: String,
        /// The name as given.
        given: String,
        /// A snake_case spelling of it.
        suggestion: String,
    },
    /// A Rust keyword or a name the platform reserves.
    #[error("invalid {what} name `{given}`: {reason}; pick another name")]
    Reserved {
        /// What was being named.
        what: String,
        /// The name as given.
        given: String,
        /// Why it cannot be used.
        reason: &'static str,
    },
    /// Not a usable package name.
    #[error(
        "invalid app name `{given}`: use lower-case letters and digits separated by single `-` or `_`, starting with a letter, e.g. `{suggestion}`"
    )]
    BadAppName {
        /// The name as given.
        given: String,
        /// A usable spelling of it.
        suggestion: String,
    },
}

/// Rust's strict and reserved keywords (edition 2024): none of them can name
/// a module or a crate.
const KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// File names Windows reserves (a directory or file with one of these names
/// cannot be created).
const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Crate names an app cannot take: Rust's built-in crates, and `navree`, which
/// the app depends on.
const RESERVED_CRATES: &[&str] = &["alloc", "core", "navree", "proc_macro", "std", "test"];

/// Why `snake` cannot be used as a Rust identifier or file name, if it cannot.
fn reserved(snake: &str) -> Option<&'static str> {
    if KEYWORDS.contains(&snake) {
        Some("it is a Rust keyword")
    } else if WINDOWS_RESERVED.contains(&snake) {
        Some("Windows reserves it as a file name")
    } else {
        None
    }
}

/// A validated `snake_case` name (`blog_posts`), usable as a Rust module
/// name and a file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    snake: String,
}

impl Name {
    /// Validates `given` as the name of a `what` (used in the error).
    pub fn parse(what: &str, given: &str) -> Result<Self, NameError> {
        if !is_snake_case(given) {
            return Err(NameError::NotSnakeCase {
                what: what.to_owned(),
                given: given.to_owned(),
                suggestion: suggest(given, '_'),
            });
        }
        if let Some(reason) = reserved(given) {
            return Err(NameError::Reserved {
                what: what.to_owned(),
                given: given.to_owned(),
                reason,
            });
        }
        Ok(Self {
            snake: given.to_owned(),
        })
    }

    /// `blog_posts`.
    pub fn snake(&self) -> &str {
        &self.snake
    }

    /// `BlogPosts`.
    pub fn pascal(&self) -> String {
        self.snake.to_upper_camel_case()
    }

    /// `blog-posts`.
    pub fn kebab(&self) -> String {
        self.snake.to_kebab_case()
    }

    /// `Blog posts`.
    pub fn title(&self) -> String {
        sentence(&self.snake.replace('_', " "))
    }

    /// The singular of the last word: `blog_posts` → `blog_post`.
    pub fn singular(&self) -> Name {
        Name {
            snake: map_last_word(&self.snake, singular_word),
        }
    }

    /// The plural of the last word: `blog_post` → `blog_posts` (a plural
    /// stays as it is).
    pub fn plural(&self) -> Name {
        Name {
            snake: map_last_word(&self.snake, plural_word),
        }
    }
}

/// A validated app (package) name: `blog`, `my-app` or `my_app`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppName {
    package: String,
}

impl AppName {
    /// Validates `given` as an app name.
    pub fn parse(given: &str) -> Result<Self, NameError> {
        let normalised = given.replace('-', "_");
        if !is_snake_case(&normalised) {
            return Err(NameError::BadAppName {
                given: given.to_owned(),
                suggestion: suggest(given, '-'),
            });
        }
        let reason = reserved(&normalised).or_else(|| {
            RESERVED_CRATES
                .contains(&normalised.as_str())
                .then_some("the name is taken by a crate every app depends on")
        });
        if let Some(reason) = reason {
            return Err(NameError::Reserved {
                what: "app".to_owned(),
                given: given.to_owned(),
                reason,
            });
        }
        Ok(Self {
            package: given.to_owned(),
        })
    }

    /// The package name, as given: `my-app`.
    pub fn package(&self) -> &str {
        &self.package
    }

    /// The library crate name: `my_app`.
    pub fn crate_name(&self) -> String {
        self.package.replace('-', "_")
    }

    /// `My app`.
    pub fn title(&self) -> String {
        sentence(&self.package.replace(['-', '_'], " "))
    }
}

/// `[a-z][a-z0-9]*(_[a-z0-9]+)*`.
fn is_snake_case(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.split('_').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// A usable spelling of `given` with words joined by `separator`, or
/// `my_name` when nothing usable is left.
fn suggest(given: &str, separator: char) -> String {
    let snake: String = given
        .to_snake_case()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .collect();
    let snake = snake.trim_start_matches(|c: char| c.is_ascii_digit() || c == '_');
    let parts: Vec<&str> = snake.split('_').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        format!("my{separator}name")
    } else {
        parts.join(&separator.to_string())
    }
}

/// Upper-cases the first letter.
fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Applies `f` to the last `_`-separated word of `snake`.
fn map_last_word(snake: &str, f: fn(&str) -> String) -> String {
    match snake.rsplit_once('_') {
        Some((head, last)) => format!("{head}_{}", f(last)),
        None => f(snake),
    }
}

/// Irregular (singular, plural) pairs, and pairs the suffix rules get wrong.
const IRREGULAR: &[(&str, &str)] = &[
    ("person", "people"),
    ("child", "children"),
    ("man", "men"),
    ("woman", "women"),
    ("mouse", "mice"),
    ("goose", "geese"),
    ("tooth", "teeth"),
    ("foot", "feet"),
    ("status", "statuses"),
    ("bus", "buses"),
    ("campus", "campuses"),
    ("virus", "viruses"),
    ("cache", "caches"),
    ("niche", "niches"),
    ("movie", "movies"),
    ("leaf", "leaves"),
    ("life", "lives"),
    ("knife", "knives"),
    ("wife", "wives"),
    ("half", "halves"),
    ("analysis", "analyses"),
    ("crisis", "crises"),
    ("criterion", "criteria"),
];

/// Words with the same singular and plural.
const UNCOUNTABLE: &[&str] = &[
    "data",
    "equipment",
    "feedback",
    "fish",
    "information",
    "media",
    "metadata",
    "money",
    "news",
    "series",
    "sheep",
    "species",
    "staff",
];

/// The singular of one word (English rules, best effort).
fn singular_word(word: &str) -> String {
    if UNCOUNTABLE.contains(&word) {
        return word.to_owned();
    }
    if let Some((singular, _)) = IRREGULAR.iter().find(|(s, p)| *p == word || *s == word) {
        return (*singular).to_owned();
    }
    let strip = |n: usize| word[..word.len() - n].to_owned();
    if word.len() > 3 && word.ends_with("ies") {
        format!("{}y", strip(3))
    } else if word.ends_with("sses")
        || word.ends_with("xes")
        || word.ends_with("zzes")
        || word.ends_with("ches")
        || word.ends_with("shes")
    {
        strip(2)
    } else if word.ends_with("ss") || word.ends_with("us") || word.ends_with("is") {
        word.to_owned()
    } else if word.len() > 1 && word.ends_with('s') {
        strip(1)
    } else {
        word.to_owned()
    }
}

/// The plural of one word (English rules, best effort); a plural stays.
fn plural_word(word: &str) -> String {
    if UNCOUNTABLE.contains(&word) {
        return word.to_owned();
    }
    if let Some((_, plural)) = IRREGULAR.iter().find(|(s, p)| *s == word || *p == word) {
        return (*plural).to_owned();
    }
    if singular_word(word) != word {
        return word.to_owned();
    }
    let consonant_y = word.len() > 1
        && word.ends_with('y')
        && !matches!(
            word.as_bytes()[word.len() - 2],
            b'a' | b'e' | b'i' | b'o' | b'u'
        );
    if consonant_y {
        format!("{}ies", &word[..word.len() - 1])
    } else if word.ends_with('s')
        || word.ends_with('x')
        || word.ends_with('z')
        || word.ends_with("ch")
        || word.ends_with("sh")
    {
        format!("{word}es")
    } else {
        format!("{word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_snake_case() {
        for ok in ["posts", "blog_posts", "post2", "v2_api"] {
            assert_eq!(Name::parse("module", ok).unwrap().snake(), ok);
        }
    }

    #[test]
    fn rejects_names_that_are_not_snake_case() {
        let error = Name::parse("module", "BlogPosts").unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid module name `BlogPosts`: use snake_case (lower-case letters, digits and single underscores, starting with a letter), e.g. `blog_posts`"
        );
        for bad in [
            "",
            "_posts",
            "posts_",
            "blog__posts",
            "2posts",
            "blog-posts",
            "pósts",
        ] {
            assert!(
                Name::parse("module", bad).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_rust_keywords_and_reserved_names() {
        for bad in [
            "mod", "type", "match", "self", "crate", "super", "async", "gen", "try",
        ] {
            let error = Name::parse("module", bad).unwrap_err();
            assert!(
                error.to_string().contains("is a Rust keyword"),
                "{bad}: {error}"
            );
        }
        let error = Name::parse("module", "con").unwrap_err();
        assert!(error.to_string().contains("Windows reserves it"), "{error}");
    }

    #[test]
    fn case_conversions() {
        let name = Name::parse("module", "blog_posts").unwrap();
        assert_eq!(name.pascal(), "BlogPosts");
        assert_eq!(name.kebab(), "blog-posts");
        assert_eq!(name.title(), "Blog posts");
    }

    #[test]
    fn singular_and_plural() {
        let cases = [
            ("posts", "post"),
            ("blog_posts", "blog_post"),
            ("companies", "company"),
            ("addresses", "address"),
            ("foxes", "fox"),
            ("batches", "batch"),
            ("wishes", "wish"),
            ("statuses", "status"),
            ("status", "status"),
            ("news", "news"),
            ("people", "person"),
            ("inventory", "inventory"),
        ];
        for (plural, singular) in cases {
            let name = Name::parse("module", plural).unwrap();
            assert_eq!(name.singular().snake(), singular, "singular of {plural}");
        }
        let plurals = [
            ("post", "posts"),
            ("company", "companies"),
            ("day", "days"),
            ("address", "addresses"),
            ("fox", "foxes"),
            ("batch", "batches"),
            ("person", "people"),
            ("posts", "posts"),
        ];
        for (singular, plural) in plurals {
            let name = Name::parse("entity", singular).unwrap();
            assert_eq!(name.plural().snake(), plural, "plural of {singular}");
        }
    }

    #[test]
    fn app_names_may_use_hyphens() {
        let app = AppName::parse("my-app").unwrap();
        assert_eq!(app.package(), "my-app");
        assert_eq!(app.crate_name(), "my_app");
        assert_eq!(app.title(), "My app");
        assert_eq!(AppName::parse("blog").unwrap().crate_name(), "blog");
        for bad in [
            "My-App", "my--app", "my-_app", "-app", "app-", "1app", "my app", "", "navree", "test",
            "self",
        ] {
            assert!(AppName::parse(bad).is_err(), "{bad:?} must be rejected");
        }
    }
}
