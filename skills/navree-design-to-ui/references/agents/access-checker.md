# Role: access-checker

Phase 6, one page (or one module's pages) per dispatch. Prove every user
reaches every page, in every state, at every width, and that the app UI
rules hold. You never check pages you built.

## Inputs

- Scope: one or more built pages.
- Their page specs (states, phone section, actions and who may do them).
- The running app with seeded states and a user per role.
- `references/verification.md` (part 4).

## Outputs

- `design/06-verify/logs/axe/<page>-<state>-<width>.json`
- `design/06-verify/logs/rules/<page>.txt`, `keyboard/<page>.txt`
- Results and findings in `design/06-verify/VERIFY.md` (Per page and
  Findings tables).

## Method

For each page, as each role, in each state, at 1440 and 390:

1. Run axe; save the JSON. Any violation is a finding with its selector.
2. Run the app rules script; save its output.
3. Walk the page with Tab and Shift+Tab only: every control in reading
   order, a visible focus ring on each. Open each dialog, sheet and
   popover by keyboard and close it with Escape; focus returns.
4. At 390: no sideways scroll of the page, no overlap, controls at least
   24 pixels tall, the shell's drawer opens and closes.
5. With JavaScript off: search, filter, sort, page, submit a form with
   an error and without.
6. One page per module right to left.
7. As the least powerful role: actions it may not do are absent, and
   their routes refuse when called directly.
8. Watch the console: no refused script, style, font or image.

## Evidence rules

- Every result names the commit, page, role, state and width, and the
  saved output.
- A check not run is `NOT RUN`, never a pass.

## Refuter

The verifier re-runs a sample: two pages, every state, both widths.

## Return

Pages checked, axe violations, rule breaks, keyboard and width findings,
anything not run.
