# Navree design system

One page for everyone building navree.com. The tokens live in
`src/app/globals.css`, the fonts in `src/app/fonts.ts`, the primitives in
`src/components/ui/` (shadcn/ui) and the motion pieces in
`src/components/magicui/` (Magic UI). All restyled to the brand: neon green
on black, dark only. `/styleguide/` shows the docs components.

## 1. Color

| Token (Tailwind name) | Value | Use |
|---|---|---|
| `ink-0` | `#040605` | Page background |
| `ink-1` | `#0a0e0c` | Cards, panels, code, the footer of a card |
| `ink-2` | `#111714` | Hover surfaces, chips, inputs |
| `ink-3` | `#16201b` | Active tab, raised chip, tooltip |
| `line` | `#1c2420` | Every 1px border and divider (the default border color) |
| `line-2` | `#2a3530` | Stronger borders: buttons, inputs, diagram strokes |
| `fg` | `#e9efeb` | Headings, primary text |
| `fg-1` | `#cfd8d3` | Strong secondary text |
| `fg-2` | `#a9b5ae` | Body copy on marketing pages, descriptions |
| `fg-3` | `#8a968f` | Captions, metadata, eyebrows (still AA on every ink) |
| `accent` | `#3dff8f` | **Emphasis only**: the primary button, links, focus, the logo, the one highlighted element in a diagram |
| `accent-ink` | `#03130a` | Text on an accent fill |
| `amber` | `#ffbf5e` | Warnings and the "In development" badge only |
| `danger` | `#ff6b6b` | Errors (shadcn `destructive`) |

Green tints for glows and fills: `bg-accent/10`, `border-accent/25`,
`rgb(61 255 143 / 0.x)`. Never purple, blue, pink or rainbow gradients.

shadcn semantic names are mapped onto the same tokens, so both work:
`background`=ink-0, `card`/`popover`=ink-1, `muted`/`secondary`=ink-2,
`muted-foreground`=fg-2, `primary`=accent, `border`=line, `input`=line-2,
`ring`=accent. **`accent` stays Navree green** (it is not shadcn's grey
hover color): never write `hover:bg-accent` for a hover state, use
`hover:bg-ink-2`.

## 2. Type

| Face | Tailwind | Where |
|---|---|---|
| **Mona Sans** (variable, set semi-expanded `wdth 112`) | `font-display` | Every `h1` and `h2` automatically; big numbers; the wordmark |
| **Geist** (variable) | `font-sans` (default) | Body, UI, `h3` and below |
| **Monaspace Neon** | `font-mono` | Code, commands, crate names, eyebrows |

All self-hosted through `next/font/local`; no runtime font requests.
`h1`/`h2` get the display face from a base style; add `font-sans` to opt out.
Use `font-display-wide` (wdth 125) for a single oversized word at most.

Display scale (fluid between 390 and 1440 px; size, line height and
tracking come together):

| Class | Size | Use |
|---|---|---|
| `text-display-2xl` | 44 to 80 px | The homepage H1 only |
| `text-display-xl` | 38 to 60 px | Page H1 (Ecosystem, Architecture, AI, Performance) |
| `text-display-lg` | 32 to 48 px | Section H2 |
| `text-display-md` | 24 to 32 px | Sub-section heading, big card title |
| `text-display-sm` | 20 px | Card title in display face |
| `text-eyebrow` | 12 px, +0.14em | Use the `.eyebrow` class |

Weights: headings `font-semibold` (600). Body copy: `text-lg leading-relaxed
text-fg-2` for lead paragraphs, `text-[0.9375rem]` or `text-base` for card
copy. Keep measure under ~70 characters (`max-w-2xl`). Use `text-balance` on
headings and `text-pretty` on paragraphs.

`.eyebrow` is the small mono label above a section heading, with a green
running light: `<p className="eyebrow">Workspaces</p>`.

## 3. Layout and spacing

- **Container:** `container-site` (max 90rem = 1440 px, 16 px gutters on
  phones, 24 px from `sm`, 32 px from `lg`). Header, footer, docs and every
  section use it, so edges line up down the page. Narrow text inside it with
  `max-w-2xl`/`max-w-3xl`, not a second container.
- **Section rhythm:** `section-y` (72 to 128 px top and bottom). Separate
  sections with a `border-t border-line` hairline rather than background
  color changes.
- **Grid:** 4 px base, 8 px steps (`gap-2`, `gap-3`, `gap-4`, `gap-6`,
  `gap-8`, `gap-12`). Bento grids use `gap-3`. Heading to body `mt-5`/`mt-6`;
  eyebrow to heading `mt-4`/`mt-5`; heading block to content `mt-12` to
  `mt-16`.
- **Section headers:** eyebrow, H2 (`text-display-lg`), then the lead. When
  the section's content runs full width below (grids, mocks, tables), put
  the heading in 7 of 12 columns and the lead, plus at most one arrow link,
  in the other 5, bottom-aligned (`SectionHeading split` on the homepage).
  Every homepage section and page section follows one of these two shapes.
- **Header height:** `var(--header-h)` (4rem). Sticky things sit at
  `top-(--header-h)`.
- **Responsive:** perfect at 390 and 1440, no horizontal scroll from 320 up.
  Everything collapses to one column below `md`/`lg`; nothing shrinks text
  below 12 px.

## 4. Radii, borders and glows

- Radii: buttons and badges `rounded-full`; inputs and small chips
  `rounded-lg`; cards `rounded-2xl` (16 px); big frames `.bezel` (20 px outer,
  14 px core). Nested radii are concentric (outer minus padding).
- Borders: always 1px. `border-line` by default, `border-line-2` for
  interactive edges, `hover:border-line-2` on hoverable cards.
- Surfaces: `.surface` (ink-1, line border, faint top highlight
  `shadow-bezel`). `.bezel` + `.bezel-core` is the double frame for product
  mocks, terminals and hero panels.
- Glows are green light, never dark drop shadows: `shadow-glow-sm`,
  `shadow-glow` (primary button hover), `shadow-glow-lg` (one hero element),
  `shadow-float` (popovers and dialogs only).

## 5. Components

### shadcn/ui (`@/components/ui/*`, Radix based)

`button`, `badge`, `card`, `tabs`, `tooltip`, `separator`, `sheet`,
`accordion`, `scroll-area`, `navigation-menu`. Add more with
`pnpm dlx shadcn@latest add <name>` (components.json is set up), then
restyle: replace `bg-accent`/`text-accent-foreground` hover states with
`bg-ink-2`/`text-fg`, and change `from "cn"` imports if the CLI writes them
(it did once): the helper is `cn` from `@/lib/utils`.

- `Button` variants: `default` (green pill, the one call to action per
  view), `outline`, `secondary`, `ghost`, `link`, `destructive`; sizes `xs`,
  `sm`, `default` (40 px), `lg` (48 px), `icon*`. Use `asChild` with
  `next/link`. Icons in buttons need no size class.
- `Badge` variants: `default` (green tint), `secondary`, `outline`, `mono`
  (crate names, commands), `warning`, `destructive`.
- `Tabs` `TabsList variant="default"` (segmented pill) or `"line"` (green
  underline, for code panes).
- `Sheet` always needs a `SheetTitle` (use `sr-only` if hidden).
- `TooltipProvider` is already in the root layout.
- The older CSS classes still work and are used by existing pages: `.btn
  .btn-primary .btn-secondary`, `.link`, `.kbd`, `.inline-code`, `.numeric`,
  `.prose-navree`, the search dialog and the docs drawer.

### Magic UI (`@/components/magicui/*`)

All recolored to Navree green and ink, all respect `prefers-reduced-motion`,
all work in the static export. Client components are marked `"use client"`.

| Component | Export | Use it for | Reduced motion |
|---|---|---|---|
| `terminal` | `Terminal`, `TypingAnimation`, `AnimatedSpan` | Install and CLI sessions (`title="~/blog"`) | Lines shown at once |
| `word-rotate` | `WordRotate` | One rotating word inside a heading | First word, still |
| `morphing-text` | `MorphingText` | One short gooey-morphing word | First text, still |
| `text-animate` | `TextAnimate` | Word-by-word reveal of a short line | Plain text |
| `animated-gradient-text` | `AnimatedGradientText` | A word or two in moving green | Still gradient |
| `animated-beam` | `AnimatedBeam` | Flows between boxes (requests, events) | Line only, no pulse |
| `bento-grid` | `BentoGrid`, `BentoCard` | The pillars; 6-column asymmetric grid from `lg` | n/a |
| `magic-card` | `MagicCard` | A card whose border lights green near the pointer | Plain card |
| `border-beam` | `BorderBeam` | A streak running around one important frame | Hidden |
| `shine-border` | `ShineBorder` | A slow sheen on one frame | Still |
| `marquee` | `Marquee` | The ecosystem strip (`[--duration:40s] [--gap:1rem]`) | Still; pauses on hover/focus |
| `number-ticker` | `NumberTicker` | Performance numbers (HTML holds the real value) | Final value |
| `dot-pattern`, `dot-pattern-glow` | `DotPattern` (static, no JavaScript), `DotPatternGlow` | Background texture (`DotPatternGlow glow` for a few green pulses) | Static dots |
| `grid-pattern` | `GridPattern` | Hairline grid background, `squares` to light cells | Static |
| `flickering-grid` | `FlickeringGrid` | Status-light field behind one hero or CTA | One still frame |
| `animated-list` | `AnimatedList` | The Lighthouse issue feed | Whole list |
| `orbiting-circles` | `OrbitingCircles` | Modules orbiting the DI container | Items at rest |
| `blur-fade` | `BlurFade` | Section entrance (fade up from a light blur) | Rendered in place |

Backgrounds (`DotPattern`, `GridPattern`, `FlickeringGrid`) are absolutely
positioned: put them in a `relative overflow-hidden` parent and fade them
with a mask, e.g. `[mask-image:radial-gradient(ellipse_at_center,black,transparent_70%)]`.

### Site pieces (`@/components/site/*`)

`Logo`, `LogoMark`, `SiteHeader`, `SiteFooter`, `PlannedBadge`
(`{site.showPlannedBadges && status === "planned" ? <PlannedBadge /> : null}`).
The nav lives in `site.nav` (`site.config.ts`); `site.headline` is the
category line, `site.tagline` the short tagline, `site.getStarted` the
"Get started" link.

## 6. Motion

- Library: `motion` (`import { motion } from "motion/react"`), plus CSS
  keyframes for loops (marquee, orbit, shine, gradient).
- **One signature motion per section**, not five. Most of the page is still.
- Transform and opacity only; never animate width, height, top or left.
  Nothing shifts layout: reserve space for anything that changes.
- Easing: `ease-(--ease-out-quint)` for UI (150 to 300 ms), springs for
  pointer-follow. No `linear` except continuous loops.
- Entrances: `BlurFade inView` with 0.06 to 0.1 s stagger. Never hide the
  H1, the hero copy or the primary button behind an entrance animation.
- `prefers-reduced-motion`: a global rule stops CSS animations; every
  Magic UI component also checks `useReducedMotion()`. New motion code must
  do the same (`motion-safe:` / `motion-reduce:` or the hook).
- Hover: buttons `active:scale-[0.98]`; trailing arrows nudge
  `group-hover:translate-x-0.5`.

## 7. Diagrams (spec section 7)

- Hand-built SVG (or HTML+SVG) React components in
  `src/components/diagrams/`, one per diagram, usable from pages and MDX
  (`<Diagram name="..." />`).
- Draw on an 8 px grid. Boxes align; lines meet box edges at the centre of a
  side; no line crosses a box or a label; arrowheads show direction; every
  box and line that matters has a label.
- One accent element per diagram (the thing it is about); everything else in
  ink and line tokens: boxes `fill ink-1/ink-2`, `stroke line-2`, labels
  `fg-1`/`fg-2` at 12 px or more, mono for crate and type names.
- A short legend when there are more than two kinds of line.
- `role="img"`, a `<title>` and a `<desc>` that says in words what the
  picture shows.
- On phones the diagram re-lays out (stacks vertically) instead of shrinking
  text below 12 px. Test at 390 px.
- Animated beams show flow (a request, an event, a message) and stop under
  reduced motion (`AnimatedBeam` does this for you).

## 8. Do and don't

Do:
- Say what Navree is and does; real product mocks (terminals, code, a
  Lighthouse feed, a Helm table) instead of illustrations.
- One green thing per view that matters most.
- Crisp 1px lines, generous space, a strict grid aligned to `container-site`.
- Visible focus (the global green outline), `aria-label` on icon buttons,
  semantic headings in order, keyboard reach for everything.

Don't:
- Compare Navree with any other framework, anywhere.
- Purple or blue gradients, emoji icons, icons in coloured circles, generic
  three-column feature cards, stock illustrations.
- Dark drop shadows, glass blur on scrolling content (`backdrop-blur` is for
  the sticky header and overlays only), text below 12 px.
- `hover:bg-accent` (that is the green), raw hex colors in components (use
  tokens), new fonts.

## 9. App UI (products, dashboards and product mocks)

Everything that is an application rather than a marketing page follows these
rules: the CRM (Next and Nuxt), Helm, Lighthouse, Fleet, Logbook, Navigator,
any server-rendered dashboard (the shared kit in `crates/navree-ui`), the
starter kits, and every product mock on this site. Marketing pages keep
sections 1–8.

**The shell** follows the shadcn/ui Sidebar pattern (the `sidebar-07`
layout), built with the shadcn `Sidebar` component in React apps and its
equivalent in Vue and in server-rendered HTML:
- Width 16rem expanded, 3rem collapsed to icons; `Ctrl/Cmd+B` toggles; the
  state persists; on phones it becomes an off-canvas sheet.
- Full height, sticky, `ink-1` background, a 1px `line` border on the right.
- **Header**: the workspace or app switcher: a 32px square tile with the
  logo mark (4px radius), two lines (name in `fg` 14px medium, plan or
  context in `fg-3` 12px) and a `chevrons-up-down` icon. No glowing tiles.
- **Groups**: a label 12px, `fg-3`, sentence case (no letter-spacing, no
  uppercase), 32px tall; items below it.
- **Items**: 32px tall, 8px horizontal padding, 8px gap, a 16px lucide icon
  (stroke 1.75) and 14px text in `fg-2`. Hover: `ink-2` background, `fg`
  text. Active: `ink-3` background, `fg` text, medium weight. No left bar,
  no green text, no glow, no letter or avatar tiles in place of icons.
  Counts and badges sit right-aligned in mono 12px `fg-3`.
- **Sub-items**: indented, with a 1px `line` guide on the left.
- **Footer**: the user menu: a 32px square avatar (4px radius), name and
  email on two lines, `chevrons-up-down`.
- **Top bar**: 48px, a sidebar trigger, a separator, the breadcrumb, then
  search (`Ctrl/Cmd+K`) and page actions on the right.

**Corners**: 4px for buttons, inputs, menu items, badges, tabs and
avatars; 6px for cards, popovers and dialogs. No pills and no fully rounded
shapes in app UI (status dots excepted).

**Color**: the green accent is for the primary action, focus rings, and
positive data (won amounts, success states). Navigation, selection and
hover use the ink steps.

**Density**: 14px body text, 13px secondary; table rows 40px with 12px
cell padding; page padding 24px; headings 20–24px semibold.
