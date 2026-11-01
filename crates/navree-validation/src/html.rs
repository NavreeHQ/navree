//! [`HtmlPolicy`], the rich-text policy (section `validation.html`), and
//! `SafeHtml` (feature `html`), a string sanitized by it while
//! deserializing.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[cfg(feature = "html")]
mod clean;
#[cfg(feature = "html")]
mod tree;

/// Tags whose content goes with them, which a policy can never allow.
const CONTENT_TAGS: [&str; 2] = ["script", "style"];

/// What rich text may keep (configuration section `validation.html`). The
/// sanitizer parses the input as an HTML fragment and writes it back with
/// only these tags, attributes and URL schemes:
///
/// - a tag that is not allowed is removed and its text kept (`<u>hi</u>`
///   becomes `hi`), except `script` and `style`, which lose their content;
/// - an attribute that is not allowed is removed, so every event handler
///   (`onclick`, `onerror`) and `style` go;
/// - a URL attribute (`href`, `src`) with a scheme that is not allowed
///   (`javascript:`, `data:`) is removed; relative URLs stay;
/// - comments are removed, and text is escaped (`<` becomes `&lt;`).
///
/// The default keeps what a rich-text editor produces: paragraphs,
/// headings, emphasis, lists, quotes, code, tables and links (`href`,
/// `title`), with `dir` and `lang` on every tag (right-to-left text keeps
/// its direction), `http`, `https`, `mailto` and `tel` links, and
/// `rel="noopener noreferrer nofollow"` on every link. Images are not in
/// the default; allow them with `allow_tags(["img"])` and
/// `allow_attributes("img", ["src", "alt"])`.
///
/// ```toml
/// [validation.html]
/// tags = ["p", "br", "strong", "em", "a", "ul", "ol", "li", "img"]
/// generic_attributes = ["dir", "lang"]
/// url_schemes = ["https", "mailto"]
/// link_rel = "noopener noreferrer"
///
/// [validation.html.tag_attributes]
/// a = ["href", "title"]
/// img = ["src", "alt"]
/// ```
///
/// `script` and `style` can't be allowed, and `rel` can't be allowed while
/// `link_rel` is set; `ValidationModule` refuses to boot with such a policy
/// ([`check`](HtmlPolicy::check)).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HtmlPolicy {
    /// The tags kept.
    pub tags: BTreeSet<String>,
    /// Attributes allowed on every kept tag.
    pub generic_attributes: BTreeSet<String>,
    /// Attributes allowed on one tag (`a = ["href", "title"]`).
    pub tag_attributes: BTreeMap<String, BTreeSet<String>>,
    /// The URL schemes `href` and `src` may use.
    pub url_schemes: BTreeSet<String>,
    /// The `rel` set on every link; empty leaves `rel` out.
    pub link_rel: String,
}

fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
    items.into_iter().map(str::to_owned).collect()
}

impl Default for HtmlPolicy {
    fn default() -> Self {
        Self {
            tags: set([
                "a",
                "b",
                "blockquote",
                "br",
                "code",
                "del",
                "em",
                "h1",
                "h2",
                "h3",
                "h4",
                "h5",
                "h6",
                "hr",
                "i",
                "li",
                "ol",
                "p",
                "pre",
                "s",
                "span",
                "strong",
                "sub",
                "sup",
                "table",
                "tbody",
                "td",
                "th",
                "thead",
                "tr",
                "u",
                "ul",
            ]),
            generic_attributes: set(["dir", "lang"]),
            tag_attributes: BTreeMap::from([
                ("a".to_owned(), set(["href", "title"])),
                ("ol".to_owned(), set(["start"])),
                ("td".to_owned(), set(["colspan", "rowspan"])),
                ("th".to_owned(), set(["colspan", "rowspan", "scope"])),
            ]),
            url_schemes: set(["http", "https", "mailto", "tel"]),
            link_rel: "noopener noreferrer nofollow".to_owned(),
        }
    }
}

impl HtmlPolicy {
    /// No tags at all: the text is kept, escaped, and every tag removed.
    pub fn text_only() -> Self {
        Self {
            tags: BTreeSet::new(),
            generic_attributes: BTreeSet::new(),
            tag_attributes: BTreeMap::new(),
            ..Self::default()
        }
    }

    /// With `tags` kept as well.
    pub fn allow_tags<const N: usize>(mut self, tags: [&str; N]) -> Self {
        self.tags.extend(set(tags));
        self
    }

    /// With `attributes` allowed on `tag` as well.
    pub fn allow_attributes<const N: usize>(mut self, tag: &str, attributes: [&str; N]) -> Self {
        self.tag_attributes
            .entry(tag.to_owned())
            .or_default()
            .extend(set(attributes));
        self
    }

    /// With exactly `schemes` allowed in URLs.
    pub fn url_schemes<const N: usize>(mut self, schemes: [&str; N]) -> Self {
        self.url_schemes = set(schemes);
        self
    }

    /// With `rel` set on every link (empty for none).
    pub fn link_rel(mut self, rel: &str) -> Self {
        rel.clone_into(&mut self.link_rel);
        self
    }

    /// Whether the policy is safe to use: `Err` names the key at fault
    /// (`validation.html.tags: ...`) when it allows `script` or `style`, or
    /// `rel` while [`link_rel`](HtmlPolicy::link_rel) is set.
    pub fn check(&self) -> Result<(), String> {
        if let Some(tag) = CONTENT_TAGS.iter().find(|tag| self.tags.contains(**tag)) {
            return Err(format!(
                "validation.html.tags: `{tag}` can't be allowed (scripts and styles are always removed)"
            ));
        }
        if let Some(tag) = CONTENT_TAGS
            .iter()
            .find(|tag| self.tag_attributes.contains_key(**tag))
        {
            return Err(format!(
                "validation.html.tag_attributes: `{tag}` can't be allowed (scripts and styles are always removed)"
            ));
        }
        if !self.link_rel.is_empty() {
            if self.generic_attributes.contains("rel") {
                return Err("validation.html.generic_attributes: `rel` can't be allowed while `link_rel` is set".to_owned());
            }
            if let Some(tag) = self
                .tag_attributes
                .iter()
                .find_map(|(tag, attributes)| attributes.contains("rel").then_some(tag))
            {
                return Err(format!(
                    "validation.html.tag_attributes: `rel` on `{tag}` can't be allowed while `link_rel` is set"
                ));
            }
        }
        Ok(())
    }

    /// `html` sanitized by this policy. Parts of the policy that
    /// [`check`](HtmlPolicy::check) refuses are ignored here.
    ///
    /// ```
    /// use navree_validation::HtmlPolicy;
    ///
    /// let policy = HtmlPolicy::default();
    /// assert_eq!(
    ///     policy.clean(r#"<p onclick="steal()">Hi <script>alert(1)</script><b>there</b></p>"#),
    ///     "<p>Hi <b>there</b></p>"
    /// );
    /// assert_eq!(HtmlPolicy::text_only().clean("<b>1 < 2</b>"), "1 &lt; 2");
    /// ```
    #[cfg(feature = "html")]
    pub fn clean(&self, html: &str) -> String {
        self.sanitizer().clean(html)
    }

    #[cfg(feature = "html")]
    fn sanitizer(&self) -> clean::Sanitizer<'_> {
        clean::Sanitizer {
            tags: &self.tags,
            generic_attributes: &self.generic_attributes,
            tag_attributes: &self.tag_attributes,
            url_schemes: &self.url_schemes,
            link_rel: Some(self.link_rel.as_str()).filter(|rel| !rel.is_empty()),
        }
    }
}

#[cfg(feature = "html")]
pub use safe::SafeHtml;

#[cfg(feature = "html")]
mod safe {
    use std::fmt;
    use std::ops::Deref;

    use serde::de::{self, Deserializer, Visitor};
    use serde::{Deserialize, Serialize, Serializer};

    use super::HtmlPolicy;
    use crate::context;
    use crate::failure::serde_error;
    use crate::rules::{Length, Measure, RuleInput, Unit};

    /// Rich text from the client, sanitized by the app's [`HtmlPolicy`]
    /// while it is deserialized: a handler holding a `SafeHtml` never holds
    /// a script, an event handler or a `javascript:` link, so it can store
    /// and render it as HTML.
    ///
    /// - In the extractors the policy is the app's (`validation.html`, see
    ///   [`InputContext`](crate::InputContext)); elsewhere the default
    ///   policy, unless the code runs in an `InputContext::scope`.
    /// - It never fails on a string: anything unsafe is removed. Rules run
    ///   on the sanitized HTML, so `length(chars, max = 10000)` measures
    ///   what is stored.
    /// - It serializes as the sanitized string; [`as_str`](SafeHtml::as_str)
    ///   and [`into_inner`](SafeHtml::into_inner) give it back.
    ///
    /// ```
    /// use navree_validation::Validate;
    /// use navree_validation::SafeHtml;
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize, Validate)]
    /// struct NewComment {
    ///     #[validate(length(min = 1, max = 10_000))]
    ///     body: SafeHtml,
    /// }
    ///
    /// let input: NewComment = serde_json::from_str(
    ///     r#"{"body": "<p>Nice <img src=x onerror=alert(1)>post</p>"}"#,
    /// )
    /// .unwrap();
    /// assert_eq!(input.body.as_str(), "<p>Nice post</p>");
    /// ```
    #[derive(Clone, PartialEq, Eq, Hash, Default)]
    pub struct SafeHtml(String);

    impl SafeHtml {
        /// `html` sanitized by the default [`HtmlPolicy`].
        pub fn new(html: &str) -> Self {
            Self::with_policy(html, &HtmlPolicy::default())
        }

        /// `html` sanitized by `policy`.
        pub fn with_policy(html: &str, policy: &HtmlPolicy) -> Self {
            SafeHtml(policy.clean(html))
        }

        /// The sanitized HTML.
        pub fn as_str(&self) -> &str {
            &self.0
        }

        /// The sanitized HTML, owned.
        pub fn into_inner(self) -> String {
            self.0
        }
    }

    impl Deref for SafeHtml {
        type Target = str;

        fn deref(&self) -> &str {
            &self.0
        }
    }

    impl AsRef<str> for SafeHtml {
        fn as_ref(&self) -> &str {
            &self.0
        }
    }

    impl fmt::Display for SafeHtml {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl fmt::Debug for SafeHtml {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_tuple("SafeHtml").field(&self.0).finish()
        }
    }

    impl From<SafeHtml> for String {
        fn from(html: SafeHtml) -> Self {
            html.0
        }
    }

    impl Serialize for SafeHtml {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_str(&self.0)
        }
    }

    impl<'de> Deserialize<'de> for SafeHtml {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct HtmlVisitor;

            impl Visitor<'_> for HtmlVisitor {
                type Value = SafeHtml;

                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("a string")
                }

                fn visit_str<E: de::Error>(self, html: &str) -> Result<SafeHtml, E> {
                    match context::current() {
                        Some(context) => Ok(SafeHtml::with_policy(html, context.html())),
                        // The extractor resolves the app's policy and runs again.
                        None => Err(serde_error("html", "the HTML policy is not resolved yet")),
                    }
                }
            }

            deserializer.deserialize_str(HtmlVisitor)
        }
    }

    impl RuleInput for SafeHtml {
        fn rule_str(&self) -> Option<&str> {
            Some(&self.0)
        }
    }

    impl Length for SafeHtml {
        fn measure(&self, unit: Unit) -> Option<Measure> {
            self.0.measure(unit)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_policy_is_safe() {
        assert_eq!(HtmlPolicy::default().check(), Ok(()));
        assert_eq!(HtmlPolicy::text_only().check(), Ok(()));
    }

    #[test]
    fn check_names_the_key_at_fault() {
        let cases = [
            (
                HtmlPolicy::default().allow_tags(["style"]),
                "validation.html.tags: `style` can't be allowed",
            ),
            (
                HtmlPolicy::default().allow_attributes("script", ["src"]),
                "validation.html.tag_attributes: `script` can't be allowed",
            ),
            (
                HtmlPolicy::default().allow_attributes("a", ["rel"]),
                "validation.html.tag_attributes: `rel` on `a`",
            ),
            (
                HtmlPolicy {
                    generic_attributes: set(["rel"]),
                    ..HtmlPolicy::default()
                },
                "validation.html.generic_attributes: `rel`",
            ),
        ];
        for (policy, message) in cases {
            let error = policy.check().unwrap_err();
            assert!(error.starts_with(message), "{error}");
        }
        // Without `link_rel`, links may carry their own `rel`.
        let policy = HtmlPolicy::default()
            .link_rel("")
            .allow_attributes("a", ["rel"]);
        assert_eq!(policy.check(), Ok(()));
    }

    #[test]
    fn options_deserialize_with_defaults() {
        let policy: HtmlPolicy = serde_json::from_value(serde_json::json!({
            "tags": ["p"],
            "tag_attributes": { "img": ["src"] },
        }))
        .unwrap();
        assert_eq!(policy.tags, set(["p"]));
        assert_eq!(policy.tag_attributes["img"], set(["src"]));
        assert_eq!(policy.url_schemes, HtmlPolicy::default().url_schemes);
        assert_eq!(policy.link_rel, "noopener noreferrer nofollow");
    }

    #[cfg(feature = "html")]
    #[test]
    fn clean_never_panics_on_a_refused_policy() {
        let policy = HtmlPolicy {
            generic_attributes: set(["rel", "lang"]),
            ..HtmlPolicy::default()
                .allow_tags(["script", "style"])
                .allow_attributes("a", ["rel"])
                .allow_attributes("style", ["media"])
        };
        assert!(policy.check().is_err());
        assert_eq!(
            policy.clean(
                r#"<a href="https://x.example" rel="opener" lang="en">x</a><script>a()</script><style>b</style>"#
            ),
            r#"<a href="https://x.example" lang="en" rel="noopener noreferrer nofollow">x</a>"#
        );
    }

    #[cfg(feature = "html")]
    #[test]
    fn safe_html_sanitizes_with_the_scope_policy() {
        use crate::{InputContext, SafeHtml};

        let input = r#""<p>a <img src=\"https://x.example/a.png\"></p>""#;
        let default: SafeHtml = serde_json::from_str(input).unwrap();
        assert_eq!(default.as_str(), "<p>a </p>");

        let images = InputContext::new().with_html(
            HtmlPolicy::default()
                .allow_tags(["img"])
                .allow_attributes("img", ["src"]),
        );
        let html: SafeHtml = images.scope(|| serde_json::from_str(input)).unwrap();
        assert_eq!(
            html.as_str(),
            r#"<p>a <img src="https://x.example/a.png"></p>"#
        );
        assert_eq!(serde_json::to_value(&html).unwrap(), html.as_str());
        assert_eq!(SafeHtml::new("<b onclick=x>b</b>").into_inner(), "<b>b</b>");
    }
}
