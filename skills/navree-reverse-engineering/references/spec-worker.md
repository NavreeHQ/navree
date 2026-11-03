# Template: workers

`04-specs/<module>/workers.md` describes the work that runs outside a
request: queued jobs, cron entries, scheduled tasks, daemons that poll a
table, scripts someone runs by hand every month. Old workers often assume
they run once, at a fixed hour, on one server, in the server's time zone.
In the new product a job can run twice (a retry, a second worker), so each
one must be safe to repeat.

## Format, one row per worker, then details

```text
## Workers

| ID | Worker | Trigger | Input | Does | Side effects | Idempotency | Retries | Schedule and zone | Concurrency | On failure | Old form | Source | Known by | Conf. | Status |
```

## The columns

- **Trigger**: `schedule` (with the expression), `event` (which change
  starts it), `user action` (which button), `manual` (who runs it and
  when), `poll` (a loop reading a table: name the table and the flag).
- **Input**: ids and parameters, never whole records or secrets.
- **Does**: the steps, in order, each pointing at a service rule.
- **Side effects**: rows written, mail and messages sent, files written,
  calls to other systems. Each external effect needs its own idempotency.
- **Idempotency**: how a second run with the same input changes nothing:
  a natural key and a unique index, a "sent at" column checked before
  sending, a status that moves only once, a key passed to the other
  system. `none` is a finding.
- **Retries**: attempts and the wait between them; which errors retry
  (time-outs, the other system down) and which never do (bad data).
- **Schedule and zone**: when, in which time zone, and for whom ("08:00 in
  each branch's zone"). Old cron entries run in the server's zone; say
  which one it was.
- **Concurrency**: may two run at once? Per what (per customer, per
  branch, globally)?
- **On failure**: who learns of it and how; what is left half done.
- **Old form**: `cron`, `queue`, `daemon`, `trigger`, `manual script`,
  with the command line or file.

## Worked example

```text
| invoices.worker.1 | Mark overdue | schedule: daily 00:15 | none | invoices.service.4 for every sent invoice of the branch | status updates, audit entries | status moves sent -> overdue once; re-run finds nothing | 3, exponential | daily 00:15 in each branch's zone (old: 00:15 server time, Europe/London, for all branches) | one per branch | logged; next run catches up | cron | crontab line 4: php cron/overdue.php | code | high | confirmed |
| invoices.worker.2 | Overdue reminders | schedule: Mondays 09:00 | branch | mail each customer with overdue invoices one summary | mail, reminder_log row | reminder_log unique (customer, week); old: none, sent twice when cron overlapped (mail log 2023-04-03) | 5, fixed 10 min, only on mail transport errors | Mondays 09:00 branch zone | one per branch | failed jobs list; accountant sees "reminders not sent" | cron | crontab line 7; cron/remind.php | code | high | confirmed |
| invoices.worker.3 | Accounting export | manual: accountant on the 1st of the month | month | CSV of the month's invoices to the accountant's share | file on a disk, export_log row | export_log unique (branch, month); re-run replaces the file | 0 | manual | one at a time | message on screen | manual script | owner, stop 1; tools/export.php | running | medium | question Q-014 |
```

## Checks before the file is done

- Every `JB` and `SH` map row owned by the module has a worker row.
- Every crontab line, queue consumer, daemon and database trigger was
  checked, and every manual script the owner named.
- Every worker has an idempotency rule that is not `none`, or a question.
- Every schedule names its time zone.
- Every external side effect has its own guard against repeats.
