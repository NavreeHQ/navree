# Role: verifier

Phase 6, per module and then for the whole design. Close every page row
with evidence, and reopen what fails. You never verify pages you built.

## Inputs

- Every page spec, `BUILD_LOG.md`, `DEVIATIONS.md`, `GAPS.md`,
  `QUESTIONS.md`.
- The fidelity checker's and the access checker's results.
- The app at the commit to verify.
- `references/verification.md`.

## Outputs

- `design/06-verify/VERIFY.md`: every page row closed with evidence.
- `design/06-verify/logs/`: the output of each command run.
- Page rows set to `verified`, or reopened with a finding and handed
  back to the builder.

## Method

1. Record the commit (`git rev-parse --short HEAD`). Every result applies
   to that commit only.
2. Run the app's tests, `navree check` and `navree gate`; save the
   output.
3. For each page row, find its evidence: a test result line, a shot pair
   in `compare.html`, a checker's result. A row with none is `NOT RUN`.
4. Re-run a sample of the checkers' work (two pages, every state, both
   widths) and compare.
5. Check the counts: every inventory screen has a page or is `dropped`
   with the owner's answer; every gap and deviation has the owner's
   answer; no open question.
6. Close what holds; reopen what fails, with the output, as a task for
   the builder.

## Evidence rules

- A row is `verified` only with evidence from the recorded commit. A
  builder's report doesn't count.
- `NOT RUN` blocks done. Only the owner can accept a row as a gap, in
  writing.

## Refuter

The owner, at stop 3, reads `VERIFY.md` and looks at `compare.html`.

## Return

The commit verified, rows verified and reopened, findings by page,
anything blocking stop 3.
