# navree-ui in real browsers

Tests of the toolkit that only a browser can settle.

```sh
cargo run --release -p ui-proof -- serve &   # http://127.0.0.1:8090
cd e2e/ui
npm ci
npx playwright install chromium firefox
npm test
```

| File | Proves |
|---|---|
| `tests/morph.test.mjs` | Random trees and random edits reach the target and keep identity, focus, caret, selection, typed values and open state, in Chromium and Firefox; an IME composition survives a morph in Chromium. Needs no server. `MORPH_ROUNDS` sets the rounds. |
| `tests/proof.test.mjs` | What ships on each page, forms with and without JavaScript, pending within a frame, morphing tables, boosted links, the sidebar, native overlays, commands with no network, asset headers. |
| `tests/conformance.test.mjs` | axe in every state with no violation and no CSP report, the app design rules on every page, keyboard reach and a visible focus ring. |

`UI_PROOF_URL` points the suites at another server.

axe-core is MPL-2.0 and stays here: nothing in the shipped toolkit includes it.
