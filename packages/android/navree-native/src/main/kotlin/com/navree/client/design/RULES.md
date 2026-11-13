# The Android design rules

A page says what each thing is: a screen with a title, actions with roles, a
list, a form, a sheet. This package decides where each goes on Android and
which Material 3 component draws it. It follows Material 3 and Android's own
guidance as they stand, and when the brand and the guidelines disagree, the
guidelines win.

Every rule here is a pure function of two things: what the tree says, and the
window (`WindowInfo`: width and height in dp, and the user's text size). None
of them touches the device, so each has a test that states it in a sentence:
`DesignRulesTest`. The files of this package hold the rules; the `ui` package
only draws what they decide.

Read against these sources on 12 November 2026:

- Material 3: navigation bar, navigation rail, top app bar, search, floating
  action button, lists, text fields, date and time pickers, segmented buttons,
  chips, bottom sheets, dialogs, snackbar (m3.material.io).
- Android: window size classes, adaptive navigation, edge-to-edge, predictive
  back, principles of navigation (developer.android.com).
- The library itself: Compose Material 3 1.5.0-alpha18, whose components carry the
  measurements, so no rule here repeats a size the component already has.

## The window (`Window.kt`)

Width classes: compact below 600 dp, medium to 839, expanded to 1199, large to
1599, extra-large beyond. Height classes: compact below 480 dp, medium to 899,
expanded beyond. A rule never asks what kind of device it is on, only how much
room the window has: a phone on its side, half a tablet in split screen and a
foldable half open are each just a window of some size.

Text size: at 1.3 times and above, things that sit side by side stack. At 1.5
times and above, the app bar gives up an action so the title can still be read.

## Top-level navigation (`NavigationRules.kt`)

| Window | Component | Holds |
|---|---|---|
| compact width, or compact height | navigation bar | up to 5 destinations |
| medium or expanded width | navigation rail | up to 7 |
| large or extra-large width | the rail, expanded | all, under their group headings |

- One destination, or none: no navigation component.
- Two destinations still get a bar.
- What does not fit is reached from a menu button on first screens. It opens
  the expanded rail over the content with every destination in its group.
- The navigation drawer is not used. Material 3 retired it for the expanded
  rail, and Android's own adaptive scaffold never picks a drawer by itself.
- On a medium or wider window that is short (a phone on its side), the bar's
  items sit side by side with their icons, in the middle of the bar.
- A destination's count is a badge on its icon.
- Choosing the destination in front goes back to its first screen. Each
  destination keeps its own stack. Back from any other destination returns to
  the first one before it leaves the app.

## A screen's frame (`ScreenRules.kt`)

### The app bar

- A small top app bar, with the title on one line, that stays put. It takes the
  scrolled colour once content is under it.
- A screen with a subtitle shows it on a line under the title, unless the
  window is short or the text is at its largest. (Material 3's two-row
  flexible bar is not reachable in the 1.4 library; when it is, this rule is
  the one to change.)
- In a short window the bar leaves as the content scrolls down and comes back
  on the first scroll up.
- A first screen with search has a search bar in the app bar's place. The
  destination's label, lit in the navigation, names the screen. What is typed
  is asked for after a short pause, and the screen's own list answers.
- The search may be the screen's own, or the search box of its main table's
  query. The query's other fields (choices and switches) are a row of filter
  chips over the rows: a chip a filter, lit when it is in force, opening a
  menu of its choices. Choosing asks for the rows again at once.
- A screen with no chrome (a sign-in) has no bar.

### Navigation

- Destinations take the Material Symbol their icon's name maps to
  (`Symbols.kt`), filled when selected and outlined otherwise; an icon whose
  name maps nowhere keeps the page's own shape.
- A destination's label is never cut: at large text it takes a second line.

### The navigation icon

- A first screen: none, or the menu button when destinations did not fit.
- A deeper screen: the back arrow (mirrored in right-to-left layouts).
- A presented screen: an X. A screen pushed inside a presented one: the arrow.
- The detail pane beside its list: none; the list is beside it, not under it.

### Actions, by role

- **Create** is the floating action button: the first `create` action (or
  standard `add`) of a screen in a stack. There is at most one, and none on a
  screen with a floating toolbar: then creation is an icon in the bar.
  - On a first screen it carries its label, and gives the label up when the
    list scrolls and at the largest text sizes.
  - On a deeper screen it is the icon alone.
  - Beside a navigation rail it moves to the rail's head (with its label when
    the rail is expanded).
  - A presented screen has none.
- **Primary, then secondary** go in the top app bar as icon buttons, primary
  first. A compact window shows two, a medium one three, wider ones four; one
  fewer at the largest text sizes, and one fewer for each of the search icon
  and a form's submit when the bar holds them.
- When they do not all fit, the last place is the overflow menu and the rest
  go into it, in the order the page gave them.
- An action with no icon goes into the overflow. One primary action with no
  icon may be a text button in the bar.
- A menu with an icon is an icon button that opens it. A menu without one is a
  headed group in the overflow.
- **Destructive** actions (role `destructive` or standard `delete`) are never
  the floating action button, never in the bar and never a filled button.
  They end the overflow menu, under a divider, in the error colour.
- A destructive action that writes asks first, in an alert dialog whose
  headline is the action's own words as a question ("Cancel order?") and
  whose affirming button repeats them in the error colour. One that can be
  undone (`undoable`) does not ask: Android's way is to do it and offer Undo
  in the snackbar. One that is a dialog's own answer is not asked twice.
- **Standard actions** (share, delete, add, edit, search, done, cancel, more)
  are drawn with the system's Material Symbol whatever icon the page sent.
- **A toolbar** of the page is Material 3's floating toolbar at the bottom of
  the content: its items are icon buttons named by their titles (with a
  tooltip), menus where an item has one.
- **Cancel** on a presented screen is the X; it is not drawn a second time.
- A search bar holds two things after its field. The signed-in user's avatar
  is one of them on first screens; the overflow, or one action, is the other.

### Forms

- A form that is the whole screen is finished from the bar: its submit is a
  text button at the bar's end, where the keyboard cannot cover it.
- A form among other content, or on a screen with no chrome, keeps its
  button where the page put it.
- A form in a dialog is finished from the dialog's foot: its button is the
  dialog's affirming button, beside "Cancel".
- A form is at most 560 dp wide; fields do not stretch across a wide window.

### Presenting

- An address the shell presents (`sheet` or `full`) is a task of its own. On a
  compact or short window it is a full-screen dialog; on a wider one a basic
  dialog, at most 560 dp wide. `full` is full screen at any size.
- A sheet that is part of a screen's tree is a modal bottom sheet. `medium`
  may rest half open (not in a short window); `large` opens fully; `full`
  fills the height.
- A dialog that is part of the tree is an alert dialog. The buttons that go
  ahead sit at the end; the one that backs out sits before them.
- Toasts are snackbars, one at a time, above the floating action button and
  above the keyboard when it is up. A toast's action is the snackbar's action. Material's snackbar has no tones, so
  a toast's tone changes nothing but how long it stays.

### Two panes

In an expanded or wider window that is not short, a first screen that is a
collection (a list or a table) and the screen opened from it are drawn side by
side: the list pane 360 dp wide (412 dp in a large window), the detail in the
rest, in a rounded container of its own on the surface tone. A pane whose role
is `list` counts as a collection. The row that is open is marked selected.
With nothing opened, the list has the window.

### Pull to refresh, loading, failing

- A screen that says it can be refreshed is pulled to refresh. A presented
  screen is not.
- A screen with nothing to show yet has a progress indicator in its middle,
  after a moment, so a quick load does not flash one.
- A screen showing its last tree while a fresh one is fetched has a thin
  linear indicator under the bar.
- A screen that could not be fetched says so in its middle with a way to try
  again. One that could not be refreshed keeps what it shows and says so in a
  snackbar.

## Content (`ContentRules.kt`)

### Lists and tables

- A row is a Material list item: leading avatar, icon or image; title; second
  line; trailing value with the row's status under it. At the largest text the
  value and the status move under the second line, so neither squeezes it.
- A status in a row is a word after a small dot in its tone's colour, as
  plain as the row's other lines; a filled label would shout louder than the
  amount beside it.
- A row that leads somewhere is one touch target. There is no chevron.
- A row's menu opens from a button at its end and from a long press. Its swipe
  action runs on a swipe from the end. Both are also offered to TalkBack as
  actions of the row, because a swipe and a long press are not discoverable.
- Rows of an inset section share one tonal container with rounded ends.
- Below 600 dp of content a table is a list, each row a list item folded by
  the role the server gave each column: `title` (or the first column) is the
  first line, `leading` leads the row, `subtitle` gives the lines under it,
  `status` goes on a line of its own, `trailing` (or the first numeric
  column) ends the row, `actions` and the row's own actions are its menu; a
  `secondary` column is left out. A row opens what the server says it opens;
  when it says nothing and the naming cell holds one control, the whole row
  takes that control's action. From 600 dp it is rows of cells under a header
  row; a sortable heading takes the table's region to its other address. A table with more columns than fit at
  112 dp each is a list at any width.
- A list asks for its next page when its end is drawn, never before.

### Fields

| Input | Control |
|---|---|
| text | outlined text field: label in the outline, help and errors under it, a counter with a most, a show-or-hide button for a password; the keyboard, capitals and autofill follow the kind |
| switch | a row with the switch at its end; the whole row toggles |
| checkbox | a row with the checkbox at its start |
| one of a few, `segmented` | a connected button group (up to 4 on a compact window, 5 on wider); segmented buttons are no longer Material's |
| one of a few, `radio` | radio buttons (up to 5) |
| one of several, `menu` | a field that opens a menu (up to 15) |
| one of many, or `list` | a dialog with a list of radio buttons |
| several of a few | filter chips (up to 8) |
| several of many | a dialog with a list of checkboxes |
| date | a field that opens the calendar in a dialog |
| time | a field that opens the clock dial in a dialog |
| slider | a slider with its value beside its label |

- In a short window a date and a time are typed instead of picked.
- A required field's label ends in an asterisk.
- An error shows under its field, in the error colour, and is said by TalkBack
  with the field. When a post comes back with errors, the first field in error
  is scrolled to and focused. An error goes when its field is changed.

### Type, colour, space

- Text levels map to Material's type scale: large title to headline medium,
  title to title large, headline to title medium, body to body large, callout
  to body medium, caption to body small. Sizes are the user's.
- Tones map to colour roles: accent to primary, danger to error. Success,
  warning and info have no role in Material 3; their colours are made the way
  Material makes its own, from fixed hues turned towards the seed.
- A step of the page's spacing scale is 4 dp, Material's grid. A screen's side margin is 16 dp on a compact
  window and 24 dp on wider ones.
- A screen's content stands in one column, in the middle of its pane, and
  everything in it (words, grids, cards, rows) shares the column's start
  edge and its widest width. The column is at most 840 dp wide, and 1,240 dp
  where some cards stand side by side. Forms are 560 dp, tables 1,440 dp.
- Cards stand side by side from 840 dp of pane (two; three from 1,200 dp, fewer
  at larger text). A line with fewer cards than columns (a card alone, the
  last of a run) stretches its cards across the line: no cell stands empty.
  The cards of a line are as tall as the tallest wherever every part of them
  can say its height before it is laid out.
- A line of parts across (a horizontal stack) is a column when its parts
  would be squeezed below the least width each keeps: a ring its diameter,
  rings with a legend their dial and legend, a grid one column, a chart 160 dp.
  Parts that grow share what the others leave.
- The last line of a grid with fewer parts than the others stretches its
  parts across the line when they are whole things in themselves (cards and
  figures: five pillars in three columns are three, then two wider ones);
  words, rings and fields keep the width of the other lines' columns, as
  Material's own grids do. A screen that wants all five in one line says so
  with a carousel or by naming the number of columns (`cols`).
- Amounts joined to their unit in one string of the client's own are one run
  read left to right, so "540/600 kcal" does not turn into "kcal 540/600" in a
  right to left line.
- A card that asks for `transition: zoom` shares its bounds with the screen it
  opens (Material's container transform, drawn by shared bounds); that screen
  says it was opened from the card, and goes back into it.

## Theme (`../theme`)

- The brand gives one seed colour. The scheme is Material 3's tonal spot
  scheme for that seed: the seed gives the hue, and every role is a fixed tone
  of a palette at a fixed chroma, for light and for dark. A neon seed does not
  make a neon app.
- Text on every pair of roles has a contrast of at least 4.5 to 1, for any
  seed. A test checks it for seeds from black to white.
- An app that sets no accent (neither the shell nor its own seed) takes the
  person's wallpaper colours on Android 12 and later; an app with an accent
  takes them only when it opts in.
- Type and elevation stay the system's; motion is Material 3 Expressive's
  springs, or the standard scheme when animations are off. Only a `sharp`
  corner style changes the shape scale.
- Every call to Material 3's Expressive components (a pre-release in the
  library this builds against) is in `expressive/Expressive.kt`.

## What every screen does

- Draws edge to edge; each screen pads itself by the insets, and the keyboard
  never covers the field in focus.
- Leaves back to the system when there is nowhere in the app to go back to, so
  the system's back-to-home animation plays. Inside a stack, a back gesture
  shows the screen under the one in front while the finger is down.
- Uses start and end, never left and right.
- Names every control for TalkBack from the tree: an icon button by its label,
  a heading as a heading, a switch as a switch with its state, a field with
  its error, a banner as a live region.
