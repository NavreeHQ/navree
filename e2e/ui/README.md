# navree-ui in real browsers

Tests of the toolkit that only a browser can settle.

```sh
cargo run --release -p ui-proof -- serve &   # http://127.0.0.1:8090
cd e2e/ui
npm ci
npx playwright install chromium firefox
npm test
```

The tests of Logbook's pages need a second build, and skip without one:

```sh
UI_PROOF_LOGBOOK=1 PORT=8092 cargo run --release -p ui-proof --features logbook -- serve
UI_LOGBOOK_URL=http://127.0.0.1:8092 npm test
```

| File | Proves |
|---|---|
| `tests/morph.test.mjs` | Random trees and random edits reach the target and keep identity, focus, caret, selection, typed values and open state, in Chromium and Firefox; an IME composition survives a morph in Chromium. Needs no server. `MORPH_ROUNDS` sets the rounds. |
| `tests/proof.test.mjs` | What ships on each page, forms with and without JavaScript, pending within a frame, morphing tables, boosted links, the sidebar, native overlays, commands with no network, asset headers. |
| `tests/datatable.test.mjs` | The data table in Chromium, Firefox and WebKit against a real database: every state, selection across pages, bulk actions, failed and late replies, JavaScript off, keyboard, axe. Needs `ui-table-proof` serving, see the file's head. |
| `tests/hostile.test.mjs` | The runtime against a hostile server and hostile markup, in Chromium and Firefox: the committed `dist/` under the exact content security policy, served by `tests/support/fixture.mjs`, with a second origin that records anything sent to it. Payloads through every path that inserts markup, off-origin navigation, prefetch of state-changing links and stale prefetch, typed values across navigation, named elements in forms and documents, commands, regions. The last tests use the proof app. |
| `tests/logbook.test.mjs` | Logbook in Chromium and Firefox, against the proof app with Logbook on: the list and an entry side by side at 1440px and one at a time at 390px, an address for every entry with the back button and forward, the entry types and their counts in the sidebar and its icon rail, no sideways scroll at 390px with very long and hostile requests recorded, dark colours whatever the system asks for, the empty, bad-filter and missing-entry states, axe, the app rules, the filter keeping focus beside an open entry, Replay and Clear. Needs `UI_LOGBOOK_URL`. |
| `tests/icons-flags.test.mjs` | Icons and flags in Chromium and Firefox (`ICON_BROWSERS="chromium firefox webkit"` adds WebKit), against the proof app's `/icons-flags`: every icon and flag drawn, no policy report, axe clean in dark and light, a hidden copy of a flag changing no pixel of the visible ones, names for assistive technology, own icons, 390 px, right to left. |
| `tests/conformance.test.mjs` | axe in every state with no violation and no CSP report, the app design rules on every page, keyboard reach and a visible focus ring. |

`UI_PROOF_URL` points the suites at another server.

axe-core is MPL-2.0 and stays here: nothing in the shipped toolkit includes it.
