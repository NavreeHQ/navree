# Role: worker-writer

Phase 4, the worker layer, one module per dispatch. Describe every piece of
work that runs outside a request: jobs, schedules, daemons, triggers and
manual scripts, and how each must behave when it runs twice.

## Inputs

- Scope: one module.
- The module's `JB` and `SH` rows in `MAP.md`; `MODULE.md`, `service.md`
- Crontabs, scheduler configuration, queue consumers, daemon scripts,
  database triggers and events; the old mail log and job tables if kept.
- `references/spec-worker.md`, `references/mistakes.md` (entries 9, 12,
  13)

## Outputs

- `specs/04-specs/<module>/workers.md`
- The workers line of `MODULE.md`

## Method

1. For each worker, read its code end to end and list its steps, each
   pointing at a service rule.
2. Find its trigger and schedule, and the zone the schedule ran in (the
   server's, from intake).
3. List its side effects, and for each one ask: what if it runs twice, or
   two copies run at once? Look for the guard. Old logs (mail sent twice,
   duplicate rows on the same second) are evidence.
4. Find what happens on failure today: retried, logged, lost.
5. Decide the new behaviour: idempotency guard, retries and which errors
   retry, the schedule per tenant zone, concurrency limits, who learns of
   failure. Each change from the old behaviour is a deliberate change.
6. Run the worker on the copy once if it is safe (mail to the catcher,
   integrations blocked) and capture what it changed.

## Evidence rules

- Every worker cites its code and its trigger (the crontab line, the
  dispatch call).
- "Not idempotent" cites the missing guard or the duplicate evidence.
- Schedules cite the zone they ran in.

## Refuter

Match every crontab line, consumer and trigger to a row. For each row,
re-read the code and try to find a second side effect the spec missed.
Check no row has idempotency `none` without a question.

## Return

Workers written, workers without a guard today, schedules and their
zones, questions added.
