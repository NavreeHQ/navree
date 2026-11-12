//! [`SiteHeader`]: the site's name, its main links, its calls to action, and a menu for
//! phones that opens with no script.

use std::borrow::Cow;

use super::Brand;
use crate::components::icon;
use crate::html::Href;
use crate::icons;
use crate::node::{Element, IntoNodes, Node, Uses, el, into_node};

/// The header of a public site: the brand on the left, the main links in the middle and
/// the calls to action on the right, held at the top as the page scrolls. Below 48 rem
/// the links and actions fold into a menu behind one button: a `<details>`, so it opens
/// and closes with no script, by keyboard and by touch, and says whether it is open.
///
/// ```
/// use navree_ui::marketing::{Brand, SiteHeader};
/// use navree_ui::{Button, render};
///
/// let header = SiteHeader::new(Brand::new("Kesar", "/"))
///     .link("Features", "/#features")
///     .current("Pricing", "/pricing/")
///     .action(Button::ghost("Sign in").href("/login"))
///     .action(Button::primary("Start free").href("/signup"));
/// let html = render(&header.into());
/// assert!(html.contains(r#"<a href="/pricing/" aria-current="page">Pricing</a>"#));
/// assert!(html.contains(r#"<details data-slot="site-menu">"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct SiteHeader {
    el: Element,
    brand: Brand,
    links: Vec<(Cow<'static, str>, Href, bool)>,
    actions: Vec<Node>,
    menu_label: Cow<'static, str>,
    nav_label: Cow<'static, str>,
}

impl SiteHeader {
    /// A header with `brand` on the left.
    pub fn new(brand: Brand) -> Self {
        Self {
            el: el("header").slot("site-header").uses(Uses::MARKETING),
            brand,
            links: Vec::new(),
            actions: Vec::new(),
            menu_label: Cow::Borrowed("Menu"),
            nav_label: Cow::Borrowed("Main"),
        }
    }

    /// Adds a main link.
    pub fn link(mut self, label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        self.links.push((label.into(), href.into(), false));
        self
    }

    /// Adds the link to the page the reader is on, marked as current.
    pub fn current(mut self, label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        self.links.push((label.into(), href.into(), true));
        self
    }

    /// Adds a call to action on the right: a [`Button`](crate::Button) with an `href`, the
    /// primary one last.
    pub fn action(mut self, action: impl IntoNodes) -> Self {
        action.push_into(&mut self.actions);
        self
    }

    /// The menu button's name for assistive technology, in the page's language: `Menu`.
    pub fn menu_label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.menu_label = label.into();
        self
    }

    /// The links' landmark name, in the page's language: `Main`.
    pub fn nav_label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.nav_label = label.into();
        self
    }

    fn list(&self) -> Element {
        el("ul").child(
            self.links
                .iter()
                .map(|(label, href, current)| {
                    el("li").child(
                        el("a")
                            .href(href.clone())
                            .attr_opt("aria-current", current.then_some("page"))
                            .child(label.clone()),
                    )
                })
                .collect::<Vec<_>>(),
        )
    }

    fn into_node(self) -> Node {
        let has_links = !self.links.is_empty();
        let nav = has_links.then(|| {
            el("nav")
                .slot("site-nav")
                .attr("aria-label", self.nav_label.clone())
                .child(self.list())
        });
        let actions = (!self.actions.is_empty())
            .then(|| el("div").slot("site-actions").child(self.actions.clone()));
        let menu = (has_links || !self.actions.is_empty()).then(|| {
            el("details").slot("site-menu").children((
                el("summary").slot("site-menu-button").children((
                    el("span")
                        .slot("site-menu-open")
                        .attr("aria-hidden", "true")
                        .child(icon(icons::MENU)),
                    el("span")
                        .slot("site-menu-close")
                        .attr("aria-hidden", "true")
                        .child(icon(icons::X)),
                    el("span")
                        .flag("data-ui-sr", true)
                        .child(self.menu_label.clone()),
                )),
                el("div").slot("site-menu-panel").children((
                    has_links.then(|| {
                        el("nav")
                            .slot("site-menu-nav")
                            .attr("aria-label", self.nav_label.clone())
                            .child(self.list())
                    }),
                    (!self.actions.is_empty()).then(|| {
                        el("div")
                            .slot("site-menu-actions")
                            .child(self.actions.clone())
                    }),
                )),
            ))
        });
        self.el
            .child(
                el("div")
                    .slot("site-header-bar")
                    .children((self.brand, nav, actions, menu)),
            )
            .into()
    }
}

into_node!(SiteHeader);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Button, render};

    #[test]
    fn the_menu_holds_the_same_links_and_actions() {
        let html = render(
            &SiteHeader::new(Brand::new("Kesar", "/"))
                .link("Features", "/#features")
                .link("FAQ", "/#faq")
                .action(Button::primary("Start free").href("/signup"))
                .menu_label("मेन्यू")
                .into(),
        );
        assert_eq!(html.matches(r##"href="/#features""##).count(), 2, "{html}");
        assert_eq!(html.matches(r#"href="/signup""#).count(), 2, "{html}");
        assert!(html.contains("<span data-ui-sr>मेन्यू</span>"), "{html}");
        let summary = html.find("<summary").unwrap();
        let panel = html.find("site-menu-panel").unwrap();
        assert!(summary < panel);
    }

    #[test]
    fn a_header_with_only_a_brand_has_no_menu() {
        let html = render(&SiteHeader::new(Brand::new("Kesar", "/")).into());
        assert!(!html.contains("<details"), "{html}");
        assert!(!html.contains("<nav"), "{html}");
    }

    #[test]
    fn hostile_links_go_nowhere() {
        let html = render(
            &SiteHeader::new(Brand::new("<b>", "javascript:alert(1)"))
                .link("<i>", "javascript:alert(1)")
                .into(),
        );
        assert!(!html.contains("javascript:"), "{html}");
        assert!(!html.contains("<i>") && !html.contains("<b>"), "{html}");
    }
}
