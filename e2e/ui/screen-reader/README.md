# Screen reader scripts

Written scripts for people to run with NVDA (Windows, Firefox and Chrome) and VoiceOver (macOS, Safari), one script for each family of modules in the toolkit. Each script lists the keys to press, what the page must do, and what the reader must say. At the end of each script is a table to record what you heard.

## Why these scripts exist

The browser suites in `e2e/ui/tests` check the semantics of every module: the roles, names and states in the DOM, the accessibility tree that Chromium, Firefox and WebKit build from it, and axe-core. All of that is machine-readable, and none of it says what a person hears. A screen reader reads the accessibility tree, and then it adds its own rules: what to say first, what to leave out, when to repeat, when to interrupt.

Announcements are where readers differ most, so they are where these scripts spend their time:

- Live regions: a status that changes, a count of new rows, a chart's tooltip, a toast. A region can be right in the tree and still be silent, spoken twice, or spoken over the user's own typing.
- Focus moves: when a dialog closes, a row is removed, or a page is replaced in place, the reader must say where focus landed.
- Widget patterns: a grid, a tree, a combobox, a menu bar and a slider each have a mode and keys of their own in a reader (browse mode and focus mode in NVDA, interacting in VoiceOver), and the reader decides how to enter and leave it.
- Position and state: "3 of 8", "expanded", "selected", "sorted ascending", "invalid entry". Whether these are spoken depends on the role and on the reader.

Run a script when a module changes, and before a release.

## What you need

| | Windows | macOS |
|---|---|---|
| Reader | NVDA 2024.1 or later (2025 or later is better) | VoiceOver, built in |
| Browsers | Firefox (current release) and Chrome (current release), one pass in each | Safari 17 or later |
| Keep | Note the exact versions in the sign-off block | The same |

Test on a real machine, not a virtual one without a sound path, and with the speech on. A speech viewer is not a replacement for hearing it once, but it is the record that you paste into the results table.

### Set up NVDA

1. Install NVDA from nvaccess.org, and start it. Keep the defaults: do not turn off any of the "Report ..." options in Settings > Document formatting. These scripts assume the defaults.
2. Open the speech viewer: NVDA menu > Tools > Speech viewer. Leave it open beside the browser. Copy what it shows into the results table. (To keep it from taking focus, put the browser and the viewer side by side.)
3. Turn on "Show speech viewer on startup" if you will run several scripts.
4. Learn the two modes. In **browse mode** (the default on a page) the arrow keys move a virtual cursor through the text, and letter keys jump: H for headings, B for buttons, F for form fields, T for tables, D for landmarks, K for links, L for lists. In **focus mode** the keys go to the page, which is what a widget such as a menu, a slider or a grid needs. NVDA switches to focus mode itself when focus lands on a form field or on a widget, and says a short sound for each switch. NVDA+Space switches by hand. A script says "browse mode" or "focus mode" at each step where it matters.
5. Keys used in the scripts (NVDA key is Insert or CapsLock, desktop layout):

| Task | Keys |
|---|---|
| Move focus | Tab, Shift+Tab |
| Read the line, next, previous | NVDA+Up arrow (this line), Down arrow, Up arrow (in browse mode) |
| Say what has focus | NVDA+Tab |
| Read from here | NVDA+Down arrow |
| Read the title | NVDA+T |
| Toggle browse and focus mode | NVDA+Space |
| List headings, links, form fields, buttons, landmarks | NVDA+F7 |
| Table navigation (in browse mode) | Ctrl+Alt+arrows to move by cell; row and column headers are spoken when they change |
| Stop speech | Ctrl |

6. Chrome needs accessibility switched on. NVDA turns it on by itself. If Chrome is silent on a page, open `chrome://accessibility` and choose "Enable native accessibility APIs".

### Set up VoiceOver

1. Turn it on with Command+F5 (or Touch ID triple-press). Open VoiceOver Utility (VO+F8) > Visuals and turn on **Caption panel**. The caption panel shows what is said, in a bar at the foot of the screen; copy it into the results table. Optionally also turn on Speech > "Show last phrase".
2. In Safari, open Settings > Advanced and turn on "Press Tab to highlight each item on a webpage". Without it Tab skips buttons and links.
3. VO is Control+Option (or CapsLock if you set it). Keys used in the scripts:

| Task | Keys |
|---|---|
| Move to next and previous item | VO+Right arrow, VO+Left arrow |
| Click the item with the cursor (press a button, follow a link) | VO+Space |
| Move focus between controls | Tab, Shift+Tab |
| Interact with a group, table or widget; stop interacting | VO+Shift+Down arrow; VO+Shift+Up arrow |
| Read from here | VO+A |
| Say the last phrase again | VO+Z |
| The web rotor | VO+U, then Left and Right arrow choose a list (Headings, Links, Form controls, Landmarks, Tables, Buttons...), Up and Down choose an item, Enter goes there, Escape closes |
| Table cell navigation | VO+Arrow keys while inside a table (after VO+Shift+Down arrow) |
| Open the VO menu or the help | VO+M (menu), VO+H H (help) |

4. With VoiceOver on, a key that a page handles (an arrow key in a menu, Escape in a dialog) goes to the page only when VoiceOver is not intercepting it. For arrow-key widgets, **move focus to the widget with Tab, then press the arrow key by itself** (without VO). If a widget does not respond, VoiceOver is still in its own cursor: press Tab to move focus, not VO+Right arrow.

## Start the proof apps

Most scripts use one app, the UI proof app. It has no database.

```sh
cargo run --release -p ui-proof -- serve      # http://127.0.0.1:8090
PORT=9000 cargo run --release -p ui-proof -- serve   # another port
```

The page to open in each script is a path on this app (for example `/menus`). `e2e/ui/README.md` lists what each page shows. The app's pages are linked from its sidebar, but open the path in each script directly.

A few scripts need another app. Each one says so in "Page to open":

| App | Start it | Used by |
|---|---|---|
| Data table proof, on a database | `DT_DATABASE_URL=postgres://... ui-table-proof seed 1000000` once, then `DT_DATABASE_URL=postgres://... PORT=8097 ui-table-proof serve`, http://127.0.0.1:8097 | data-table.md |
| Inbox demo | `INBOX_SCALE=2000 INBOX_SIMULATOR=off INBOX_TICK=off INBOX_TEST_HOOKS=1 cargo run --release -p inbox-demo`, http://127.0.0.1:8096/inbox | split-lazy-inbox.md |
| Board demo | `PORT=8098 BOARD_SIMULATOR=off BOARD_TEST_HOOKS=1 cargo run --release -p board-demo`, http://127.0.0.1:8098 | sortable-board.md |

The data table proof reads its database from `DT_DATABASE_URL` (a PostgreSQL address). If a port is taken, choose another with `PORT`.

The scripts for live updates, the inbox, the board and the data table use `curl` in a second terminal to do what another person would do while you listen: add a row, send a message, move a card, cut a connection. Each script lists the exact commands in "Page to open". Those test routes exist only in the proof and example apps, and the inbox and board ones only when started with the test-hook variable shown above.

## Run a script

Run one script at a time, in one reader and one browser. Open the script and the app side by side.

1. Fill in the sign-off block at the end of the script (tester, date, OS, browser and version, reader and version).
2. Start the reader, then open the page. If a script asks for it, start from a fresh load, so that nothing from an earlier page is left open.
3. Go through the numbered steps in order. Each step has the keys, what the page must do, and what the reader must say.
4. Write what you heard in the results table (copy it from the speech viewer or caption panel), mark the step **pass** or **fail**, and add a note for anything odd, even on a pass.

### How to read a step

Each step is written like this:

> **7. Open the menu**
> - Keys: NVDA: Enter. VoiceOver: VO+Space.
> - The page: the menu opens below the button, and focus moves to the first item.
> - The reader says: the menu's name, "menu", the first item's name, "menu item", and its place ("1 of 3").

Words in quotation marks are exact only where the code produces those words (the toolkit's own texts, an error message, a live-region text). Where the words come from the reader (the role names, "expanded", "selected", "3 of 8") the script lists what must be **conveyed**: the name, the role, the state, the value or the position. Readers phrase these in their own way, and the order can change between NVDA and VoiceOver, between Firefox and Chrome. A step passes when each listed thing is conveyed once, in a way a person can understand, and nothing wrong is said (the wrong name, a missing state, the same text twice).

How to mark a step:

- **Pass**: everything listed is conveyed, once.
- **Fail**: something listed is missing, wrong, repeated, said late (a live region should be spoken within about two seconds unless the step says otherwise), said when it should be silent, or focus is in a place the step does not expect.
- **N/A**: the reader or the browser cannot do what the step asks (say why in the notes). A step is never skipped without a note.

### The kinds of check you will see

- **Role, name, state**: what the control is, what it is called, and its state ("checked", "expanded", "selected", "pressed", "invalid", "dimmed").
- **Position and value**: "3 of 8", a slider's value, a progress percentage, the sort direction of a column.
- **Live announcements**: text the page puts in a live region. Each such step says how soon, and says if it must be said once. If the reader says it twice, that is a failure.
- **Focus return**: after a dialog, menu, popover or sheet closes, focus goes back to what opened it, and the reader says so.
- **No double announcement**: when a control has both a label and a description, or a tooltip, the reader must not say the same words twice in one go.
- **Errors**: the error text is spoken when the field gets focus, and (where the step says) when the error first appears.
- **JavaScript off**: the pages that claim to work without script. Turn script off in the browser (Firefox: `javascript.enabled` false in `about:config`; Chrome: Settings > Privacy and security > Site settings > JavaScript; Safari: Develop > Disable JavaScript), reload, and run the steps marked "JavaScript off". Turn it on again afterwards.

## Record a result

Each script ends with a sign-off block and a results table with one row for each step. The same layout is in `results-template.md`.

| Column | Put in it |
|---|---|
| Step | The step number from the script |
| Expected | What the script says must be conveyed (already filled in) |
| NVDA heard / pass | What NVDA said (paste from the speech viewer), then **pass**, **fail** or **n/a**. Say which browser if the two differ |
| VoiceOver heard / pass | The same for VoiceOver in Safari |
| Notes | Anything odd, a version, a workaround, the issue number |

At the top of the table fill in the tester, the date, the operating system, each browser and version, and each reader and version. Keep the filled copy in the pull request or in the release's folder of test records; do not edit the script itself to record a result.

A run is complete when every step has a result for NVDA (in Firefox and in Chrome) and for VoiceOver (in Safari).

## When a step fails

A failing line is a bug in the module, not in the script. The scripts are written from what the code renders, so the code is the thing to change.

1. Check that the step was run as written: the right page, the right mode, the keys exactly as given, script on or off as the step says. Run the step once more.
2. If it fails again, open an issue with: the module name, the script file and the step number, the reader and browser with versions, what was heard (paste it) and what the script expected.
3. The fix goes in the module (its component, or its runtime file in `crates/navree-ui/runtime/src`), with a test in `e2e/ui/tests` for the part a browser can check, such as the role, the name or the text of a live region.
4. After the fix, run the step again in the reader and browser that failed, and in the others of that script, and record the new result.
5. Change the script only when the module's behaviour changed on purpose, and in the same change as the module.

## Scripts

Each takes 10 to 20 minutes for one reader and browser.

| Script | Modules |
|---|---|
| `base-components.md` | Button, link button, toggle and button group, text fields with help and errors, native select, Checkbox, Switch, Radio group, Alert, Badge, Spinner, Progress, Breadcrumb, Pagination, Avatar, Card, Empty, Item, a plain table, the commands that change a page with no request |
| `data-table.md` | Data table: headers, sorting, selection, bulk actions, row menu, paging, empty and failed states |
| `select-combobox-command.md` | Select, multi-select, combobox, command palette |
| `tabs-menus-toast.md` | Tabs, toggle group, dropdown menu, toasts |
| `overlays.md` | Dialog, alert dialog, sheet, drawer, popover, tooltip, hover card, accordion, collapsible, the details dialog of a table row |
| `menubar-context-navigation.md` | Context menu, menu bar, navigation menu |
| `charts.md` | The chart types, their tables, keyboard tooltips and live region, legend toggles, the report page and the big chart |
| `dates.md` | Calendar, date picker, date range, time |
| `phone.md` | Phone input |
| `slider-stepper-tree-toolbar.md` | Slider, stepper, tree view, toolbar |
| `wizard-forms.md` | Wizard, and the field errors and count announced by a form |
| `split-lazy-inbox.md` | Split panes, lazy lists, and the inbox (list, thread, reply, keys) |
| `sortable-board.md` | Sortable list and board |
| `upload.md` | Upload |
| `otp.md` | One-time code field |
| `carousel.md` | Carousel |
| `markdown-code.md` | Markdown and code blocks |
| `timeline-activity-live.md` | Timeline, activity feed, fact cards, badges and outcome chips, live feeds and live tables |
| `pages-errors.md` | Error pages, sign-in, states inside a page |
| `shell-theme-icons.md` | App shell and sidebar, skip link, theme menu, icons and flags |
| `results-template.md` | The blank results table and sign-off block |

## Rules for writing a script

- Base each expected sentence on what the code renders: the component in `crates/navree-ui/src`, the runtime in `crates/navree-ui/runtime/src`, and the tests in `e2e/ui/tests`. Never write an announcement the code does not produce.
- Quote a text only when the code makes it. Otherwise list what must be conveyed.
- One action per step, with the keys for both readers where they differ.
- Plain English, sentence case, Indian demo data.
