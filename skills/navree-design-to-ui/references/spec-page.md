# Template: a page spec

`03-mapping/pages/<page>.md` says how one screen of the design becomes one
Navree page: its frame, every element and its component, every value and
its source, every action and its route, and every state. The builder
builds from this file alone; the verifier checks against it.

`03-mapping/COMPONENTS.md` holds the repeated components (`DC-` rows)
once, with their navree-ui form; a page row points at them instead of
mapping them again.

## Format

```text
# <Page name> (<page id>, screen <DS-id>, mode keep|translate)

Route: GET /deals    Module: deals    Access: #[can(Read, Deal)]
From reverse engineering: specs/04-specs/deals/screens.md SC-041, lists.md deals.list.1..12 (or "none")

## Frame
| ID | Part | Design | Navree | Source | Known by | Conf. | Status |

## Elements
| ID | Element | Design | Navree | Data (source) | Mistakes | Source | Known by | Conf. | Status |

## Actions
| ID | Action | Control | Who | Enabled when | Route | Answer | Confirm | Source | Known by | Conf. | Status |

## States
| ID | State | How it is reached | What the page shows | Navree | Source | Known by | Conf. | Status |

## Phone (390 pixels)
| ID | Part | What changes | Navree | Source | Known by | Conf. | Status |

## Skills
<the Navree skills the builder loads for this page>

## Questions
## Deviations
## Gaps
```

## The columns

- **Frame**: the shell's parts this page uses: its sidebar item (active),
  title, breadcrumbs, page actions.
- **Navree**: the component and its builder calls, a form field and its
  attributes, or a table column and its `#[col(..)]`, exact enough to
  type (`Badge::new(..).variant(BadgeVariant::Warning)`,
  `#[col(sortable, filter = multi, options = STAGES, badge = tone)]`).
  A gap writes `gap GAP-n` and the interim form, if the owner chose one.
- **Data (source)**: where the value comes from: a model field, a service
  method, a query, or a reverse-engineering spec row. A value with no
  source is a question.
- **Mistakes**: the number of each `mistakes.md` entry the row fixes.
- **Actions**: the route each one calls, the access it needs, the state
  that enables it (checked again on the server), and what the user sees
  after: a redirect and a toast, a part redrawn, the form with errors.
- **States**: default, empty, loading, error, read-only and forbidden,
  every one, plus any the design adds (a filter that matches nothing, a
  selection across pages, an item being saved). Each says how the
  verifier reaches it.
- **Phone**: what changes at 390 pixels: columns hidden, a grid that
  stacks, actions that move into a menu.

## Worked example

```text
# Deals (deals-index, screen DS-04, mode keep)

Route: GET /deals    Module: deals    Access: #[can(Read, Deal)]
From reverse engineering: none

## Frame
| deals-index.1 | sidebar | "Deals" active, count | NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE) | BASE-04-1440 | shot | high | confirmed |
| deals-index.2 | header | title "Deals", button "New deal" | shell.page("Deals").actions(Button::primary("New deal").icon(icons::PLUS).href("/deals/new")) | source/deals.html:12 | file | high | confirmed |

## Elements
| deals-index.5 | stage chip | pill, green for won | #[col(badge = tone)]: Badge, BadgeVariant::Success for won | Deal.stage | 15 | source/deals.html:101 | file | high | confirmed (DV-03) |
| deals-index.6 | value column | "$12,500" right-aligned | #[col(sortable, numeric, filter = range, kind = money)] amount: Money | Deal.amount (minor units) | | source/deals.html:104 | file | high | confirmed |
| deals-index.9 | stat "Pipeline" | "$4.2M +3.1%" | Stat::new("Pipeline", ..).trend(.., true) | DealService::pipeline (open deals, workspace zone, 7 days) | 9 | source/deals.html:52 | inferred | medium | question Q-03 |

## Actions
| deals-index.12 | Archive selected | bulk button | sales manager | rows selected | POST /deals/bulk (archive) | toast "Archived {n}", table redrawn | yes, with the count | source/deals.html:80 | file | high | confirmed |

## States
| deals-index.20 | empty | a workspace with no deals | "No deals yet" and New deal | DataTable::empty(Empty::new("No deals yet").actions(..)) | none drawn | inferred | high | confirmed |
| deals-index.21 | read-only | a viewer | no checkboxes, no row menu, no New deal | can_select(false), no row_actions, no actions | none drawn | inferred | high | confirmed |
| deals-index.22 | forbidden | a user with no deals.read | 403 from #[can], drawn by UiModule as a page in the shell | the error page; inside a card, `Forbidden::new().what("deals")` | none drawn | inferred | high | confirmed |

## Phone (390 pixels)
| deals-index.30 | table | Company and Close date hidden; Deal sticky | #[col(hide_on_mobile)], #[col(sticky)] | BASE-04-390 | shot | high | confirmed |

## Skills
navree-orm, navree-access-control, navree-design-to-ui (building-pages.md)
```

## Checks before the file is done

- Every inventory row of this screen appears here, or is `dropped` with a
  reason.
- Every repeated component points at its `DC-` row.
- Every element has a Navree form or a gap; nothing says "custom".
- Every value names its source; every action its route, access and
  answer.
- All six states have rows, and each says how to reach it.
- The phone section exists, even if it says "no change".
- The file ends with `## Questions`, `## Deviations` and `## Gaps`.
