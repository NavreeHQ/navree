# Template: the inventory

`01-inventory/INVENTORY.md` lists everything the design holds, with ids,
before anything is judged. Later phases tick rows off it; a row nobody
maps is a gap in the work.

## Format

```text
# Inventory

Design: design/source (INTAKE.md: 23 files, hash 4f2a...)
Commands:
  find design/source -name "*.html" | sort        -> 6 screens
  rg -c "<table" design/source                   -> 3 tables
  rg -c "<form|<input|<select|<textarea" design/source -> 2 forms, 14 fields

## Screens

| ID | Screen | File or frame | Route (proposed) | Mode | Shows | States drawn | States missing | Shots | Source | Known by | Conf. | Status |

## Repeated components

| ID | Component | Used on | Anatomy | Variants and states drawn | Source | Known by | Conf. | Status |

## Navigation

| ID | Item | Group | Target screen | Icon | Count shown | Source | Known by | Conf. | Status |

## Forms

| ID | Screen | Field | Control drawn | Data it holds | Required | Choices drawn | Messages drawn | Source | Known by | Conf. | Status |

## Tables

| ID | Screen | Columns | Sort drawn | Filters drawn | Search | Paging drawn | Selection and bulk | Row actions | Source | Known by | Conf. | Status |

## Actions

| ID | Screen | Action | Control | What it implies | Source | Known by | Conf. | Status |

## Implied data

| ID | Screen | Value shown | Implies | Source | Known by | Conf. | Status |

## Raw values

| ID | Kind | Value | Count | Where | Source | Known by | Conf. | Status |

## Questions
## Deviations
## Gaps
```

## The columns

- **Mode** is the owner's keep or translate choice from `INTAKE.md`.
- **States drawn / missing**: of default, empty, loading, error,
  read-only and forbidden, which the design shows and which it lacks. A
  missing state is not a gap: it is work for the page spec.
- **Shots**: the `BASE-` screenshots of the screen at 1440 and 390
  pixels, and of each drawn state (`BASE-04-1440`, `BASE-04-390`,
  `BASE-04-empty-1440`). A design with no phone frame gets no 390 shot of
  its own: say "none drawn".
- **Repeated components** are patterns on two or more screens (a stat
  tile, a filter chip, a page header). Each becomes one row of
  `COMPONENTS.md`, so every page maps it the same way.
- **What it implies / Implies**: the server work behind the pixels. "+12%
  this week" implies two counts over two windows in a zone; "Assign to"
  implies a list of users this user may assign to and a permission. Say
  only what the design shows; a guess is `inferred` and a question.
- **Raw values** feed `tokens.md`: every colour, font, size, weight, line
  height, gap, radius, shadow and duration, with its count.

## Worked example

```text
## Screens
| DS-04 | Deals | source/deals.html, frame "Deals / list" | /deals | keep | table of deals, 4 stat tiles, New deal | default | empty, loading, error, read-only, forbidden | BASE-04-1440, BASE-04-390 | source/deals.html | file | high | confirmed |

## Repeated components
| DC-03 | Stat tile | DS-01, DS-04 | label, figure, change with arrow | up, down | source/deals.html:40 | file | high | confirmed |

## Tables
| DS-04.t1 | DS-04 | Deal, Company, Stage, Value, Close date | arrows on all five | stage chips | yes | none drawn | checkbox column, "Archive" | "..." menu: View, Edit, Delete | source/deals.html:88 | file | high | confirmed |

## Implied data
| DS-04.d2 | DS-04 | "Pipeline $4.2M +3.1%" | sum of open deal values now and 7 days ago, in one currency | source/deals.html:52 | inferred | medium | question Q-03 (which currency, whose week) |
```

## Checks before the file is done

- Every file and frame in `source/` is a screen row, or listed as not a
  screen (an asset, a style guide) with its reason.
- Every screen has its shots at 1440 and 390, or "none drawn".
- Every form field, table, action and figure on every screen has a row.
- Every raw value counted by the commands has a row.
- The file ends with `## Questions`, `## Deviations` and `## Gaps`; an
  empty one says "none".
