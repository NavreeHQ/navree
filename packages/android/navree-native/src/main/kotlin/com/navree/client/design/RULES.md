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
- A destination's count is a badge on its icon. In the expanded rail, where
  the label stands beside the icon, the count stands beside the label: on the
  icon it would cover the label's first letters.
- A bar's labels grow with the text setting only as far as the longest word of
  any label still fits in an item's share of the width (never below the
  default size), so no word is broken in the middle at the largest text.
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
- A task sheet's bar holds an X, the title and at most one affirming action
  (the form's own submit, or the one primary action). Every other action
  stands at the end of the content, in a line that wraps, the destructive
  ones last; actions that took the bar would leave the title no room.
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
  128 dp each (more at larger text sizes) is a list at any width. A column of
  words takes the room its longest cell needs, between a short status and a
  long place, so a place is not broken in the middle.
- The lines under a row's title keep runs of badges (a status, a priority, who
  has it) in one line that wraps; one under the other they would make a row
  four lines taller.
- Facts stand in two columns only where the room they are drawn in holds two
  columns of 300 dp.
- A list asks for its next page when its end is drawn, never before.
- A list whose rows are moved (a board's lane of cards, a checklist) is laid
  lazily, whatever it holds: where the window shows one lane at a time, the
  lane in hand is part of the screen's own list under its tabs; where lanes
  stand side by side, each is a lazy list as tall as the window; a list that
  stands where nothing bounds its height is drawn whole and moved by its
  menus. A row is dragged by its handle and is the same item wherever it
  stands (its id is its key), so it keeps what it showed. The row in hand
  follows the finger, takes its place among the rows beside it as the finger
  passes their middles, and the list scrolls by itself while the finger is
  near an edge, only the way the finger went. A drag moves a row within its
  own list: to another list a row goes by its handle's menu.

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
| colour | a field that shows the colour as a swatch and its hex value and opens a dialog (see below) |
| file | an outlined box: the label, a row for each file chosen (symbol, name, size, a button to remove it) and a button that opens the system's picker |

- In a short window a date and a time are typed instead of picked.
- A required field's label ends in an asterisk.
- An error shows under its field, in the error colour, and is said by TalkBack
  with the field. When a post comes back with errors, the first field in error
  is scrolled to and focused. An error goes when its field is changed.

### A colour field (`ColourRules.kt`, `ui/ColourField.kt`)

- Material 3 has no colour picker (its component list for Compose has none).
  The field is a picker field like the date's: the label, the hex value in
  words (`#16a34a`) and a swatch of the colour at its end. It opens a dialog.
- The dialog has a palette of 15 swatches in rows of five (the protocol's colour
  input carries no choices, so the palette is the client's own), a slider each
  for hue (degrees), saturation and lightness (percent), and a box for the hex
  value. All of them change one draft; nothing is sent until OK, which is not
  offered while the hex box holds what is not a colour. Cancel and a tap outside
  send nothing.
- The hex box reads `#rrggbb`, `rrggbb` and `#rgb`, in either case. What is not
  a colour is said under the box, with a symbol, and read by TalkBack as the
  box's error. What is sent is always `#rrggbb` in lower case, as the protocol
  says.
- A swatch is 48 dp square (its circle 40 dp) and is a radio button of a
  group. The chosen one has a thicker ring and a tick drawn in black or white,
  whichever reads better on it, so it is never marked by colour alone.
- A screen reader hears a swatch as its family and its value ("Green,
  #16a34a"), a slider as its name with its value in words ("Hue, 120
  degrees"), the field as its label and value. A colour that is not set is
  an outlined swatch and no value.

### A file field (`FileRules.kt`, `ui/FileField.kt`, `platform/PickedFiles.kt`)

- Material 3 has no file upload component. The field is composed from its
  parts: an outlined box (as a text field's) with the label, a row for each file
  (a symbol, the name, the size; a button to remove it with the file's name in
  its description) and a button to choose with. The button's description starts
  with its words and ends with the field's label, so each is told apart.
- The button opens the system's own picker and asks for no permission. A field
  that takes only pictures and video (by the protocol's own rule) opens the photo
  picker, one picture or several; any other opens the system's file picker,
  limited to the kinds the field names. The camera the protocol's `capture` may
  ask for is not used.
- What comes back is checked against the field's `accept`: an extension
  against the end of the name, a family (every picture) against the start of
  the type, any other kind against the whole type. A file that does not fit is
  said so at the field, with what the field takes, and is not added. The
  protocol gives a file field no largest size; the app's configuration does
  (`NavreeConfig.mostFileBytes`, 100 MiB by default, a guard on the device's
  storage and not the server's limit). It holds for one file and for all the
  files of one field together; the words of the refusal say it.
- A provider that gives nothing of a file while it is read, no answer and no
  byte for twenty seconds, is given up: the file is refused at the field in
  words (`FileRules.Refusal.NoAnswer`), its copy is deleted, and the field can
  be used again. A slow provider that keeps giving bytes is waited for.
- Each file is copied at once, off the main thread and as a stream, into the
  app's cache folder (no backup takes it) under a token the client makes; the
  core holds the token, name, type and size. The form's request streams the copy
  into its multipart body. A copy that is gone is shown as gone ("No longer
  available. Choose it again.") when the app comes back to the front, and no
  part is sent for it, never an empty one. The copies are removed when the
  form's write has its last answer (a 307 or a 308 is not one: the same body is
  sent again, with its files), when the person leaves the form's screen, when a
  file is removed or replaced, at the end of a session, at the next start, and
  a day after they were made.
- Unless the field says `keep_location`, the place a picture was taken is taken
  out of the copy, by what its own first bytes say it is, never by its type or
  name: from a JPEG, PNG or WebP every GPS tag, an XMP packet that names a
  place, the Exif blocks after the first, other programs' metadata blocks and
  what follows the end of the picture; the copy is read again and refused when
  a place is still in it. A HEIC, AVIF or GIF that holds one is refused; a HEIC
  below API 28, an AVIF below API 31, a TIFF and a raw picture are refused
  outright, as the system cannot read them for it. The place in a video is not
  taken out.
- While the form is on its way the field cannot be changed. A field that takes
  one file replaces it (a second choice replaces the first); one that takes
  several adds, with no limit on the count.

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
- A screen of days (one that leads with a day pager) turns to the day before
  or after when it is swiped along: a swipe toward the end shows the day
  before, as the page moves with the finger; where the line runs right to
  left, the other way round. The chevrons and the date picker do the same.
- A chart says its marks to a finger as well as to TalkBack: a finger held on
  a bar or a line draws a rule down the mark and a small card with its label
  and values (and says them to a screen reader as the chart's state). A
  finger that goes up, or that the page takes to scroll, reads nothing: a
  drag that scrolls the page is the page's. A slide along a chart, past the
  touch slop, is the chart's: it reads each mark it passes and does not also
  turn the day of a screen of days; a day is turned by a swipe that starts
  off a chart, or by the pager's arrows. A chart takes a slide along it
  wherever it stands, so a part that scrolls along (a row of lanes, a carousel)
  is not swiped from a chart in it but from beside it; a finger held still for
  a moment reads the mark under it, and a scroll that starts after that takes
  the card away again.
- Everything the client opens over a screen (a menu, a question, a picker, a
  sheet) has the layout direction of the screen under it. The app declares
  `android:supportsRtl="true"` (a window of its own resolves its direction only
  for an app that does; the client logs a warning at start when it is missing),
  and a dialog or a popup takes its direction from the composition it is opened
  from. The right to left look of the device tests is the composition's
  direction: the system's own bars, its keyboard and its back gesture are not
  turned by it, and the language the system holds is not changed.
- A keyboard's Tab goes through a window in the order a person reads it, on
  every window size: on a phone the top bar (back, title, search, actions),
  then the content, then the bottom navigation; on a tablet and wider the
  navigation at the start edge, then the top bar, then the content. Right to
  left mirrors nothing in this order. Compose's Tab search goes by the order the
  layout nodes are placed in, so the order is the layout's: a window is a
  `Column` or a `Row` whose children are written in reading order
  (`BottomNavigationWindow`, `SideNavigationWindow`, `ScreenFrame`), and
  Material's `Scaffold`, which places its content before its bars, is not used
  for the parts of a window. The messages and the floating action button come
  after the content. Each bar consumes the window insets it covers
  (`consumeWindowInsets`) and the content gets the rest. Shift+Tab is the exact
  mirror and enters each part at its last control. The two panes of a list and
  its detail keep the order they are laid out in. A table that loads rows as Tab
  goes has no end: whatever stands after it is reached only backwards, so on a
  phone the bottom navigation is reached by Shift+Tab from the top.
- With the focus nowhere, a keyboard comes into a window at the first stop of
  that order, laid out left to right or right to left: the navigation's first
  control on a tablet, the top bar's on a phone. The system does not search in
  Tab's direction then. It gives the window its default focus, and Compose
  answers that with the control nearest the window's top left corner, which is
  the first a person reads only where the window is laid out left to right. So
  the window names a default focus of its own (`KeyboardEntry`), a view of no
  size beside the composition that never takes the focus and hands the request
  to the composition going forward. A dialog, a menu and a sheet are windows of
  their own and keep the system's default: laid out right to left, the first
  Tab inside one goes to the control nearest its top left corner.
- The web view's client is promised only the first address of each request a
  page makes: where a redirect then takes a picture or a script it is not
  asked (a fetch's hop is, on the tested web view): what the app's server
  redirects elsewhere is held by the page's own content security policy, which
  the engine checks on every hop.
- The end of a session leaves nothing its pages stored in the web view for a
  page of the next. Where the system's web view cannot delete every kind of
  site data itself, a document of the client's own on each origin unregisters
  its service workers and deletes its caches (`SiteWipe`). Until the web view
  has said that everything is gone a web page is not shown: the failed screen
  with "Try again", as without the two guards, and "Try again" sets the
  deletion going again.
- A service worker a page registers is held to the origins the web view is:
  its requests to any other origin are answered 403. A page's own scripts open
  a socket or start a worker only for an address on the app's origins or a
  host the app names for the web view (`webHosts`), checked
  by the script at the start of every document with what it took before any
  script of the page ran. What the page hands over is read once, before the
  check, and the real constructor gets the absolute address that was checked;
  so it holds against a page's scripts that run after it, in the documents it
  runs in, whatever they put into the document while a socket or a worker is
  made. Two ways round it remain: the first, empty
  document of a frame the page creates, where the script does not run, which
  gives an unchecked socket and worker; and what a script does inside a
  worker. What holds in every case is the policy the app's server sends
  (`connect-src`, `worker-src`): a worker loaded from an address
  follows the policy sent with its own script, not the page's, so the worker's
  script carries its own `connect-src`, and the page's `worker-src 'self'` is
  what stops workers from `blob:`, `data:` and other origins.
