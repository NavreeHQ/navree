//! [`SiteFooter`]: the site's columns of links, its small print and a [`ThemeToggle`].

use std::borrow::Cow;

use super::{Brand, container};
use crate::components::icon;
use crate::form::csrf_field;
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{Element, Node, Uses, el, into_node};
use crate::theme::{Mode, SETTINGS_PATH};

/// Light, dark or the device's own, as three buttons in a plain form: each posts its
/// mode to the toolkit's appearance route, which keeps it in the reader's cookie and
/// sends them back, so it works with no script. Draw the page with
/// [`Appearance::for_reader`](crate::theme::Appearance::for_reader) and pass its mode
/// here, so the button of the mode in use is pressed.
///
/// ```
/// use navree_ui::marketing::ThemeToggle;
/// use navree_ui::render;
/// use navree_ui::theme::Mode;
///
/// let html = render(&ThemeToggle::new(Mode::Dark).back("/pricing/").into());
/// assert!(html.contains(r#"<form data-slot="theme-toggle" method="post" action="/_navree/ui/appearance">"#));
/// assert!(html.contains(r#"name="mode" value="dark" aria-pressed="true""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ThemeToggle {
    current: Mode,
    action: Href,
    back: Option<String>,
    csrf: Option<String>,
    legend: Cow<'static, str>,
    words: [Cow<'static, str>; 3],
}

impl ThemeToggle {
    /// A toggle with `current` pressed.
    pub fn new(current: Mode) -> Self {
        Self {
            current,
            action: Href::from(SETTINGS_PATH),
            back: None,
            csrf: None,
            legend: Cow::Borrowed("Theme"),
            words: [
                Cow::Borrowed(Mode::Light.label()),
                Cow::Borrowed(Mode::Dark.label()),
                Cow::Borrowed(Mode::System.label()),
            ],
        }
    }

    /// Where to come back to: this page's path.
    pub fn back(mut self, path: impl Into<String>) -> Self {
        self.back = Some(path.into());
        self
    }

    /// Posts to `href`, where the app keeps the choice with the user, in place of the
    /// toolkit's route. The form sends `mode` and `back`.
    pub fn action(mut self, href: impl Into<Href>) -> Self {
        self.action = href.into();
        self
    }

    /// Adds the session's CSRF token, for an app whose posts need one.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// The words, in the page's language: the group's name and the three modes.
    pub fn labels(
        mut self,
        legend: impl Into<Cow<'static, str>>,
        light: impl Into<Cow<'static, str>>,
        dark: impl Into<Cow<'static, str>>,
        system: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.legend = legend.into();
        self.words = [light.into(), dark.into(), system.into()];
        self
    }

    fn into_node(self) -> Node {
        let [light, dark, system] = self.words;
        let modes: [(Mode, Icon, Cow<'static, str>); 3] = [
            (Mode::Light, icons::SUN, light),
            (Mode::Dark, icons::MOON, dark),
            (Mode::System, icons::MONITOR, system),
        ];
        let buttons: Vec<Element> = modes
            .into_iter()
            .map(|(mode, glyph, word)| {
                el("button")
                    .slot("theme-toggle-button")
                    .attr("type", "submit")
                    .attr("name", "mode")
                    .attr("value", mode.id())
                    .attr(
                        "aria-pressed",
                        if mode == self.current {
                            "true"
                        } else {
                            "false"
                        },
                    )
                    .attr("title", word.clone())
                    .children((icon(glyph), el("span").flag("data-ui-sr", true).child(word)))
            })
            .collect();
        el("form")
            .slot("theme-toggle")
            .attr("method", "post")
            .attr("action", self.action)
            .uses(Uses::MARKETING)
            .children((
                self.csrf.map(csrf_field),
                self.back.map(|back| {
                    el("input")
                        .attr("type", "hidden")
                        .attr("name", "back")
                        .attr("value", back)
                }),
                el("fieldset").slot("theme-toggle-group").children((
                    el("legend").flag("data-ui-sr", true).child(self.legend),
                    buttons,
                )),
            ))
            .into()
    }
}

/// One column of a [`SiteFooter`]: a title and its links.
#[derive(Clone, Debug)]
#[must_use]
pub struct FooterColumn {
    title: Cow<'static, str>,
    links: Vec<(Cow<'static, str>, Href)>,
}

impl FooterColumn {
    /// A column titled `title`.
    pub fn new(title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            title: title.into(),
            links: Vec::new(),
        }
    }

    /// Adds a link.
    pub fn link(mut self, label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        self.links.push((label.into(), href.into()));
        self
    }
}

/// The foot of every page: the brand and a line about the site, columns of links, its
/// accounts elsewhere, the small print, and a [`ThemeToggle`].
///
/// ```
/// use navree_ui::marketing::{Brand, FooterColumn, SiteFooter, ThemeToggle};
/// use navree_ui::render;
/// use navree_ui::theme::Mode;
///
/// let footer = SiteFooter::new(Brand::new("Kesar", "/"))
///     .tagline("Your health data, read calmly.")
///     .column(FooterColumn::new("Product").link("Pricing", "/pricing/").link("FAQ", "/#faq"))
///     .column(FooterColumn::new("Company").link("Contact", "/contact/").link("Privacy", "/privacy/"))
///     .legal("© 2026 Kesar Health Pvt. Ltd., Mohali")
///     .theme_toggle(ThemeToggle::new(Mode::System).back("/"));
/// let html = render(&footer.into());
/// assert!(html.contains(r#"<nav data-slot="site-footer-nav" aria-label="Footer">"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct SiteFooter {
    el: Element,
    brand: Brand,
    tagline: Option<Node>,
    columns: Vec<FooterColumn>,
    social: Vec<(Icon, Cow<'static, str>, Href)>,
    legal: Vec<Node>,
    toggle: Option<ThemeToggle>,
    nav_label: Cow<'static, str>,
}

impl SiteFooter {
    /// A footer with `brand` first.
    pub fn new(brand: Brand) -> Self {
        Self {
            el: el("footer").slot("site-footer").uses(Uses::MARKETING),
            brand,
            tagline: None,
            columns: Vec::new(),
            social: Vec::new(),
            legal: Vec::new(),
            toggle: None,
            nav_label: Cow::Borrowed("Footer"),
        }
    }

    /// A line under the brand.
    pub fn tagline(mut self, text: impl Into<Node>) -> Self {
        self.tagline = Some(text.into());
        self
    }

    /// Adds a column of links.
    pub fn column(mut self, column: FooterColumn) -> Self {
        self.columns.push(column);
        self
    }

    /// Adds a link to an account elsewhere, drawn as its icon and named `label`.
    pub fn social(
        mut self,
        glyph: Icon,
        label: impl Into<Cow<'static, str>>,
        href: impl Into<Href>,
    ) -> Self {
        self.social.push((glyph, label.into(), href.into()));
        self
    }

    /// Adds a line of small print: the company, its address, a registration number.
    pub fn legal(mut self, text: impl Into<Node>) -> Self {
        self.legal.push(text.into());
        self
    }

    /// The light and dark toggle, at the end of the small print.
    pub fn theme_toggle(mut self, toggle: ThemeToggle) -> Self {
        self.toggle = Some(toggle);
        self
    }

    /// The links' landmark name, in the page's language: `Footer`.
    pub fn nav_label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.nav_label = label.into();
        self
    }

    fn into_node(self) -> Node {
        let social = (!self.social.is_empty()).then(|| {
            el("ul").slot("site-footer-social").child(
                self.social
                    .into_iter()
                    .map(|(glyph, label, href)| {
                        el("li").child(
                            el("a")
                                .href(href)
                                .attr("aria-label", label.clone())
                                .attr("title", label)
                                .child(icon(glyph)),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        });
        let nav = (!self.columns.is_empty()).then(|| {
            el("nav")
                .slot("site-footer-nav")
                .attr("aria-label", self.nav_label.clone())
                .child(
                    self.columns
                        .into_iter()
                        .map(|column| {
                            el("div").slot("footer-column").children((
                                el("h2").slot("footer-title").child(column.title),
                                el("ul").child(
                                    column
                                        .links
                                        .into_iter()
                                        .map(|(label, href)| {
                                            el("li").child(el("a").href(href).child(label))
                                        })
                                        .collect::<Vec<_>>(),
                                ),
                            ))
                        })
                        .collect::<Vec<_>>(),
                )
        });
        self.el
            .child(
                container().children((
                    el("div").slot("site-footer-top").children((
                        el("div").slot("site-footer-brand").children((
                            self.brand,
                            self.tagline
                                .map(|tagline| el("p").slot("site-footer-tagline").child(tagline)),
                            social,
                        )),
                        nav,
                    )),
                    el("div").slot("site-footer-bottom").children((
                        el("div").slot("site-footer-legal").child(
                            self.legal
                                .into_iter()
                                .map(|line| el("p").child(line))
                                .collect::<Vec<_>>(),
                        ),
                        self.toggle,
                    )),
                )),
            )
            .into()
    }
}

into_node!(ThemeToggle, SiteFooter);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    #[test]
    fn the_toggle_is_a_form_that_works_without_script() {
        let html = render(
            &ThemeToggle::new(Mode::System)
                .back("/privacy/?x=\"1\"")
                .csrf("tok\"en")
                .labels("थीम", "हल्का", "गहरा", "डिवाइस")
                .into(),
        );
        assert!(!html.contains("data-ui-form"), "a plain post: {html}");
        assert_eq!(html.matches(r#"type="submit""#).count(), 3, "{html}");
        assert!(
            html.contains(r#"value="system" aria-pressed="true""#),
            "{html}"
        );
        assert!(
            html.contains(r#"value="light" aria-pressed="false""#),
            "{html}"
        );
        assert!(
            html.contains(r#"name="back" value="/privacy/?x=&quot;1&quot;""#),
            "{html}"
        );
        assert!(html.contains(r#"value="tok&quot;en""#), "{html}");
        assert!(html.contains("<legend data-ui-sr>थीम</legend>"), "{html}");
    }

    #[test]
    fn a_footer_names_its_columns_and_its_accounts() {
        let html = render(
            &SiteFooter::new(Brand::new("Kesar", "/"))
                .column(FooterColumn::new("Product").link("Pricing", "/pricing/"))
                .social(icons::GLOBE, "Kesar on the web", "https://kesar.example/")
                .legal("© 2026")
                .into(),
        );
        assert!(
            html.contains(r#"<h2 data-slot="footer-title">Product</h2>"#),
            "{html}"
        );
        assert!(html.contains(r#"aria-label="Kesar on the web""#), "{html}");
        assert!(html.contains("<p>© 2026</p>"), "{html}");
    }
}
