# @navree/theme

The Navree look for your app: near-black ink surfaces, off-white text in four steps, one green accent and three self-hosted faces. It gives Tailwind CSS 4 tokens, shadcn/ui and shadcn-vue colors, every built-in theme in light and dark, and the tokens as data.

## Install

```sh
npm install @navree/theme
```

## Quick start

```css
@import "tailwindcss";
@import "@navree/theme/theme.css";
@import "@navree/theme/fonts.css";
```

Put `class="dark"` on `<html>`: the theme declares the `dark` variant on that class, so the `dark:` classes that shadcn components use apply.

## Other themes and light mode

Import `themes.css` after `theme.css`, and set `data-theme` and `data-mode` on `<html>`. These are the seven themes Navree UI has, with the same colors:

```css
@import "@navree/theme/themes.css";
```

`applyAppearance` sets both attributes and keeps the `dark` class in step. In `system` mode it follows the visitor's system until you call the function it returns.

```ts
import { applyAppearance } from "@navree/theme";

applyAppearance(document.documentElement, { theme: "ocean", mode: "system" });
```

`appearanceScript()` returns a small script for `<head>` that applies a stored choice before the first paint, so the page does not flash. A theme made in Navree UI's theme builder works here too: `themeFromBuilderCss(css)` renames its colors to the ones Tailwind uses.

## Files

| File | What it is |
|---|---|
| `theme.css` | The Tailwind CSS 4 preset: tokens, shadcn semantic colors, radii, base styles. |
| `themes.css` | The built-in themes (`navree`, `neutral`, `ocean`, `violet`, `sunset`, `rose`, `contrast`) in light and dark, chosen by `data-theme` and `data-mode`. |
| `fonts.css` | `@font-face` rules for the three faces and their font variables. |
| `tokens.css` | The tokens as plain custom properties, for pages without Tailwind. |
| `registry/navree.json` | A shadcn registry theme for the shadcn CLI. |

Documentation: https://navree.com/docs/frontend/theming/

## License

MIT OR Apache-2.0
