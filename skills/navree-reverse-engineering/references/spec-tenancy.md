# Template: tenancy

`05-cross/TENANCY.md` says whose data each row is and how tenants stay
apart. Old products show three shapes: one install per customer (a
database each), one database with a `company_id` column that some queries
forget, or no tenants at all. The new product names a tenant (in Navree, a
workspace, optionally under an organization), scopes every query to it,
and proves with isolation tests that no route reads, changes or confirms
another tenant's rows.

## Format

```text
## The tenant

| Property | Value | Source | Known by | Conf. | Status |
| Old tenant | <the column, database or install that separates customers> | | | | |
| New tenant | workspace / organization with workspaces / none | | | | |
| Above it | <a group of tenants that shares users or settings, if any> | | | | |
| Below it | <divisions inside a tenant that limit what members see: teams, regions> | | | | |
| Membership | how users join, whether one user belongs to several tenants | | | | |
| Isolation | rows scoped by column / schema per tenant / database per tenant | | | | |

## Tables

| ID | Table | Kind | Old tenant column | Gaps found | Source | Known by | Conf. | Status |

## Leaks in the old product

| ID | Where | What leaks | Evidence | Source | Known by | Conf. | Status |
```

## The columns

- **Kind** per table: `tenant-owned` (each row belongs to one tenant),
  `shared` (every tenant reads the same rows: countries, currencies),
  `library` (global rows plus each tenant's own copies or additions), or
  `the tenant itself`. Every table has exactly one kind.
- **Old tenant column**: the column, or `none` (then how the old product
  decided ownership: through a parent row, through the user).
- **Gaps found**: queries in the old code that read this table without the
  tenant condition, with `file:line`. Search every query that touches the
  table. Each gap is a leak row unless the query is a deliberate
  cross-tenant one (an operator's report).
- **Leaks in the old product**: what one tenant could see or change of
  another's, found in code or shown on the running copy (a capture).
  These are told to the owner at stop 1 or stop 3: they may need action in
  the old product too.

## Worked example

```text
## The tenant
| Old tenant | branches.id; most tables have branch_id | SCHEMA.md | code | high | confirmed |
| New tenant | workspace per branch, one organization above | owner, stop 2 | running | high | confirmed |
| Membership | a user belongs to one branch; managers to several (users_branches) | SQL-052 | data | high | confirmed |

## Tables
| tenancy.3 | invoices | tenant-owned | branch_id | invoices/search.php:15 searches all branches | invoices/search.php:15 | code | high | confirmed |
| tenancy.4 | products | library | branch_id null = shared catalogue | | SQL-053 | data | medium | question Q-002 |
| tenancy.5 | currencies | shared | none | | | code | high | confirmed |

## Leaks in the old product
| tenancy.20 | invoices/view.php?id= | any user opens any branch's invoice by id | CAP-070 (user of North opens a South invoice) | view.php:3 | running | high | confirmed |
```

## Checks before the file is done

- Every table in `SCHEMA.md` has a kind.
- Every tenant-owned table's queries in the old code were searched for the
  tenant condition.
- Every leak has evidence and was shown to the owner.
- Membership and any division below the tenant are spelled out, since they
  drive the record scopes in `ROLES.md`.
