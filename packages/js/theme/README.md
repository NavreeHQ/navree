# @navree/theme

The Navree look for your app: near-black ink surfaces, off-white text in four steps, one green accent and three self-hosted faces. It gives Tailwind CSS 4 tokens, shadcn/ui and shadcn-vue colors, and the tokens as data.

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

## Files

| File | What it is |
|---|---|
| `theme.css` | The Tailwind CSS 4 preset: tokens, shadcn semantic colors, radii, base styles. |
| `fonts.css` | `@font-face` rules for the three faces and their font variables. |
| `tokens.css` | The tokens as plain custom properties, for pages without Tailwind. |
| `registry/navree.json` | A shadcn registry theme for the shadcn CLI. |

Documentation: https://navree.com/docs/frontend/theming/

## License

MIT OR Apache-2.0
