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
| `chart-1` to `chart-5` | `#3dff8f`, `#6cb6ff`, `#b79cff`, `#ffbf5e`, `#ff8aa8` | `#0b7a3e`, `#0b5cad`, `#6b46c1`, `#8a5a00`, `#b3254f` | chart series |
| `radius` | `4px` | `4px` | controls: buttons, inputs, badges |
| `radius-lg` | `6px` | `6px` | surfaces: cards, dialogs, tables |
| `sidebar-w` | `16rem` | `16rem` | the open sidebar |
| `sidebar-w-icon` | `3rem` | `3rem` | the sidebar as icons |
| `topbar-h` | `3rem` | `3rem` | the top bar |
| `page-pad` | `1.5rem` | `1.5rem` | the page's padding |
| `font-sans` | Geist, then the system stack | | all text |
| `font-mono` | the system monospace stack | | code |

Control heights are fixed by `Size`: 24, 28, 32 (the default) and 36
pixels. Gaps in `Stack`, `Row` and `Grid` are steps, not pixels. There
are no shadows on app surfaces and no glow.

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
| chart series | `chart-1` to `chart-5` (neutral bars: `bar`) |
| control corners | `radius` (4 px) |
| card and dialog corners | `radius-lg` (6 px) |
| body and heading font | `font-sans` (Geist, or the theme's system stack) |
| code font | `font-mono` |

For each `DT-` row, write one of:

- **Same:** the value equals the token, or differs by less than the eye
  can see beside it (compare the two swatches in one screenshot).
- **Mapped:** a different value with the same role. In translate mode this
  is fine. In keep mode it is a deviation (`DV-`), with both swatches, or a
  theme of the app's own (below).
- **Gap:** a role a theme cannot hold: a gradient, a font outside the list
  (Geist and the System, Humanist, Geometric and Serif stacks), a colour
  token the table does not have. It goes to `GAPS.md`.
- **Refused:** a value that breaks the app rules: corners other than 4 or
  6 px, pill shapes, glow or other shadows on surfaces, letter tiles. It
  becomes a deviation with the Navree form, for the owner to approve. The
  theme builder offers other corner sizes, so a custom theme can hold them
  once the owner approves the deviation.

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
   the corners, the density and a font, or any of the 28 colour tokens in
   light and dark, shows real components in both palettes and a contrast
   report, and exports a Rust function returning a `Theme`. It refuses text
   that fails WCAG AA, so a design pair that fails stays a finding for the
   owner (see "Contrast"). Give the result to `Shell::appearance` or
   `Shell::themes`.
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
| DT-12 | 9999px | 18 | .chip, .avatar | chip corners | radius | refused (DV-03): no pills | source/app.css:88 | file | high | confirmed |
| DT-15 | "Inter" | 1 | body | body font | font-sans | mapped (DV-04) | source/app.css:1 | file | high | question Q-02 |
| DT-20 | #ffffff on #e2e8f0 | 3 | .tag-muted | tag text | fg-2 on ink-3 | design pair 1.2:1 fails | source/app.css:120 | file | high | confirmed |

## Questions
## Deviations
## Gaps
```
