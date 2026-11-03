# Role: token-mapper

Phase 2, once per design. Map every raw value of the design to a Navree
token, or name it a deviation or a gap.

## Inputs

- `design/01-inventory/INVENTORY.md` (its raw values), `design/source/`.
- `INTAKE.md` (keep or translate per screen).
- `references/tokens.md`; `crates/navree-ui/src/tokens.rs` for the
  current table.

## Outputs

- `design/02-tokens/TOKENS.md`, one `DT-` row per distinct value, rows
  `draft` or `question`, with its closing sections (questions,
  deviations, gaps).
- `design/04-reconcile/dv/DV-*.png` for each deviation: the design's
  swatch or part beside Navree's.

## Method

1. Read the token table from `tokens.rs`, not from memory: it changes.
2. Group the raw values by role (`tokens.md`, Mapping).
3. Give each value its verdict: same, mapped, gap or refused. Keep mode
   turns every `mapped` into a deviation.
4. Check the contrast of every text pair the design uses, before and
   after mapping (`navree::ui::tokens::contrast`).
5. Collect the design's fonts, radii, shadows and motion. A font other
   than Geist, a corner other than 4 or 6 pixels, any shadow or glow on a
   surface: a deviation with the Navree form.
6. A light design, a second accent, a gradient: a gap under the theme
   builder.

## Evidence rules

- Every row cites the inventory's raw-value row and the file and line.
- Every contrast figure is computed, with both colours written.

## Refuter

Pick ten values at random and find them in `source/`; check their role.
Recompute three contrast figures. Grep `source/` for colours the file
lacks.

## Return

Values mapped by verdict, deviations and gaps added, pairs below 4.5:1.
