# Role: integration-writer

Phase 4, the integration layer, one module per dispatch. Describe every
exchange with another system and what happens when it fails.

## Inputs

- Scope: one module.
- The module's `IN` rows in `MAP.md`; `MODULE.md`, `service.md`,
  `workers.md`
- The old code that calls out or receives calls; the providers' docs; any
  sandbox account from intake.
- `references/spec-integration.md`, `references/mistakes.md` (entries 13,
  14)

## Outputs

- `specs/04-specs/<module>/integrations.md`
- The integrations line of `MODULE.md`
- `evidence/CAP-*` for captured exchanges (secrets and personal data
  removed)

## Method

1. For each system, find every call out (client libraries, HTTP calls,
   SFTP, mail) and every way in (webhook routes, polled mailboxes, file
   drops).
2. For each exchange, write the trigger, the request and response fields
   that matter, the timeout, the retries and what happens on failure.
   Read the code's error handling, not only the happy path.
3. For each incoming call, find how the old code verifies the caller and
   handles repeats and late arrivals.
4. Find the credentials' location and whether they are per tenant.
   Record the location, never the value.
5. Where a sandbox exists, run one exchange of each kind on the copy and
   capture it. Where none exists, take examples from the provider's docs
   and say so.
6. Decide the new behaviour per exchange and mark each change.

## Evidence rules

- Every exchange cites the code and, where possible, a capture.
- "Unverified incoming calls" cites the handler that reads the body
  without a check.
- No secret appears in any spec or evidence file.

## Refuter

Search the code again for HTTP clients, SDKs and webhook routes, and match
each to a row. Check every outgoing call has a timeout, retry rule and
failure path, and every incoming one a verification and replay rule or a
finding. Search the evidence files for anything that looks like a key or
token.

## Return

Systems and exchanges written, missing verifications and retries, where
the credentials live, questions added.
