---
name: navree-native-apple
description: "What the Apple client of Navree Native does with each part of a page on iPhone and iPad (SwiftUI, Liquid Glass), as built: the tab bar and the sidebar-adaptable tab view by width, large titles, where a screen's actions go by role (the plus in the top bar, the one prominent trailing action, the overflow menu, destructive actions in a section of their own, Cancel and Done in sheets), sheets, alerts and confirmation dialogs, no toasts, search, lists and tables as rows, forms with system controls, symbols, charts and the web view; what Liquid Glass is and where the system applies it (so nobody asks for it on content); which systems and OS versions are supported and which are not built; what an author should and should not do; the server pieces an Apple app needs (app ids and the association file, passkeys, Sign in with Apple, push through APNs); a yes/no review checklist for screenshots; common mistakes with the fix; and dated sources. Use when building, reviewing or debugging a Navree app on iPhone or iPad, when a native screen looks wrong on Apple, or when asked what Apple's guidelines or Liquid Glass mean for a Navree page. Load `navree-native` and `navree-native-design` first."
---

# Navree Native on Apple systems

The Apple client (`packages/apple`, a Swift package: `NavreeProtocol`,
`NavreeDesign` and `NavreeShell`) draws a Navree page with SwiftUI, so the
system's own components do the work and the system's version decides the look.
This skill says what it does with each part, what that means for the page, and
how to review the result. The guidelines it follows are Apple's Human
Interface Guidelines at their current state; this skill summarises them and
links the pages (`references/sources.md`, read on 2026-11-12, describing iOS 27
and the 26 releases). It never copies them.

The words an author has are in [`navree-native`](../navree-native/SKILL.md);
the rules that hold on every system in
[`navree-native-design`](../navree-native-design/SKILL.md).

## What is built, and what is not

| System | State |
|---|---|
| iPhone (compact width) | Built. Run in cloud simulators on Xcode 26.6 (iOS 26.5) and Xcode 27.2 (iOS 27.2): light, dark, largest text and right to left. The last run was green on 26.6; on 27.2 the unit tests were green and two screen tests were lost to the simulator not starting the app (the same tests passed in earlier runs). Not run on a device. |
| iPad (regular width) | Built: the tab bar at the top that turns into a sidebar, tables as columns. Run in a simulator. The list beside its detail is not built yet. |
| Mac | The package targets macOS 26. No Mac pass has been run, and the Mac's menu bar (every toolbar action is also a menu command), settings scene and windows are not built. |
| iPhone Duo, Apple Watch, Apple TV, Apple Vision Pro | Not built. The watch and TV have no web view, so they will take a strict subset. |

The floor is iOS and macOS 26 (the package's `platforms`): the current system
and the one before it, built with Xcode 26 or 27. Everything Liquid-Glass-
specific then comes from system components. Navree never keeps the old design:
under the 27 SDKs the system ignores the opt-out (`UIDesignRequiresCompatibility`).

## What the client does with each part

Placement is chosen by the window's **size class**, never by the device kind:
an iPhone app resizes on iPadOS and in iPhone Mirroring, so a compact window is
a phone whatever it runs on. Each rule has a test in
`packages/apple/Tests/NavreeDesignTests`.

| Your page says | The client draws |
|---|---|
| The shell's destinations (`NavItem`s) | A tab bar at the bottom in a compact window (3 to 5 tabs, a filled symbol and a label each; the rest in the system More tab); in a regular window a tab view that can turn into a sidebar, the shell's groups as its sections when there is more than one group. Never a hamburger, never a drawer, never a custom bar. |
| A screen's title | A large title at the first screen of a destination, an inline title on a pushed screen or in a sheet. The app's own name is never a screen's title. A page shown in the web view has an inline title. |
| `Role::Create` or `add` | A plus at the trailing end of the top bar. Never a floating button. |
| The one `Primary` action | The one prominent item at the trailing end of the top bar; in a sheet, Done or Save at the top trailing corner. A second primary goes to the overflow menu. |
| `Secondary` actions | The system's overflow menu at the trailing end; no overflow menu of your own. |
| `Destructive` actions | A section of their own at the end of the content, in red, never prominent. They ask first, in a confirmation dialog anchored to the button with the destructive choice first and Cancel last, because Navree cannot yet say an action can be undone. |
| `Cancel` in a sheet | The top leading corner, as a close symbol. Swiping the sheet down stays on. |
| A sheet (`medium`, `large`, `full`) | Medium: rests at half height with a grabber, can be pulled up. Large: full height. Full: a full-screen cover, for a camera or a long flow. One sheet at a time. A dialog that holds a form is a sheet; one that only asks is an alert or, for a destructive question, a confirmation dialog. |
| A toast, a banner | There is no toast on Apple systems. A failure is an alert; anything else is a dismissible line of status at the top of the content, with a symbol, a haptic (success, warning or error) and a VoiceOver announcement. |
| A table's records | In a compact window each record is a row: its title, lines under it, its state as words with a symbol in the tone's colour, and its trailing value (an amount) at the end. In a regular window with more than two columns, a `Table` with sortable headings. A row's forms and buttons become its swipe actions and its context menu. |
| A table's search, filters, sort | A search field under the title (compact) or at the trailing end of the toolbar (regular); filters in a sheet of choices with checkmarks; sort as a menu. |
| Lists | Records as plain rows edge to edge; a record's facts, sections and forms as inset grouped lists with title-case headers. A row that opens a page shows the system's disclosure chevron; one that opens a sheet does not. |
| Fields | A grouped form. A switch for a toggle (a checkbox is drawn as a switch), an inline picker with checkmarks for a radio group (iOS has no radio buttons), a segmented control for at most five short choices, a menu picker by default, a screen of choices for many, the compact date picker, the matching keyboard and autofill for email, password, phone, number and one-time code. A field's error shows under it in words and the first field in error takes focus. |
| Empty, loading, failed | The system's content-unavailable layout (a symbol, a title, a description, an action); cached content first, placeholders while loading. |
| Icons | The SF Symbol the icon's name maps to, filled when selected; the standard symbols for standard actions (`plus`, `square.and.arrow.up`, `trash`, `xmark`, `checkmark`, `ellipsis`) even when the page sent its own; the page's shapes only for a name no symbol matches. |
| A chart | Swift Charts with the theme's colours and a text for VoiceOver. |
| A page that falls back | A web view inside the native bars. No script bridge to the device; links to other sites open in Safari; a tapped link to an app screen pushes it. |
| The theme's accent | The app's tint. Bars, tab bars, sheets and type stay the system's. |

## Liquid Glass: what it is, and where the system applies it

Liquid Glass is Apple's material for the **navigation layer** that floats above
the content: toolbars and their items, the tab bar, sidebars, sheets, menus,
popovers, alerts, search fields in a toolbar, and controls while they are
touched. It arrived with the 26 releases; the 27 releases refine it (more
diffusion over busy content, a user setting from clear to tinted, a uniform
toolbar when content scrolls under it) and do not let an app keep the old
look. Standard SwiftUI components get it with no code, and older systems get
their own look from the same components.

- **Never ask for it on content.** Rows, cards, banners, empty states, charts,
  tables and backgrounds are the content layer; Apple's guidelines say not to
  use glass there. The client never applies glass to anything the server sends.
- **No custom chrome.** Painted bars, brand-coloured tab bars and a sheet
  with a solid background fight the material. Brand colour lives in the
  content and in the accent.
- A page cannot ask for glass, blur or a corner radius, and should not try:
  there is no word for them, on purpose.

## What an author should and should not do

Do:
- Say the roles (`navree-native`) and let the client place them.
- Keep one main action, put creation in `Role::Create`, and keep destructive
  actions for what cannot be undone.
- Give every destination an icon and a short title (under about 15
  characters; not the app's name).
- Make records a table with a title, a subtitle, a status and a trailing
  column, or `Item`s.
- Put a form's submit in the form (`FormView::submit`), a cancel in the sheet's
  own close.
- Let a failure be an error state and a success be a visible result (the row
  appears, the sheet closes).

Do not:
- Ask for a floating button, a hamburger menu, a custom tab bar, a toast or a
  popover.
- Put "Back", "Close" or "Cancel" text next to the system's controls.
- Use ALL CAPS section headers; the system now uses title case.
- Use a checkbox or radio group and expect it to be drawn as on the web.
- Use a coloured pill for a status. Write words; the tone adds the colour.
- Switch the appearance in the app. Follow the system's light, dark and
  contrast settings.
- Set fixed text sizes or widths.

## What an Apple app needs from the server

The app is built on a Mac with Xcode 27 (Apple silicon, macOS Tahoe 26.6 or
later) or by a cloud build that runs on one; `navree native:doctor` says what
is missing and never prints a secret. The server pieces already exist; link to
their skills and do not repeat them:

| Need | Where | Skill |
|---|---|---|
| App ids (team id, bundle id), the `apple-app-site-association` file served at `/.well-known/`, the Associated Domains entitlement (`applinks:` and `webcredentials:`) | `auth.apps.apple.<name>` | `navree-auth` |
| Passkeys from the app (relying party id, `https://<rp_id>` origin, `webcredentials`) | `auth.passkeys`, `auth.apps` | `navree-auth` |
| Sign-in in the system's sign-in session and the hand-over to the app (a one-time code with a PKCE verifier; the callback is an https universal link) | `auth.apps.apple.<name>.callback` | `navree-auth` |
| Confirming one action with a passkey assertion over the action's own details | `ConfirmAction` | `navree-auth` |
| Sign in with Apple from the system sheet (the identity token for the bundle id, the nonce hashed as lower-case hex SHA-256) | `social.providers`, `auth.apps.apple` | `navree-social` |
| Push through Apple's service (a `.p8` key, `aps-environment`, the app registers its token with its session) | `notify.push.apns` | `navree-notification-channels` |

A paid Apple Developer Program membership is needed for Associated Domains,
passkeys, push and Sign in with Apple. What the client does about each is in
`references/availability.md`.

## Review checklist

Run on a screenshot of the iPhone and iPad passes (`navree-native-design`,
"Review a native screen"), light and dark, at the largest text, right to left
and with Increase Contrast. Answer yes or no; a "no" is a defect.

**iPhone (compact width)**
1. Top-level destinations are a tab bar at the bottom of 3 to 5 tabs, each with a
   label and a filled symbol; no hamburger, no custom bar, no action in the bar.
2. The first screen of a destination has a large title; pushed screens an
   inline title; no title is the app's name.
3. The back control is the system's chevron with no "Back" text.
4. The screen's main action is in the top bar's trailing end, not a full-width
   button in the content, and there is no floating action button.
5. At most one toolbar item is prominent, and it is not destructive.
6. Toolbar items are symbols with at most three groups; extra actions are in
   the system overflow; the ellipsis means overflow only.
7. Bars have no custom background, gradient or brand fill.
8. In a sheet, Cancel is at the top leading corner and Done or Save at the top
   trailing; Cancel, Done and Back are not all shown; a half-height sheet has a
   grabber; only one sheet is up.
9. Destructive actions are red and never the prominent button; a routine delete
   has no alert; a dialog's destructive choice is first and Cancel last.
10. Lists use a system style; rows that open a page show the chevron; section
    headers are title case, not capitals.
11. Rows that have quick or destructive actions have swipe actions, and the
    same actions are in the row's context menu.
12. Forms are grouped, with no full-width Submit at the bottom; a field's error
    is in words under it.
13. Switches are only in list or form rows; there is no checkbox or radio button.
14. Search is a field under the title above its list, or a search tab.
15. An empty screen has a symbol, a title, a description and an action; an
    empty search names the text searched.
16. No toast or snackbar floats; status is a line in the content; an error that
    needs action is an alert with a specific title and verb buttons (not "OK").
17. No popover; small tasks are sheets.
18. Glass appears only on bars, controls and system overlays; no row, card,
    banner or background is glass.
19. The brand colour is on a few interactive elements, not on bar labels over
    colourful content.
20. All text uses system styles; at the largest size nothing important is cut
    and the layout stacks instead of squeezing.
21. Standard actions use the system's symbols; custom icons match their weight.
22. Light and dark both look intended; nothing white glares in dark.
23. Every tappable element is at least 44 by 44 points.
24. Progress is a system spinner or bar; a pending button shows its spinner
    inside it.
25. Status is words with a coloured symbol, not a red pill; red badges are for
    unread counts only.
26. Content avoids the Dynamic Island, the home indicator and the camera.

**iPad (regular width)**
1. The top tab bar can become a sidebar; the sidebar shows at most two levels.
2. A list with a detail shows columns, with the selection highlighted (not yet
   built: expect a pushed screen).
3. Leading toolbar items do not collide with the window controls.
4. Search is at the trailing end of the toolbar.
5. Small related tasks are popovers or centred form sheets, not full-screen.
6. The layout adapts at narrow, intermediate and wide widths without losing
   functions.
7. Tables show several sortable columns.
8. Pointer hover and keyboard focus are visible on custom controls.

## Common mistakes

| Mistake | Fix |
|---|---|
| A floating button for "create" | `Role::Create`: a plus in the top bar. |
| A full-width Save under a form | The form's own submit, in the toolbar. |
| Toasts and snackbars | A visible result, a status line, an alert for a failure. |
| "Are you sure?" on every delete | Reserve the destructive role for what cannot be undone; reversible actions get a toast with Undo. |
| A hamburger menu or drawer | The shell's destinations: a tab bar, a sidebar on a wide window. |
| A custom navigation bar in the brand colour | The system bars, with the accent. |
| Painting bars or sheets under Liquid Glass | Nothing: let the system separate content. |
| Glass cards and frosted rows | Standard backgrounds; glass is for floating controls only. |
| Upper-case section headers | Title case. |
| Fixed font sizes | System text styles only, so Dynamic Type works. |
| Choosing a layout by device ("if iPad") | The window's size class. |
| Radio buttons and checkboxes | Switches in rows; inline pickers with checkmarks. |
| A web table squeezed onto a phone | Rows with a title and a subtitle (column roles). |
| A custom share or delete glyph | The standard icon; the client maps it. |
| Red pill status badges | Words with a symbol. |
| A logo in every header, a splash with a delay | The first screen's content; a launch screen that looks like it. |
| An in-app light and dark switch | The system's appearance. |
| A blank screen while loading | Cached content, then placeholders. |
| A web view as a browser | Native bars always; other sites open in Safari. |

## Docs

- https://navree.com/docs/ui/native/
- Apple's pages are listed in `references/sources.md`.

References: [`references/availability.md`](references/availability.md) (what
the systems offer by OS version, and what Navree has of it),
[`references/sources.md`](references/sources.md) (the guideline pages used,
with the date read).
