# The four screens

Each screen is built twice, once with Navree UI and once with React and shadcn/ui, from the data and the steps below. The harness checks that the two builds agree before it measures anything (the "parity check" in `README.md`), so this file is the single description both are held to.

Both builds are bare pages: a heading and the controls, with no app shell, so what is measured is the components and not the chrome around them. The window is 1280 by 800 pixels in both profiles. The clock for "today" is fixed at 3 November 2026 where a date picker needs it.

## Shared data

Three plain text files in `data/`, one entry per line, read by the Navree page (in Rust), the React build (in TypeScript) and the harness (in JavaScript):

| File | Lines | Used for |
|---|---|---|
| `cities.txt` | 300 | The options of the select and the combobox, in file order, and the cities of the table. |
| `first-names.txt` | 40 | Customer first names. |
| `surnames.txt` | 25 | Customer surnames. |

The 500 table rows are made from them with integer arithmetic only. For `i` from 0 to 499:

| Column | Value |
|---|---|
| id | `i + 1` |
| Name | `first[i % 40] + " " + surname[(i * 7 + i / 40) % 25]` (integer division) |
| City | `city[(i * 17) % 300]` |
| Amount | `((i * 7919 + 211) % 503) * 4217 + 12500` rupees, written with lakh grouping: `₹12,500`, `₹1,00,000`, `₹21,29,334` |
| Status | `["Paid", "Pending", "Overdue", "Refunded"][(i * 3 + i / 5) % 4]` |

Every name is different and every amount is different, so a sort has one right answer.

## 1. Table

500 rows, 50 to a page, ten pages. Columns in this order: Name, City, Amount (right aligned), Status (a badge). Name, City and Amount sort. A search box filters the Name column only, ignoring case, by whether the name contains the text.

| Step | Action | Expected state |
|---|---|---|
| Filter (first interaction) | Type `kapoor` into the search box | Only the Kapoor rows, first row as the data gives it |
| Sort | Click the Amount heading once | `aria-sort="ascending"` on Amount, rows in rising amount |
| Next page | Click Next | Page 2 of the sorted rows, page marker `2` |
| Filter | Type `kapoor` into the search box (page 1 again) | The Kapoor rows in rising amount |

What the page must expose, on both builds:

- The whole table sits in an element with the id `cmp-table`.
- The search box is an `input` inside an element with `role="search"`.
- A `th` for each column has `data-col` set to `name`, `city`, `amount` or `status`, and `aria-sort` when it is the sort.
- Each body row is a `tr` with `data-key` set to the row id; the first cell is the name.
- A `nav` with `aria-label="Pagination"` holds the control labelled `Next page` and, when there is more than one page, an element with `aria-current="page"` showing the page number.

How the two builds differ by design: the Navree table asks the server for each sort, search and page and swaps the answer in, as it does with JavaScript off; the React table holds all 500 rows and does the work in the browser.

## 2. Select and combobox

A heading, a select and a combobox, each with the same 300 cities in file order.

| Step | Action | Expected state |
|---|---|---|
| Open the select (first interaction) | Click the select | The list of cities is showing |
| Choose in the select | Click `Ahmedabad` | The select shows `Ahmedabad` |
| Open the combobox | Click the combobox | Its list is showing |
| Type in the combobox | Type `Ludh` | The first option is `Ludhiana` |
| Choose in the combobox | Click `Ludhiana` | The combobox shows `Ludhiana` |

What the page must expose:

- The select's button has the id `cmp-select-trigger` and shows `Choose a city` until a city is chosen.
- The Navree combobox is an input with the id `cmp-combo-input` that shows the chosen city as its value. The React combobox is a button with the id `cmp-combo-trigger` that shows the chosen city as its text, and opens a popover with a search box.

How the two builds differ by design: the shadcn select is a list to choose from and does not filter as you type, so the select steps are open and choose only; the filtering step belongs to the combobox on both sides.

## 3. Date picker

A heading and one date picker, empty, showing `Pick a date`, opening on November 2026.

| Step | Action | Expected state |
|---|---|---|
| Open (first interaction) | Click the picker | The calendar is showing, captioned `November 2026` |
| Next month | Click the next month button | The caption reads `December 2026` |
| Choose a day | Click 18 | The picker shows `12/18/2026` |

What the page must expose:

- The Navree picker has the id `cmp-date`; the React picker's button has the id `cmp-date-trigger`.
- The calendar's caption is the month and year as text.
- Weeks start on Sunday in both.

## 4. Tabs and a menu

A heading, three tabs, a dropdown menu and a line of status text.

| Step | Action | Expected state |
|---|---|---|
| Open the menu (first interaction) | Click `Actions` | The menu is showing |
| Switch tab | Click `Activity` | `Activity` is the selected tab and its panel, `The activity panel.`, is showing |
| Open the menu | Click `Actions` | The menu is showing |
| Choose an item | Click `Duplicate` | The status line reads `Chosen: Duplicate` |

The tabs are `Overview` (`The overview panel.`), `Activity` (`The activity panel.`) and `Settings` (`The settings panel.`), `Overview` selected first. The menu has three items, `Edit`, `Duplicate` and `Archive`; choosing one writes `Chosen: ` and its name into the status line, which starts as `Nothing chosen yet.`

What the page must expose:

- The menu button has the id `cmp-menu-trigger`; the status line has the id `cmp-chosen`.
- Tabs, tab panels, the menu and its items carry the usual `tab`, `tabpanel`, `menu` and `menuitem` roles.

## Where a step ends

A step has ended when its expected state is on the screen and two animation frames have been drawn after it appeared. The harness reads the state from the page itself, with a mutation observer, so it needs no more than the markers above and the visible text.
