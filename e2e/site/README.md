# navree.com in a real browser

Tests of the built website that only a browser can settle. They read the
static export in `apps/website/out` and serve it with
`apps/website/scripts/serve.ts` on a free port.

```sh
cd apps/website && pnpm install --frozen-lockfile && pnpm build && cd ../..
cd e2e/site
npm ci
npx playwright install chromium
npm test
```

| File | Proves |
|---|---|
| `tests/about-controls.test.mjs` | The live picture on `/about/`: it starts, hidden from assistive technology, with the still and its description in the page; the opening is marked as seen once per visit; the pause button, and the choice kept in `localStorage`; the main thread's work paused against playing; a paused visitor getting the still after a reload, without the scene's files; play loading them; leaving the page removing the canvas; JavaScript off showing the still with Navree code on its screen. |

Settings: `ABOUT_URL` (the address of `/about/` on a server that is already
running), `ABOUT_SHOTS` (a folder for a screenshot of the still),
`ABOUT_CHANNEL=chrome ABOUT_HEADED=1` (the installed Chrome with a window and
the real GPU). Headless Chromium draws the picture with software WebGL, which
is slower than a GPU, so the measurement of the main thread (paused against
playing) is read most reliably in headed Chrome.
