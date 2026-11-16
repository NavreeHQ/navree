//! `/shell/options`: a shell that uses the options of its brand row, its foot, its top bar and
//! its page header, with a workspace switcher drawn as a card, on a theme that sets the sizes,
//! type and colours they read. `?mode=dark` draws it dark; `?mono` draws the title and the
//! breadcrumb's last part in the mono face; `?icons` lets the sidebar fold to its icons. The browser suite measures what it draws
//! (`e2e/ui/tests/shell.test.mjs`).

use std::sync::Arc;

use http::{HeaderMap, Method};
use navree::http::extract::RawQuery;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::theme::{
    Appearance, Corner, Corners, Elevation, Face, Mode, Rgb, Roles, Shape, SidebarSizes, Sizes,
    TextStyle, Theme, ThemeId, Token, TopbarSizes, TypeScale,
};
use navree::ui::*;
use navree::workspace::{SwitcherVariant, WorkspaceSwitcher};

use crate::app_blocks::MARK;
use crate::themes::DISPLAY;

/// The proof app's mark for a dark sidebar: the same face on a lighter disc.
pub static MARK_DARK: AppImage = AppImage::new(
    "proof-mark-dark",
    include_bytes!("../assets/mark-dark.svg"),
    4 * 1024,
);

/// A theme with what the shell's options read: a sidebar 248 px wide with 16 and 12 px insets,
/// 18 px between its blocks and 36 px links; a top bar 60 px high; badges 22 px high and pill;
/// 8 px avatar corners, 12 px fields, 10 px rows; an 18 px display brand, a 13 px user name,
/// a 14 px breadcrumb whose last part is 500, a 28 px page title; white cards on a tinted
/// sidebar and the shell's muted text in its own colour.
pub fn shell_theme() -> Theme {
    let rgb = Rgb::hex;
    let mut spec = *Theme::NEUTRAL.spec();
    spec.light.set(Token::Ink1, rgb(0xfafaf9));
    let shape = Shape::new()
        .corners(
            Corners::new()
                .button(Corner::Pill)
                .badge(Corner::Pill)
                .avatar(Corner::Px(8))
                .field(Corner::Px(12))
                .nav(Corner::Px(10))
                .card(Corner::Px(16)),
        )
        .type_scale(
            TypeScale::new()
                .page_title(
                    TextStyle::px(28)
                        .line(1.15)
                        .weight(600)
                        .tracking(-0.025)
                        .face(Face::Display),
                )
                .brand(
                    TextStyle::px(18)
                        .line(1.2)
                        .weight(600)
                        .tracking(-0.03)
                        .face(Face::Display),
                )
                .sidebar_user(TextStyle::px(13).weight(500))
                .breadcrumb(TextStyle::px(14).weight(500))
                .badge(TextStyle::px(11).weight(500)),
        )
        .sizes(
            Sizes::new()
                .badge_height(22)
                .sidebar(
                    SidebarSizes::new()
                        .width(248)
                        .inset_y(16)
                        .inset_x(12)
                        .gap(18)
                        .item_height(36),
                )
                .topbar(TopbarSizes::new().height(60).inset_x(32)),
        )
        .elevation(Elevation::Soft);
    let roles = Roles::new()
        .card(rgb(0xffffff), rgb(0x121212))
        .field(rgb(0xffffff), rgb(0x121212))
        .shell_muted(rgb(0x5e5e5a), rgb(0xb7b7b2));
    Theme::custom(
        ThemeId::parse("proof-shell").expect("a theme id"),
        "Shell",
        spec,
    )
    .and_then(|theme| theme.with_shape(shape))
    .and_then(|theme| theme.with_roles(roles))
    .expect("the shell theme reads and its values are in range")
}

/// The page: every option of the shell, and avatars of initials in the body.
fn page(headers: &HeaderMap, query: Option<&str>) -> AppPage {
    let asks = |word: &str| query.is_some_and(|query| query.split('&').any(|pair| pair == word));
    let mono = asks("mono");
    let path = match query {
        Some(query) if !query.is_empty() => format!("/shell/options?{query}"),
        _ => "/shell/options".to_owned(),
    };
    let switcher = WorkspaceSwitcher::new("Narmada Roadways")
        .variant(SwitcherVariant::Card)
        .media(Avatar::initials("Narmada Roadways").solid())
        .description("Ludhiana · Admin")
        .workspace_with(
            "narmada",
            "Narmada Roadways",
            true,
            Avatar::initials("Narmada Roadways").solid().size(Size::Sm),
        )
        .workspace_with(
            "gulati",
            "Gulati Exports",
            false,
            Avatar::initials("Gulati Exports")
                .tone(Tone::Success)
                .size(Size::Sm),
        )
        .label("Organisations")
        .next(path.clone());
    let seats = Card::new().body(Stack::gap(2).children((
        Stat::new("Seats", "12 of 20"),
        Button::primary("Add seats").size(Size::Sm),
    )));
    let shell = Shell::new("Kesar")
        .font(&DISPLAY)
        .logo(&MARK)
        .logo_dark(&MARK_DARK)
        .logo_style(LogoStyle::Bare)
        // No fold button, unless the page is asked to fold to its icons.
        .collapse(if asks("icons") {
            Collapse::Icons
        } else {
            Collapse::Never
        })
        .appearance(Appearance::new(shell_theme(), Mode::Light))
        .group(
            NavGroup::plain()
                .item(NavItem::new("Overview", "/shell/overview").icon(icons::LAYOUT_GRID))
                .item(
                    NavItem::new("Depots", "/shell/options")
                        .icon(icons::BUILDING_COMPLEX)
                        .count("3"),
                ),
        )
        .for_request(path, headers)
        .badge(Badge::new("Internal").variant(BadgeVariant::Solid))
        .header_slot(switcher)
        .footer_slot(seats)
        .user_menu(
            UserMenu::new("Dalbir Oberoi", "Operations")
                .avatar(Avatar::initials("Dalbir Oberoi").shape(AvatarShape::Circle))
                .chevron(false)
                .action("Sign out", "/sign-out"),
        );
    let mut page = shell
        .page(if mono { "INV-2041" } else { "LDH-1, Ludhiana" })
        .crumb("Depots", "/shell/options")
        .current_crumb(if mono { "INV-2041" } else { "LDH-1" })
        .title_aside((
            Badge::new("Paid").variant(BadgeVariant::Success),
            Badge::new("Annual"),
        ))
        .subtitle("Wholesale · Gulati Exports · Ludhiana")
        .topbar_actions((
            Button::ghost("Download invoice"),
            Button::outline("Pause account"),
            Button::primary("Close account"),
        ))
        .actions(Button::outline("Export CSV"))
        .body(
            Card::titled("People").body(
                Row::gap(2).children((
                    Avatar::initials("Dalbir Oberoi").shape(AvatarShape::Circle),
                    Avatar::initials("Harleen Kaur")
                        .tone(Tone::Info)
                        .shape(AvatarShape::Circle),
                    Avatar::initials("ਗੁਰਪ੍ਰੀਤ ਸਿੰਘ")
                        .tone(Tone::Success)
                        .shape(AvatarShape::Circle),
                    Avatar::initials("Narmada Roadways").solid(),
                )),
            ),
        );
    if mono {
        page = page.title_face(Face::Mono).current_crumb_mono();
    }
    page
}

pub struct ShellPages;

impl Injectable for ShellPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for ShellPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/shell/options",
                |headers: HeaderMap, RawQuery(query): RawQuery| async move {
                    page(&headers, query.as_deref())
                },
            )
            .public(),
        ]
    }
}
