# Tokens: from the design's values to Navree's

Phase 2, by the `token-mapper`. A design holds hundreds of raw values. A
Navree page uses one table of named tokens, `navree::ui::tokens::TOKENS`
(`crates/navree-ui/src/tokens.rs`), written to the stylesheet as CSS
custom properties. This file explains the table and how to map a design
onto it.

## What Navree has

Navree UI has themes. Seven ship in the stylesheet (`Theme::NAVREE`,
`NEUTRAL`, `OCEAN`, `VIOLET`, `SUNSET`, `ROSE` and `CONTRAST`), each with a
light and a dark palette, and a mode (`Mode::Light`, `Mode::Dark` or
`Mode::System`, which follows the device) picks between them. A page with
nothing set is dark Navree. An app sets its theme on the shell
(`Shell::appearance(Appearance::new(Theme::OCEAN, Mode::System))`), and a
workspace or a person can lay a choice of their own over it. An app can
make a theme of its own: colours, corners, density and a font, built in the
theme builder or with `Theme::custom` (see "Themes" below).

The table is the Navree theme, the one a page gets by default. Every other
theme sets the same tokens to other values.

| Token | Dark | Light | Use |
|---|---|---|---|
| `ink-0` | `#040605` | `#ffffff` | the page behind everything |
| `ink-1` | `#0a0e0c` | `#f7f8f7` | surfaces: cards, the sidebar |
| `ink-2` | `#111714` | `#eff1f0` | raised parts, hover |
| `ink-3` | `#16201b` | `#e5e8e6` | selected, pressed |
| `line` | `#1c2420` | `#dfe3e0` | borders |
| `line-2` | `#2a3530` | `#c8cecb` | stronger borders, inputs |
| `line-3` | `#3a4741` | `#aeb6b2` | the strongest borders |
| `fg` | `#e9efeb` | `#0a0e0c` | text |
| `fg-1` | `#cfd8d3` | `#1c2420` | text, one step down |
| `fg-2` | `#a9b5ae` | `#3f4a44` | secondary text |
| `fg-3` | `#8a968f` | `#55615b` | hints, placeholders |
| `accent` | `#3dff8f` | `#0b7a3e` | the one accent: primary buttons, focus, links |
| `accent-ink` | `#03130a` | `#ffffff` | text on the accent |
| `accent-text` | `#3dff8f` | `#0b763c` | the accent as text: a link, a sorted column, a chosen item |
| `ring` | `#3dff8f` | `#0b7a3e` | the focus ring |
| `success` | `#3dff8f` | `#096835` | good news |
| `amber` | `#ffbf5e` | `#7e5200` | warnings |
| `danger` | `#ff6b6b` | `#9e2119` | errors, destructive actions |
| `info` | `#6cb6ff` | `#0b5cad` | information |
| `violet`, `cyan`, `rose` | `#b79cff`, `#4fd6e6`, `#ff8aa8` | `#6b46c1`, `#0a6b78`, `#b3254f` | tones for labels |
| `bar` | `#5b6862` | `#8a968f` | neutral chart bars |
| `chart-1` to `chart-8` | `#3987e5`, `#d95926`, `#199e70`, `#c98500`, `#d55181`, `#008300`, `#9085e9`, `#e66767` | `#2a78d6`, `#dc5a24`, `#029566`, `#b07702`, `#ca6089`, `#008300`, `#6250d6`, `#e34948` | chart series, in the order series take them |
| `radius` | `4px` | `4px` | controls: buttons, inputs, badges |
| `radius-lg` | `6px` | `6px` | surfaces: cards, dialogs, tables |
| `radius-button` to `radius-overlay` | unset | unset | a kind of part's own corner, from a theme's shape (below) |
| `card`, `field` | unset (`ink-1`) | unset (`ink-1`) | cards and fields apart from the sidebar: a theme's roles (below) |
| `ghost-fg`, `shell-muted` | unset (`fg-2`, `fg-3`) | unset | a ghost button's text; the sidebar's quieter text |
| `segment-track`, `segment-on`, `row-selected` | unset (`ink-2`, `ink-3`) | unset | the track and chosen item of segmented tabs and of a segmented button group; a selected row |
| `sidebar-w` | `16rem` | `16rem` | the open sidebar; a theme's `SidebarSizes::width` |
| `sidebar-w-icon` | `3rem` | `3rem` | the sidebar as icons |
| `topbar-h` | `3rem` | `3rem` | the top bar; a theme's `TopbarSizes::height` |
| `page-pad` | `1.5rem` | `1.5rem` | the page's padding |
| `control-xs` to `control-lg`, `field-xs` to `field-lg`, `field-px`, `badge-h`, `sidebar-pad-y`, `sidebar-pad-x`, `sidebar-gap`, `sidebar-item-h`, `topbar-px`, `page-pad-top`, `page-pad-x`, `page-pad-bottom`, `page-gap`, `page-max`, `card-pad`, `stat-pad-y`, `stat-pad-x` | unset | unset | sizes in px from a theme's `Sizes` (below) |
| `font-sans` | Geist, then the system stack | | all text |
| `font-display` | unset: the text face | | headings to `h3`, the page title, the brand: an `AppFont` with `as_face(Face::Display)` |
| `font-mono` | the system monospace stack | | code |
| `text-<kind>-size`, `-line`, `-weight`, `-tracking`, `-face` | unset | unset | each kind of text, from a theme's `TypeScale` (below) |

Control heights come from `Size`: 24, 28, 32 (the default) and 36 pixels,
unless a theme's shape says what each step is (`Sizes`, below). Gaps in
`Stack`, `Row` and `Grid` are steps, not pixels. Surfaces are flat by
default; a theme's `Elevation::Soft` gives raised and floating parts two
soft shadows (`--shadow-raised`, `--shadow-floating`). There is no glow.

`navree::ui::theme::pairs()` lists every colour pair the stylesheet draws:
text must reach WCAG AA (4.5:1), and the focus ring, icons and chart colours
3:1. `ThemeSpec::shortfalls` lists the pairs a theme misses, in light and
dark, and a test holds every built-in theme to none.

## Extracting the design's values

From HTML and CSS, collect every value where it is written: CSS custom
properties, `<style>` blocks, stylesheets, `style=""` attributes, utility
classes with literal values (`bg-[#1a2b3c]`), and SVG fills. From a
design-tool export, take its styles and variables (colour styles, text
styles, effect styles), and sample the frames' screenshots only for what
has no style. Keep the command you ran and its count.

```sh
rg -o --no-filename "#[0-9a-fA-F]{3,8}\b|rgba?\([^)]*\)|hsla?\([^)]*\)|oklch\([^)]*\)" design/source | sort | uniq -c | sort -rn
rg -o --no-filename "border-radius:\s*[^;]+|box-shadow:\s*[^;]+|font-family:\s*[^;]+" design/source | sort | uniq -c | sort -rn
```

Each distinct value is a `DT-` row with its count and where it appears.

## Mapping

Group the values by role, then give each role its Navree token:

| Design role | Navree token |
|---|---|
| page background | `ink-0` |
| card, panel, sidebar | `ink-1` |
| cards and inputs apart from the sidebar (white cards on an off-white sidebar) | `card`, `field`: a theme's `Roles`; the sidebar and a table's header keep `ink-1` |
| a dark card, band, code panel or side panel on a light page (or the other way) | the inverse surface: `Inverse` in a theme's `Roles`, drawn by `CardVariant::Inverse`, `StatVariant::Inverse` and `CodeBlock::inverse()` |
| a ghost or icon button's text, the sidebar's secondary lines, a segment's track and chosen tab, a selected row or the row a page shows (`DataTable::current_row`) | `ghost-fg`, `shell-muted`, `segment-track`, `segment-on`, `row-selected`: a theme's `Roles` |
| control and field heights (40 px buttons, 44 px inputs on one page), a badge's height | a theme's `Sizes`: `ControlSizes`, `FieldSizes` with `Input::size` for a page's step (`#[form(size = lg)]` for a derived form's fields), `badge_height` |
| the sidebar's width and insets, the top bar's height, the page's padding and width, card padding | a theme's `Sizes`: `SidebarSizes`, `TopbarSizes`, `PageSizes`, `CardSizes` |
| a soft shadow under a chosen segment, the current link, a popover or a dialog | a theme's `Elevation::Soft`: `shadow-raised`, `shadow-floating` |
| hover, raised | `ink-2` |
| selected, pressed | `ink-3` |
| borders, dividers, input outlines | `line`, `line-2`, `line-3` |
| text, strongest to faintest | `fg`, `fg-1`, `fg-2`, `fg-3` |
| brand or primary colour | `accent` (text on it: `accent-ink`; the accent as text: `accent-text`) |
| focus ring | `ring` |
| success | `success`, through `BadgeVariant::Success`, `AlertTone::Success` |
| warning | `amber` |
| error, destructive | `danger` |
| information | `info` |
| label colours | `violet`, `cyan`, `rose` |
| chart series | `chart-1` to `chart-8` (neutral bars: `bar`) |
| control corners | `radius` (4 px) |
| card and dialog corners | `radius-lg` (6 px) |
| one kind of part's corners: buttons, badges, toggles and segmented tabs, avatars, fields, nav and menu rows, cards, panels, popups | `radius-button`, `radius-badge`, `radius-toggle`, `radius-avatar`, `radius-field`, `radius-nav`, `radius-card`, `radius-panel`, `radius-overlay`: a theme's `Corners` |
| body and heading font | `font-sans` (Geist, or the theme's system stack, or an `AppFont` with `as_face(Face::Text)`) |
| a second face for headings, the page title and the wordmark | `font-display`: an `AppFont` with `as_face(Face::Display)` |
| code font, figures | `font-mono`, or an `AppFont` with `as_face(Face::Mono)` |
| the size, line height, weight and tracking of a kind of text (page title 28 px, card titles 15 px, stat figures in mono) | a theme's `TypeScale`: `page_title`, `h1` to `h4`, `card_title`, `brand`, `body`, `label`, `button`, `badge`, `input`, `breadcrumb`, `stat_value`, `table_head`, `sidebar_user` |

For each `DT-` row, write one of:

- **Same:** the value equals the token, or differs by less than the eye
  can see beside it (compare the two swatches in one screenshot).
- **Mapped:** a different value with the same role. In translate mode this
  is fine. In keep mode it is a deviation (`DV-`), with both swatches, or a
  theme of the app's own (below).
- **Gap:** a role a theme cannot hold: a gradient, a font outside the list
  (Geist and the System, Humanist, Geometric and Serif stacks, or a woff2
  file of the app's own as an `AppFont`), a colour for a thing neither a
  token nor a role paints. It goes to `GAPS.md`.
- **Theme:** a corner other than 4 or 6 px, a pill, a 16 px card, white
  cards beside an off-white sidebar, a dark card on a light page. A theme of
  the app's own holds it: its shape's `Corners` sets a corner for each kind
  of part, from 0 to 32 px or a pill (`Corner::Px(16)`, `Corner::Pill`), and
  its `Roles` a colour for cards, fields and the other things one token
  paints, and the inverse surface. Record it as a mapping to the theme's
  value. `with_roles` refuses a role whose text fails WCAG AA, as the
  palette's own check does.
- **Refused:** a value that breaks the app rules: glow, a shadow other than
  the theme's two soft levels (`Elevation::Soft`: a small one under the
  chosen segment and the current link, a wider one under popovers, menus
  and dialogs). It becomes a deviation with the Navree form,
  for the owner to approve.

## Themes

A design that is light, or has its own brand colours, is not a gap. Pick the
nearest of three ways, in this order, and record the choice as a mapping or a
deviation:

1. **A built-in theme and a mode.** `Theme::NAVREE`, `NEUTRAL`, `OCEAN`,
   `VIOLET`, `SUNSET`, `ROSE` or `CONTRAST`, and `Mode::Light`,
   `Mode::Dark` or `Mode::System`. Compare the theme beside the design in
   one screenshot, as for any token.
2. **A theme of the app's own.** The theme builder
   (https://navree.com/docs/ui/theme-builder/) takes an accent, the greys,
   the corners, the density and a font, or any of the 31 colour tokens in
   light and dark, shows real components in both palettes and a contrast
   report, and exports a Rust function returning a `Theme`. It refuses text
   that fails WCAG AA, so a design pair that fails stays a finding for the
   owner (see "Contrast"). Give the result to `Shell::appearance` or
   `Shell::themes`. What the design says beyond the spec goes on that
   function with `Theme::with_shape`: a `Shape` holding the corners of each
   kind of part.

   ```rust
   use navree::ui::theme::{Corner, Corners, Shape, Theme, ThemeId};

   pub fn app_theme() -> Theme {
       let shape = Shape::new().corners(
           Corners::new()
               .button(Corner::Pill)
               .badge(Corner::Pill)
               .field(Corner::Px(10))
               .card(Corner::Px(16)),
       );
       Theme::NAVREE
           .renamed(ThemeId::parse("sutlej").expect("a theme id"), "Sutlej")
           .with_shape(shape)
           .expect("every value of the shape in range")
   }
   ```
3. **A theme per workspace or per person.** `shell.workspace(ThemeSetting)`
   (`.locked()` keeps it for everyone in the workspace) and `shell.user_theme(..)`
   lay a choice over the app's. `Shell::theme_menu()` lets a reader choose.

The shell can also take other layouts: `Shell::layout` with
`ShellLayout::Sidebar`, `Inset`, `Floating` or `Header`, and
`Shell::collapse` with `Collapse::Icons`, `Offcanvas` or `Never`.

## Contrast

Check every text and background pair the design uses, after mapping, with
the same function the toolkit's own test uses:

```rust
use navree::ui::theme::Theme;
use navree::ui::tokens::{contrast, dark};

/// Every pair below 4.5:1, with its ratio: each is a finding.
pub fn low_contrast(pairs: &[(&str, &str)]) -> Vec<(String, String, f64)> {
    pairs
        .iter()
        .filter_map(|&(text, background)| {
            let ratio = contrast(dark(text)?, dark(background)?)?;
            (ratio < 4.5).then(|| (text.to_owned(), background.to_owned(), ratio))
        })
        .collect()
}

/// The design's own colours, before mapping: `contrast("#9aa3a0", "#ffffff")`.
pub fn design_ratio(text: &str, background: &str) -> Option<f64> {
    contrast(text, background)
}

/// Navree's own pairs all pass, in light and dark.
pub fn navree_pairs_pass() -> bool {
    Theme::NAVREE.spec().shortfalls().is_empty()
}
```

A design pair that fails before mapping is a finding to show the owner
even when its Navree token passes: the design asked for something
unreadable.

## The file

`02-tokens/TOKENS.md`:

```text
# Tokens

Command: rg -o ... design/source | sort | uniq -c   (41 distinct colours)

| ID | Value | Count | Where | Role | Navree | Verdict | Source | Known by | Conf. | Status |
| DT-01 | #0b0d0c | 212 | body, .app-bg | page background | ink-0 | same | source/app.css:3 | file | high | confirmed |
| DT-07 | #22c55e | 64 | .btn-primary, a | primary | accent | mapped (DV-02) | source/app.css:40 | file | high | confirmed |
| DT-12 | 9999px | 18 | .chip, .avatar | chip corners | radius-badge | theme: `Corners::badge(Corner::Pill)` | source/app.css:88 | file | high | confirmed |
| DT-15 | "Inter" | 1 | body | body font | font-sans | mapped (DV-04) | source/app.css:1 | file | high | question Q-02 |
| DT-20 | #ffffff on #e2e8f0 | 3 | .tag-muted | tag text | fg-2 on ink-3 | design pair 1.2:1 fails | source/app.css:120 | file | high | confirmed |

## Questions
## Deviations
## Gaps
```
