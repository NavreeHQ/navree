---
name: navree-browser-testing
description: "Browser tests with navree-browser: `Browser::launch()` / `Browser::builder()` (headless Chrome, Chromium or Edge over the Chrome DevTools Protocol), `Page` actions (`type_into`, `append`, `select`, `check`, `attach`, `press`, `click`, `keys`, `drag`, `scroll_to`, `back`, `refresh`), assertions that wait (`assert_see`, `assert_path_is`, `assert_count`, `assert_no_console_errors`, ...), `wait_for_request`, targets (`#css`, `@testid`, label or button text, `Target::containing/nth`, `within`), failure screenshots under `target/navree-browser/<test>/`, `TestServer` (an app on a real port), `FrontEnd`/`WebSession` (a Next.js front end with a BFF pointed at the test's app), `Viewport::phone()`, the `NAVREE_BROWSER_*` variables and `navree browser:install`. Use when testing a user journey end to end in a real browser (sign-in forms, the front end and its BFF, drag and drop, phone layouts), when a browser test fails or flakes, when CI has no Chrome, or when reading a navree-browser failure report."
---

# Browser testing

`navree-browser` (crate `navree-browser`, a dev-dependency; or the opt-in
facade feature `browser` in `[dev-dependencies]`, as `navree::browser`)
drives a real browser from `cargo test`. The source is
`crates/navree-browser/src`; its tests drive a fixture app in
`crates/navree-browser/tests/browser/`, and the CRM journeys are in
`apps/crm-api/tests/browser.rs`. Docs page: `/docs/packages/browser-testing/`.

## Steps

1. Add the dev-dependencies:

```toml
[dev-dependencies]
navree-browser = { git = "https://github.com/NavreeHQ/navree" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

2. Make sure a browser is found: `NAVREE_BROWSER_CHROME`, an installed
   Chrome or Chromium, Chrome for Testing from `navree browser:install`,
   Playwright's Chromium, or Microsoft Edge (in that order). On a machine
   with none: `navree browser:install` (pinned Chrome for Testing into
   `~/.cache/navree/chrome/<version>`, `%LOCALAPPDATA%\navree` on Windows,
   `NAVREE_CACHE_DIR` to move it; `--version`, `--force`).

3. Put the tests in their own file (`tests/browser.rs`), each
   `#[tokio::test]` returning `Result<(), BrowserError>`:

```rust
use navree_browser::{Browser, BrowserError, TestServer};
use navree_testing::TestApp;

#[tokio::test]
async fn a_rep_signs_in() -> Result<(), BrowserError> {
    let app = TestApp::builder().module(AppModule).build().await;
    let server = TestServer::start(app.ctx()).await?; // the real router on 127.0.0.1:<free port>
    let browser = Browser::launch().await?;            // its own browser context

    let page = browser.visit(server.url("/login")).await?;
    page.type_into("Email", "rep1@navree.test").await?;
    page.type_into("Password", "password123").await?;
    page.press("Sign in").await?;
    page.assert_path_is("/").await?;
    page.assert_see("Pipeline by stage").await?;
    page.assert_no_console_errors().await?;
    Ok(())
}
```

4. With a separate front end (Next.js and a BFF), start it through
   `FrontEnd`; it gets the address of navree-browser's proxy in its API
   variable, and `serve` points the proxy at this test's app:

```rust
use navree_browser::FrontEnd;

fn crm_web() -> FrontEnd {
    FrontEnd::new("crm-web") // one process per name per test binary
        .command("pnpm", ["--dir", "../crm-web", "start", "--port", "3100"])
        .url("http://127.0.0.1:3100") // serve waits until this answers
        .api_env("NAVREE_API_URL")
        .env("NAVREE_COOKIE_SECURE", "0") // `next start` over plain http
}

let web = crm_web().serve(crm.ctx()).await?; // a WebSession: this test's turn
let page = browser.visit(web.url("/login")).await?;
```

   Build the front end first (`pnpm --dir apps/crm-web build`).

5. Run them: `cargo test --test browser`. Watch one:
   `NAVREE_BROWSER_HEADLESS=false NAVREE_BROWSER_SLOW_MO=250 cargo test --test browser <name>`.

## Templates

Targets (only visible elements match; first match wins):

```rust
use navree_browser::Target;

page.click("#deal-title").await?;          // CSS: starts with #, . or [, or "css=..."
page.click("@deal-card").await?;           // data-testid="deal-card"
page.type_into("Email", "a@b.test").await?; // a field by label, aria-label, aria-labelledby, placeholder or name
page.press("Sign in").await?;              // a button or link by text or accessible name
page.click(Target::from("@deal-card").containing("Lovelace").nth(0)).await?;
let card = page.within(Target::from("@deal-card").containing("Lovelace")).await?;
card.press("Edit").await?;                 // Element: the same methods, inside the card
let cards = page.all("@deal-card").await?; // Vec<Element>, visible matches now
let text = cards[0].inner_text().await?;
```

Actions, assertions, waits and reads:

```rust
page.append("Notes", " Call back Monday.").await?;
page.select("Stage", "Negotiation").await?;           // <select> by text or value; or a combobox + option
page.check("Send me a summary").await?;               // clicks the label when the input is hidden
page.attach("Logo", "tests/fixtures/acme.png").await?; // a file input, even display:none
page.keys("Control+K").await?;
page.drag(Target::from("@deal-card").containing("Lovelace"), "[data-stage=won]").await?;
page.wait_for_request("/api/v1/deals/*/move").await?; // after the action; returns PageRequest
page.assert_see_in("[data-stage=won]", "Lovelace").await?;
page.assert_count("@deal-card", 3).await?;            // visible matches
page.with_timeout(Duration::from_secs(20)).assert_see("Export ready").await?;
let total: f64 = page.script("return window.__DEAL_TOTAL__").await?; // a function body
let cookie = page.cookie("navree_at").await?;         // Option<Cookie>, HttpOnly too
page.screenshot("board-after-move").await?;           // NN-board-after-move.png
```

Viewports and options:

```rust
use navree_browser::Viewport;

let page = browser.visit_with(url, Viewport::phone()).await?; // 390x844, ratio 3, touch, Android UA
page.resize(768, 1024).await?;
let browser = Browser::builder()
    .window(2200, 1100)                   // default 1440x900
    .timeout(Duration::from_secs(10))     // NAVREE_BROWSER_TIMEOUT_MS, default 5 s
    .artifacts("target/browser-failures") // NAVREE_BROWSER_ARTIFACTS
    .launch()
    .await?;
```

Skip on machines without a browser:

```rust
if let Err(error) = Browser::find_chrome() {
    eprintln!("skipped: {error}");
    return Ok(());
}
```

## Checks

- `cargo test --test browser` passes; a failing step prints
  `<step> failed after <time>: <reason> on <url>` and the paths of
  `NN-failure.png`, `NN-failure.html` and `NN-console.log` under
  `<target dir>/navree-browser/<module>/<test>/`. Read the console log
  first: console messages, uncaught exceptions, dialogs and failed requests.
- No `sleep`s in browser tests: every action and assertion waits (5 s by
  default; navigations up to 30 s).
- After an action whose API call the test checks through the API, call
  `wait_for_request(..)` first: the UI often updates before the call ends.
- In CI, upload `target/navree-browser/` on failure; without Chrome on the
  runner, run `navree browser:install` or set `NAVREE_BROWSER_CHROME`.

## Gotchas

- Text targets match exactly (after collapsing whitespace), case included:
  `"Sign in"` does not match `"Sign In"`. A field target looks at labels,
  not at the text near the input; add `<label for>` or `aria-label`.
- Hidden elements never match: `assert_visible("Secret")` fails with
  `"Secret" is not visible` when the text exists but is hidden, and with
  `no element matches` when it does not exist.
- `assert_no_console_errors` does not wait and ignores failed network
  requests (only `console.error` and uncaught exceptions count).
- `Browser` contexts are per test; one Chrome is shared by the tests of a
  binary running at the same time and stops when the last `Browser` is
  dropped. A `Page` keeps its browser context alive.
- `FrontEnd` sessions of one name run one at a time (each test's `serve`
  waits for the previous `WebSession` to drop). Drop the `WebSession` (end
  the test) before another test can use the front end. If something
  already answers at its URL, `serve` fails: stop the stray server.
- `drag` presses beside links inside the source (a press on a link starts
  the browser's own URL drag) and drops at the middle of the target's
  visible part; give boards a wide window (`.window(2200, 1100)`) so the
  target column is on screen.
- Dialogs (`alert`, `confirm`, `prompt`) are accepted automatically.
- `TestServer` runs on the test's runtime: don't block the test thread
  (no `std::thread::sleep`) while the browser needs the server.
- `Viewport` user agents: desktop pages see `Chrome/...`, not
  `HeadlessChrome`; phones and tablets see Chrome's Android user agent.
