# Template: integrations

`04-specs/<module>/integrations.md` describes every other system the
product talks to: payment providers, accounting packages, couriers, mail
and SMS gateways, single sign-on, partner APIs, file drops, and other
modules of the same company reached over the network. Old integrations
often lack retries, verify no signatures, keep keys in the code and fail
silently. For each one, the spec says what crosses the boundary and what
happens when it fails.

## Format, one section per integration

```text
## <System> (<IN-id>)

| Property | Value | Source | Known by | Conf. | Status |
| Purpose | <why the product talks to it> | | | | |
| Direction | outgoing calls / incoming calls / both / files | | | | |
| Transport | HTTP API, webhook, SFTP or file share, mail, message broker | | | | |
| Auth | API key, OAuth client, OAuth per tenant, signed webhook, certificate | | | | |
| Credentials | where they live today; who owns them; per tenant or global | | | | |
| Environments | sandbox available? test accounts? | | | | |

### Exchanges

| ID | Exchange | Direction | When | Request | Response | Idempotency | Timeout | Retries | On failure | Source | Known by | Conf. | Status |

### Incoming events

| ID | Event | Verified by | Replay protection | Handler | Order and duplicates | Source | Known by | Conf. | Status |
```

## The columns

- **Credentials**: never copy a secret into the spec. Say where it is
  ("hard-coded in config.php:12", "in the database table settings, plain
  text") and mark it as a mistake to fix; the new product keeps it in
  configuration or encrypted per tenant.
- **Request / Response**: the fields that matter, with types, and a
  captured example in `evidence/` (secrets and personal data removed).
- **Idempotency**: how a repeated call does no harm (an idempotency key
  the other system honours, a reference it rejects twice, a check before
  calling).
- **Timeout and Retries**: how long to wait; what to retry (network
  errors, 5xx, 429 with its wait) and what never (4xx validation errors).
  Calls that change something on the other side retry only with an
  idempotency key.
- **On failure**: who learns of it; what the user sees; what is left to
  do later (a queued retry, a "needs attention" status).
- **Verified by**: how an incoming call proves it came from the system: a
  signature header and the secret, a shared token, a source address list.
  `nothing` is a finding.
- **Replay protection**: a timestamp window, an event id stored once.
- **Order and duplicates**: providers send events late, twice and out of
  order; the handler must cope (store the event id, compare versions or
  timestamps).

## Worked example

```text
## Card payments provider (IN-002)

| Property | Value | Source | Known by | Conf. | Status |
| Direction | both: payment links out, payment events in | payments/link.php, hooks/pay.php | code | high | confirmed |
| Auth | API key out; incoming events unsigned | config.php:12 (key in code), hooks/pay.php (no check) | code | high | confirmed |
| Environments | sandbox account exists, owner has access | owner, stop 1 | running | high | confirmed |

### Exchanges
| invoices.integration.1 | Create payment link | out | Send action when the customer pays by card | invoice number, amount in cents, currency, return URL | link URL, provider id | provider idempotency key = invoice id + attempt | 10 s | 2, on network errors and 5xx | invoice sent without link; accountant sees "link missing", can retry | payments/link.php:20 | code | high | confirmed |

### Incoming events
| invoices.integration.5 | payment succeeded | signature header with the endpoint secret (old: none) | event id stored once; 5-minute timestamp window | record payment (invoices.service.9) | duplicates ignored by event id; a payment for a void invoice is refunded and flagged | hooks/pay.php | code | high | confirmed |
```

## Checks before the file is done

- Every `IN` map row owned by the module has a section.
- Every outgoing call has a timeout, a retry rule and a failure path.
- Every incoming call has verification and replay protection, or a
  finding.
- Every credential's current location is recorded, none is copied.
- Every exchange has a captured example or a reason there is none.
