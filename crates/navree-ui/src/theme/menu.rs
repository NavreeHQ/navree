//! [`ThemeMenu`]: the reader's own theme and mode, in a popover.

use super::{Appearance, Mode, SETTINGS_PATH, Theme};
use crate::cmd::{Cmd, Trigger};
use crate::components::{Button, ButtonVariant, Size, icon};
use crate::form::csrf_field;
use crate::html::Href;
use crate::icons;
use crate::node::{Element, Node, Uses, el};

/// A button that opens the reader's theme and mode: a card for each theme the
/// app offers, each drawn in its own colours, and Light, Dark and System. It is
/// a plain form: with JavaScript off it posts with its Apply button; with it on,
/// a choice applies as it is made, in one round trip.
///
/// By default it posts to the toolkit's own route, which keeps the choice in a
/// cookie; [`action`](Self::action) sends it to yours, to keep it with the user.
///
/// ```
/// use navree_ui::theme::{Appearance, Mode, Theme, ThemeMenu};
/// use navree_ui::render;
///
/// let menu = ThemeMenu::new(&Appearance::new(Theme::OCEAN, Mode::System), &Theme::BUILT_IN);
/// let html = render(&menu.back("/deals").into());
/// assert!(html.contains(r#"value="ocean" checked"#));
/// assert!(html.contains(r#"value="system" checked"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ThemeMenu {
    current: Appearance,
    themes: Vec<Theme>,
    action: Href,
    back: Option<String>,
    csrf: Option<String>,
    id: String,
    modes: bool,
}

impl ThemeMenu {
    /// A menu showing `current`, offering `themes`.
    pub fn new(current: &Appearance, themes: &[Theme]) -> Self {
        Self {
            current: current.clone(),
            themes: themes.to_vec(),
            action: Href::from(SETTINGS_PATH),
            back: None,
            csrf: None,
            id: "ui-theme-menu".to_owned(),
            modes: true,
        }
    }

    /// Posts the choice to `href`, where the app keeps it: the form sends
    /// `theme` (an id or a short form, see [`Theme::from_short_form`]), `mode`
    /// and `back`.
    pub fn action(mut self, href: impl Into<Href>) -> Self {
        self.action = href.into();
        self
    }

    /// Where to come back to after the choice: this page.
    pub fn back(mut self, path: impl Into<String>) -> Self {
        self.back = Some(path.into());
        self
    }

    /// Adds the session's CSRF token, for an app whose posts need one.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Sets the popover's id, when a page has two menus.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Leaves the mode out: for an app that keeps one mode for everyone.
    pub fn without_modes(mut self) -> Self {
        self.modes = false;
        self
    }

    /// The custom themes the menu draws, whose stylesheets the page must link.
    pub fn sheets(&self) -> Vec<Theme> {
        self.themes
            .iter()
            .filter(|theme| !theme.is_builtin())
            .cloned()
            .collect()
    }

    fn card(&self, theme: &Theme) -> Element {
        let chosen = theme.key() == self.current.theme().key();
        // The swatches stand in a scope of the theme itself, so they show its
        // real colours with no style attribute.
        let swatches = el("span")
            .slot("theme-swatches")
            .attr("data-theme", theme.key().to_owned())
            .attr("data-mode", self.current.mode().id())
            .attr("aria-hidden", "true")
            .children((
                el("span").slot("theme-swatch").attr("data-token", "ink-1"),
                el("span").slot("theme-swatch").attr("data-token", "accent"),
                el("span").slot("theme-swatch").attr("data-token", "fg"),
            ));
        el("label").slot("theme-card").children((
            el("input")
                .slot("theme-card-input")
                .attr("type", "radio")
                .attr("name", "theme")
                .attr("value", theme.short_form().to_owned())
                .flag("checked", chosen),
            swatches,
            el("span").slot("theme-card-text").children((
                el("span")
                    .slot("theme-card-name")
                    .child(theme.name().to_owned()),
                theme
                    .description()
                    .map(|line| el("span").slot("theme-card-description").child(line)),
            )),
            icon(icons::CHECK),
        ))
    }

    fn into_node(self) -> Node {
        let form_id = format!("{}-form", self.id);
        let cards: Vec<Element> = self.themes.iter().map(|theme| self.card(theme)).collect();
        let modes = self.modes.then(|| {
            el("fieldset").slot("theme-modes").children((
                el("legend").slot("theme-menu-legend").child("Mode"),
                el("div").slot("theme-mode-group").child(
                    Mode::ALL
                        .into_iter()
                        .map(|mode| {
                            let glyph = match mode {
                                Mode::Light => icons::SUN,
                                Mode::Dark => icons::MOON,
                                Mode::System => icons::MONITOR,
                            };
                            el("label").slot("theme-mode").children((
                                el("input")
                                    .slot("theme-mode-input")
                                    .attr("type", "radio")
                                    .attr("name", "mode")
                                    .attr("value", mode.id())
                                    .flag("checked", mode == self.current.mode()),
                                icon(glyph),
                                el("span").child(mode.label()),
                            ))
                        })
                        .collect::<Vec<_>>(),
                ),
            ))
        });
        let form = el("form")
            .slot("theme-menu-form")
            .attr("id", form_id.clone())
            .attr("method", "post")
            .attr("action", self.action)
            .flag("data-ui-form", true)
            .on(Trigger::Change, Cmd::submit(form_id))
            .uses(Uses::CORE)
            .children((
                self.csrf.map(csrf_field),
                self.back.map(|back| {
                    el("input")
                        .attr("type", "hidden")
                        .attr("name", "back")
                        .attr("value", back)
                }),
                el("fieldset")
                    .slot("theme-cards")
                    .children((el("legend").slot("theme-menu-legend").child("Theme"), cards)),
                modes,
                Button::primary("Apply")
                    .size(Size::Sm)
                    .submit()
                    .attr("data-theme-apply", ""),
            ));
        let trigger = Button::icon_only(icons::SUN_MOON, "Appearance")
            .variant(ButtonVariant::Ghost)
            .size(Size::Sm)
            .attr("popovertarget", self.id.clone())
            .attr("aria-haspopup", "dialog");
        let popup = el("div")
            .slot("popover-content")
            .attr("data-variant", "theme-menu")
            .attr("id", self.id)
            .attr("popover", "auto")
            .attr("role", "dialog")
            .attr("aria-label", "Appearance")
            .attr("tabindex", -1_i64)
            .flag("autofocus", true)
            .flag("data-ui-place", true)
            .attr("data-side", "bottom")
            .attr("data-align", "end")
            .uses(Uses::OVERLAY)
            .child(form);
        Node::list((trigger, popup))
    }
}

impl From<ThemeMenu> for Node {
    fn from(menu: ThemeMenu) -> Self {
        menu.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;
    use crate::theme::ThemeId;

    #[test]
    fn the_menu_is_a_form_that_works_without_script() {
        let menu = ThemeMenu::new(&Appearance::new(Theme::ROSE, Mode::Light), &Theme::BUILT_IN)
            .back("/deals?page=2")
            .csrf("tok\"en");
        let html = render(&menu.into());
        assert!(
            html.contains(r#"method="post" action="/_navree/ui/appearance""#),
            "{html}"
        );
        assert!(
            html.contains(r#"name="back" value="/deals?page=2""#),
            "{html}"
        );
        assert!(html.contains(r#"value="tok&quot;en""#), "{html}");
        assert_eq!(
            html.matches(r#"name="theme""#).count(),
            Theme::BUILT_IN.len()
        );
        assert!(html.contains(r#"value="rose" checked"#), "{html}");
        assert!(html.contains(r#"value="light" checked"#), "{html}");
        assert!(
            html.contains(r#"type="submit""#),
            "an Apply button for no script: {html}"
        );
        assert!(
            html.contains(r#"data-theme="ocean" data-mode="light""#),
            "each card in its own colours: {html}"
        );
        assert!(!html.contains("style="), "{html}");
        assert!(html.contains(r#"aria-label="Appearance""#), "{html}");
    }

    #[test]
    fn a_custom_theme_posts_its_short_form_and_shows_its_name_as_text() {
        let mine = Theme::custom(
            ThemeId::parse("acme").unwrap(),
            "<b>Acme</b>",
            *Theme::OCEAN.spec(),
        )
        .unwrap();
        let menu = ThemeMenu::new(
            &Appearance::new(mine.clone(), Mode::Dark),
            &[Theme::NAVREE, mine.clone()],
        );
        assert_eq!(menu.sheets(), vec![mine.clone()]);
        let html = render(&menu.into());
        assert!(
            html.contains(&format!(r#"value="{}" checked"#, mine.short_form())),
            "{html}"
        );
        assert!(
            html.contains(&format!(r#"data-theme="{}""#, mine.key())),
            "{html}"
        );
        assert!(html.contains("&lt;b&gt;Acme&lt;/b&gt;"), "{html}");
    }
}
