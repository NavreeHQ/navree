# Task list and test cases

Phase 8 turns the confirmed specs into `08-plan/TASKS.md` (what to build,
in order) and `08-plan/TESTS.md` (how each spec row is proved). Phase 10
runs them and closes rows in `10-verify/VERIFY.md` and `PARITY.md`. A spec
row with no test can't be verified, so it can't be done.

## TASKS.md

```text
| ID | Module | Layer | Task | Spec rows | Tests | Skill | Depends on | Status | Commit |
```

- One task is one reviewable change that ends green on `navree gate`:
  a model and its migration, a service rule and its tests, a list route, a
  job, a Helm resource, an import command.
- **Order:** modules in the build order of `MODULES.md`; inside a module,
  the layer order in `navree-mapping.md` (models, access, services, routes,
  workers, workflows, integrations, reports, screens, isolation).
- **Tests** lists the test ids the task makes pass. A task is written test
  first: the tests exist and fail before the code.
- **Skill** names the Navree skill the builder loads for it.
- **Status:** `todo`, `doing`, `built` (tests pass, gate green, commit
  recorded), `verified` (the verifier re-ran its tests).

## TESTS.md

```text
| ID | Kind | Covers | Level | Given | When | Then | Data | Status | Evidence |
```

- **Kind:** `acceptance`, `parity`, `role`, `tenancy`, `migration`, `edge`.
- **Covers:** the spec row ids (and journey ids) this test proves. Every
  spec row is covered by at least one test.
- **Level:** `unit` (a pure function: a cleaning rule, a calculation),
  `service` (a service on a test database), `http` (`TestApp`, acting as a
  user), `browser` (a journey through the front end), `sql` (a query pair
  on the old copy and the new database), `old` (a request to the old
  product).
- **Given / When / Then:** concrete: real values from the profile and the
  spec examples, not "valid data".
- **Data:** a factory, a seeder, a fixture of old rows, or the migrated
  copy.
- **Status / Evidence:** filled in phase 10: `PASS` with the command and
  its result line, `FAIL` with the output, `N/A` with the owner's
  acceptance, `NOT RUN` (which blocks done).

## From spec rows to test cases

| Spec row | Tests to write |
|---|---|
| CRUD field | each rule at its boundary (max length, max length + 1, empty when required); the type (a date in the old wrong formats is refused); a select's value outside its options is refused |
| CRUD operation | it works for an allowed role and scope; refused (403) for a role without it; 404 for a record outside the user's reach; its side effects happen (rows, events, jobs, mail in the fake) |
| List | each filter alone and two together; each sort both ways; paging (page 2 holds the right rows, the total is right); search; the record scope per role; related columns load in a bounded number of queries |
| Option source | the options endpoint returns matches for a search, at most a page, only rows the user may see |
| Screen | each journey end to end (browser level for the main ones); the forbidden state; the empty state |
| Service rule | each example from the spec as given; each refusal with its error code |
| Calculation | each worked example, to the cent, with the spec's rounding |
| Worker | runs once and does its effects; runs twice and the second run changes nothing; a failure retries and then lands in failed jobs; the schedule's next run in the right zone |
| Workflow | every allowed transition by the allowed role; every move not in the table refused; guards; timers fire, and do nothing if the record moved on |
| Report | each known old number for its closed period, exactly; the scope per role; day boundaries in the tenant's zone |
| Integration | success with the HTTP fake; time-out and 5xx retried; 4xx not retried; an incoming event with a bad signature refused; a duplicate event ignored |
| Role cell | one `role` test per matrix cell and per sensitive field |
| Tenancy | the module's isolation suite; any old leak from `TENANCY.md` as its own test |
| Migration rule | the cleaning function on real old rows (one per rule); the full run's checks from `MIGRATION.md`; a second run changes nothing |

Add `edge` tests where the old product broke or the data profile points:
empty tables, the profile's largest volumes (a list over 60 000 rows must
still page fast), long text, names in other scripts, a time zone day
boundary, two users saving the same record, a double click on a submit.

## Parity tests

A parity test runs the same case on the old product and the new one and
compares the outcome. It catches rules nobody wrote down.

1. **Pick cases** from the journeys, the reports' known old numbers, the
   calculations' examples and the most used routes.
2. **Capture the old side** on the running copy: the request and the
   response (`evidence/CAP-*`), the rows it changed (an SQL query before
   and after), the mail it sent (the mail catcher). Capture once and keep
   the files; the old product doesn't need to run during phase 10 when the
   captures exist.
3. **Run the new side** against the migrated copy of the same data, or a
   fixture built from the same old rows.
4. **Compare what matters:** values, totals, statuses, rows created, mail
   recipients and subjects. Normalise what must differ: ids, timestamps,
   formats (`12,50` and `12.50`), field names. Write the normalisation in
   the test case so it can't hide a real difference.
5. **Deliberate changes** (from the modules' tables) expect the new
   behaviour, and cite the change.

Where the old product can't run, the case is `N/A` with the reason and the
owner's acceptance at stop 4; never skip it silently.

`PARITY.md`:

```text
| Test | Case | Old (evidence) | New (evidence) | Normalised | Result | Note |
| T-210 | Invoice #4411 total | 1 433.9999 (CAP-034) | 143400 cents (test output) | old x 100 rounded half up | PASS | |
| T-212 | Monthly sales, North, 2025-09 | 1 284 331.20 (SHOT-090) | 128433120 cents | x 100 | PASS | |
| T-215 | Void a paid invoice as accountant | allowed, status cancel (CAP-040) | 403 (test output) | | PASS (deliberate change invoices.workflow.13) | |
```

## The role matrix test

One test per cell of `ROLES.md` is a lot of tests; write them as tables.
Two levels:

- **Rules:** call the abilities function with each role's permissions and
  a record in each state, and check every cell. Fast, and it covers record
  conditions.
- **HTTP:** for each route in `navree routes`, as a user of each role with
  a record inside and outside their scope, expect 2xx, 403 or 404
  (`navree-testing`: `TestApp`, `acting_as`). This proves the routes carry
  the right attributes and lists filter by scope.

```rust
use navree::access::{AbilityCtx, READ, Rules, field};

pub const VOID: &str = "void";

pub fn define(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules); // invoices.read, invoices.void, ... from the user's roles
    if let Some(user) = &ctx.user
        && user.has_role("sales_rep")
    {
        rules.can(READ, "Invoice").when(field("rep_id").eq(user.id)); // own customers' invoices
    }
    rules
        .cannot(VOID, "Invoice")
        .when(field("status").is_in(["draft", "paid", "void"]))
        .because("Only sent or overdue invoices can be voided.");
}

#[cfg(test)]
mod role_matrix {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use navree::access::{AbilityCtx, READ, Rules};
    use serde_json::json;
    use uuid::Uuid;

    use super::{VOID, define};

    /// ROLES.md roles.12 to roles.14: role, its permissions, action, record, allowed.
    #[test]
    fn invoice_cells_match_the_roles_spec() {
        let me = Uuid::now_v7();
        let subjects = Arc::new(BTreeMap::from([("invoices".to_owned(), "Invoice".to_owned())]));
        let mine = json!({ "status": "sent", "rep_id": me });
        let paid = json!({ "status": "paid", "rep_id": me });
        let theirs = json!({ "status": "sent", "rep_id": Uuid::now_v7() });
        let accountant = ["invoices.read", "invoices.void"];
        let cases = [
            ("accountant", &accountant[..], READ, &theirs, true),
            ("accountant", &accountant[..], VOID, &mine, true),
            ("accountant", &accountant[..], VOID, &paid, false),
            ("sales_rep", &[][..], READ, &mine, true),
            ("sales_rep", &[][..], READ, &theirs, false),
            ("sales_rep", &[][..], VOID, &mine, false),
        ];
        for (role, permissions, action, record, allowed) in cases {
            let permissions = permissions.iter().map(|p| (*p).to_owned()).collect();
            let ctx = AbilityCtx::for_user(me, vec![role.to_owned()], permissions)
                .with_subjects(subjects.clone());
            let mut rules = Rules::new();
            define(&ctx, &mut rules);
            let ability = rules.build();
            assert_eq!(ability.can(action, "Invoice", Some(record)), allowed, "{role} {action} {record}");
        }
    }
}
```

## Tenancy tests

Every workspace-owned module gets an isolation suite
(`app.isolation_suite::<InvoicesModule>()` with a factory per record type;
`navree-module-conformity`): from workspace A, every route of the module
with workspace B's records must answer 404, lists must not show B's rows,
and a create naming B's workspace must stay in A. Add a test for each leak
found in the old product (`TENANCY.md`), named after it.

## Migration checks

From `MIGRATION.md`, as `sql` level tests run after each rehearsal:

- each cleaning rule's function on real old rows (unit level, in the
  module's tests);
- the counts, sums and statuses per tenant and per month, old query against
  new query, exactly or within an approved tolerance;
- orphans in the new database: zero;
- a second run on the same copy: nothing created, changed or rejected;
- the old product's reports for a closed period against the new ones.

Results go in `MIGRATION_RUN.md` (`data-migration.md`, section 5).

## VERIFY.md

```text
| Spec row | Tests | Result | Evidence | Verified on (commit) | By |
| invoices.crud.7 | T-031, T-032 | PASS | cargo test invoices::status -> 2 passed (log 10-verify/logs/T-031.txt) | a1b2c3d | verifier |
```

Every row quotes the result line of the command that ran, on the final
commit. A row changed after it was verified is stale: run its tests again.
