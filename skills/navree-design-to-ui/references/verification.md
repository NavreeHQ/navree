# Verification

Phase 6. Three roles check every page: the `fidelity-checker` (does it
look like the design), the `access-checker` (does everyone reach it, in
every state, at every width), and the `verifier` (is every row closed
with evidence). Every command's output goes to `06-verify/logs/`, and
every result names the commit (`git rev-parse --short HEAD`).

## 1. Tests in the app

Each page gets plain tests with `TestView`, which reads a page with no
browser: what it shows, where a click goes, what its form would post.

```rust
use navree::ui::test_view::Clicked;
use navree::ui::{Button, Empty, Form, FormView, Shell, TestView, Validate};
use serde::Deserialize;

#[derive(Debug, Deserialize, Validate, Form)]
pub struct NewContact {
    /// Name
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    /// Email
    #[validate(email)]
    pub email: String,
}

#[test]
fn the_empty_list_offers_the_first_action() {
    let view = Shell::new("Acme CRM")
        .page("Contacts", "/contacts")
        .body(Empty::new("No contacts yet").actions(Button::primary("New contact").href("/contacts/new")))
        .to_view();
    let page = TestView::of(&view);
    assert!(page.sees("No contacts yet"));
    assert_eq!(page.click("New contact"), Clicked::Link("/contacts/new".to_owned()));
}

#[test]
fn the_form_refuses_a_bad_email_under_its_field() {
    let form = TestView::of_node(FormView::<NewContact>::new().post("/contacts"))
        .fill("name", "Ada Lovelace")
        .fill("email", "not an email");
    let errors = form.submit_as::<NewContact>().unwrap_err();
    assert!(errors.contains_key("email"));
    assert!(!errors.contains_key("name"));
}
```

Routes, access and the database get HTTP tests with `TestApp`
(`navree-testing`): each page as each role answers 200, 403 or 404 as its
spec says; each form posts and redirects; each bulk action refuses a
stale count.

## 2. Screenshots, the design's and the pages'

Serve the design copy (`preservation.md`) and run the app with seeded
data for every state in the page specs. Then take every screenshot with
one script, so both sides come from the same browser at the same sizes.
This one uses Playwright from Node (`npm i -D playwright`, then
`npx playwright install chromium`):

```js
// design/shots.mjs: node design/shots.mjs shots.json
// shots.json: [{"id": "BASE-04", "url": "http://127.0.0.1:8091/deals.html"},
//              {"id": "APP-04", "url": "http://127.0.0.1:8080/deals", "dir": "rtl"}]
import { readFile, mkdir } from "node:fs/promises";
import { chromium } from "playwright";

const shots = JSON.parse(await readFile(process.argv[2], "utf8"));
const browser = await chromium.launch();
for (const width of [1440, 390]) {
  const context = await browser.newContext({ viewport: { width, height: width > 400 ? 900 : 844 }, colorScheme: "dark" });
  const page = await context.newPage();
  for (const shot of shots) {
    await page.goto(shot.url, { waitUntil: "networkidle" });
    await page.evaluate(() => document.fonts.ready);
    const folder = shot.id.startsWith("BASE") ? "design/01-inventory/shots" : "design/06-verify/shots";
    await mkdir(folder, { recursive: true });
    await page.screenshot({ path: `${folder}/${shot.id}-${width}.png`, fullPage: true });
  }
  await context.close();
}
await browser.close();
```

The navree-browser crate (the `navree-browser-testing` skill) takes the
same screenshots from a Rust test when the app is driven there
(`Viewport::phone()` is 390 by 844; `page.screenshot(..)`).

## 3. The side-by-side page

`06-verify/compare.html` shows every pair: one row per page and state,
the design on the left, the page on the right, 1440 above 390. It is a
local file for the owner's eyes, not part of the app.

```html
<!doctype html>
<meta charset="utf-8">
<title>Design and pages, side by side</title>
<style>
  body { background: #040605; color: #e9efeb; font: 14px system-ui; margin: 24px; }
  section { margin: 0 0 48px; }
  .pair { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; align-items: start; }
  img { width: 100%; border: 1px solid #2a3530; }
  h2 { font-size: 16px; } p { color: #a9b5ae; }
</style>
<section>
  <h2>DS-04 Deals, default, 1440</h2>
  <p>Deviations: DV-02, DV-03. Findings: none.</p>
  <div class="pair">
    <img src="../01-inventory/shots/BASE-04-1440.png" alt="Design">
    <img src="shots/APP-04-1440.png" alt="Page">
  </div>
</section>
<!-- one section per page, state and width -->
```

The fidelity checker looks at every pair and writes, per pair: what
differs, whether a deviation covers it, and whether the page looks worse.
"Looks worse" is a finding even when no rule names it: crowded spacing,
weak hierarchy, misaligned edges, a heavier or lighter weight than the
design, text that wraps badly at 390.

## 4. Accessibility and the rules, in the browser

For every page, as each role, in every state, at 1440 and 390:

- **axe** (`axe-core` in Playwright, rules `wcag2a`, `wcag2aa`,
  `wcag21aa`): no violation. Save the JSON in `06-verify/logs/axe/`.
- **Keyboard:** Tab from the top reaches every control in reading order,
  each with a visible focus ring; dialogs take focus, keep it, and give it
  back on Escape; the table's keys work when `keyboard_grid()` is on. A
  select, combobox or multi-select opens with the arrow keys, picks with
  Enter and closes on Escape (a combobox narrows as you type); a command
  palette opens on Ctrl or Cmd and K.
- **JavaScript off:** search, filter, sort, pages and forms still work
  (links and plain posts); errors still land under their fields. A select
  posts its native `<select>`. A combobox searched on the server shows its
  chosen option and can't change it; the page spec says so.
- **390 pixels:** the page doesn't scroll sideways
  (`document.documentElement.scrollWidth <= innerWidth`); controls are at
  least 24 pixels tall; nothing overlaps.
- **Right to left:** one page per module with `dir="rtl"`: the shell,
  table and forms mirror, numbers stay readable.
- **The app rules:** run this in each page and keep the output:

```js
// The app UI rules: corners 4 or 6 px (radios and spinners are round),
// no shadow or glow, no style attribute, no text in an avatar.
const bad = [];
for (const el of document.querySelectorAll("body *")) {
  const style = getComputedStyle(el);
  const slot = el.getAttribute("data-slot") || el.tagName;
  const radius = parseFloat(style.borderTopLeftRadius);
  const round = ["radio", "spinner"].includes(slot) || el.closest("[data-slot=spinner]");
  if (radius && ![2, 4, 6].includes(radius) && !round) bad.push(`radius ${radius}px on ${slot}`);
  if (style.boxShadow !== "none") bad.push(`shadow on ${slot}`);
  if (el.hasAttribute("style") && !el.hasAttribute("popover")) bad.push(`style attribute on ${slot}`);
  if (slot === "avatar" && el.textContent.trim()) bad.push(`text in an avatar`);
}
[...new Set(bad)];
```

- **Content security policy:** the browser's console shows no refused
  script, style, font or image.

## 5. The gate

`navree check` and `navree gate` pass on the commit being verified.

## 6. VERIFY.md

```text
# Verify

Commit: 4e5f6a7    Date: 2026-11-05    Verifier: <agent>

| Row | Result | Evidence |
| deals-index.5 | verified | APP-04-1440 beside BASE-04-1440: badge "Closed won", success tone; DV-03 covers the corners |
| deals-index.20 | verified | APP-04-empty-1440; cargo test deals::the_empty_list -> 1 passed |
| deals-index.22 | gap | GAP-04 accepted by the owner on 2026-11-04 |

## Per page
| Page | Shots | axe (violations) | Keyboard | JS off | 390 | RTL | Rules | Looks |
| deals-index | 12 of 12 | 0 | pass | pass | pass | pass | pass | approved |

## Findings
| ID | Page | State | Width | What | Evidence | Back to |
```

A row is `verified` only with evidence from the recorded commit. A
change after verification makes the rows it touches stale: take their
screenshots and checks again.
