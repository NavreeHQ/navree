# Speed of four screens, Navree UI and React with shadcn/ui

Each module of `navree-ui` has a size budget, but a budget says nothing about how it feels next to another toolkit. This folder builds four screens twice, once with Navree UI and once with React 19 and shadcn/ui, loads each in Chromium under the same conditions, and writes down what it finds. It is a measurement, not a test: the run exits 0 whoever is ahead, and the report lists the places where Navree UI is behind as plainly as the places where it is ahead.

```sh
cd e2e/ui-compare
npm ci
node run.mjs
```

That builds the proof app (`cargo build --release -p ui-proof`) and starts it on a free port, installs and builds the React pages in `react/`, starts a static host for them, checks that the two builds agree, and runs five interleaved rounds in both profiles. It takes a while: figure on 20 to 40 minutes after the builds.

| Needs | |
|---|---|
| Node 20 or later | for the harness and for building the React pages |
| A Rust toolchain | to build the proof app, unless `NAVREE_UI_URL` names a server that is already running |
| Chromium for Playwright 1.64.1 | `run.mjs` installs it when it is missing (`npx playwright install chromium` does the same) |

Options (all optional): `--rounds N`, `--screens table,select,dates,tabs`, `--profiles mobile,desktop`, `--skip-react-build`. `COMPARE_LIMIT_MINUTES` (default 240) stops a run that goes on too long.

`run.mjs` stops what it started, and only that: the proof app by its process id, the static host, the browser. It never touches a server it was only told about with `NAVREE_UI_URL`.

## What is measured

The four screens are described in [`SCREENS.md`](SCREENS.md): a table of 500 payments, a select and a combobox of 300 cities, a date picker, and tabs with a menu. The data is Indian (names, cities, rupees with lakh grouping) and comes from three text files in `data/` that the Navree page, the React build and the harness all read.

For each screen, build, profile and round the harness makes two loads, each in a fresh browser context with an empty cache:

1. **The paint load.** Nothing is pressed. It reads the bytes sent, by type, from the DevTools protocol's own accounting, the JavaScript after decompression, the first and largest contentful paint, and the time the main thread was blocked after the first paint.
2. **The interaction load.** From the moment the response starts, a real click or key goes to the screen's first control every 60 ms until the screen reacts. That moment is the time to the first interaction. Then the first interaction is undone and every step of the screen is done in order and timed.

Two loads, because input stops the browser from reporting the largest paint.

| Profile | What it is |
|---|---|
| Mobile | 4x CPU slowdown and Slow 4G (1.6 Mbit/s down, 750 kbit/s up, 150 ms latency), both set through the DevTools protocol before the first request |
| Desktop | No slowdown |

The window is 1280 by 800 pixels in both, so layout differences do not mix into the numbers. The rounds are interleaved: round 1 loads every screen of both builds, then round 2, and so on, with the order of the two builds swapped every other round, so a change in how busy the machine is touches both alike. The browser is started again every round.

## What each number means

All times are on the page's own clock. Lower is better for every number.

| Number | Meaning |
|---|---|
| HTML, JavaScript, CSS, fonts, other files sent | Bytes that crossed the network for that kind of file, headers included, after compression. The browser's own request for a site icon is left out. |
| Everything sent | The sum of those. |
| JavaScript once decompressed | What the browser had to parse, whatever it cost to send. |
| First contentful paint | From the start of the navigation to the first text or picture drawn. |
| Largest contentful paint | To the largest text or picture drawn, as the browser reports it. |
| Main thread blocked after first paint | The part beyond 50 ms of every long task that started after the first paint, added up. |
| First interaction works | From the moment the response starts to the second frame drawn after the screen's first control did its job. For a build that renders on the server this includes the server's answer. |
| A step | From the time of the press or key that started it to the second frame drawn after the expected state appeared on the screen. It is measured inside the page, so the harness's own work is not in it; a trip to the server is. |
| Bytes fetched by the steps | What the steps asked the network for, after the page was ready. |

A step has ended when the screen shows the state `SCREENS.md` expects, found by reading the page itself. If a step never reaches it, within 20 seconds, the load is recorded as failed, the run goes on, and the run exits with code 1 at the end with the failed step named in `results/latest.md`.

Each cell of the report is the median of the rounds with the least and greatest after it. The ratio is Navree UI divided by React with shadcn/ui: below 1.00 favours Navree UI, above 1.00 favours React. A difference under 5 percent, or under 256 bytes or 5 ms, is called no difference.

## Reading the report

`results/latest.md` opens with the places where Navree UI is behind, then the places where it is ahead, then a table for each screen and profile. A loss line reads like this:

> React is faster at the step "next page" by 140 ms (Navree 210 ms, React 70 ms, ratio 3.00)

Before treating a loss as a defect, look at what the two builds do differently by design:

- **Where the work happens.** The Navree table asks the server for each sort, search and page and swaps the answer in, as it does with JavaScript off. The React table holds all 500 rows and sorts, filters and pages in the browser. In the mobile profile every trip to the server costs the 150 ms of the slow network, so the table steps are where Navree UI is most likely to be behind; its page also arrives drawn, which the first paint shows.
- **What arrives first.** The React pages are single-page builds: the HTML is nearly empty and the screen is drawn once the scripts have run. The Navree pages arrive drawn. A React setup that renders on the server would be a third build, and `entrants.json` is where it would be named.
- **What a control does before its script arrives.** Some Navree controls are native HTML that works before any script (a popover opens, a link follows). The first interaction of such a screen is therefore early. Where a control needs script on both builds, both wait for it.
- **Fonts.** Navree UI ships its own fonts; the React build uses the system font stack that shadcn/ui starts with. "Fonts sent" shows the difference.
- **Default look.** Each build keeps its toolkit's own look. Colours and spacing are not part of the comparison.
- **Server-rendered tables and filters.** The filter step on the Navree side includes the 100 ms pause the table waits for typing to stop.

Numbers from one machine are for comparing the two builds on that machine, in that run. Compare runs from different machines only by their ratios.

## The two builds

**Navree UI.** The pages are in `apps/ui-proof/src/compare.rs`, served at `/compare/table`, `/compare/select`, `/compare/dates` and `/compare/tabs` by the proof app. They use the toolkit as an app would, with no shell around them. The proof app's other pages are galleries of states inside an app shell, with data of their own, so none of them is one of these screens; these four are added beside them. The program is built in release mode and sent the files the way the toolkit sends them: scripts and styles compressed ahead of time at brotli 11, the page compressed for each request.

**React with shadcn/ui.** The project in `react/` is Vite, React 19, TypeScript and Tailwind 4. The component files in `react/src/components/ui` are shadcn/ui's source for button, input, label, badge, table, select, popover, command, dialog, calendar, tabs and dropdown menu (the new-york style), with Radix, cmdk and react-day-picker underneath and TanStack Table for the data table. Every dependency is pinned to an exact version in `react/package.json`. To pull the files again from shadcn/ui, run `npx shadcn@latest add button input label badge table select popover command dialog calendar tabs dropdown-menu --overwrite` in `react/`.

Each screen is its own page (`table.html`, `select.html`, `dates.html`, `tabs.html`), so a screen loads only the code it uses, and the bundler splits out what the pages share. `run.mjs` builds them with `vite build` and serves `react/dist` like a normal host: text files compressed ahead of time at brotli 11 and gzip 9, files under `assets/` cached for a year, pages not cached.

### The lockfile

`react/package-lock.json` is made once, without installing anything:

```sh
cd e2e/ui-compare/react
npm install --package-lock-only
```

Commit it. Until it exists `run.mjs` falls back to `npm install` and says so; with it, `npm ci` gives the same packages every time.

## Do the two builds agree?

Before any timing, `run.mjs` loads every screen of both builds once and compares what it shows with what `lib/data.mjs` computes from the data files: the headings and the first page of rows of the table, every option of the select and the combobox, the empty date picker, the tabs, the menu items. It then does each screen's steps once. If anything differs it prints the difference and exits with code 1 without measuring.

## Files

| File | What it is |
|---|---|
| `run.mjs` | The run: builds, serves, checks, measures, writes the results. |
| `lib/screens.mjs` | The steps of each screen, how to find each control on each build, and the state each step must end in. |
| `lib/data.mjs` | The data and the states the harness expects. |
| `lib/measure.mjs` | One load, measured. |
| `lib/inpage.js` | The recorder that runs inside every page. |
| `lib/servers.mjs` | Starting and stopping the proof app, the static host, the React build. |
| `lib/report.mjs`, `lib/stats.mjs` | Medians, ranges and the report. |
| `entrants.json` | The route of each screen on each build. |
| `SCREENS.md` | The four screens. |
| `data/` | The text files the data comes from. |
| `react/` | The React and shadcn/ui build. |
| `results/<date>.json` | Every run, as measured, with the medians. Kept on the machine that measured; `results/latest.json` and `results/latest.md` are the newest. |

## Limits

- Chromium only.
- The network profile slows requests to localhost, so it shows what a slow network costs each build, not what a real network does.
- Five rounds give a median and a range, not a confidence interval. If the ranges of the two builds overlap, the difference is within the noise of the machine.
- There is no hand-written page to act as a floor for the numbers yet. A third entry in `entrants.json`, with its own routes and selectors in `lib/screens.mjs`, is how one would be added.
