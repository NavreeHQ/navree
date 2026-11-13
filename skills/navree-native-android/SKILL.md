---
name: navree-native-android
description: "What the Android client of Navree Native does with each part of a page on Android phones and tablets (Jetpack Compose with Material 3), as built: navigation by window width (the navigation bar, the rail and the expanded rail), the floating action button for creation, the app bar and its overflow menu, search bar and filter chips, lists and tables as list items, forms with Material fields, sheets, dialogs and snackbars, dynamic colour and the brand's seed, edge to edge and predictive back, two panes on wide windows, and the web view; what an author should and should not do; the server pieces an Android app needs (package and fingerprints, assetlinks, passkeys, Sign in with Google, push through Firebase); a yes/no review checklist for screenshots; common mistakes with the fix; Google's own Android skills to load beside it; and dated sources. Use when building, reviewing or debugging a Navree app on an Android phone, tablet or foldable, when a native screen looks wrong on Android, or when asked what Material 3 means for a Navree page. Load `navree-native` and `navree-native-design` first."
---

# Navree Native on Android

The Android client (`packages/android`, the library `navree-native`, plus
`navree-push-fcm`) draws a Navree page with Jetpack Compose and Material 3,
and places each part by Android's own guidance: window size classes,
Material's navigation, app bar, floating action button, lists, sheets,
dialogs and snackbar. This skill says what it does with each part, what that
means for the page, and how to review the result. It summarises Google's
guidance and links the pages (`references/sources.md`, read on 2026-11-12,
describing Android 16 and 17 and Material 3 Expressive); it never copies them.

The words an author has are in [`navree-native`](../navree-native/SKILL.md);
the rules for every system in
[`navree-native-design`](../navree-native-design/SKILL.md).

Google publishes agent skills for Android (https://github.com/android/skills:
`jetpack-compose/adaptive`, `system/edge-to-edge`, `navigation/navigation-3`,
`wear/wear-compose-m3`, `system/glance-canonical-layouts`, also through the
Android CLI's `android skills add <name>`). Load them for work inside the
client or in a native Android module of your own; they are the authority on
Compose, edge-to-edge and adaptive layouts, and this skill does not restate
them or contradict them.

## What is built, and what is not

| Target | State |
|---|---|
| Android phone (compact width) | Built. Run in a cloud emulator (API 35, a Pixel 7 profile) in four looks: light, dark, 200% text and right to left, against the store demo served beside it. Not run on a physical device. |
| Tablet and foldable (medium to extra-large) | Built: the rail and the expanded rail, two panes, tables as rows. Run in a Pixel Tablet emulator. |
| Wear OS | Not built. It has no web view, so it will take a strict subset. |
| Android TV, Android Auto, widgets, Live Updates | Not built. |

The client builds with compile SDK 36 and supports Android 8.0 (API 26) and
later. It uses Material 3 `1.5.0-alpha18`, the newest build with the Expressive
components that compiles against platform 36; every call to an Expressive
component is in one file, `design/expressive/Expressive.kt`, so the library's
renames cost one place. The stable 1.5.0 needs platform 37.

## What the client does with each part

Placement follows the **window**, never the device: a phone on its side, half
a tablet in split screen and a half-open foldable are each a window of some
width. Width classes: compact below 600 dp, medium to 839, expanded to 1199,
large to 1599, extra-large beyond. The rules are pure functions with a test
each (`DesignRulesTest`) and are written out in `design/RULES.md` of the client.

| Your page says | The client draws |
|---|---|
| The shell's destinations | Compact (or a short window): a navigation bar of up to 5, an icon and a label each, the selected one filled with a pill indicator. Medium and expanded: a navigation rail of up to 7. Large: the expanded rail, with the shell's groups as headings. What does not fit is reached from a menu button that opens the expanded rail. No drawer: Material 3 retired it. A bar and a rail never show together. Each destination keeps its own stack, and Back from another destination returns to the first before it leaves the app. |
| A screen's title | A small top app bar with the title on one line; it takes the scrolled colour once content is under it, and in a short window leaves as the content scrolls. |
| `Role::Create` or `add` | The floating action button, at most one: extended, with its label, on a first screen, the icon alone on a deeper one; beside a rail it moves to the rail's head. None in a presented screen, and none beside a floating toolbar (creation is then an icon in the bar). |
| `Primary` then `Secondary` actions | Icon buttons in the top app bar, primary first: two in a compact window, three in a medium one, four in wider ones (one fewer at the largest text). The rest go into the overflow menu in the order the page gave them. An action with no icon goes to the overflow. |
| `Destructive` actions | Never the floating button, never in the bar, never a filled button: the end of the overflow menu, under a divider, in the error colour. One that writes asks first, in an alert dialog whose headline is the action's own words as a question ("Cancel order?") and whose affirming button repeats them. |
| `Cancel` and `Confirm` | A presented screen's cancel is the close (X) in the app bar. In a dialog the affirming button is at the end and the button that backs out before it. A form that is the whole screen is finished from the bar by a text button at its end. |
| A toast | A snackbar, one at a time, above the floating action button and the keyboard; the toast's action is the snackbar's action. The toast's tone changes only how long it stays (Material's snackbar has no tones). |
| A sheet | A sheet inside a screen is a modal bottom sheet (medium may rest half open); one the shell presents (`sheet`, `full`) is a task of its own: a full-screen dialog on a compact window, a basic dialog at most 560 dp wide on a wider one. A dialog in the tree is an alert dialog. |
| A table or a list's records | Below 600 dp of content each record is a list item folded by the column roles: `title` the first line, `subtitle` the lines under it, `status` on its own line as a word after a dot in the tone's colour, `trailing` ending the row; there is no chevron, the whole row is one touch target; a row's menu opens from a button at its end and on long press, and both are offered to TalkBack. From 600 dp, rows of cells under a header; a sortable heading re-asks the table. |
| Search, filters, sort | A search bar in the app bar's place on a first screen, asked for after a short pause; the query's other fields are a row of filter chips over the rows, each opening a menu of its choices. |
| Two panes | In an expanded or wider window that is not short, a first screen that is a list or a table and the screen opened from it side by side (list 360 dp, detail in a rounded container); the open row is marked selected. A sheet a row opens still opens over the list: the client does not yet use whether it is a task or a glance. |
| Fields | An outlined text field (label in the outline, help and error under it), a switch row (the whole row toggles), a checkbox row, a connected button group for up to 4 or 5 short choices, radio buttons up to 5, a menu field up to 15, a dialog of choices for more, filter chips for several of a few, a date or time field that opens a dialog (typed in a short window). A required field's label ends in an asterisk; the first field in error is scrolled to and focused. |
| Refresh, loading, failure | Pull to refresh on a screen that says it can refresh; a progress indicator in the middle after a moment; a thin linear indicator while a fresh tree loads; a screen that failed says so with a way to try again, one that could not refresh keeps what it shows and says so in a snackbar. |
| Icons | The Material Symbol the icon's name maps to, filled when selected and outlined otherwise; standard actions take the system's symbol whatever the page sent. |
| A chart | Drawn with the theme's colours, with a text for TalkBack. |
| A page that falls back | A web view with no script bridge; every navigation in it goes back to the client. |

Type is Material's scale at the user's size (at 1.3 times and above things that
sit side by side stack); a step of the page's spacing is 4 dp; a screen's side
margin is 16 dp compact and 24 dp wider; a column of content is at most 840 dp.

## Colour, type and shape: dynamic colour and the brand's seed

- The brand gives **one seed colour** (the theme's accent). The client builds
  Material 3's tonal-spot scheme from it, for light and dark: the seed gives
  the hue and every role is a fixed tone of a palette, so a neon seed does not
  make a neon app. Text on every pair of roles has at least 4.5 to 1 contrast
  for any seed; a test checks seeds from black to white.
- An app with **no accent** (neither the shell nor its own seed) takes the
  person's wallpaper colours on Android 12 and later (dynamic colour). An app
  with an accent takes them only when it opts in (`navree_dynamic_colour`).
  Android's own guidance is to prefer the user's palette; an app that sets an
  accent has decided to be recognisable instead.
- Type and elevation stay the system's; motion is Material 3 Expressive's
  springs, or the standard scheme when animations are off. Success, warning and
  info have no role in Material 3, so their colours are made the way Material
  makes its own, from fixed hues turned toward the seed.
- Follow the system's dark mode; dark surfaces are tonal, never pure black.

## What every screen does

- **Draws edge to edge**, padded by the insets; the keyboard never covers the
  field in focus. Edge-to-edge is mandatory for apps targeting API 35 and
  cannot be opted out at 36.
- **Back is the system's.** Predictive back is on at 36: inside a stack the
  gesture shows the screen under the one in front while the finger is down;
  with nowhere left to go the client leaves to the system so the back-to-home
  animation plays.
- **Start and end, never left and right**, so layouts mirror.
- **TalkBack** names every control from the tree: an icon button by its label,
  a heading as a heading, a switch with its state, a field with its error, a
  banner as a live region. Touch targets are at least 48 dp.

## What an author should and should not do

Do:
- Say the roles (`navree-native`) and let the client place them: creation is
  `Role::Create`, the one main action is `primary`.
- Give every destination an icon and a short label; the bar shows both.
- Make records a table with `title`, `subtitle`, `status` and `trailing`
  columns, or `Item`s; give the table `search` and `filter` columns so the
  search bar and chips appear.
- Make a toast's Undo a `ToastAction` when an action can be reversed.
- Keep a sheet that only shows something free of a form, so it is a glance:
  Android's tablet apps show a glance in the detail pane, and the client will
  once it uses the difference (it does not yet).

Do not:
- Ask for a drawer, a bottom tab bar of your own, an iOS-style segmented
  control, a toast that is a dialog, or a full-width form button.
- Hard-code colours, fonts or corners on the page; the client has the
  palette, the type scale and the shape scale.
- Lock the orientation or assume a tablet by device: windows resize.
- Stretch buttons and fields across a wide window; the client caps forms at
  560 dp.

## What an Android app needs from the server

`navree native:doctor` checks these (the JDK 17, the Android SDK and platform
36, the package, the fingerprints, the association file) and never prints a
secret. The server pieces already exist; their skills hold the detail:

| Need | Where | Skill |
|---|---|---|
| The package name and the signing certificate's SHA-256 fingerprints; the `assetlinks.json` file served at `/.well-known/`; the app's link filter | `auth.apps.android.<name>` | `navree-auth` |
| Passkeys from the app (the origin is `android:apk-key-hash:<fingerprint>`; `get_login_creds` in the asset links) | `auth.passkeys`, `auth.apps` | `navree-auth` |
| Sign-in in the system's browser tab and the hand-over to the app (a one-time code with a PKCE verifier; an App Link callback) | `auth.apps.android.<name>.callback` | `navree-auth` |
| Confirming one action with a passkey assertion over the action's own details | `ConfirmAction` | `navree-auth` |
| Sign in with Google from the system sheet (Credential Manager; the web client id is `serverClientId`) | `social.providers.google` | `navree-social` |
| Push through Firebase Cloud Messaging (a service account, the app's package, the Firebase options in the app's resources, `POST_NOTIFICATIONS` on Android 13+) | `notify.push.fcm` | `navree-notification-channels` |

Apple's sign-in is not offered on Android; the server's web flow is the way.
What the client does about each device feature is in
`references/availability.md`.

## Review checklist

Run on a screenshot of the phone and tablet passes (`navree-native-design`,
"Review a native screen"), light and dark, at 200% text and right to left.
Answer yes or no; a "no" is a defect.

**Phone (compact width)**
1. The status bar and navigation bar areas are filled by the app bar or the
   content (edge to edge); no band above or below; nothing hidden under the
   gesture bar.
2. A first screen has a bottom navigation bar of 3 to 5 destinations, each with
   an icon and a one- or two-word label, none wrapped or cut; not a drawer, not
   tabs at the top.
3. Exactly one destination is active: a filled icon with a pill indicator, the
   others outlined.
4. A pushed screen has a back arrow at the leading end and a first screen does
   not; a pushed screen that shows a bottom toolbar has no navigation bar.
5. The app bar is the page's colour at the top and tinted when content scrolls
   under it; the title is one line.
6. The app bar has at most two trailing actions and an overflow; only one is
   filled or tonal.
7. At most one floating action button, at the bottom end, above the navigation
   bar, never covering it; creation is that button, not a full-width button or
   a plus in the bar.
8. A destructive action is not the floating button and not the app bar's main
   slot.
9. Lists are Material list items: a leading visual, a headline, supporting
   text, a trailing value; dividers only where gaps would not do; not every row
   boxed in a card.
10. Text fields have always-visible labels, one style per screen; errors are red
    supporting text under the field with the outline in the error colour.
11. Switches are for immediate settings, checkboxes for multiple choice, radio
    buttons for one choice.
12. A choice between views is a connected button group of 2 to 5 short options.
13. Every tap target is at least 48 dp.
14. A dialog has a short, specific headline (not "Are you sure?"), at most two
    buttons, the confirm at the end; a destructive confirm repeats the verb and
    names what goes.
15. A bottom sheet is full width with a drag handle and rounded top corners.
16. Messages are snackbars (one line, at most one text button), above the
    floating button and the navigation bar, never a dialog or a system toast.
17. Search is a search bar with "Search" in it; a refreshable list pulls to
    refresh with no refresh button.
18. An empty state has an icon, a title, one line and at most two actions,
    centred; loading shows an indicator or placeholder rows, never a blank.
19. Tabular data is rows with a headline and supporting text, not a shrunken
    table.
20. Text is in Material's type scale, in sp, nothing under 12 sp but legal
    captions; the colours come from one palette with 4.5 to 1 contrast for text
    and 3 to 1 for icons.
21. Dark mode uses tonal dark surfaces, not pure black or inverted light colours.
22. At 200% text nothing is clipped, overlapped or hidden, and in right to left
    the back arrow, the floating button and the navigation order are mirrored.

**Tablet and foldable (medium to extra-large)**
1. At medium width and up, navigation is a rail on the leading edge (or a bar
   with horizontal items at medium), never a stretched phone bar; a bar and a
   rail never show together.
2. At expanded width the floating button sits at the rail's head, not at the
   bottom right.
3. A list with a detail shows two panes at expanded width, the open row
   selected and no back arrow in the detail pane; at compact and medium it is
   one pane at a time.
4. The content width is capped; fields and buttons are not stretched across the
   screen.
5. Bottom sheets have a maximum width; dialogs are basic dialogs, not
   full-screen, at medium width and up.
6. Menus are anchored next to their item, not full width; grids add columns
   with the width.
7. The layout fills the window with no letterbox, in split screen and when
   resized; after rotating or folding the scroll position, typed text and
   selected item are kept.
8. Hover, focus rings, keyboard navigation, Esc and right-click menus work.

## Common mistakes

| Mistake | Fix |
|---|---|
| An iOS-style tab bar, or a drawer as the main navigation | The shell's destinations: a bar, a rail, an expanded rail. |
| A large title that never collapses, or a header of your own | The top app bar; it is the client's. |
| "Edit" and "Save" text actions at the top right for everything | Roles: `Create` is the floating button; the app bar holds one or two icons. |
| A floating button on every screen, or several | At most one, and none when no action is clearly the main one. |
| Full-width buttons and wide inputs on a tablet | Leave widths to the client; align buttons to the end. |
| Hard-coded hex colours and white backgrounds | Nothing on the page; the theme's accent seeds the palette. |
| Fixed heights and one-line labels everywhere | Let text wrap; test at 200%. |
| A web-style modal that covers a tablet | A basic dialog on a wide window; a snackbar for results. |
| A system toast for results | A `Toast` in the page: the client draws a snackbar that can carry Undo. |
| An iOS segmented control | A choice field: the client draws a connected button group. |
| A custom pull-to-refresh spinner | `refresh` on the screen; the client draws the system's. |
| A table shrunk to a phone | Column roles: rows with a headline and supporting text. |
| Locking the orientation, or checking for a tablet | Nothing: windows resize and fold. |
| Treating the web view as a screen for lists and forms | Native lowering; the web view is only the declared fallback. |

## Docs

- https://navree.com/docs/ui/native/
- Google's pages are listed in `references/sources.md`.

References: [`references/availability.md`](references/availability.md) (what
Android offers by API level, and what Navree has of it),
[`references/sources.md`](references/sources.md) (the guidance pages used, with
the date read).
