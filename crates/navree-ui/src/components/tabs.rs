//! Tabs. Every tab is a link to the same page with its key in the query, so a tab
//! works with no script; the `menus` module switches them in the page, keeps the
//! address in step, and gives the list the keys of the tabs pattern. A tab's panel
//! can be drawn in the page, or loaded the first time it is shown.

use std::borrow::Cow;

use super::{Size, icon};
use crate::form::{encode, encode_into};
use crate::html::Href;
use crate::icons::Icon;
use crate::node::{AttrValue, IntoNodes, Node, Uses, el, into_node};

/// The way a list of tabs, or a group of toggles, runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Orientation {
    /// Left to right (right to left in a right-to-left page): the default.
    #[default]
    Horizontal,
    /// Top to bottom.
    Vertical,
}

impl Orientation {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

/// How the list of tabs looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TabsVariant {
    /// A line under (or beside) the chosen tab: the default.
    #[default]
    Underline,
    /// The tabs sit in one box and the chosen one is raised, like a segmented control.
    Segmented,
}

impl TabsVariant {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Underline => "underline",
            Self::Segmented => "segmented",
        }
    }
}

/// When a tab opens as the arrow keys move over it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Activation {
    /// Moving to a tab opens it: the default, right when the panels are ready at once.
    #[default]
    Automatic,
    /// Moving only moves focus; Enter or Space opens the tab. Right when opening one
    /// is slow, such as a panel loaded from the server.
    Manual,
}

/// One tab and its panel.
#[derive(Clone, Debug)]
#[must_use]
pub struct Tab {
    key: String,
    label: Option<Node>,
    aria_label: Option<String>,
    start: Option<Icon>,
    end: Option<Icon>,
    count: Option<Cow<'static, str>>,
    disabled: bool,
    panel: Vec<Node>,
    src: Option<Href>,
    fresh: bool,
}

impl Tab {
    /// A tab shown as `label`. `key` goes in the address (`?tab=key`) and picks
    /// the tab when the page is drawn.
    pub fn new(key: impl Into<String>, label: impl Into<Node>) -> Self {
        Self {
            key: key.into(),
            label: Some(label.into()),
            aria_label: None,
            start: None,
            end: None,
            count: None,
            disabled: false,
            panel: Vec::new(),
            src: None,
            fresh: false,
        }
    }

    /// A tab with an icon and no text. It must say what it is, so the accessible
    /// label is required.
    pub fn icon_only(key: impl Into<String>, glyph: Icon, label: impl Into<String>) -> Self {
        let mut tab = Self::new(key, Node::Empty);
        tab.label = None;
        tab.start = Some(glyph);
        tab.aria_label = Some(label.into());
        tab
    }

    /// Adds an icon before the label.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.start = Some(glyph);
        self
    }

    /// Adds an icon after the label.
    pub fn icon_end(mut self, glyph: Icon) -> Self {
        self.end = Some(glyph);
        self
    }

    /// Adds a count after the label (`"12"`, `"99+"`).
    pub fn count(mut self, count: impl Into<Cow<'static, str>>) -> Self {
        self.count = Some(count.into());
        self
    }

    /// Disables it: no link, skipped by the arrow keys, never opened.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets what the panel holds.
    pub fn panel(mut self, panel: impl IntoNodes) -> Self {
        self.panel.clear();
        panel.push_into(&mut self.panel);
        self
    }

    /// Makes the panel load from `src` the first time the tab is shown, and keep
    /// what it got. `src` answers the panel's content as a fragment, or a page
    /// holding an element with the panel's id. With script off the tab is a link
    /// to the page, so give the tab that is open its [`panel`](Self::panel) too.
    pub fn lazy(mut self, src: impl Into<Href>) -> Self {
        self.src = Some(src.into());
        self
    }

    /// Loads a [lazy](Self::lazy) panel again each time its tab is shown.
    pub fn fresh(mut self) -> Self {
        self.fresh = true;
        self
    }
}

/// A list of tabs and their panels.
///
/// ```
/// use navree_ui::{Tab, Tabs, render};
///
/// let tabs = Tabs::new("deal", "Deal sections")
///     .tab(Tab::new("overview", "Overview").panel("The overview"))
///     .tab(Tab::new("notes", "Notes").count("3").panel("Three notes"))
///     .active("notes");
/// let html = render(&tabs.into());
/// assert!(html.contains(r#"href="?tab=notes""#));
/// assert!(html.contains(r#"role="tabpanel""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Tabs {
    id: String,
    label: String,
    param: Cow<'static, str>,
    tabs: Vec<Tab>,
    active: Option<String>,
    variant: TabsVariant,
    orientation: Orientation,
    activation: Activation,
    size: Size,
    stretch: bool,
    findable: bool,
    keep: Vec<(String, String)>,
    failed: Cow<'static, str>,
    retry: Cow<'static, str>,
}

impl Tabs {
    /// Tabs with the id `id`, named `label` for assistive technology.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            param: Cow::Borrowed("tab"),
            tabs: Vec::new(),
            active: None,
            variant: TabsVariant::Underline,
            orientation: Orientation::Horizontal,
            activation: Activation::Automatic,
            size: Size::Md,
            stretch: false,
            findable: false,
            keep: Vec::new(),
            failed: Cow::Borrowed("This tab didn't load."),
            retry: Cow::Borrowed("Retry"),
        }
    }

    /// Adds a tab.
    pub fn tab(mut self, tab: Tab) -> Self {
        self.tabs.push(tab);
        self
    }

    /// Sets the open tab by its key. A key no enabled tab has opens the first one.
    pub fn active(mut self, key: &str) -> Self {
        self.active = Some(key.to_owned());
        self
    }

    /// Sets the open tab from the query parameter, which may be missing.
    pub fn active_from(mut self, key: Option<&str>) -> Self {
        self.active = key.map(str::to_owned);
        self
    }

    /// Names the query parameter that holds the open tab (`tab` by default). Two
    /// sets of tabs on one page want two names.
    pub fn param(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.param = name.into();
        self
    }

    /// Keeps another query parameter in every tab's link, such as a filter.
    pub fn keep(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.keep.push((name.into(), value.into()));
        self
    }

    /// Sets the look.
    pub fn variant(mut self, variant: TabsVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Lays the list out down the side.
    pub fn vertical(mut self) -> Self {
        self.orientation = Orientation::Vertical;
        self
    }

    /// Sets when a tab opens as the arrow keys move over it.
    pub fn activation(mut self, activation: Activation) -> Self {
        self.activation = activation;
        self
    }

    /// Sets the size of the tabs.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Shares the width of the list among the tabs.
    pub fn stretch(mut self) -> Self {
        self.stretch = true;
        self
    }

    /// Lets the browser's find-in-page search the panels that are not showing, and
    /// open the tab that matches.
    pub fn findable(mut self) -> Self {
        self.findable = true;
        self
    }

    /// Sets the words of a panel that failed to load.
    pub fn labels(
        mut self,
        failed: impl Into<Cow<'static, str>>,
        retry: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.failed = failed.into();
        self.retry = retry.into();
        self
    }

    /// The index of the tab that is open.
    fn open_index(&self) -> Option<usize> {
        self.active
            .as_deref()
            .and_then(|key| {
                self.tabs
                    .iter()
                    .position(|tab| tab.key == key && !tab.disabled)
            })
            .or_else(|| self.tabs.iter().position(|tab| !tab.disabled))
    }

    fn href_of(&self, key: &str) -> Href {
        let mut query = String::new();
        if !self.keep.is_empty() {
            let pairs: Vec<(&str, &str)> = self
                .keep
                .iter()
                .filter(|(name, _)| *name != self.param)
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            query.push_str(&encode(&pairs));
            if !pairs.is_empty() {
                query.push('&');
            }
        }
        encode_into(&mut query, &self.param);
        query.push('=');
        encode_into(&mut query, key);
        Href::from(format!("?{query}"))
    }

    fn into_node(self) -> Node {
        let open = self.open_index();
        let slugs = slugs(&self.tabs);
        let any_lazy = self.tabs.iter().any(|tab| tab.src.is_some());
        let labels = serde_json::json!({ "failed": self.failed, "retry": self.retry }).to_string();
        let mut buttons = Vec::with_capacity(self.tabs.len());
        let mut panels = Vec::with_capacity(self.tabs.len());
        let hrefs: Vec<Href> = self.tabs.iter().map(|tab| self.href_of(&tab.key)).collect();
        for (at, ((tab, slug), href)) in self.tabs.into_iter().zip(slugs).zip(hrefs).enumerate() {
            let selected = open == Some(at);
            let tab_id = format!("{}-tab-{slug}", self.id);
            let panel_id = format!("{}-panel-{slug}", self.id);
            let mut button = el("a")
                .slot("tab")
                .attr("role", "tab")
                .attr("id", tab_id.clone())
                .attr("aria-selected", if selected { "true" } else { "false" })
                .attr("aria-controls", panel_id.clone())
                .attr("data-key", tab.key.clone())
                .flag("data-active", selected)
                .flag("data-ui-noboost", true);
            if tab.disabled {
                button = button
                    .attr("aria-disabled", "true")
                    .flag("data-disabled", true);
            } else {
                button = button.href(href);
            }
            button = button
                .attr_opt("aria-label", tab.aria_label.clone())
                .children((
                    tab.start.map(icon),
                    tab.label,
                    tab.count
                        .map(|count| el("span").slot("tab-count").child(count)),
                    tab.end.map(icon),
                ));
            buttons.push(button);

            let lazy = tab.src.is_some();
            let has_content = !tab.panel.is_empty();
            let mut panel = el("div")
                .slot("tabs-panel")
                .attr("role", "tabpanel")
                .attr("id", panel_id)
                .attr("aria-labelledby", tab_id)
                .attr("tabindex", 0_i64)
                .flag("data-active", selected);
            if !selected {
                panel = if self.findable && !lazy {
                    panel.attr("hidden", "until-found")
                } else {
                    panel.flag("hidden", true)
                };
            }
            if let Some(src) = tab.src {
                panel = panel
                    .flag("data-ui-region", true)
                    .attr("data-ui-src", src)
                    .flag("data-ui-fresh", tab.fresh)
                    // The open tab is loaded by the page itself, so showing it again is not a load.
                    .flag("data-ui-loaded", has_content || selected);
                if !has_content {
                    panel = panel.flag("aria-busy", true);
                    if selected {
                        panel = panel.flag("data-ui-lazy", true);
                    }
                }
                panel = panel.uses(Uses::CORE);
            }
            panels.push(if lazy && !has_content {
                panel.children((
                    crate::components::Skeleton::new(crate::components::SkeletonShape::Line),
                    crate::components::Skeleton::new(crate::components::SkeletonShape::Short),
                ))
            } else {
                panel.child(tab.panel)
            });
        }
        let list = el("div")
            .slot("tabs-list")
            .attr("role", "tablist")
            .attr("aria-label", self.label)
            .attr_opt(
                "aria-orientation",
                (self.orientation == Orientation::Vertical).then_some("vertical"),
            )
            .child(buttons);
        let root = el("div")
            .slot("tabs")
            .attr("id", self.id)
            .attr("data-variant", self.variant.as_str())
            .attr("data-orientation", self.orientation.as_str())
            .attr_opt(
                "data-activation",
                (self.activation == Activation::Manual).then_some("manual"),
            )
            .attr_opt("data-size", self.size.as_str())
            .flag("data-stretch", self.stretch)
            .flag("data-findable", self.findable)
            .attr("data-ui-tabs", AttrValue::from(self.param.into_owned()))
            .attr("data-l", labels)
            .uses(Uses::MENUS);
        let root = if any_lazy {
            root.uses(Uses::CORE)
        } else {
            root
        };
        root.children((list, panels)).into()
    }
}

/// Ids can't hold what a key can: each key becomes letters, digits, `-` and `_`, and
/// a key that changed (or repeats) gets its place in the list too, so no two collide.
fn slugs(tabs: &[Tab]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::with_capacity(tabs.len());
    for (at, tab) in tabs.iter().enumerate() {
        let mut slug: String = tab
            .key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        if slug.is_empty() || slug != tab.key || seen.contains(&slug) {
            slug = format!("{slug}-{at}");
        }
        seen.push(slug);
    }
    seen
}

into_node!(Tabs);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn html(tabs: Tabs) -> String {
        render(&tabs.into())
    }

    fn deal() -> Tabs {
        Tabs::new("deal", "Deal sections")
            .tab(Tab::new("overview", "Overview").panel("A"))
            .tab(Tab::new("notes", "Notes").panel("B"))
            .tab(Tab::new("files", "Files").disabled(true).panel("C"))
    }

    #[test]
    fn the_first_enabled_tab_opens_when_none_is_asked_for() {
        let out = html(deal());
        assert!(
            out.contains(r#"id="deal-tab-overview" aria-selected="true""#),
            "{out}"
        );
        assert!(
            out.contains(r#"id="deal-tab-notes" aria-selected="false""#),
            "{out}"
        );
        assert_eq!(out.matches("hidden").count(), 2, "two panels wait: {out}");
    }

    #[test]
    fn an_unknown_or_disabled_key_opens_the_first_tab() {
        for key in ["nope", "files", ""] {
            let out = html(deal().active(key));
            assert!(
                out.contains(r#"id="deal-tab-overview" aria-selected="true""#),
                "{key}: {out}"
            );
        }
        let out = html(deal().active_from(None));
        assert!(out.contains(r#"id="deal-tab-overview" aria-selected="true""#));
    }

    #[test]
    fn the_asked_for_tab_opens_and_its_panel_shows() {
        let out = html(deal().active("notes"));
        assert!(
            out.contains(r#"id="deal-tab-notes" aria-selected="true""#),
            "{out}"
        );
        let panel = out.split(r#"id="deal-panel-notes""#).nth(1).unwrap();
        let tag = panel.split('>').next().unwrap();
        assert!(!tag.contains("hidden"), "{tag}");
        let other = out.split(r#"id="deal-panel-overview""#).nth(1).unwrap();
        assert!(other.split('>').next().unwrap().contains("hidden"));
    }

    #[test]
    fn roles_and_the_links_between_tab_and_panel_hold() {
        let out = html(deal());
        assert!(
            out.contains(r#"role="tablist" aria-label="Deal sections""#),
            "{out}"
        );
        assert!(out.contains(r#"aria-controls="deal-panel-notes""#), "{out}");
        assert!(out.contains(r#"aria-labelledby="deal-tab-notes""#), "{out}");
        assert!(out.contains(r#"role="tabpanel""#), "{out}");
        assert!(
            out.contains(r#"tabindex="0""#),
            "a panel is a tab stop: {out}"
        );
    }

    #[test]
    fn every_tab_is_a_link_so_it_works_with_no_script() {
        let out = html(deal());
        assert!(out.contains(r#"href="?tab=overview""#), "{out}");
        assert!(out.contains(r#"href="?tab=notes""#), "{out}");
        // A boosted link would fetch the whole page: the module switches tabs itself.
        assert_eq!(out.matches("data-ui-noboost").count(), 3, "{out}");
    }

    #[test]
    fn a_disabled_tab_has_no_link() {
        let out = html(deal());
        let files = out.split(r#"id="deal-tab-files""#).nth(1).unwrap();
        let tag = files.split('>').next().unwrap();
        assert!(!tag.contains("href"), "{tag}");
        assert!(tag.contains(r#"aria-disabled="true""#), "{tag}");
    }

    #[test]
    fn other_parameters_stay_in_the_links_and_the_tab_parameter_is_encoded() {
        let out = html(
            Tabs::new("t", "T")
                .param("section")
                .keep("q", "a b&c")
                .keep("section", "stale")
                .tab(Tab::new("a/b", "A")),
        );
        assert!(
            out.contains(r#"href="?q=a%20b%26c&amp;section=a%2Fb""#),
            "{out}"
        );
        assert!(out.contains(r#"data-ui-tabs="section""#), "{out}");
    }

    #[test]
    fn keys_that_cannot_be_ids_still_give_unique_ids() {
        let out = html(
            Tabs::new("t", "T")
                .tab(Tab::new("a b", "One"))
                .tab(Tab::new("a-b", "Two"))
                .tab(Tab::new("a-b", "Three"))
                .tab(Tab::new("", "Four")),
        );
        let mut ids: Vec<&str> = out
            .match_indices(r#"id="t-tab-"#)
            .map(|(at, _)| out[at + 4..].split('"').next().unwrap())
            .collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(total, 4);
        assert_eq!(ids.len(), 4, "{out}");
    }

    #[test]
    fn labels_are_text() {
        let out = html(
            Tabs::new("t", "<b>T</b>")
                .tab(Tab::new("x", "<img src=x onerror=alert(1)>").count("<1>")),
        );
        assert!(!out.contains("<img"), "{out}");
        assert!(!out.contains("<b>"), "{out}");
        assert!(out.contains("&lt;img src=x onerror=alert(1)&gt;"), "{out}");
        assert!(out.contains("&lt;1&gt;"), "{out}");
    }

    #[test]
    fn looks_orientation_and_activation_are_attributes() {
        let out = html(
            deal()
                .variant(TabsVariant::Segmented)
                .vertical()
                .activation(Activation::Manual)
                .size(Size::Sm)
                .stretch(),
        );
        assert!(out.contains(r#"data-variant="segmented""#), "{out}");
        assert!(out.contains(r#"data-orientation="vertical""#), "{out}");
        assert!(out.contains(r#"aria-orientation="vertical""#), "{out}");
        assert!(out.contains(r#"data-activation="manual""#), "{out}");
        assert!(out.contains(r#"data-size="sm""#), "{out}");
        assert!(out.contains("data-stretch"), "{out}");
        let plain = html(deal());
        assert!(!plain.contains("data-activation"), "{plain}");
        assert!(!plain.contains("aria-orientation"), "{plain}");
    }

    #[test]
    fn an_icon_only_tab_carries_its_label() {
        let out = html(Tabs::new("t", "T").tab(Tab::icon_only("a", crate::icons::STAR, "Starred")));
        assert!(out.contains(r#"aria-label="Starred""#), "{out}");
    }

    #[test]
    fn a_lazy_tab_is_a_region_that_loads_when_shown() {
        let out = html(
            Tabs::new("t", "T")
                .tab(Tab::new("a", "A").panel("A"))
                .tab(Tab::new("b", "B").lazy("/b").fresh()),
        );
        let b = out.split(r#"id="t-panel-b""#).nth(1).unwrap();
        let tag = b.split('>').next().unwrap();
        assert!(tag.contains(r#"data-ui-src="/b""#), "{tag}");
        assert!(tag.contains("data-ui-region"), "{tag}");
        assert!(tag.contains("data-ui-fresh"), "{tag}");
        assert!(!tag.contains("data-ui-lazy"), "waits for its tab: {tag}");
        assert!(!tag.contains("data-ui-loaded"), "{tag}");
        assert!(out.contains("data-slot=\"skeleton\""), "{out}");
    }

    #[test]
    fn an_open_lazy_tab_loads_on_arrival_unless_it_was_given_content() {
        let bare = html(Tabs::new("t", "T").tab(Tab::new("a", "A").lazy("/a")));
        assert!(bare.contains("data-ui-lazy"), "{bare}");
        assert!(bare.contains("aria-busy"), "{bare}");
        assert!(
            bare.contains("data-ui-loaded"),
            "it is being loaded: {bare}"
        );
        let given = html(Tabs::new("t", "T").tab(Tab::new("a", "A").lazy("/a").panel("Ready")));
        assert!(!given.contains("data-ui-lazy"), "{given}");
        assert!(given.contains("data-ui-loaded"), "{given}");
        assert!(given.contains("Ready"), "{given}");
    }

    #[test]
    fn findable_tabs_hide_their_panels_until_found() {
        let out = html(deal().findable());
        assert_eq!(out.matches(r#"hidden="until-found""#).count(), 2, "{out}");
        assert!(!html(deal()).contains("until-found"));
    }

    #[test]
    fn the_module_loads_and_core_only_for_lazy_panels() {
        let plain: Node = deal().into();
        let found = crate::render::measure(&plain).uses;
        assert!(found.has(Uses::MENUS));
        assert!(!found.has(Uses::CORE));
        let lazy: Node = Tabs::new("t", "T")
            .tab(Tab::new("a", "A").lazy("/a"))
            .into();
        assert!(crate::render::measure(&lazy).uses.has(Uses::CORE));
    }

    #[test]
    fn no_tabs_draws_an_empty_list() {
        let out = html(Tabs::new("t", "T"));
        assert!(out.contains(r#"role="tablist""#), "{out}");
        assert!(!out.contains("tabpanel"), "{out}");
    }
}
