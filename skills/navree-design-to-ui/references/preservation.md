# Keeping the design: the copy, the mode, the deviations

Phase 0 sets these up; phases 2 to 6 use them. The design is the thing
every page is compared against, so it must not move while the work runs.

## The copy

Copy the design into `design/source/` exactly as it came: every HTML,
CSS, script, image, font and export, in its own folders, with its own
names.

```sh
mkdir -p design/source
cp -R /path/to/handoff/. design/source/
diff -r /path/to/handoff design/source && echo "identical"
```

- Never edit, format, rename or "clean up" a file in `source/`. A comment
  in the design may be the designer's note.
- An archive: unpack it into `source/` and keep the archive beside it.
- A design-tool file: ask for an export of every frame (HTML or SVG, and
  PNG at 1440 and 390 pixels wide). Keep the tool's link and the export's
  date in `INTAKE.md`. A link alone is not a copy: the file changes.
- Screenshots only: put them in `source/` as they came, named by screen.
- Record the copy's size, file count and a hash in `INTAKE.md`
  (`find design/source -type f | sort | xargs sha256sum | sha256sum`), so
  a later change shows.

Commit `design/` with the app unless the owner says the design is
private; then keep it beside the repository and say where in `INTAKE.md`.

## Serving it in development

Serve the copy on its own port while you work, so it opens in a browser
the same way every time and screenshots of it and of the app come from
the same browser:

```sh
python3 -m http.server 8091 --bind 127.0.0.1 --directory design/source
# the design at http://127.0.0.1:8091/, the app at http://127.0.0.1:8080/
```

Any static file server does. Serve it only on your own machine: it is not
part of the app, never ships, and never needs a route in it. If a screen
needs a script of the design's to show a state (a menu open, a tab
chosen), note the clicks in `INVENTORY.md` so the screenshots repeat them.

## Keep or translate, per screen

The owner chooses at intake, and `INTAKE.md` records the choice for each
screen:

| | Keep | Translate |
|---|---|---|
| What must match | layout, order, spacing and density, type sizes and weights, colours as far as tokens allow, icons, wording | content, order, actions and states |
| What may differ | only approved deviations | the look: Navree's tokens and components |
| Token rows | a `mapped` value is a deviation | a `mapped` value is fine |
| Comparison at verify | side by side, each difference named | side by side, content and actions checked |

Keep mode never means copying the design's markup. Pages are always
built from components, which carry Navree's tokens, its 4 and 6 pixel
corners and its accessibility. Keep mode means choosing the components,
sizes, gaps and variants that come closest, and naming what still
differs.

## Deviations

A deviation is a place where the page will look or act differently from
the design. Every one has a row in `04-reconcile/DEVIATIONS.md` and the
owner's answer. A deviation the owner hasn't approved blocks stop 2.

```text
# Deviations

| ID | Screen | What the design has | What the page will have | Why | Picture | Owner | Date |
| DV-02 | all | primary #22c55e | accent #3dff8f (dark theme) | one accent until the theme builder | 04-reconcile/dv/DV-02.png | approved | 2026-11-04 |
| DV-03 | DS-02, DS-04 | pill-shaped chips | Badge with 4 px corners | app rule: no pills | 04-reconcile/dv/DV-03.png | approved | 2026-11-04 |
| DV-05 | DS-04 | the table sorts in the browser | sort on the server, in the URL | mistakes.md 2 | none | approved | 2026-11-04 |
```

Reasons fall into four kinds: an app rule (corners, pills, letter tiles,
glow, inline style), a missing token or component (a gap with an interim
form), a design mistake (`mistakes.md`), or accessibility (a pair below
4.5:1, a target too small, a missing label). The picture is the design's
part beside the page's part, cut from the screenshots; write "none" only
when the difference can't be seen.

## Comparing

Compare by eye, beside each other, at the same width and in the same
state: the design's screenshot (`BASE-*`) and the page's (`APP-*`).
`verification.md` builds the page that shows them. A pixel difference
tool is a hint for where to look, not a verdict: Navree's components have
their own anti-aliasing, fonts and corners, so a perfect match is not the
goal. The goal is that nobody looking at the two prefers the design for a
reason the owner hasn't approved.
