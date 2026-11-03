---
name: navree-design-to-ui
description: "Design to Navree UI: turns a design (an HTML and CSS mock-up, a Figma, Sketch, Penpot or other design-tool export, a design handoff bundle, or screenshots) into Navree pages built from navree-ui's typed Rust components, rendered on the server: `AppShell` pages, forms from `#[derive(Form)]`, server-side tables from `#[derive(TableRow)]`, the design tokens. Phases that each write files to a design folder (intake with the design copied and served in development, inventory of screens, components, states, tokens, implied data and actions, token mapping, an element-by-element component map with gaps named instead of improvised, page specs, reconcile, build page by page, verify), one subagent brief per role with a refuter for every file, owner stop points, a catalog of design mistakes (wrong control for the data, client-only tables, options that must come from the server, missing empty, loading, error, read-only and forbidden states, right to left, phone width) and their Navree form, and verification with screenshots at 1440 and 390 pixels beside the design, axe, the keyboard and the app UI rules. Works alone from a design, or from the screen specs of a navree-reverse-engineering job. Use when asked to build pages from a design, mock-up, Figma or other design export, handoff or screenshots, to convert a design or prototype into Navree UI, to keep an existing look while moving to Navree, or to check built pages against a design."
---

# From a design to Navree UI

A design shows how a product should look. It rarely says where its data
comes from, what a list does with ten thousand rows, or what a page shows
when it is empty, fails or belongs to someone else. This skill turns a
design into Navree pages: it lists what the design holds, maps every
value and element to what navree-ui offers, writes a spec per page, builds
the pages in Rust on the server, and proves them against the design by
eye and by tool.

Each phase writes files to one design folder, so a long job survives
restarts and every claim points at its source. Each job has a written
brief for the agent that does it, and every file meets a second agent
whose only job is to prove it wrong.

## The rules

1. **Files, not memory.** Every phase writes its files before it ends.
   `design/STATUS.md` says where to resume; the agent running the whole
   job keeps it, roles don't edit it.
2. **Evidence on every row.** Each row says where it comes from (a design
   file and line, a frame name, a screenshot `BASE-*`), how it is known
   (`file`, `shot`, `note`, `owner`, `inferred`) and how sure you are
   (`high`, `medium`, `low`). Every `inferred` row is also a question for
   the owner.
3. **The design stays as it came.** Copy it byte for byte into
   `design/source/`, never edit it, and serve it in development so every
   comparison is against the same thing.
4. **One owner per file.** One role owns each file; another role refutes
   it. The agent that builds never grades its own work.
5. **Only real navree-ui.** Map each element to a component that exists
   (`references/components.md`). Where none fits, the row is a `gap` and
   names what is coming; never improvise a component, a stylesheet or a
   script to fill it.
6. **Navree's rules win, the owner decides.** Control corners are 4 px,
   surfaces 6 px. No pills, no letter tiles, no glow. No inline script and
   no `style=""`: the content security policy refuses them. Where the
   design breaks a rule, the row records a deviation and the owner
   approves it.
7. **The data comes from the server.** Lists page, sort, filter and search
   on the server. Options come from the backend. Every rule runs on the
   server. A design that shows otherwise is a catalog entry
   (`references/mistakes.md`), not a requirement.
8. **Beauty is checked by eye.** Screenshots of each page at 1440 and 390
   pixels go beside the design's own. Someone looks at them, finds what
   looks worse, and fixes it. A passing tool run is not enough.

## The design folder

Choose its place at intake and write it in `INTAKE.md`. A good default is
`design/` beside the app. When a reverse-engineering job exists, keep
`design/` beside its `specs/`.

```text
design/
  STATUS.md                     phase, state, owner role, files, gate, sign-off
  source/                       the design exactly as received: never edited
  00-intake/INTAKE.md           what the design is, its format, how it is served, keep or translate, linked specs
  01-inventory/INVENTORY.md     screens, repeated components, states, raw tokens, implied data and actions
  01-inventory/shots/           BASE-*: the design's screens at 1440 and 390 pixels
  02-tokens/TOKENS.md           every design value, its Navree token, or a gap
  03-mapping/COMPONENTS.md      every repeated component and its navree-ui form, or a gap
  03-mapping/pages/<page>.md    one page spec per screen: shell, header, body, forms, tables, states, data, actions
  04-reconcile/refutations/     one verdict file per spec file
  04-reconcile/QUESTIONS.md     what only the owner can answer
  04-reconcile/DEVIATIONS.md    where the pages will differ from the design, and why
  04-reconcile/GAPS.md          what navree-ui doesn't offer yet, per page
  05-build/BUILD_LOG.md         per page: commit, tests, gate
  06-verify/VERIFY.md           every page row closed with evidence
  06-verify/shots/              APP-*: the built pages at 1440 and 390 pixels
  06-verify/compare.html        the design and the pages side by side
  06-verify/logs/               the output of every command the verifier ran
```

Writers that run side by side never share a file. Every file in
`01-inventory`, `02-tokens` and `03-mapping` ends with the same three
sections, and the reconciler collects them: `## Questions` (into
`QUESTIONS.md`), `## Deviations` (into `DEVIATIONS.md`) and `## Gaps`
(into `GAPS.md`).

## Ids and the row format

| Kind | Id | Example |
|---|---|---|
| Screens of the design | `DS-` | `DS-04` |
| Repeated components | `DC-` | `DC-02` |
| Design tokens | `DT-` | `DT-11` |
| Rows of a page spec | `<page>.<n>` | `deals-index.7` |
| Design screenshots, page screenshots | `BASE-`, `APP-` | `BASE-04-390` |
| Questions, deviations, gaps | `Q-`, `DV-`, `GAP-` | `GAP-03` |

Every row ends with the same columns:

```text
| ... | Source                          | Known by | Conf. | Status    |
| ... | source/deals.html:88, BASE-04   | file     | high  | confirmed |
```

A writer sets its rows to `draft`, or `question` when only the owner can
settle them. Then a row moves `draft` → `confirmed` (the refuter could not
break it) or `refuted` (back to its writer) → `built` (the builder's page
passed its gate) → `verified` (the verifier quoted the evidence). A row
with no navree-ui form is `gap` until the component lands or the owner
chooses another form. `dropped` needs the owner's answer in
`QUESTIONS.md`.

## The phases

| # | Phase | Role (brief in `references/agents/`) | Writes | Template |
|---|---|---|---|---|
| 0 | Intake | `intake` | `00-intake/INTAKE.md`, `source/`, `STATUS.md` | in the brief, `preservation.md` |
| 1 | Inventory | `inventory` | `01-inventory/*` | `spec-inventory.md` |
| | **Stop 1: the owner confirms the inventory and keep or translate** | | | |
| 2 | Tokens | `token-mapper` | `02-tokens/TOKENS.md` | `tokens.md` |
| 3 | Mapping | `component-mapper` | `03-mapping/*` | `components.md`, `spec-page.md` |
| 4 | Reconcile | `refuter` (every file), `reconciler` | `04-reconcile/*` | in the briefs |
| | **Stop 2: the owner approves the page specs, deviations and gaps** | | | |
| 5 | Build | `builder` | the pages, `05-build/BUILD_LOG.md` | `building-pages.md` |
| 6 | Verify | `fidelity-checker`, `access-checker`, `verifier` | `06-verify/*` | `verification.md` |
| | **Stop 3: the owner looks at the pages beside the design and accepts** | | | |

### 0. Intake

Learn what the design is: its format (HTML and CSS, a design-tool export,
a handoff bundle, screenshots), the tool and version, who made it, which
screens are final, and which product it belongs to. Copy it into
`design/source/` as it came and serve that folder in development
(`references/preservation.md`). A design-tool file that only opens in its
tool is not enough: ask for an export of every frame as HTML, SVG or PNG
at 1440 and 390 pixels wide.

Ask the owner, per screen, which mode applies:

- **Keep:** the owner wants this look. The pages follow the design's
  layout, spacing, density, type sizes and colours as far as Navree's
  tokens and rules allow, and every difference is a deviation.
- **Translate:** the design is a sketch of what the page holds. The pages
  use Navree's look, and only the content, order and actions must match.

### 1. Inventory

List everything the design holds, with ids: screens, repeated components,
the states each screen shows and the ones it lacks, every raw value
(colours, type, spacing, radii, shadows, motion), and the data and
actions each screen implies. Derive lists from the files where they exist
(a grep over the HTML and CSS, the export's frame list) and keep the
command with its count. Screenshot every design screen at 1440 and 390
pixels (`BASE-*`). Nothing is judged here.

### 2. Tokens

Group the raw values by role (surfaces, lines, text, accent, status,
radii, spacing, type) and map each to a Navree token
(`references/tokens.md`). A value with no token is a gap or a deviation:
navree-ui has one token table and no per-app theme yet. Check contrast
for every text pair with `navree::ui::tokens::contrast`.

### 3. Mapping

For every repeated component, then every screen, map each element to its
navree-ui form (`references/components.md`): a component, a form field
from `#[derive(Form)]`, a table column from `#[derive(TableRow)]`, or a
part of the shell. Write one page spec per screen (`spec-page.md`): where
each value comes from, each action and its route, the options and their
source, and every state, including the ones the design forgot. Check each
row against the mistakes catalog.

### 4. Reconcile

A `refuter` takes each file and tries to break every row: it opens the
cited design file or screenshot, looks for the element elsewhere in the
design, checks the component exists in the code, and checks the mistakes
catalog. Then the `reconciler` collects questions, deviations and gaps,
and finds what no row covers.

### 5. Build

Build the shell first (the design's navigation as `NavGroup`s and
`NavItem`s), then page by page in the order the owner set: the data and
services the page needs (with the other Navree skills), the page with
`AppShell`, its forms and tables, every state, and its tests. Each page
ends green on `navree gate`; its commit goes in `BUILD_LOG.md`.

### 6. Verify

A `fidelity-checker` screenshots each page at 1440 and 390 pixels, in each
state, and builds `compare.html` with the design's screenshot beside each.
An `access-checker` runs axe in every state, walks each page with the
keyboard, with JavaScript off and right to left, and checks the app UI
rules. A `verifier` (never the builder) closes each page row with quoted
evidence and reopens what fails.

## Stop points

Stop, show the owner a short summary with counts, and wait. Record the
answer and the date in `STATUS.md`.

1. **After the inventory.** The screens, the states the design lacks, the
   implied data, keep or translate per screen, and anything that can't be
   opened.
2. **After reconcile.** The page specs, every deviation with a picture,
   every gap with its interim form, and every question with options and
   a recommendation.
3. **After verify.** `compare.html`, `VERIFY.md`, and anything still open.

## Working with navree-reverse-engineering

The two skills share the owner and the order of work, and either can run
alone.

- **From a reverse-engineering job.** Its `04-specs/<module>/screens.md`
  gives each page's purpose, actions, journeys and states; `crud.md`
  gives each field, its control and where its options come from;
  `lists.md` gives each table's columns, filters, sort and bulk actions.
  The page specs here cite those rows (`invoices.screen.5`) instead of
  writing them again, and add only the look: layout, components, tokens.
  The old product's screenshots are the design when the owner wants its
  look kept; `SHOT-*` captures become `BASE-*`.
- **Into a reverse-engineering job.** Its builder's last layer is screens.
  For a navree-ui front end, the builder runs this skill's phases 3 to 6
  per module, after the module's routes and services pass their gate.
- **Alone, from a design.** The inventory's implied data and actions
  become the data the pages need. Stop 1 settles it with the owner, and
  the builder creates the models, services and routes with the Navree
  skills each page names, before the page itself.

## Subagents

**With subagents.** Dispatch one role per agent, giving it its brief
(`references/agents/<role>.md`), its scope (the design, a screen, a
page), the paths it reads and the files it writes. Agents write files and
return only a short summary and the list of files. Run in parallel:

- phase 1: one `inventory` agent per group of screens, then one to merge
  repeated components;
- phases 2 and 3: the `token-mapper` and the `component-mapper` for the
  shared components side by side, then one `component-mapper` per page;
- phase 4: one refuter per file, never the agent that wrote it;
- phase 6: the `fidelity-checker` and the `access-checker` side by side
  per page, then the `verifier`.

Build one page at a time: pages share the shell, `src/app.rs` and the
module's controller.

```text
Role: component-mapper (brief: skills/navree-design-to-ui/references/agents/component-mapper.md)
Scope: screen DS-04, page `deals-index`
Read: design/01-inventory/INVENTORY.md, design/02-tokens/TOKENS.md,
      design/03-mapping/COMPONENTS.md, design/source/ (read only),
      specs/04-specs/deals/lists.md (from the reverse-engineering job)
Write: design/03-mapping/pages/deals-index.md, using references/spec-page.md
Return: the file written, row counts by status, gaps and questions added. No other prose.
```

**Without subagents.** Run the same briefs yourself, in phase order, one
role at a time. Before each role, read its brief and only the files it
lists. Refute after a break: read the file cold, as the refuter brief
says, before trusting it.

## Done

The pages are done when every box holds, each with its evidence in
`VERIFY.md`:

- [ ] Every screen in `INVENTORY.md` has a page that is `verified`, or is
      `dropped` with the owner's answer.
- [ ] Every page row is `verified`, or a `gap` the owner accepted with its
      interim form.
- [ ] `QUESTIONS.md` has no open question; every deviation in
      `DEVIATIONS.md` has the owner's approval.
- [ ] Every page has its empty, loading, error, read-only and forbidden
      states, each screenshotted.
- [ ] `compare.html` holds every page and state at 1440 and 390 pixels
      beside the design, and the owner looked at it.
- [ ] axe reports no violation in any state; the keyboard reaches every
      control in order with a visible focus ring; nothing scrolls sideways
      at 390 pixels; the app UI rules hold on every page.
- [ ] Forms and tables work with JavaScript off.
- [ ] `navree gate` passes on the final commit.
- [ ] The owner signed stop 3 in `STATUS.md`.

## Common mistakes

- Porting the design's HTML and classes. The stylesheet reads only
  `data-*` attributes; classes do nothing and `style=""` is dropped.
  Build from components.
- Improvising a missing component with raw elements, a script or an
  inline style. Record the gap; the owner picks an interim form.
- Taking the design's sample rows as the data. Lists come from the
  server, page by page, and the design's ten rows say nothing about ten
  thousand.
- Drawing only the state the design shows. Every page also needs empty,
  loading, error, read-only and forbidden.
- Comparing against the live design file instead of the copy. The file
  changes; the copy in `source/` doesn't.
- Calling a page done from a tool run. Look at the screenshots.
- Building every page before the shell and the first page are accepted.
- Checking only at 1440 pixels, only left to right, only as an
  administrator.

## References

| File | Read when |
|---|---|
| `references/agents/<role>.md` | Before running or dispatching that role (10 briefs, listed below) |
| [`references/preservation.md`](references/preservation.md) | Phase 0: copying and serving the design; keep or translate; deviations |
| [`references/spec-inventory.md`](references/spec-inventory.md) | Phase 1: the inventory and its screenshots |
| [`references/tokens.md`](references/tokens.md) | Phase 2: Navree's tokens and how design values map to them |
| [`references/components.md`](references/components.md) | Phases 3 and 5: every navree-ui component, what design element it takes, and the gaps |
| [`references/spec-page.md`](references/spec-page.md) | Phase 3: the page spec |
| [`references/mistakes.md`](references/mistakes.md) | Phases 3 and 4: design mistakes and their Navree form |
| [`references/building-pages.md`](references/building-pages.md) | Phase 5: the shell, pages, forms, tables, states and their tests, in code that compiles |
| [`references/verification.md`](references/verification.md) | Phase 6: screenshots, the side-by-side page, axe, keyboard, the rules |

The briefs, in phase order:
[`intake`](references/agents/intake.md),
[`inventory`](references/agents/inventory.md),
[`token-mapper`](references/agents/token-mapper.md),
[`component-mapper`](references/agents/component-mapper.md),
[`refuter`](references/agents/refuter.md),
[`reconciler`](references/agents/reconciler.md),
[`builder`](references/agents/builder.md),
[`fidelity-checker`](references/agents/fidelity-checker.md),
[`access-checker`](references/agents/access-checker.md),
[`verifier`](references/agents/verifier.md).
Load the `navree` skill before phase 5; each page names the other skills
it needs. The `navree-reverse-engineering` skill covers moving an old
product, its rules and its data.

## Docs

- https://navree.com/docs/architecture/facade-and-features/
- https://navree.com/docs/basics/validation/
- https://navree.com/docs/basics/sessions-and-csrf/
- https://navree.com/docs/basics/access-control/
- https://navree.com/docs/testing/http-tests/
- https://navree.com/docs/packages/browser-testing/
