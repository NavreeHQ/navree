# Template: roles and permissions

`05-cross/ROLES.md` says who may do what to which records. It is a matrix:
each role, each action on each kind of record, and the scope of records the
role may touch. Old products often check roles only in templates (a hidden
button), compare role names in code (`if ($user->role == 'admin')`), or not
at all. The new product checks every action on the server, and the role
matrix test proves each cell.

## Format

```text
## Roles

| ID | Role | Who holds it | Users today | Old form | Level | Source | Known by | Conf. | Status |

## Record scopes

| Scope | Meaning | Defined by |

## Matrix

| ID | Subject | Action | <role 1> | <role 2> | ... | Old enforcement | Source | Known by | Conf. | Status |

## Fields

| ID | Subject.field | Read by | Changed by | Old enforcement | Source | Known by | Conf. | Status |

## Rules that are not a cell

| ID | Rule | Source | Known by | Conf. | Status |
```

## The columns

- **Old form**: a role column, a group table, a bit mask, hard-coded user
  ids or emails, "everyone logged in". Record the exact check and its
  `file:line`.
- **Level**: `app` (staff of the whole product, such as support),
  `workspace` (a member's role in one tenant), or both. This decides
  whether it becomes an app-wide role or a workspace role.
- **Record scopes**: name each scope once and reuse the names in the
  matrix: `all` (every record the tenant has), `own` (`owner_id` is the
  user), `team` (the user's team), `assigned` (the user is assigned),
  `none`. Say which field defines it.
- **Matrix cell**: a scope, or `none`, plus a condition when there is one
  (`own, while draft`). Actions are `read`, `create`, `update`, `delete`,
  and every custom action from the CRUD and workflow specs (`send`,
  `void`, `approve`, `export`, `import`).
- **Old enforcement**: `server` (a check before the change), `template`
  (only hidden), `none`. Every `template` and `none` cell the owner wants
  enforced is a deliberate change.
- **Fields**: sensitive fields with their own rules: cost prices, salaries,
  personal data, credit limits. Hidden from a role means not sent, not just
  not drawn; changed by a role means other roles get a refusal if they send
  it.
- **Rules that are not a cell**: who may grant roles, the last owner can't
  leave, nobody approves their own request, a user can't raise their own
  limit.

## Worked example

```text
## Roles
| roles.1 | owner | the branch manager who signs contracts | 3 | users.level = 9 | workspace | users table; SQL-050 | data | high | confirmed |
| roles.2 | accountant | finance staff | 7 | users.level = 5 | workspace | SQL-050 | data | high | confirmed |
| roles.3 | sales rep | field sales | 41 | users.level = 1 | workspace | SQL-050 | data | high | confirmed |
| roles.4 | support | the vendor's support staff, every branch | 2 | hard-coded emails in auth.php:18 | app | auth.php:18 | code | high | confirmed |

## Record scopes
| own customers | customers.rep_id is the user | customers.crud.4 |

## Matrix
| ID | Subject | Action | owner | accountant | sales rep | support | Old enforcement | Source | Known by | Conf. | Status |
| roles.12 | Invoice | read | all | all | own customers | all, read only | template (list), none (view.php?id=) | view.php:3 | code | high | confirmed |
| roles.13 | Invoice | send | all | all | none | none | template | view.php:140 | code | high | confirmed |
| roles.14 | Invoice | void | all | all | none | none | none | actions.php:30 | code | high | confirmed |
| roles.15 | Invoice | export | all | all | own customers | none | none (export ignored the scope) | list.php?export=1 | code | high | confirmed |

## Fields
| roles.30 | Product.cost_price | owner, accountant | owner | sent to everyone in list JSON | CAP-061 | running | high | confirmed |

## Rules that are not a cell
| roles.40 | Only an owner may give the owner role | admin/users.php:50 (any level 5+ could) | code | high | confirmed |
```

## Checks before the file is done

- Every role found in the data and the code has a row, with its user
  count.
- Every action in every CRUD, workflow, list (bulk, export) and screen
  spec appears in the matrix for every role.
- Every cell is a named scope or `none`; no empty cells.
- Every sensitive field has a row.
- Every `template` or `none` enforcement the owner wants enforced is listed
  as a deliberate change.
- The file ends with `## Questions`, `## Deliberate changes` and
  `## New map rows` (formats in `spec-module.md`); an empty one says
  "none".
