# Template: screens

`04-specs/<module>/screens.md` describes every page and dialog a user
reaches: why it exists, what it shows, which lists and forms it holds,
what each button does, and what it shows while loading, when empty, on an
error and when the user may not see it. The CRUD and list specs hold the
fields and the tables; a screen spec puts them on a page and names the
journeys that pass through it.

## Format, one section per screen

```text
## <Screen name> (<SC-id>, old route <RT-id>)

| Property | Value | Source | Known by | Conf. | Status |
| Purpose | <what the user comes here to do> | | | | |
| Reached from | <menu item, link, button, deep link> | | | | |
| Who | <roles; and what the others see: absent from the menu, 404, 403> | | | | |
| Contains | lists <ids>, form <entity>, panels <names> | | | | |
| Loading | <skeleton, spinner, nothing> | | | | |
| Empty | <text and action> | | | | |
| Error | <text and retry> | | | | |
| Phone | <works at 375 px wide: yes / what changes> | | | | |

### Actions

| ID | Action | Control | Who | Enabled when | Does | Confirm | After | Source | Known by | Conf. | Status |

### Shown data

| ID | Element | Data (spec row) | Who sees it | Source | Known by | Conf. | Status |

### Journeys through this screen

| ID | Journey | Steps | Ends with | Source | Known by | Conf. | Status |
```

## The columns

- **Who** and the forbidden state: the new product checks on the server; a
  screen a role may not see is absent from that role's menu and answers
  "not found" (another tenant's record) or "not allowed" (a missing
  permission) when opened by URL.
- **Contains** points at list and CRUD rows instead of repeating them.
- **Actions / Does**: the operation it calls (a CRUD operation id, a
  workflow transition, a service rule). **Enabled when** is the state rule,
  which the server checks again. **After**: where the user lands and what
  they see (a message, the refreshed list).
- **Shown data** covers what is not a list or form: header figures, badges,
  side panels, totals. Each points at the spec row that computes it.
- **Journeys** are the tasks people do end to end ("raise an invoice from
  an order and send it"). Every journey becomes an acceptance test and,
  where the old product runs, a parity test. Get them from the owner, the
  old product's help pages and its most used routes (access logs on the
  copy, if kept).

## Worked example

```text
## Invoice detail (SC-041, old route RT-052 /invoices/view.php?id=)

| Property | Value | Source | Known by | Conf. | Status |
| Purpose | check an invoice, record a payment, send or void it | owner, stop 1 | running | high | confirmed |
| Who | accountant, manager: any in the branch; sales rep: own customers', read only; others: absent from menu, 404 by URL | ROLES.md roles.12..roles.16 | code | medium | question Q-011 (old: any logged-in user, any branch, by changing ?id=) |
| Contains | invoice header, lines (invoices.crud.30..36), payments list invoices.list.40, activity panel | SHOT-042 | running | high | confirmed |
| Error | "Could not load this invoice. Try again." with Retry | SHOT-043 (old: PHP warning on the page) | running | high | confirmed |

### Actions
| invoices.screen.5 | Send | primary button | accountant | status draft, has lines, customer has an email | invoices.workflow.2 | no | stays; status badge Sent; message "Invoice sent" | view.php:140 | code | high | confirmed |
| invoices.screen.6 | Record payment | button opening a dialog | accountant | status sent or overdue | invoices.service.9 | no | payments list refreshed; status Paid when settled | view.php:160 | code | high | confirmed |
| invoices.screen.7 | Void | destructive button | accountant, manager | status sent or overdue | invoices.crud.20 | yes, reason required | status Void; credit note link | actions.php:30 | code | high | confirmed |

### Journeys
| invoices.screen.20 | Raise and send an invoice from an order | open order, Invoice, check lines, Send | invoice Sent, mail in the catcher, order Invoiced | owner; CAP-030..CAP-034 | running | high | confirmed |
```

## Checks before the file is done

- Every screen in `MAP.md` owned by the module has a section.
- Every button, link and menu item on the old screen appears as an action
  or as `dropped` with a reason. A button that did nothing in the old
  product is a finding, not a feature.
- Every screen has loading, empty and error states and a forbidden state.
- Every action names its operation and the state that enables it.
- Every journey ends with something checkable (a status, a row, a mail).
- The file ends with `## Questions`, `## Deliberate changes` and
  `## New map rows` (formats in `spec-module.md`); an empty one says
  "none".
