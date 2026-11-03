# Mistakes in designs, and their Navree form

A design shows one moment of a page: some sample rows, one user, the happy
path, a wide screen. Built as drawn, the page fails the first time the
data, the user or the screen differ. Each entry says what the mistake
looks like, how to find it in the design, and what the page does instead.

When a page spec row fixes one, write the entry's number in the row. When
the fix changes what the owner will see, add a deviation
(`preservation.md`) so the owner approves it. When the design came from a
reverse-engineering job, the same mistake may already be in that job's
`mistakes.md` with its server-side form; cite both.

## 1. The sample data taken for the data

**Looks like:** ten tidy rows, round numbers, short names, a count of
"128", one currency, every field filled.

**Detect:** every list, count, total and chart in the inventory. Ask
where each comes from and how large it grows.

**Navree form:** each value names its source in the page spec (a model
field, a service, a query). Lists come from the server a page at a time.
The verifier runs every page with 0 rows, 1 row, more than a page, very
long text and missing optional fields.

## 2. Tables that sort, filter, search or page in the browser

**Looks like:** sort arrows and a search box over a table with no page
links, or "Show all"; in an HTML mock-up, a script that sorts the rows it
has; a grid library.

**Detect:** `rg -n "sort\(|filter\(|DataTable\(|ag-grid|tabulator|list\.js" design/source`;
a table with no pagination; a filter with no URL.

**Navree form:** `#[derive(TableRow)]` and `DataTable` (`building-pages.md`):
search, filters, sort, pages and page size run on the server, on the rows
this user may read, and live in the URL. Selection across pages and bulk
actions post their state, and the server checks the count before acting.

## 3. The wrong control for the data

**Looks like:** a date typed as text, an amount in a plain text box, a
phone or email with no type, a status as free text, a yes-or-no as a
select of "Yes" and "No", a reference to another record as a text box,
long notes in a one-line input.

**Detect:** for every form field in the inventory, write the data it
holds, then compare with the control drawn.

**Navree form:** the field's Rust type picks the control (`components.md`,
Forms): `NaiveDate` a date input, `Money` an amount, `bool` a checkbox or
switch, an `Options` enum a select or radios, `widget = email | tel | url`,
`widget = textarea`, a reference a select with server-side choices.

## 4. A select over a large or growing table

**Looks like:** a "Company" or "Customer" dropdown drawn with five
entries.

**Detect:** every select whose choices are records, not a fixed list. Ask
how many there will be in a year.

**Navree form:** a few dozen at most: a select whose choices come from the
server (`FormView::options`, limited and ordered). More, or growing: a
combobox that searches on the server, which is coming
(`components.md`, Coming). Record the gap; don't ship a select of
thousands.

## 5. Options typed into the design

**Looks like:** stages, categories, owners or countries written into the
mock-up's markup or a script's array.

**Detect:** `rg -n "<option" design/source`; arrays of labels in scripts;
a filter's checkbox list.

**Navree form:** a fixed set the code owns is an `Options` enum (its
values stored, its labels in the code). A set users or admins change is
a table, read by the server for the form (`FormView::options`) and the
filter (`DataTable::options`). The design's labels become the enum's
labels or seed rows.

## 6. Missing states

**Looks like:** only the full, happy page.

**Detect:** for each screen, look for its empty, loading, error,
read-only and forbidden versions. Most designs have none.

**Navree form:** every page spec has a row for each of the five
(`spec-page.md`), built from `Empty`, `Skeleton`, `Region`, `Alert`,
`DataTable::failed`, and the route's access attribute. The verifier
screenshots each.

## 7. Errors the design never drew

**Looks like:** a form with required marks and no message under any
field; a save button with no failure.

**Detect:** for each form, look for a field in its error state and a
failed save.

**Navree form:** the struct's `#[validate]` rules run in the browser and
again on the server; `Rejected` draws the page again with each message
under its field and the values kept. Write each message in the page spec
(`#[form(error = "...")]` where the rule's own message isn't right).

## 8. Actions with no answer

**Looks like:** a button that leads nowhere in the mock-up; a delete with
no confirmation; "Archive" on a selection with no word about what
happens next.

**Detect:** for every button and link, write what it does, who may do it,
what the user sees after.

**Navree form:** each action in the page spec names its route and its
answer: `Reply::redirect` with a toast, `Reply` ops that morph the part
that changed, or the form again with errors. A destructive action has an
`AlertDialog` or `BulkAction::confirm`. A state-changing action is a POST.

## 9. Totals and charts from the rows on screen

**Looks like:** a sum under a table, a chart beside a list, a "12 overdue"
badge.

**Detect:** every figure that summarizes other data.

**Navree form:** computed on the server over the whole set the user may
see, not the page shown: `DataTable::foot`, `Stat`, `Bars`, from a
service query. Name the query in the page spec.

## 10. Permissions shown, not enforced

**Looks like:** an admin's view and a member's view, the difference being
which buttons appear.

**Detect:** screens drawn per role; buttons marked "admin only".

**Navree form:** the route's access attribute (`#[can(..)]`,
`#[permissions(..)]`) decides; the page hides what the user can't do
(`DataTable::can_select(false)`, no row actions, no New button) as a
courtesy. Every hidden action's route refuses on its own
(`navree-access-control`).

## 11. Built for one width

**Looks like:** a 1440-pixel frame and nothing else; a table with twelve
columns; fixed widths in pixels.

**Detect:** the design's width; a phone frame or none.

**Navree form:** every page works at 390 pixels with no sideways scroll of
the page: the shell turns into a drawer, `Grid` columns stack, table
columns that matter less get `hide_on_mobile`, wide tables scroll inside
their own frame, sticky first column. Screenshots at 390 are part of done.

## 12. Left to right only

**Looks like:** arrows that point "forward" to the right, text aligned
left by rule, numbers in a mixed run.

**Detect:** does the product have, or plan, Arabic, Hebrew, Persian or
Urdu users?

**Navree form:** `Direction::rtl(..)` or the page's language; components
use logical sides and the table mirrors itself. Check one page right to
left at verify.

## 13. Text that doesn't fit

**Looks like:** short English names everywhere.

**Detect:** fields whose real values are long (company names, emails,
addresses), and translations.

**Navree form:** `#[col(truncate)]` or `wrap`, long words broken, the full
value in the detail page or a `Tooltip`. The verifier fills a page with
long, Unicode and hostile strings.

## 14. Colour as the only signal, and low contrast

**Looks like:** status dots with no word; grey text on grey.

**Detect:** every colour that means something; every text pair
(`tokens.md`, Contrast).

**Navree form:** a `Badge` with its word and tone; an icon with text;
token pairs that pass 4.5:1. axe runs in every state.

## 15. Shapes the app rules refuse

**Looks like:** pill buttons and chips, round avatars with initials,
glowing focus or cards, soft shadows, corners of 8, 12 or 16 pixels.

**Detect:** `rg -n "9999px|rounded-full|box-shadow|initials" design/source`;
look at every chip, avatar and card.

**Navree form:** 4 px corners on controls, 6 px on surfaces, `Badge` for
chips, `Avatar::image` or the neutral `Avatar::placeholder`, no shadow or
glow. Each is a deviation the owner approves.

## 16. Script, style and embeds inline

**Looks like:** `onclick=`, `<script>` in the page, `style=""`, a web
font or chart from another site, an embedded map or video.

**Detect:** `rg -n "on[a-z]+=|<script|style=|https?://" design/source`.

**Navree form:** behaviour from `Cmd` (show, hide, open, copy, focus,
submit) and the runtime; style from components and tokens; files only
from the app's own origin under the content security policy. A third-party
embed is a gap for the owner.

## 17. Controls with no name

**Looks like:** icon-only buttons, an unlabeled search box, a placeholder
as the only label.

**Detect:** every control in the inventory without visible text.

**Navree form:** `Button::icon_only(icon, "Name")`, a `Field` or form label
for every input (the struct's doc comment), the placeholder only as a
hint.

## 18. Hover-only actions

**Looks like:** row actions that appear on hover; menus that open on
hover.

**Detect:** hover states in the design with nothing in the rest state.

**Navree form:** row actions in a visible menu (`row_actions`), reachable
by keyboard and by touch. A `HoverCard` or `Tooltip` only adds
information already reachable another way.

## 19. Links that change things

**Looks like:** "Sign out", "Delete", "Approve" drawn as links.

**Detect:** every link whose target changes data.

**Navree form:** a form that posts (`UserMenu::action`, `RowAction::post`,
a `Button` in a form). Links are fetched ahead of a click, so a link must
never change anything.

## 20. Dates and times with no zone

**Looks like:** "Today, 09:30", "Due 3 Nov".

**Detect:** every time on screen: whose clock is it?

**Navree form:** a calendar day is `NaiveDate`; an instant is stored in
UTC and shown in the user's or workspace's zone (`navree-time-zones`).
Say which in the page spec.

## 21. Two tables, one URL

**Looks like:** a page with two lists, each with its own search and
pages.

**Detect:** count the tables per screen.

**Navree form:** a table's state lives in the URL's plain parameters, so
one URL holds one. Put the second on its own route, in a `Region`, or in
a tab that is its own page.
