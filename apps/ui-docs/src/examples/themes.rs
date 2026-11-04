//! Themes, modes, the theme menu, the shell's layouts and the theme builder.

use navree::ui::theme::{Appearance, BuilderDraft, Mode, Theme, ThemeBuilder, ThemeMenu};
use navree::ui::*;

use crate::catalog::{Example, gallery, page, single};

const SOURCE: &str = include_str!("themes.rs");

pub fn all() -> Vec<Example> {
    vec![
        gallery!(
            "themes",
            "built-in",
            "Every built-in theme, dark",
            themes_dark
        ),
        gallery!(
            "themes",
            "light",
            "Every built-in theme, light",
            themes_light
        ),
        single!(
            "themes",
            "modes",
            "Light, dark and the device's",
            themes_modes
        )
        .start(),
        // The menu, drawn open, is 526 px tall: the frames have room for it under its button.
        single!("themes", "menu", "The theme menu, open", themes_menu)
            .show()
            .height(640),
        page!("themes", "inset", "The inset layout", shell_inset)
            .fill()
            .width(1100)
            .height(620),
        page!("themes", "floating", "The floating layout", shell_floating)
            .fill()
            .width(1100)
            .height(620),
        page!("themes", "header", "Links in the top bar", shell_header)
            .fill()
            .width(1100)
            .height(620),
        page!(
            "theme-builder",
            "default",
            "The theme builder",
            builder_default
        )
        .fill()
        .width(1280)
        .height(1400),
    ]
}

/// A card of components drawn in `theme`, in `mode`: a scope of its own.
fn sample(theme: &Theme, mode: Mode) -> Element {
    el("div")
        .attr("data-theme", theme.key().to_owned())
        .attr("data-mode", mode.id())
        .child(Card::new().body(Stack::gap(3).children((
            Row::gap(2).wrap().children((
                Button::primary("Save"),
                Button::new("Cancel"),
                Badge::new("Won").variant(BadgeVariant::Success),
            )),
            Text::muted("A hint on a card."),
        ))))
}

/// The built-in themes, by name.
const BUILT_IN: [(&str, Theme); 7] = [
    ("Navree", Theme::NAVREE),
    ("Neutral", Theme::NEUTRAL),
    ("Ocean", Theme::OCEAN),
    ("Violet", Theme::VIOLET),
    ("Sunset", Theme::SUNSET),
    ("Rose", Theme::ROSE),
    ("High contrast", Theme::CONTRAST),
];

fn themes_dark() -> Vec<(&'static str, Element)> {
    BUILT_IN
        .iter()
        .map(|(name, theme)| (*name, sample(theme, Mode::Dark)))
        .collect()
}

fn themes_light() -> Vec<(&'static str, Element)> {
    BUILT_IN
        .iter()
        .map(|(name, theme)| (*name, sample(theme, Mode::Light)))
        .collect()
}

fn themes_modes() -> Row {
    Row::gap(4).wrap().children((
        sample(&Theme::OCEAN, Mode::Light),
        sample(&Theme::OCEAN, Mode::Dark),
        sample(&Theme::OCEAN, Mode::System),
    ))
}

fn themes_menu() -> ThemeMenu {
    ThemeMenu::new(
        &Appearance::new(Theme::OCEAN, Mode::System),
        &Theme::BUILT_IN,
    )
    .back("/deals")
}

/// The docs' app, with a theme menu in the top bar.
fn acme() -> Shell {
    Shell::new("Acme CRM")
        .context("Production")
        .theme_menu()
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(
                    NavItem::new("Deals", "/deals")
                        .icon(icons::HANDSHAKE)
                        .count("312"),
                )
                .item(NavItem::new("Contacts", "/contacts").icon(icons::USERS))
                .item(
                    NavItem::new("Reports", "/reports")
                        .icon(icons::CHART_COLUMN)
                        .sub(NavItem::new("Pipeline", "/reports/pipeline"))
                        .sub(NavItem::new("Forecast", "/reports/forecast")),
                ),
        )
        .user(UserMenu::new("Ada Lovelace", "ada@example.com").action("Sign out", "/sign-out"))
}

fn deals(page: AppPage) -> AppPage {
    page.actions(
        Button::primary("New deal")
            .icon(icons::PLUS)
            .href("/deals/new"),
    )
    .body(Grid::cols(3).gap(4).children((
        Stat::new("Open deals", "312").trend("+12 this week", true),
        Stat::new("Pipeline value", "$4.2M").trend("+3.1%", true),
        Stat::new("Won this month", "$640K").trend("-2.4%", false),
    )))
}

fn shell_inset() -> AppPage {
    let shell = acme().layout(ShellLayout::Inset);
    deals(shell.page("Deals", "/deals"))
}

fn shell_floating() -> AppPage {
    let shell = acme().layout(ShellLayout::Floating);
    deals(shell.page("Deals", "/deals"))
}

fn shell_header() -> AppPage {
    let shell = acme().layout(ShellLayout::Header);
    deals(shell.page("Deals", "/deals"))
}

fn builder_default() -> AppPage {
    let draft = BuilderDraft::from_query("base=ocean&mode=dark");
    ThemeBuilder::new(&draft, "/settings/theme")
        .save("/settings/theme/save")
        .import("/settings/theme/import")
        .page(acme().page("Theme builder", "/settings/theme"))
}
