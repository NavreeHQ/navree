//! Structured data: schema.org nodes in a JSON-LD graph, written so that a script block
//! can't be closed from inside it.

use std::borrow::Cow;
use std::fmt::Display;

use serde_json::{Map, Value, json};

use super::is_absolute_https;

/// A graph of schema.org nodes: one `<script type="application/ld+json">` block.
///
/// ```
/// use navree_ui::seo::{BreadcrumbList, JsonLd, Organization};
///
/// let data = JsonLd::new()
///     .add(Organization::new("Kesar", "https://kesar.example/").id("https://kesar.example/#organization"))
///     .add(BreadcrumbList::new().step("Home", "https://kesar.example/"));
/// let text = data.to_script();
/// assert!(text.starts_with(r#"{"@context":"https://schema.org","@graph":["#));
/// ```
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct JsonLd {
    nodes: Vec<Thing>,
}

impl JsonLd {
    /// An empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node.
    pub fn add(mut self, node: impl Into<Thing>) -> Self {
        self.nodes.push(node.into());
        self
    }

    /// The nodes, in order.
    pub fn nodes(&self) -> &[Thing] {
        &self.nodes
    }

    /// The graph as JSON.
    pub fn to_value(&self) -> Value {
        json!({
            "@context": "https://schema.org",
            "@graph": self.nodes.iter().map(Thing::to_value).collect::<Vec<_>>(),
        })
    }

    /// The graph as the text of a script block: see [`script_json`].
    pub fn to_script(&self) -> String {
        script_json(&self.to_value())
    }
}

/// `value` as JSON that is safe inside `<script>` and `</script>`: `<`, `>` and `&` are
/// written `<`, `>` and `&`, so no `</script>` or `<!--` can appear in it,
/// and U+2028 and U+2029 are escaped too. Each can only stand inside a JSON string, where
/// the escape reads back as the same character.
///
/// ```
/// use navree_ui::seo::script_json;
///
/// let text = script_json(&serde_json::json!({ "name": "</script><script>alert(1)</script>" }));
/// assert!(!text.contains('<'));
/// let back: serde_json::Value = serde_json::from_str(&text).unwrap();
/// assert_eq!(back["name"], "</script><script>alert(1)</script>");
/// ```
pub fn script_json(value: &Value) -> String {
    let text = value.to_string();
    let mut out = String::with_capacity(text.len() + 16);
    for ch in text.chars() {
        match ch {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }
    out
}

/// One schema.org node of any type: its `@type`, its `@id` and its properties. The typed
/// builders ([`Organization`], [`WebPage`], ...) become one; build a `Thing` yourself for
/// a type they don't cover.
///
/// ```
/// use navree_ui::seo::Thing;
///
/// let event = Thing::new("Event")
///     .prop("name", "Kesar Run, Amritsar")
///     .prop("startDate", "2026-12-06T06:30:00+05:30");
/// assert_eq!(event.kind(), "Event");
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct Thing {
    kind: Cow<'static, str>,
    props: Map<String, Value>,
}

impl Thing {
    /// A node of the schema.org type `kind`.
    pub fn new(kind: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind: kind.into(),
            props: Map::new(),
        }
    }

    /// Its schema.org type.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Names the node, so others can point at it: an absolute URL, usually the page's or
    /// the site's address and a fragment (`https://kesar.example/#organization`).
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.props
            .insert("@id".to_owned(), Value::String(id.into()));
        self
    }

    /// Sets a property. A later value for the same name replaces the earlier one.
    pub fn prop(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.props.insert(name.to_owned(), value.into());
        self
    }

    /// Points the property `name` at the node whose `@id` is `id`.
    pub fn link(self, name: &str, id: impl Into<String>) -> Self {
        let id: String = id.into();
        self.prop(name, json!({ "@id": id }))
    }

    /// Sets the property `name` to a node of its own.
    pub fn node(self, name: &str, node: impl Into<Thing>) -> Self {
        let node: Thing = node.into();
        self.prop(name, node.to_value())
    }

    /// Adds `value` to the list in property `name`.
    pub fn push(mut self, name: &str, value: impl Into<Value>) -> Self {
        let value = value.into();
        match self.props.get_mut(name) {
            Some(Value::Array(list)) => list.push(value),
            Some(one) => {
                let first = one.take();
                *one = Value::Array(vec![first, value]);
            }
            None => {
                self.props
                    .insert(name.to_owned(), Value::Array(vec![value]));
            }
        }
        self
    }

    /// The value of property `name`.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.props.get(name)
    }

    /// The node as JSON.
    pub fn to_value(&self) -> Value {
        let mut map = self.props.clone();
        map.insert("@type".to_owned(), Value::String(self.kind.to_string()));
        Value::Object(map)
    }
}

/// Gives a typed builder the shared methods: `id`, `prop` and `link`, and turns it into a
/// [`Thing`].
macro_rules! typed {
    ($($type:ident),+ $(,)?) => {
        $(impl $type {
            /// Names the node, so others can point at it: see [`Thing::id`].
            pub fn id(mut self, id: impl Into<String>) -> Self {
                self.0 = self.0.id(id);
                self
            }

            /// Sets any other schema.org property of the type.
            pub fn prop(mut self, name: &str, value: impl Into<Value>) -> Self {
                self.0 = self.0.prop(name, value);
                self
            }

            /// Points the property `name` at the node whose `@id` is `id`.
            pub fn link(mut self, name: &str, id: impl Into<String>) -> Self {
                self.0 = self.0.link(name, id);
                self
            }
        }

        impl From<$type> for Thing {
            fn from(node: $type) -> Self {
                node.0
            }
        })+
    };
}

/// The company or project behind a site.
#[derive(Clone, Debug)]
#[must_use]
pub struct Organization(Thing);

impl Organization {
    /// `name`, whose site is at `url`.
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self(
            Thing::new("Organization")
                .prop("name", name.into())
                .prop("url", url.into()),
        )
    }

    /// Its logo: an absolute URL of a square picture.
    pub fn logo(mut self, url: impl Into<String>) -> Self {
        self.0 = self.0.prop("logo", url.into());
        self
    }

    /// One of its profiles elsewhere: a social account, a code host. Call it again for more.
    pub fn same_as(mut self, url: impl Into<String>) -> Self {
        self.0 = self.0.push("sameAs", url.into());
        self
    }

    /// Its line, in a sentence.
    pub fn slogan(mut self, slogan: impl Into<String>) -> Self {
        self.0 = self.0.prop("slogan", slogan.into());
        self
    }

    /// What it does.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.0 = self.0.prop("description", text.into());
        self
    }

    /// The address people write to.
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.0 = self.0.prop("email", email.into());
        self
    }
}

/// The site as a whole.
#[derive(Clone, Debug)]
#[must_use]
pub struct WebSite(Thing);

impl WebSite {
    /// The site `name`, at `url`, in English unless [`in_language`](Self::in_language)
    /// says otherwise.
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self(
            Thing::new("WebSite")
                .prop("name", name.into())
                .prop("url", url.into())
                .prop("inLanguage", "en"),
        )
    }

    /// Who publishes it: the `@id` of its [`Organization`].
    pub fn publisher(mut self, id: impl Into<String>) -> Self {
        self.0 = self.0.link("publisher", id);
        self
    }

    /// Its language: `en`, `en-IN`, `hi`.
    pub fn in_language(mut self, language: impl Into<String>) -> Self {
        self.0 = self.0.prop("inLanguage", language.into());
        self
    }

    /// What it is, in a sentence.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.0 = self.0.prop("description", text.into());
        self
    }
}

/// Which kind of page a [`WebPage`] is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PageKind {
    /// A page. The default.
    #[default]
    WebPage,
    /// About the organization: give it a [`main_entity`](WebPage::main_entity).
    AboutPage,
    /// A list of other pages: a blog's index, a changelog.
    CollectionPage,
    /// How to reach the organization.
    ContactPage,
    /// Plans and prices, or a product.
    ItemPage,
}

impl PageKind {
    /// The schema.org type.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WebPage => "WebPage",
            Self::AboutPage => "AboutPage",
            Self::CollectionPage => "CollectionPage",
            Self::ContactPage => "ContactPage",
            Self::ItemPage => "ItemPage",
        }
    }
}

/// One page of the site.
#[derive(Clone, Debug)]
#[must_use]
pub struct WebPage(Thing);

impl WebPage {
    /// The page at `url`, titled `name`, described by `description`, in English unless
    /// [`in_language`](Self::in_language) says otherwise.
    pub fn new(
        url: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self(
            Thing::new("WebPage")
                .prop("url", url.into())
                .prop("name", name.into())
                .prop("description", description.into())
                .prop("inLanguage", "en"),
        )
    }

    /// Which kind of page it is.
    pub fn kind(mut self, kind: PageKind) -> Self {
        self.0.kind = Cow::Borrowed(kind.as_str());
        self
    }

    /// Its language.
    pub fn in_language(mut self, language: impl Into<String>) -> Self {
        self.0 = self.0.prop("inLanguage", language.into());
        self
    }

    /// When its content last changed: a date (`2026-11-12`) or a date and time.
    pub fn date_modified(mut self, date: impl Display) -> Self {
        self.0 = self.0.prop("dateModified", date.to_string());
        self
    }

    /// When it was first published.
    pub fn date_published(mut self, date: impl Display) -> Self {
        self.0 = self.0.prop("datePublished", date.to_string());
        self
    }

    /// The site it belongs to: the `@id` of its [`WebSite`].
    pub fn is_part_of(mut self, id: impl Into<String>) -> Self {
        self.0 = self.0.link("isPartOf", id);
        self
    }

    /// Its breadcrumb: the `@id` of its [`BreadcrumbList`].
    pub fn breadcrumb(mut self, id: impl Into<String>) -> Self {
        self.0 = self.0.link("breadcrumb", id);
        self
    }

    /// What it is about: the `@id` of a node of the page.
    pub fn main_entity(mut self, id: impl Into<String>) -> Self {
        self.0 = self.0.link("mainEntity", id);
        self
    }

    /// Its main picture: an absolute URL.
    pub fn image(mut self, url: impl Into<String>) -> Self {
        self.0 = self.0.prop("image", url.into());
        self
    }
}

/// An app or a piece of software: what a product page sells.
#[derive(Clone, Debug)]
#[must_use]
pub struct SoftwareApplication(Thing);

impl SoftwareApplication {
    /// The app `name`, in the category `category`: `HealthApplication`,
    /// `BusinessApplication`, `FinanceApplication`, `DeveloperApplication` and so on.
    pub fn new(name: impl Into<String>, category: impl Into<String>) -> Self {
        Self(
            Thing::new("SoftwareApplication")
                .prop("name", name.into())
                .prop("applicationCategory", category.into()),
        )
    }

    /// What it does, in a sentence or two.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.0 = self.0.prop("description", text.into());
        self
    }

    /// Where it runs: `Web, Android, iOS`.
    pub fn operating_system(mut self, systems: impl Into<String>) -> Self {
        self.0 = self.0.prop("operatingSystem", systems.into());
        self
    }

    /// Its address.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.0 = self.0.prop("url", url.into());
        self
    }

    /// Where to get it.
    pub fn install_url(mut self, url: impl Into<String>) -> Self {
        self.0 = self.0.prop("installUrl", url.into());
        self
    }

    /// Its version.
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.0 = self.0.prop("softwareVersion", version.into());
        self
    }

    /// Whether it can be used for nothing.
    pub fn free(mut self, free: bool) -> Self {
        self.0 = self.0.prop("isAccessibleForFree", free);
        self
    }

    /// One thing it does. Call it again for more.
    pub fn feature(mut self, feature: impl Into<String>) -> Self {
        self.0 = self.0.push("featureList", feature.into());
        self
    }

    /// One way to buy it. Call it again for more: a plan, monthly and yearly.
    pub fn offer(mut self, offer: Offer) -> Self {
        self.0 = self.0.push("offers", Thing::from(offer).to_value());
        self
    }

    /// Who makes it: the `@id` of its [`Organization`].
    pub fn publisher(mut self, id: impl Into<String>) -> Self {
        self.0 = self.0.link("publisher", id);
        self
    }
}

/// A price: for an app, a plan, a thing.
#[derive(Clone, Debug)]
#[must_use]
pub struct Offer(Thing);

impl Offer {
    /// `price` (digits and a decimal point: `0`, `299`, `2990.00`) in `currency`
    /// (`INR`).
    pub fn new(price: impl Into<String>, currency: impl Into<String>) -> Self {
        Self(
            Thing::new("Offer")
                .prop("price", price.into())
                .prop("priceCurrency", currency.into()),
        )
    }

    /// Its name: `Plus, billed yearly`.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.0 = self.0.prop("name", name.into());
        self
    }

    /// Where to buy it.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.0 = self.0.prop("url", url.into());
        self
    }

    /// What it includes.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.0 = self.0.prop("description", text.into());
        self
    }
}

/// Questions and their answers, each also on the page as written.
#[derive(Clone, Debug)]
#[must_use]
pub struct FaqPage(Thing);

impl FaqPage {
    /// The questions of the page at `url`.
    pub fn new(url: impl Into<String>) -> Self {
        Self(
            Thing::new("FAQPage")
                .prop("url", url.into())
                .prop("mainEntity", Value::Array(Vec::new())),
        )
    }

    /// Adds `question`, answered by `answer` in plain text.
    pub fn question(mut self, question: impl Into<String>, answer: impl Into<String>) -> Self {
        let question: String = question.into();
        let answer: String = answer.into();
        self.0 = self.0.push(
            "mainEntity",
            json!({
                "@type": "Question",
                "name": question,
                "acceptedAnswer": { "@type": "Answer", "text": answer },
            }),
        );
        self
    }
}

/// Where a page sits in the site, from the home page down to it.
#[derive(Clone, Debug)]
#[must_use]
pub struct BreadcrumbList(Thing, usize);

impl BreadcrumbList {
    /// No steps yet.
    pub fn new() -> Self {
        Self(
            Thing::new("BreadcrumbList").prop("itemListElement", Value::Array(Vec::new())),
            0,
        )
    }

    /// The next step down: `name` at `url`. The first is the home page and the last is
    /// the page itself.
    pub fn step(mut self, name: impl Into<String>, url: impl Into<String>) -> Self {
        self.1 += 1;
        let name: String = name.into();
        let url: String = url.into();
        self.0 = self.0.push(
            "itemListElement",
            json!({ "@type": "ListItem", "position": self.1, "name": name, "item": url }),
        );
        self
    }

    /// Names the node, so a [`WebPage`] can point at it.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.0 = self.0.id(id);
        self
    }
}

impl Default for BreadcrumbList {
    fn default() -> Self {
        Self::new()
    }
}

impl From<BreadcrumbList> for Thing {
    fn from(list: BreadcrumbList) -> Self {
        list.0
    }
}

typed!(
    Organization,
    WebSite,
    WebPage,
    SoftwareApplication,
    Offer,
    FaqPage
);

/// The properties a node of each type needs to be of use to a search engine or an answer.
const REQUIRED: &[(&str, &[&str])] = &[
    ("Organization", &["name", "url", "logo", "sameAs"]),
    ("WebSite", &["name", "url", "publisher", "inLanguage"]),
    (
        "SoftwareApplication",
        &[
            "name",
            "description",
            "applicationCategory",
            "operatingSystem",
            "offers",
        ],
    ),
    (
        "WebPage",
        &[
            "url",
            "name",
            "description",
            "inLanguage",
            "dateModified",
            "isPartOf",
            "breadcrumb",
        ],
    ),
    (
        "CollectionPage",
        &[
            "url",
            "name",
            "description",
            "inLanguage",
            "dateModified",
            "isPartOf",
            "breadcrumb",
        ],
    ),
    (
        "ContactPage",
        &[
            "url",
            "name",
            "description",
            "inLanguage",
            "dateModified",
            "isPartOf",
            "breadcrumb",
        ],
    ),
    (
        "ItemPage",
        &[
            "url",
            "name",
            "description",
            "inLanguage",
            "dateModified",
            "isPartOf",
            "breadcrumb",
        ],
    ),
    (
        "AboutPage",
        &[
            "url",
            "name",
            "description",
            "inLanguage",
            "dateModified",
            "isPartOf",
            "breadcrumb",
            "mainEntity",
        ],
    ),
    ("BreadcrumbList", &["itemListElement"]),
    ("ListItem", &["position", "name", "item"]),
    ("FAQPage", &["mainEntity", "url"]),
    ("Question", &["name", "acceptedAnswer"]),
    ("Answer", &["text"]),
    ("Offer", &["price", "priceCurrency"]),
];

/// Properties whose values are addresses, so must be absolute `https` URLs.
const URL_PROPS: &[&str] = &[
    "url",
    "item",
    "logo",
    "image",
    "sameAs",
    "installUrl",
    "contentUrl",
    "thumbnailUrl",
    "mainEntityOfPage",
];

/// What a search engine would hold against `data` on the page whose canonical address is
/// `canonical`.
pub(crate) fn problems(data: &[JsonLd], canonical: Option<&str>) -> Vec<String> {
    let mut problems = Vec::new();
    let nodes: Vec<Value> = data
        .iter()
        .flat_map(|graph| graph.nodes.iter().map(Thing::to_value))
        .collect();
    if nodes.is_empty() {
        problems.push("no structured data".to_owned());
        return problems;
    }
    let mut defined = Vec::new();
    let mut references = Vec::new();
    for node in &nodes {
        check(node, &mut problems, &mut defined, &mut references);
    }
    for reference in references {
        if !defined.contains(&reference) {
            problems.push(format!(
                "structured data points at {reference}, which no node of the page is"
            ));
        }
    }
    let kinds: Vec<&str> = nodes
        .iter()
        .filter_map(|node| node.get("@type").and_then(Value::as_str))
        .collect();
    for need in ["Organization", "WebSite", "BreadcrumbList"] {
        if !kinds.contains(&need) {
            problems.push(format!("the structured data has no {need}"));
        }
    }
    let pages = [
        "WebPage",
        "AboutPage",
        "CollectionPage",
        "ContactPage",
        "ItemPage",
    ];
    if !kinds.iter().any(|kind| pages.contains(kind)) {
        problems.push("the structured data has no node for the page".to_owned());
    }
    for crumbs in nodes
        .iter()
        .filter(|node| node.get("@type").and_then(Value::as_str) == Some("BreadcrumbList"))
    {
        let steps = crumbs
            .get("itemListElement")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let last = steps
            .last()
            .and_then(|step| step.get("item"))
            .and_then(Value::as_str);
        if let (Some(canonical), Some(last)) = (canonical, last)
            && last != canonical
        {
            problems.push(format!(
                "the breadcrumb's last step is {last}, not the page ({canonical})"
            ));
        }
    }
    problems
}

/// Checks one node and the typed nodes inside it.
fn check(
    node: &Value,
    problems: &mut Vec<String>,
    defined: &mut Vec<String>,
    references: &mut Vec<String>,
) {
    let Some(map) = node.as_object() else {
        return;
    };
    if map.len() == 1
        && let Some(id) = map.get("@id").and_then(Value::as_str)
    {
        references.push(id.to_owned());
        return;
    }
    let kind = map.get("@type").and_then(Value::as_str);
    if let Some(id) = map.get("@id").and_then(Value::as_str) {
        defined.push(id.to_owned());
    }
    if let Some(kind) = kind
        && let Some((_, need)) = REQUIRED.iter().find(|(name, _)| *name == kind)
    {
        for prop in *need {
            let missing = match map.get(*prop) {
                None | Some(Value::Null) => true,
                Some(Value::String(text)) => text.is_empty(),
                Some(Value::Array(list)) => list.is_empty(),
                Some(_) => false,
            };
            if missing {
                problems.push(format!("{kind} needs \"{prop}\""));
            }
        }
    }
    for (name, value) in map {
        let items: Vec<&Value> = match value {
            Value::Array(list) => list.iter().collect(),
            one => vec![one],
        };
        for item in items {
            match item {
                Value::Object(_) => check(item, problems, defined, references),
                Value::String(text) if URL_PROPS.contains(&name.as_str()) => {
                    if !is_absolute_https(text) {
                        problems.push(format!(
                            "{}.{name} is {text}, not an absolute https URL",
                            kind.unwrap_or("a node")
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = "https://kesar.example";

    fn graph() -> JsonLd {
        let org = format!("{SITE}/#organization");
        let site = format!("{SITE}/#website");
        let crumbs = format!("{SITE}/pricing/#breadcrumb");
        JsonLd::new()
            .add(
                Organization::new("Kesar", format!("{SITE}/"))
                    .id(&org)
                    .logo(format!("{SITE}/logo.png"))
                    .same_as("https://x.com/kesar"),
            )
            .add(
                WebSite::new("Kesar", format!("{SITE}/"))
                    .id(&site)
                    .publisher(&org),
            )
            .add(
                WebPage::new(format!("{SITE}/pricing/"), "Pricing", "Plans in rupees.")
                    .kind(PageKind::ItemPage)
                    .date_modified("2026-11-12")
                    .is_part_of(&site)
                    .breadcrumb(&crumbs),
            )
            .add(
                BreadcrumbList::new()
                    .id(&crumbs)
                    .step("Home", format!("{SITE}/"))
                    .step("Pricing", format!("{SITE}/pricing/")),
            )
    }

    #[test]
    fn a_whole_graph_has_no_problems() {
        let problems = problems(&[graph()], Some(&format!("{SITE}/pricing/")));
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn missing_properties_dangling_references_and_relative_urls_are_problems() {
        let data = JsonLd::new()
            .add(Organization::new("Kesar", "/"))
            .add(WebSite::new("Kesar", format!("{SITE}/")).publisher("#nobody"));
        let found = problems(&[data], None);
        let has = |part: &str| found.iter().any(|problem| problem.contains(part));
        assert!(has("Organization needs \"logo\""), "{found:#?}");
        assert!(has("Organization needs \"sameAs\""), "{found:#?}");
        assert!(has("Organization.url is /"), "{found:#?}");
        assert!(has("points at #nobody"), "{found:#?}");
        assert!(has("no BreadcrumbList"), "{found:#?}");
        assert!(has("no node for the page"), "{found:#?}");
    }

    #[test]
    fn a_breadcrumb_ends_at_the_page() {
        let found = problems(&[graph()], Some(&format!("{SITE}/about/")));
        assert!(found.iter().any(|p| p.contains("last step")), "{found:#?}");
    }

    #[test]
    fn steps_are_numbered_from_one() {
        let value = Thing::from(
            BreadcrumbList::new()
                .step("A", "https://a.example/")
                .step("B", "https://a.example/b"),
        )
        .to_value();
        let steps = value["itemListElement"].as_array().unwrap();
        assert_eq!(steps[0]["position"], 1);
        assert_eq!(steps[1]["position"], 2);
        assert_eq!(steps[1]["@type"], "ListItem");
    }

    #[test]
    fn script_text_never_holds_markup_and_reads_back_the_same() {
        for nasty in [
            "</script><script>alert(1)</script>",
            "<!-- x",
            "a & b > c",
            "line\u{2028}para\u{2029}",
            "\"quoted\" \\ back",
            "पंजाबी ਪੰਜਾਬੀ ₹1,00,000",
        ] {
            let text = JsonLd::new()
                .add(FaqPage::new(format!("{SITE}/")).question(nasty, nasty))
                .to_script();
            assert!(!text.contains('<'), "{text}");
            assert!(!text.contains('>'), "{text}");
            assert!(!text.contains('&'), "{text}");
            assert!(!text.contains('\u{2028}'), "{text}");
            let back: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(back["@graph"][0]["mainEntity"][0]["name"], nasty);
            assert_eq!(
                back["@graph"][0]["mainEntity"][0]["acceptedAnswer"]["text"],
                nasty
            );
        }
    }

    #[test]
    fn a_node_carries_its_type_and_id() {
        let value =
            Thing::from(Organization::new("K", "https://k.example/").id("https://k.example/#o"))
                .to_value();
        assert_eq!(value["@type"], "Organization");
        assert_eq!(value["@id"], "https://k.example/#o");
        assert_eq!(value["name"], "K");
    }
}
