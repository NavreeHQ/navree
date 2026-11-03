# Template: workflows

`04-specs/<module>/workflows.md` describes records that move through
states: an order from new to shipped, an invoice from draft to paid, a
request through approval. Old products scatter this: a `status` column
written from several screens, buttons hidden by role in the template, and
no check that the move is allowed. The spec lists every state, every move,
who may make it, what it does and what waits on a timer.

## Format, one section per workflow

```text
## <Workflow> on <Entity>.<status field>

### States

| ID | State | Meaning | Old values | Terminal | Editable fields | Source | Known by | Conf. | Status |

### Transitions

| ID | From | To | Action | Who (role and record scope) | Guard | Effects | Error when refused | Source | Known by | Conf. | Status |

### Timers and waits

| ID | In state | Waits for | Then | Zone | Source | Known by | Conf. | Status |
```

## The columns

- **Old values**: every spelling the data profile found for this state,
  so the migration can map them. Values that fit no state are a question.
- **Terminal**: no move leaves it. **Editable fields**: what may still
  change in this state (often nothing once paid or closed).
- **Action**: the verb the user or system performs (`send`, `approve`,
  `void`). One action per transition; the same verb from two states is two
  rows.
- **Who**: roles and record scope ("approver of the requester's
  department"); `system` for moves a worker or event makes.
- **Guard**: what must be true besides the state (lines exist, amount
  under a limit, a document attached).
- **Effects**: fields set (`sent_at`), records created, mail, events for
  other modules, jobs started.
- **Timers and waits**: "three days after sent, if not paid, remind";
  "wait for the customer's signature, at most 14 days". These need durable
  timers that survive restarts.

In Navree a workflow is a status field with a service method per
transition and an access rule per action; long waits become a durable
workflow (see `navree-mapping.md`). There is no separate state-machine
engine, so the spec's transition table is the contract the tests check:
every move not in the table must be refused.

## Worked example

```text
## Invoice lifecycle on Invoice.status

### States
| invoices.workflow.1 | draft | being prepared | 'new', 'NEW', '' | no | all | SQL-033 | data | high | confirmed |
| invoices.workflow.2 | sent | sent to the customer | 'sent', 'open' | no | notes | SQL-033 | data | high | confirmed |
| invoices.workflow.3 | paid | settled in full | 'paid', 'done' | yes | none | SQL-033 | data | high | confirmed |
| invoices.workflow.4 | overdue | sent, past due | (none: computed on read in the old app) | no | notes | invoices.service.4 | code | high | confirmed |
| invoices.workflow.5 | void | cancelled with a credit note | 'cancel', 'cancelled' | yes | none | SQL-033 | data | high | confirmed |

### Transitions
| invoices.workflow.10 | draft | sent | send | accountant | invoices.service.2 | sent_at, mail to customer, event invoice.sent | 409 invoice_not_sendable | view.php:140 | code | high | confirmed |
| invoices.workflow.11 | sent | overdue | (system) | system | due_on before today (branch zone) | none | | cron/overdue.php | code | high | confirmed |
| invoices.workflow.12 | sent, overdue | paid | (system) on payment | system | payments cover the total | paid_at, event invoice.paid | | payments/add.php:40 | code | high | confirmed |
| invoices.workflow.13 | sent, overdue | void | void | accountant, manager | reason given | credit note, audit, event invoice.voided | 409 invoice_not_voidable | actions.php:30 | code | high | confirmed |
| invoices.workflow.14 | paid | sent | (old: "reopen" link) | | | | | actions.php:60 | code | high | question Q-015 |

### Timers and waits
| invoices.workflow.20 | sent | 3 days after due_on, not paid | first reminder mail | branch zone | cron/remind.php:30 | code | medium | confirmed |
```

## Checks before the file is done

- Every value in the status column's data profile maps to a state.
- Every screen action that changes the status is a transition row.
- Every transition names who may make it, and that appears in
  `ROLES.md`.
- Every move the old product allowed but should not (by data: rows that
  went backwards, by code: no check) is a question or a deliberate change.
- Every timer names its zone and what happens if the record moved on
  before it fires.
