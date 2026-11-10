//! [`HtmlPolicy`], the rich-text policy (section `validation.html`), and
//! `SafeHtml` (feature `html`), a string sanitized by it while
//! deserializing.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[cfg(feature = "html")]
pub(crate) mod clean;
#[cfg(feature = "html")]
pub(crate) mod tree;

/// Tags whose content goes with them, which a policy can never allow.
const CONTENT_TAGS: [&str; 2] = ["script", "style"];

/// Attributes that act on their own and that no list can vet, which a policy
/// can never allow: `srcdoc` holds a whole document for an `iframe`, and
/// `http-equiv` makes a `meta` redirect the page to its `content`. `on*`
/// handlers are the same.
const NEVER_ATTRIBUTES: [&str; 2] = ["srcdoc", "http-equiv"];

/// The most an input may be, unless the policy says otherwise (see
/// [`HtmlPolicy::max_input_bytes`]).
const DEFAULT_MAX_INPUT_BYTES: usize = 512 * 1024;

/// The most nodes a parse may hold (see [`HtmlPolicy::max_nodes`]).
const DEFAULT_MAX_NODES: usize = 128 * 1024;

/// How deep elements may nest (see [`HtmlPolicy::max_depth`]).
const DEFAULT_MAX_DEPTH: usize = 128;

/// The deepest a policy may allow: browsers read markup past 512 levels flat (so
/// the output would not be the tree that was checked), and a page may already be
/// some levels deep where it goes.
const MOST_DEPTH: usize = 256;

/// What rich text may keep (configuration section `validation.html`). The
/// sanitizer parses the input as an HTML fragment and writes it back with
/// only these tags, attributes and URL schemes:
///
/// - a tag that is not allowed is removed and its text kept (`<u>hi</u>`
///   becomes `hi`), except `script` and `style`, which lose their content;
/// - an attribute that is not allowed is removed, so every event handler
///   (`onclick`, `onerror`) and `style` go;
/// - a URL attribute (`href`, `src`, `action`, `formaction`, `data`,
///   `poster`, `cite` and the like, on any tag) with a scheme that is not
///   allowed (`javascript:`, `data:`) is removed; relative URLs stay, but one
///   with a host of its own (`//evil.test`) only when `http` and `https` are
///   both allowed, since it leaves with the page's scheme;
/// - an SVG or MathML element where the HTML parser would not put it, and
///   SVG animation elements (`animate`, `set`), are removed;
/// - comments are removed, and text is escaped (`<` becomes `&lt;`).
///
/// The output is parsed again before it is returned, and returned only when
/// that parse is the tree that was checked: a browser builds from it exactly
/// what the policy kept. Markup that cleaning leaves for the parser to
/// rearrange (a `<p>` that ends up inside a `<p>`, a blank line a `<pre>`
/// drops) and markup no output can hold (an allowed `plaintext`) leave only
/// their text.
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
/// `script` and `style` can't be allowed, nor `on*`, `srcdoc` or `http-equiv`,
/// and `rel` can't be allowed while `link_rel` is set; `ValidationModule`
/// refuses to boot with such a policy ([`check`](HtmlPolicy::check)).
///
/// An input over a limit (`max_input_bytes`, `max_nodes`, `max_depth`, or 256
/// attributes on one tag) is refused rather than parsed further:
/// [`try_clean`](HtmlPolicy::try_clean) says so, and `SafeHtml` in the
/// extractors is a 400 on the field.
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
    /// The longest input, in bytes, the parser is given (default 512 KiB, a
    /// long article pasted from an editor): longer is refused before any of
    /// it is parsed.
    pub max_input_bytes: usize,
    /// How much markup a parse may hold (default 131,072): each element,
    /// text run and comment is 1, each attribute 1, and every 32 bytes of
    /// attributes 1. The parser copies open formatting elements for every
    /// block it meets, so this bounds what a short input can multiply into;
    /// it also pays for the parser's scans of its own stacks (64 per node).
    pub max_nodes: usize,
    /// How deep elements may nest (default 128, at most 256; text inside the
    /// deepest element is not another level). Rich text stays far below it,
    /// and it keeps the parser's scans short. `check` refuses more than 256,
    /// and `clean` holds to 256 whatever the policy says.
    pub max_depth: usize,
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
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_nodes: DEFAULT_MAX_NODES,
            max_depth: DEFAULT_MAX_DEPTH,
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

    /// With these limits on what an input may take (see
    /// [`max_input_bytes`](HtmlPolicy::max_input_bytes),
    /// [`max_nodes`](HtmlPolicy::max_nodes) and
    /// [`max_depth`](HtmlPolicy::max_depth)).
    pub fn limits(mut self, max_input_bytes: usize, max_nodes: usize, max_depth: usize) -> Self {
        self.max_input_bytes = max_input_bytes;
        self.max_nodes = max_nodes;
        self.max_depth = max_depth;
        self
    }

    /// Whether the policy is safe to use: `Err` names the key at fault
    /// (`validation.html.tags: ...`) when it allows `script` or `style`, an
    /// `on*` handler, `srcdoc` or `http-equiv`, `rel` while
    /// [`link_rel`](HtmlPolicy::link_rel) is set, a limit of 0 (it would
    /// refuse every field), or a [`max_depth`](HtmlPolicy::max_depth) over
    /// 256.
    pub fn check(&self) -> Result<(), String> {
        for (key, limit) in [
            ("max_input_bytes", self.max_input_bytes),
            ("max_nodes", self.max_nodes),
            ("max_depth", self.max_depth),
        ] {
            if limit == 0 {
                return Err(format!(
                    "validation.html.{key}: 0 would refuse every field (use a limit of 1 or more)"
                ));
            }
        }
        if self.max_depth > MOST_DEPTH {
            return Err(format!(
                "validation.html.max_depth: {} is over {MOST_DEPTH} (browsers read deeper markup flat, so they would build another tree than the one checked)",
                self.max_depth
            ));
        }
        let never = |attribute: &String| {
            NEVER_ATTRIBUTES.contains(&attribute.as_str()) || attribute.starts_with("on")
        };
        if let Some(attribute) = self.generic_attributes.iter().find(|a| never(a)) {
            return Err(format!(
                "validation.html.generic_attributes: `{attribute}` can't be allowed (it acts on its own, and no list can vet it)"
            ));
        }
        for (tag, attributes) in &self.tag_attributes {
            if let Some(attribute) = attributes.iter().find(|a| never(a)) {
                return Err(format!(
                    "validation.html.tag_attributes: `{attribute}` on `{tag}` can't be allowed (it acts on its own, and no list can vet it)"
                ));
            }
        }
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
    /// [`check`](HtmlPolicy::check) refuses are ignored here: it never keeps
    /// more than 256 levels, whatever `max_depth` says.
    ///
    /// It can't fail, and it works on the thread that calls it: an input over
    /// one of the limits comes back whole as escaped text, tags and all, so
    /// nothing in it is parsed, kept or lost. [`try_clean`](HtmlPolicy::try_clean)
    /// says so instead. A long input takes a while (see `validation.html` in
    /// the docs): in async code, call it from `tokio::task::spawn_blocking`.
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

    /// `html` sanitized by this policy, or the limit it passes.
    ///
    /// ```
    /// use navree_validation::{HtmlError, HtmlPolicy};
    ///
    /// let policy = HtmlPolicy::default().limits(16, 1024, 2);
    /// assert_eq!(policy.try_clean("<b>short</b>").unwrap(), "<b>short</b>");
    /// assert_eq!(
    ///     policy.try_clean("<b>a little too long</b>"),
    ///     Err(HtmlError::InputTooLarge { max: 16 })
    /// );
    /// assert_eq!(
    ///     policy.try_clean("<ul><ul><ul>"),
    ///     Err(HtmlError::TooDeep { max: 2 })
    /// );
    /// ```
    ///
    /// # Errors
    ///
    /// [`HtmlError`]: the input is over one of the policy's limits.
    #[cfg(feature = "html")]
    pub fn try_clean(&self, html: &str) -> Result<String, HtmlError> {
        self.sanitizer().sanitize(html).map(|(out, _)| out)
    }

    #[cfg(feature = "html")]
    pub(crate) fn sanitizer(&self) -> clean::Sanitizer<'_> {
        clean::Sanitizer {
            tags: &self.tags,
            generic_attributes: &self.generic_attributes,
            tag_attributes: &self.tag_attributes,
            url_schemes: &self.url_schemes,
            link_rel: Some(self.link_rel.as_str()).filter(|rel| !rel.is_empty()),
            limits: tree::Limits {
                max_input_bytes: self.max_input_bytes,
                max_nodes: self.max_nodes,
                // Browsers read markup past 512 levels flat: never keep more
                // than `check` allows, whatever the policy says.
                max_depth: self.max_depth.min(MOST_DEPTH),
            },
        }
    }
}

/// Why [`HtmlPolicy::try_clean`] refused an input: it passes one of the
/// policy's limits. The client's fault; in the extractors, a 400 on the field.
#[cfg(feature = "html")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HtmlError {
    /// Longer than [`max_input_bytes`](HtmlPolicy::max_input_bytes).
    #[error("the HTML is longer than {max} bytes")]
    InputTooLarge {
        /// The limit.
        max: usize,
    },
    /// More markup than [`max_nodes`](HtmlPolicy::max_nodes) pays for, or
    /// more work for the parser than that much markup takes.
    #[error("the HTML has more markup than {max} nodes")]
    TooManyNodes {
        /// The limit.
        max: usize,
    },
    /// Elements nested deeper than [`max_depth`](HtmlPolicy::max_depth).
    #[error("the HTML nests deeper than {max} levels")]
    TooDeep {
        /// The limit.
        max: usize,
    },
    /// A tag with more attributes than any tag needs.
    #[error("a tag in the HTML has more than {max} attributes")]
    TooManyAttributes {
        /// The limit.
        max: usize,
    },
}

#[cfg(feature = "html")]
impl HtmlError {
    /// The rule of the message table, and the limit it names.
    fn rule(self) -> (&'static str, usize) {
        match self {
            HtmlError::InputTooLarge { max } => ("html.input", max),
            HtmlError::TooManyNodes { max } => ("html.nodes", max),
            HtmlError::TooDeep { max } => ("html.depth", max),
            HtmlError::TooManyAttributes { max } => ("html.attributes", max),
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

    use super::{HtmlError, HtmlPolicy};
    use crate::context;
    use crate::failure::{serde_error, serde_error_with};
    use crate::rules::{Length, Measure, RuleInput, Unit};

    /// Rich text from the client, sanitized by the app's [`HtmlPolicy`]
    /// while it is deserialized: a handler holding a `SafeHtml` never holds
    /// a script, an event handler or a `javascript:` link, so it can store
    /// and render it as HTML.
    ///
    /// - In the extractors the policy is the app's (`validation.html`, see
    ///   [`InputContext`](crate::InputContext)); elsewhere the default
    ///   policy, unless the code runs in an `InputContext::scope`.
    /// - Anything unsafe is removed, so it fails on a string only when the
    ///   string is over one of the policy's limits ([`HtmlError`], a 400 on
    ///   the field). Rules run on the sanitized HTML, so
    ///   `length(chars, max = 10000)` measures what is stored, and a body over
    ///   the limit never gets as far as the parser.
    /// - Sanitizing a long field takes a while (tens of milliseconds for a
    ///   long article, see `validation.html` in the docs). The extractors read
    ///   a body of 16 KiB or more on Tokio's blocking pool, so other requests
    ///   go on; code that deserializes a `SafeHtml` itself does it on its own
    ///   thread, and in async code belongs in `tokio::task::spawn_blocking`.
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

        /// `html` sanitized by `policy`. Over one of its limits the input
        /// comes back as escaped text (see [`HtmlPolicy::clean`]).
        pub fn with_policy(html: &str, policy: &HtmlPolicy) -> Self {
            SafeHtml(policy.clean(html))
        }

        /// `html` sanitized by `policy`, or the limit it passes.
        ///
        /// # Errors
        ///
        /// [`HtmlError`]: the input is over one of the policy's limits.
        pub fn try_with_policy(html: &str, policy: &HtmlPolicy) -> Result<Self, HtmlError> {
            policy.try_clean(html).map(SafeHtml)
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
                        Some(context) => {
                            SafeHtml::try_with_policy(html, context.html()).map_err(|error| {
                                let (rule, max) = error.rule();
                                serde_error_with(rule, &[("max", &max)], error)
                            })
                        }
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

    /// Formatting elements reopened over and over until a limit is passed:
    /// what the parser makes after that is never confused with what it made
    /// before, so the input is refused and nothing underneath trips.
    #[cfg(feature = "html")]
    #[test]
    fn a_limit_passed_inside_formatting_elements_refuses_the_input() {
        let html = "\x00\x00\x00<b>>3<(>0<b/3<>a>3<(/b>\x15\x00\x00\x00\x00\x00\x00\x00Flspa\x06\x0c\x16\x15n<a>0(th>/\x0fW\x0e\x05\x07\x08\x02\x00\x0e6666666666666666\x00\x5c66666666666<b\x09><a><p></<ba><b><b>>>3<(>0<b/3<>a>3<(/b>\x15\x00\x00\x00\x00\x00\x00\x00Flspa\x06\x0c\x16\x15n<a>0(th>/\x0fW\x0e\x05\x07\x08\x02\x00\x0e6666666666666666\x00\x5c66666666666<b\x09><a><<a><p></a>";
        for policy in [HtmlPolicy::text_only(), HtmlPolicy::default()] {
            let policy = policy.limits(1024, 64, 8);
            assert!(policy.try_clean(html).is_err());
            let clean = policy.clean(html);
            assert!(!clean.contains('<'), "{clean:?}");
        }
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
