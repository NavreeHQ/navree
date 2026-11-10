# Role: intake

Phase 0, once per job. Learn what the design is, copy it so it can't
move, serve the copy, and record the owner's choices.

## Inputs

- The design as handed over: a folder, an archive, a link to a design
  tool, screenshots, a hosted mock-up.
- The owner, for what only the owner knows.
- A reverse-engineering job's `specs/`, if one exists.

## Outputs

- `design/source/`: the copy (`preservation.md`)
- `design/00-intake/INTAKE.md`
- `design/STATUS.md`: phase 0 done, the next phase, the date

```text
# Intake

## The design
| Property | Value |
| Format | HTML and CSS mock-up / design-tool export / handoff bundle / screenshots |
| Tool and version | |
| Made by, date | |
| Copy | design/source: 23 files, 4.1 MB, hash 4f2a... |
| Served at | http://127.0.0.1:8091/ (python3 -m http.server 8091 --bind 127.0.0.1 --directory design/source) |
| Widths drawn | 1440; no phone frames |
| Themes drawn | light only |
| Can't be opened | the prototype's flows (link only): asked for an export, Q-01 |

## The product
| Property | Value |
| Product, users | |
| Navree app | path, or "to be created with navree new" |
| Reverse-engineering specs | specs/ at <path>, or none |
| Data behind the screens | existing models / from the reverse-engineering specs / to be designed |

## Screens and modes
| Screen (file or frame) | Mode (keep / translate) | Order | Owner's note |

## Stop points
| Stop | Date | Owner's answer |
```

## Method

1. Copy the design into `design/source/` and check the copy with
   `diff -r`. Record size, count and hash.
2. Serve the copy and open every screen. List what doesn't open, what
   needs a click to show, and what is only a picture.
3. For a design-tool link, ask for exports of every frame at 1440 and 390
   pixels (and HTML or SVG where the tool makes them). Never work from
   the live file.
4. Ask the owner, per screen: keep or translate, and the build order.
   Ask which screens are final and which are ideas.
5. Find the product's data: an existing Navree app's models, a
   reverse-engineering job's CRUD and list specs, or nothing yet. Write
   which.

## Evidence rules

- Every property cites where it came from: a file, the owner (with the
  date), a command's output.
- What the owner hasn't answered is a question, not a default.

## Refuter

The refuter re-copies the design from the original into a scratch folder
and compares hashes, opens every screen from the served copy, and checks
that every screen in `source/` has a mode.

## Return

The copy's size and hash, the screens and their modes, what can't be
opened, questions added.
