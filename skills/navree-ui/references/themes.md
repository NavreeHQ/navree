# Themes, icons and flags

How a page looks in light, dark and the device's mode: the seven built-in themes, the theme menu and
the shell layouts; then icons and flags. Each block compiles on its own.

## Themes

A theme is a typed value: a palette for light and one for dark, corners, a
density and a font from a safe list. The server writes the reader's theme and
mode on `<html>`, so the first paint is right. Seven themes are built in:
`Theme::NAVREE` (the default, green on near-black or on white),
`NEUTRAL`, `OCEAN`, `VIOLET`, `SUNSET`, `ROSE` and `CONTRAST`. `Mode::Light`,
`Mode::Dark` and `Mode::System` are chosen apart from the theme; System
follows the device with no script. Every built-in theme passes WCAG AA in
both modes.

```rust
use navree::ui::theme::{Appearance, Mode, Theme};
use navree::ui::{Collapse, NavGroup, NavItem, Shell, ShellLayout, icons};

pub fn themed_shell() -> Shell {
    Shell::new("Sutlej CRM")
        .appearance(Appearance::new(Theme::OCEAN, Mode::System))
        .theme_menu()
        .layout(ShellLayout::Inset)
        .collapse(Collapse::Icons)
        .group(NavGroup::new("Sales").item(NavItem::new("Contacts", "/contacts").icon(icons::USERS)))
}
```

- `Shell::appearance` sets the app's default. A workspace
  (`shell.workspace(ThemeSetting)`, `.locked()` keeps its theme for
  everyone), the reader (the theme menu's cookie, or `shell.user_theme(..)` from
  your store) and `?theme=ocean&mode=light` lie over it, in that order.
- `Shell::theme_menu()` puts the Appearance button in the top bar. Its
  default route keeps the choice in a cookie; `ThemeMenu::new(&current,
  &themes).action(url)` posts to your own.
- `Shell::layout` takes `ShellLayout::Sidebar` (the default), `Inset`,
  `Floating` or `Header`. `Shell::collapse` takes `Collapse::Icons`,
  `Offcanvas` or `Never`.
- A theme of your own: build it in the theme builder
  (https://navree.com/docs/ui/theme-builder/) and paste its Rust export, or
  call `Theme::custom`, which refuses text that fails WCAG AA.
- What a theme says beyond its spec is its `Shape`, given with
  `theme.with_shape(shape)`, which refuses a value out of range (and a
  built-in theme: `renamed` makes it yours). `Corners` sets the corner of
  each kind of part, `Corner::Px(0..=32)` or `Corner::Pill`: `button`,
  `badge`, `toggle`, `avatar`, `field`, `nav`, `card`, `panel` and
  `overlay`. A part it doesn't set keeps the spec's `Radius`. Never write a
  stylesheet that restyles a component: a value the design needs goes in
  the theme.
- `TypeScale` in the shape sets each kind of text:
  `.type_scale(TypeScale::new().page_title(TextStyle::px(28).line(1.15)
  .weight(600).tracking(-0.025).face(Face::Display)))`, and the same for
  `h1` to `h4`, `card_title`, `brand`, `body`, `label`, `button`, `badge`,
  `input`, `breadcrumb`, `stat_value`, `table_head` and `sidebar_user`.
  Sizes 10 to 72 px, line heights 1 to 2, weights 400 to 700, tracking
  -0.06 to 0.1 em. A face of the app's own is an `AppFont` with
  `.as_face(Face::Text | Face::Display | Face::Mono)`, linked with
  `Shell::font`; headings to `h3`, the page title and the brand draw in the
  display face when there is one.
- `Sizes` in the shape sets heights and spacing in px:
  `Sizes::new().controls(ControlSizes::new().sm(32).md(36).lg(48))
  .fields(FieldSizes::new().md(40).lg(44).padding_x(14)).badge_height(22)
  .sidebar(SidebarSizes::new().width(248).inset_y(16).inset_x(12).gap(18)
  .item_height(36)).topbar(TopbarSizes::new().height(60).inset_x(32))
  .page(PageSizes::new().padding_top(28).padding_x(32).padding_bottom(40)
  .gap(24).max_width(1240)).cards(CardSizes::new().padding(18))`. A field
  takes a step with `Input::size(Size::Lg)`, so a sign-in page can draw
  44 px fields while the app draws 40.
- `Shape::elevation(Elevation::Soft)` gives two soft shadows: under raised
  parts (the chosen segmented tab, the current link, a toggle that is on)
  and under floating ones (popovers, menus, dialogs). Flat is the default;
  there is no glow.
- Colours beyond the 31 tokens are `Roles`, given with
  `theme.with_roles(roles)`, each in light then dark: `card` and `field`
  (white cards and inputs beside an off-white sidebar, which keeps
  `ink-1`), `ghost_text`, `shell_muted`, `segment_track`, `segment_on`,
  `selected_row`, and `inverse(Inverse::new().surface(..).raised(..)
  .line(..).text(..).muted(..))`, the colours of the inverse surface: a
  part drawn on the theme's other palette, such as a dark card on a light
  page. A role whose text fails WCAG AA is refused, in light or in dark.

```rust
use navree::ui::theme::{Corner, Corners, Shape, Theme, ThemeId};

pub fn rounded_theme() -> Theme {
    let shape = Shape::new().corners(
        Corners::new()
            .button(Corner::Pill)
            .badge(Corner::Pill)
            .field(Corner::Px(12))
            .card(Corner::Px(16)),
    );
    Theme::OCEAN
        .renamed(ThemeId::parse("rounded").expect("a theme id"), "Rounded")
        .with_shape(shape)
        .expect("every value of the shape in range")
}
```
- Store a choice with `Appearance::to_value()` and read it back with
  `Appearance::parse`. Charts and status read the tokens `--chart-1` to
  `--chart-8`, `--success`, `--amber`, `--danger` and `--info`; see
  [Foundations](https://navree.com/docs/ui/foundations/) and
  `references/tokens.md` of `navree-design-to-ui`.

## Icons and flags

Icons are lucide's set, a constant per icon (`icons::PLUS`, `icons::USERS`),
so the binary holds the ones the code names. A flag is a constant too
(`flags::IN`). Your own SVG files become icons with `icon_file!` and
`icon_set!`, which refuse anything but shapes. An icon-only button needs a
name: `Button::icon_only(icons::PLUS, "New contact")`.
