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
