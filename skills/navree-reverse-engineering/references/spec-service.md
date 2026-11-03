# Template: service (business rules)

`04-specs/<module>/service.md` holds the module's business rules: the
calculations, checks and decisions that are not a field rule. In old
products they hide in many places: stored procedures, triggers, model
callbacks, controllers, templates and, most of all, browser scripts. A rule
that ran only in the browser was never enforced; the new product runs it on
the server, once.

## Format

```text
## Rules

| ID | Rule | Inputs | Output or effect | When it runs | Where it lives now | Errors | Source | Known by | Conf. | Status |

## Calculations

| ID | Value | Formula | Rounding | Currency and units | Example (inputs -> result) | Source | Known by | Conf. | Status |

## Invariants

| ID | Always true | Checked by | Source | Known by | Conf. | Status |
```

## The columns

- **Rule**: one decision in one sentence ("an order over the customer's
  credit limit needs a manager's approval").
- **When it runs**: on create, on update of a field, on an action, on a
  schedule, on an event from another module.
- **Where it lives now**: every place in the old code, with `file:line`.
  The same rule in two places that disagree is a contradiction for the
  reconciler; record both.
- **Errors**: what the user sees when the rule says no, and a short error
  code (`credit_limit_exceeded`). The code lets front ends translate it and
  tests match it.
- **Formula**: written so a test can check it. **Rounding**: when and how
  (per line or on the total, half up or half even, to cents). Take it from
  the code and confirm it on real rows: pick ten records, compute, compare
  with what the old product stored, and record the result.
- **Example**: inputs and the exact result, from real old rows (ids, not
  personal data). These become test cases as they stand.
- **Invariants**: what must hold after every change ("a payment never
  exceeds the open amount"). **Checked by**: a database constraint, the
  service before saving, a nightly reconciliation job.

## Worked example

```text
## Rules
| invoices.service.2 | An invoice can't be sent without lines and a customer email | invoice | refusal | Send action | view.php:140 (button hidden), nothing on the server | 422 invoice_not_sendable | view.php:140 | code | high | confirmed |
| invoices.service.4 | Overdue when due_on is before today in the branch's zone and the invoice is not paid or void | invoice, branch zone | status overdue | nightly job JB-003 and on read in the old list | cron/overdue.php:12; js/invoices.js:61 (uses the browser's clock) | | cron/overdue.php:12 | code | high | confirmed |

## Calculations
| invoices.service.6 | Margin | (total excl. tax - sum(line qty x cost price)) / total excl. tax | to 0.1% half up, shown only | percent | #4411: 1 200.00, cost 900.00 -> 25.0% | js/invoices.js:55 | code | high | confirmed |
| invoices.service.7 | Invoice total | sum of lines (qty x unit price, rounded per line to cents) + tax per rate group, rounded on the group | half up per line; tax half up per group | cents, invoice currency | #4411 -> 1 434.00; old stored 1 433.9999 (float) | js/invoice.js:140; SQL-022 | code | high | confirmed |

## Invariants
| invoices.service.20 | sum(payments) <= total | service before saving a payment; nightly check | payments/add.php:22 (checked in JS only) | code | high | confirmed |
```

## Checks before the file is done

- Every browser script of the module was read; every calculation or check
  in it is a rule here or `dropped` with a reason.
- Every rule cites every place it lives, and contradictions are flagged.
- Every calculation has rounding, units and at least one real example.
- Every refusal has an error code.
- The file ends with `## Questions`, `## Deliberate changes` and
  `## New map rows` (formats in `spec-module.md`); an empty one says
  "none".
