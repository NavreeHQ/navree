---
name: navree-native-apple
description: "What the Apple client of Navree Native does with each part of a page on iPhone, iPad and the Mac (SwiftUI, Liquid Glass), as built: the tab bar, the sidebar-adaptable tab view and the Mac sidebar by width, a list beside its detail and panes in the system split view, large titles, where a screen's actions go by role (the plus in the top bar, the one prominent trailing action, the overflow menu, a page's head row of buttons as the screen's actions, destructive actions in a section of their own or, on a Mac, the menu bar), the Mac's three-column window, menu bar and Settings window, sheets, alerts and confirmation dialogs, no toasts, search, lists and tables as rows, forms with system controls, the reply composer, menus with their words, symbols, Swift Charts and the health kinds, what VoiceOver says, contrast and target sizes, the web view; what Liquid Glass is and where the system applies it (so nobody asks for it on content); which systems and OS versions are supported and which are not built; what an author should and should not do; the server pieces an Apple app needs (app ids and the association file, passkeys, Sign in with Apple, push through APNs); a yes/no review checklist for screenshots; common mistakes with the fix; and dated sources. Use when building, reviewing or debugging a Navree app on iPhone, iPad or Mac, when a native screen looks wrong on Apple, or when asked what Apple's guidelines or Liquid Glass mean for a Navree page. Load `navree-native` and `navree-native-design` first."
---

# Navree Native on Apple systems

The Apple client (`packages/apple`, a Swift package: `NavreeProtocol`,
`NavreeDesign`, `NavreeRuntime`, `NavreeCoreBridge` and `NavreeShell`) draws a
Navree page with SwiftUI, so the system's own components do the work and the
system's version decides the look. Everything that is not drawing, placement
or talking to the system (navigation, forms, sign-in, the cache) is the
shared runtime core's, reached through `NavreeCoreBridge`. This skill says
what the client does with each part, what that means for the page, and how to
review the result. The guidelines it follows are Apple's Human Interface
Guidelines at their current state; this skill summarises them and links the
pages (`references/sources.md`, read on 2026-11-12, describing iOS 27 and the
26 releases). It never copies them.

The words an author has are in [`navree-native`](../navree-native/SKILL.md);
the rules that hold on every system in
[`navree-native-design`](../navree-native-design/SKILL.md).

## What is built, and what is not

| System | State |
|---|---|
| iPhone (compact width) | Built. Run in the iOS 27.2 simulator against the store, inbox and board demos and in the cloud on Xcode 26.6 and 27.2: light, dark, largest text, right to left, Increase Contrast, Reduce Transparency; Apple's accessibility audit and VoiceOver's own words read through the main screens by the UI tests. Not run on a device. |
| iPad (regular width) | Built: the tab bar at the top that turns into a sidebar, a list beside its detail and panes as the system's split view, tables as columns. Run in the iPad simulator. |
| Mac | Built as a Mac app of its own (not the iPad app on a Mac): the sidebar, a list beside its detail in a window of three columns as Mail's, the menu bar's commands, a Settings window, each window's state kept apart. Run on a Mac from recorded answers and against the demos; the app pictures its own windows and lists its menus. |
| iPhone Duo, Apple Watch, Apple TV, Apple Vision Pro | Not built. The watch and TV have no web view, so they will take a strict subset. |

Every kind of part is drawn natively; only `web` is not (the client's
`undrawable` list), so a page that holds a `web` part is shown whole in the
web view inside the native bars, never with a hole in it.

The floor is iOS and macOS 26 (the package's `platforms`): the current system
and the one before it, built with Xcode 26 or 27. Everything Liquid-Glass-
specific then comes from system components. Navree never keeps the old design:
under the 27 SDKs the system ignores the opt-out (`UIDesignRequiresCompatibility`).

## What the client does with each part

Placement is chosen by the window's **width**, never by the device kind: an
iPhone app resizes on iPadOS and in iPhone Mirroring, so a compact window is a
phone whatever it runs on. The core is told one of three widths: **compact**
(the system's compact size class, or any window under 600 points, such as a
narrow Stage Manager window), **medium** (under 1000 points) and **expanded**.
A Mac window is never compact. Each rule has a test in
`packages/apple/Tests/NavreeDesignTests`.

| Your page says | The client draws |
|---|---|
| The shell's destinations (`NavItem`s) | A tab bar at the bottom in a compact window (3 to 5 tabs, a filled symbol and a label each; the rest in the system More tab); in a regular window a tab view that can turn into a sidebar, the shell's groups as its sections when there is more than one group. Never a hamburger, never a drawer, never a custom bar. |
| A screen's title | A large title at the first screen of a destination, an inline title on a pushed screen or in a sheet. The app's own name is never a screen's title. A page shown in the web view has an inline title. |
| `Role::Create` or `add` | A plus at the trailing end of the top bar (on a Mac, over the list's column, after its sort and filter, as Mail's compose stands after its filter), and File > New with ⌘N. Never a floating button. |
| The one `Primary` action | The one prominent item at the trailing end of the top bar; in a sheet, Done or Save at the top trailing corner; on a Mac, the action's own words ("Refund") on the sheet's default button at its foot, which Return presses. A second primary goes to the overflow menu. It is the only bar item in the app's accent: every other item of a bar (the plus, sort, filter, the account, Edit) is drawn in the bar's own colour, as the system draws Back. |
| `Secondary` actions | The system's overflow menu at the trailing end; no overflow menu of your own. A disabled one shows why under its words (`Button::reason`), as a menu item's second line. On a Mac each is an item of the window's toolbar, its symbol (its words where it has none), why it is disabled in its tooltip, the system moving into its own overflow menu what does not fit; and a command of the menu named for the destination. A toolbar's items (`toolbar` in a screen's actions) are placed the same way, by role, never by their order; a toolbar in the body stands on the bottom bar. |
| A row of buttons at a page's head (the web's page header: buttons above everything but the title's words and states) | The screen's own actions, placed by role as above, not buttons in the content. Better: send them as the screen's `actions` with icons, which every client places. |
| `Destructive` actions | A section of their own at the end of the content, in red, never prominent; on a designed screen (no list of its own) the last items of the overflow menu, in red. On a Mac, only the last command of the destination's menu, in red, never in the toolbar or the content. A disabled one says why in the section's footer. They ask first, with the page's own question (`ask`) as an action sheet whose destructive answer is red and Cancel last, unless the page says the action can be undone (`Button::undoable()`), when it asks nothing. Any other question is an alert whose answer is not red. |
| `Cancel` in a sheet | The top leading corner, as a close symbol. Swiping the sheet down stays on. On a Mac, the word Cancel on a push button at the sheet's foot, which Escape presses, on its first press even while a field of the sheet is being typed in, as the system's form sheets do (a character being composed with an input method keeps its Escape). |
| A task, a glance | A task (`Present::Task`, `Sheet.task`) is a full-height sheet with Cancel at the top leading corner and its one form's main submit as Done at the top trailing corner, whatever role its look gave it; it can't be swiped away while it holds what the person typed. A glance (`Present::Sheet`) rests at half height with a grabber, Close at the top leading corner, and is swiped away. |
| A popover | Anchored to the control that opens it on a regular width; the system's half-height sheet on a compact one. |
| Panes (`split` of `pane`s) | A split led by a `list` pane is the system's split view at every width: the list in its column, the detail beside it, the inspector beside that where the window is expanded; anywhere narrower its bar button opens it as a sheet, at half height first, so nothing of it waits out of sight beside the detail. On a compact width the split folds into one stack showing the pane the page says is `current`. A choice in the list replaces the detail rather than stacking it; the list shows the open row chosen. A split among other parts stands inside its screen: side by side where wider than compact, one over the other on a compact width. |
| A list or table whose rows all open a screen | On a medium or expanded window, the list beside the screen a row opened (Mail's messages beside a message), the open row chosen; on a compact width the rows push as ever. On a Mac every destination shares one sidebar: a stack beside it in two columns, a list beside its detail in three. |
| A conversation (`message`), an activity feed (`feed_row`) | A message as Mail sets one out in a thread: picture, author, time, words, how far it got; an internal note says so. A feed row: its symbol in the colour of its tone (or who did it), its words with names in bold, when, and a tap that opens what it is about. |
| The reply to a conversation | A form that posts, holds a field of many lines and a primary submit, and comes after the messages is the conversation's composer: it rests at the foot of the screen as Messages keeps its field (above the tab bar and the keyboard, with the system's edge effect), its field growing with what is written and its send a filled arrow inside the field's trailing end, no taller than the field, grey until there are words, as Messages sends. Its other parts (how it is sent, saved replies) stand in a row over the field while the row fits, and one under another when it does not (the largest text sizes in an iPad's narrow column). The protocol has no word for a composer yet; the client reads the shape. |
| A menu in the content (`menu`) | The system's pull-down menu, its words beside its symbol (a conversation's status reads "Open", its priority "High"), never a bare symbol. In the menu bar, commands are in title case with "…" where they ask first, and a symbol only for a standard action. |
| A sheet (`medium`, `large`, `full`) | Medium: rests at half height with a grabber, can be pulled up. Large: full height. Full: a full-screen cover, for a camera or a long flow. On a Mac, a form's width (470 points) within the window it stands on, as tall as its content up to a form's height (400), its content scrolling when taller: a sheet of one field is not mostly empty, and a list, which has no height of its own, still shows (the sheet is a form's height until the list has said how tall it is). It keeps that size as the window under it changes, and takes the room of a new window when the app's shell moves into one. Its title and subtitle stay in the sheet; the window under it keeps its own. One sheet at a time. A dialog that holds a form is a sheet; one that only asks is an alert or, for a destructive question, a confirmation dialog. |
| A toast, a banner | There is no toast on Apple systems. A failure is an alert; anything else is a dismissible line of status at the top of the content, with a symbol, a haptic (success, warning or error) and a VoiceOver announcement. |
| A table's records | In a compact window each record is a row, read by what each column says it is (`#[col(role = ...)]`): the title, the subtitle line under it, a leading picture, states as words with a symbol in the tone's colour, the trailing value (an amount) at the end; a column with no role adds its words under the title unless it is `secondary`. A tap does what the row says (`row_opens`): a push shows the chevron, a peek that fills a sheet does not. The actions column, the row's own actions and a form in any cell (its inline edit, opened as a task) become its swipe actions and its context menu. In a regular window with more than two columns, a `Table` with sortable headings; where the window is too narrow for every column, the table gives up its `secondary` ones and their words stand on a line under the record's name, as Files writes a file's date and size under its name, so nothing the row said is lost; words longer than the name's column go on to a second line rather than being cut. Rows that can be chosen use the system's edit mode, their actions on the bottom bar and "Choose All" when the table offers every match; a list that can be reordered takes the system's drag. |
| A table's search, filters, sort | Read by what each field is for (`purpose`): a search field under the title (compact) or at the trailing end of the toolbar (regular); filters in a sheet of choices with checkmarks; sort as a menu; a page-size field is left out. |
| Lists | Records as plain rows edge to edge; a record's facts, sections and forms as inset grouped lists with title-case headers. A row that opens a page shows the system's disclosure chevron; one that opens a sheet does not. |
| Fields | A grouped form. A switch for a toggle (a checkbox is drawn as a switch), an inline picker with checkmarks for a radio group (iOS has no radio buttons), a segmented control for at most five short choices, a menu picker by default, a screen of choices for many, the compact date picker, the matching keyboard and autofill for email, password, phone, number and one-time code. A span of days as the system's two compact pickers (Starts, Ends), the end never before the start; a file through the system's file browser, limited to the types it takes and sent with its bytes. A field that acts when it changes (`changed`) acts as the person changes it. A field's error shows under it in words and the first field in error takes focus. |
| Empty, loading, failed | The system's content-unavailable layout (a symbol, a title, a description, an action); cached content first, placeholders while loading. |
| Icons | One family, the SF Symbols: the icon's `symbol` when the server filled it, else the protocol's own table (`x-symbols`) for its name, else the client's own table of the toolkit's names (`AppleSymbols`, held to the system's list for iOS 26 and macOS 26, with a level for signal and Wi-Fi bars); the standard symbols for standard actions even when the page sent its own. The page's shapes are drawn only for a name neither table holds, and then at the size and line weight of the symbol beside the same words, following the person's text size and Bold Text; in a tab bar or a sidebar, at the size of the symbols there. The rupee is `indianrupeesign`. |
| A chart | Swift Charts from the page's numbers, with the theme's colours and a sentence for VoiceOver; the value axis writes numbers as the page writes them (₹40,00,000, never 4.0E6); the days along the foot are named by the plot's own width so they never run together. |
| The health kinds (`ring`, `rings`, `gauge`, `day_clock`, `day_pager`, `meter`, `columns`, `strip`, `range`, heat maps, `scatter`, `figure`, `timeline`) | Rings in the manner of the system's activity rings (a second lap past the goal), concentric rings with their values beside them, the system's gauge with its zones, a 24-hour dial with the day's moments, the day pager as the bar's middle item, the rest with Swift Charts; colours by name are the system's own. A ring's number grows with the person's text at every size and fits itself into the ring. Each says its name once to VoiceOver, then its value. |
| A time the client writes (the day clock's hours and moments) | As the person's locale writes it, on their 12 or 24 hour clock. The app tells the server the same locale (`locale=`, in BCP 47, such as `en-IN`), so words the page writes with a time in them should be written in it; better, send the time as a value (a moment's `at`) and let each client write it. |
| A page that falls back | A web view inside the native bars. No script bridge to the device; links to other sites open in Safari; a tapped link to an app screen pushes it. |
| The theme's accent | On iPhone and iPad, the app's tint: the one prominent action, what is chosen, links. A bar's other items stay the bar's colour; bars, tab bars, sheets and type stay the system's. On a Mac the accent the person chose in System Settings is every control's, as Apple asks. |
| Words in a tone or a colour (`text` with a `tone`, a status, a trend) | The tone's colour, darkened in the light as little as gives the words 4.5 to 1 on a card and on the grouped ground (the system's green and orange are too pale for words as they are); the system's own colour in the dark. A status's symbol or dot keeps the bright colour. |
| What VoiceOver says | One stop for one thing: a fact is "Subtotal, ₹428"; a row is its name, its state in words, its lines and its end, each said once (a picture beside its person's name is not said again; a bar's score is not said again by the number at the row's end; a status's check mark is never read as "selected"; a step's done, current or upcoming state is a word). A disclosure that holds a form in a row (a board card's Move) is one of the row's actions, which opens the form in a sheet; a button that only repeats the row's title is not drawn: the row is that control, its double tap, a tap anywhere on it (a click on a Mac) doing what the button did (a board card opens its peek), so the card's title stands once. On a Mac, a disclosure that holds a form in a row (a board card's Move) is the row's command: the Mac's borderless button in the accent ("Move…") at the row's foot and the same command in the row's context menu, each opening its form in a sheet. A part's value never repeats its name ("Health score, 79 of 100. Good."). |
| Small controls | On iPhone and iPad an info button, a button drawn as its words or its symbol alone, a link and a menu in the content each take a target of at least 44 by 44 points round what they draw, while they look and lay out as before: the line they stand on and a table cell keep their spacing. A bordered button in the content is the system's regular size, a capsule about 34 points tall as in Mail and Settings (never the large one, about 50), and its target reaches 44 points round the capsule. |
| A card that says `zoom` | On iPhone and iPad, a card with an id whose tap opens a screen grows into that screen, pushed or presented, and the screen shrinks back into the card when the person goes back (the system's zoom transition). With Reduce Motion the screen comes in as any screen does. A card that fills a sheet of its own screen, or has no id, opens without it; a Mac opens every screen as it always does. |
| The Mac's window | With one destination, that destination's screen and its toolbar, no sidebar, the person's account in the toolbar's trailing person button. With more, Mail's: the sidebar of destinations with the person's account at its foot, the list's column with its title, its sort and filter and its plus over it, and the detail, whose name stands at the head of its content (the window keeps the destination's name) and whose actions stand over it. Lists are the Mac's inset lists, the chosen row a rounded shape inset from the column's edges; a list's symbols stand in one 20-point column, so its labels start at one edge; a button in the content is the Mac's push button at its regular size, and a borderless one (low prominence: "Saved replies") writes its words in the accent, as the Mac's own read as controls. A detail stops at a grouped form's width (704 points, more as the person's text grows), centred, whether it is composed or a list, and keeps at least 400 points beside the sidebar (the width every screen is laid out for): the window stops at the sidebar's least beside it rather than squeezing the detail. An inspector stands beside the detail only where the window leaves the detail those 400 points beside it; elsewhere it waits behind its bar button. Search is at the toolbar's trailing end and ⌘F goes to it. |
| A composed screen on a Mac | Set out as a grouped form sets out its sections (measured on macOS 27.2): its first part 20 points under the bar, 10 between cards, sections and grids, a group's heading 30 under the part before it and 10 over its own, in the headline style (13 points, bold) as System Settings heads its sections; every word at one leading edge, the words on the ground and a grid of figures with no card set in as far as the words in the cards; a row's picture with a size of its own at a Mac list's 24 points; a large figure alone shrinking as far as a quarter of its size to fit a narrow window rather than being cut. iPhone and iPad keep the page's own steps and Health's look. |
| The Mac's menu bar | App: Settings… ⌘, and the person's account; File: what the screen makes new with ⌘N, New Window (⌥⌘N when ⌘N is taken), Share…; Edit: Find… ⌘F; View: each destination ⌘1 to ⌘9, Back ⌘[, Refresh ⌘R and the system's sidebar, toolbar and inspector commands; then a menu named for the destination with the screen's other actions, the chosen row's context actions under its name, and what destroys last, in red, asking first. While a detail is open beside its list, the list's commands stay. |
| Settings | On a Mac, App > Settings… opens the screen the user menu leads to for settings in a window of its own; never a toolbar gear. |


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

## What the app sets in its build

The app's place in the system is the app's, not a page's: it is set once in
`ShellConfiguration`, and the app declares its scenes with one line
(`var body: some Scene { NavreeScenes(configuration: .shop) }`). Every default
keeps an ordinary app in the Dock with the system's own windows.

| What | Reference |
|---|---|
| Where the app stands outside its windows: the Dock or the menu bar, its item in the menu bar, the count on its icon, its quick actions and Dock menu, opening at login, About and Help (`presence`) | [`references/presence.md`](references/presence.md) |
| Its windows, their toolbar and its icon: window sizes, tabs, the toolbar's style and labels, the icon file, and the icons a person may choose on iPhone and iPad (`windows`, `icons`) | [`references/windows-and-icons.md`](references/windows-and-icons.md) |

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

### Signing in, and what needs a team

The core runs every sign-in; the client only opens the system's sheet and
says how it ended. A feature that needs an entitlement is offered (listed in
the client's capabilities) only when the build says it is signed for it, so
an unsigned build never shows a control it cannot carry out. The build says
so in its Info.plist, from build settings: `NavreeSignedWith` =
`$(NAVREE_SIGNED_WITH)` (words apart: `associated-domains`,
`sign-in-with-apple`, `push`) and `NavreeAssociatedDomain` =
`$(NAVREE_ASSOCIATED_DOMAIN)`, which counts only when it is the origin's own
host over https.

| Feature | Unsigned, in the simulator | What it needs |
|---|---|---|
| The sign-in handed over on the app's own scheme | works | the scheme in `CFBundleURLTypes`; on the server `callback = "<scheme>:/signed-in"` with `custom_scheme = true` |
| The sign-in handed over on an https link | not offered: the session is never opened | a team; Associated Domains on the App ID; `webcredentials:<host>`; the association file on a real https host |
| Sign in with Apple | not offered | a team; Sign in with Apple on the App ID; the entitlement; a person signed in to an Apple Account on the device; `social.providers.apple` and `auth.apps.apple` on the server |
| Passkeys, made or used | not offered | as the https sign-in, with `webcredentials:<rp_id>`; and, for now, a core that fetches the passkey's options (it does not yet) |
| Confirming an action with a passkey | not offered | as passkeys, and a passkey made for the site |
| Push | not offered | a team; Push Notifications on the App ID; `aps-environment`; `notify.push.apns` with a `.p8` key on the server; delivery is proven only on a device |

For the hand-over the app lists its sign-in page among the addresses that
open the system's session (`ShellConfiguration.signInPaths`, such as
`["/sign-in", "/auth/*/redirect"]`), and the server names the app:

```toml
[auth.apps.apple.main]
team_id = "ABCDE12345"
bundle_id = "in.example.charkha"
callback = "https://charkha.example/app/signed-in"   # or "charkha:/signed-in" with custom_scheme = true
```

With `credentials` or `links` (both on by default) the server serves
`/.well-known/apple-app-site-association` itself; for the app above and
`auth.apps.paths = ["/orders/*"]` it is:

```json
{
  "applinks": { "details": [{ "appIDs": ["ABCDE12345.in.example.charkha"], "components": [{ "/": "/orders/*" }] }] },
  "webcredentials": { "apps": ["ABCDE12345.in.example.charkha"] }
}
```

A team then turns everything on with one file. The demo has it written and
off: copy `packages/apple/Demo/Signing/Signing.local.xcconfig.example` to
`Signing.local.xcconfig`, put in `DEVELOPMENT_TEAM`, the bundle id and the
domain, and run `xcodegen generate`; it adds `Signing/NavreeDemo.entitlements`
(Sign in with Apple, `webcredentials:` and `applinks:` for the domain,
`aps-environment`) and says so in the Info.plist. An app of its own does the
same, and adopts `NavreeAppDelegate` (`@UIApplicationDelegateAdaptor`) so the
push token and a tapped notification reach the core.

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
   trailing (on a Mac, words on push buttons at the foot, the confirming one
   the default button); Cancel, Done and Back are not all shown; a half-height
   sheet has a grabber; only one sheet is up; a Mac sheet stays inside its window.
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
19. The brand colour is on the one prominent action, the selection and links,
    not on the bar's other items.
20. All text uses system styles; at the largest size nothing important is cut
    and the layout stacks instead of squeezing.
21. Standard actions use the system's symbols; custom icons match their weight.
22. Light and dark both look intended; nothing white glares in dark.
23. Every tappable element is at least 44 by 44 points, and words in a tone's
    colour meet Apple's contrast audit.
24. Progress is a system spinner or bar; a pending button shows its spinner
    inside it.
25. Status is words with a coloured symbol, not a red pill; red badges are for
    unread counts only.
26. Content avoids the Dynamic Island, the home indicator and the camera.

**iPad (regular width)**
1. The top tab bar can become a sidebar; the sidebar shows at most two levels.
2. A list with a detail shows columns, with the open row chosen.
3. Leading toolbar items do not collide with the window controls.
4. Search is at the trailing end of the toolbar.
5. Small related tasks are popovers or centred form sheets, not full-screen.
6. The layout adapts at narrow, intermediate and wide widths without losing
   functions.
7. Tables show several sortable columns.
8. Pointer hover and keyboard focus are visible on custom controls.

**Mac**
1. The window is the system's: a sidebar of destinations, the list's column
   and the detail, with no custom frame.
2. The list's title, sort, filter and plus stand over the list's column; the
   detail's actions over the detail; nothing is cut, and what does not fit
   goes into the system's overflow menu.
3. The detail's name is at the head of its content; the window keeps the
   destination's name.
4. The list is inset, its chosen row a rounded shape; a content button is a
   regular push button, not a tinted shape that hugs its words.
5. Every toolbar action is a menu command; what destroys is the last command
   of the destination's menu, in red, and asks first.
6. Settings opens from App > Settings… (⌘,) in a window of its own.
7. Controls take the accent the person chose, not the app's.

## Common mistakes

| Mistake | Fix |
|---|---|
| A floating button for "create" | `Role::Create`: a plus in the top bar. |
| A full-width Save under a form | The form's own submit, in the toolbar. |
| Toasts and snackbars | A visible result, a status line, an alert for a failure. |
| "Are you sure?" on every delete | Reserve the destructive role for what cannot be undone; reversible actions get a toast with Undo. |
| A hamburger menu or drawer | The shell's destinations: a tab bar, a sidebar on a wide window. |
| A custom navigation bar in the brand colour | The system bars; the accent on the one prominent action. |
| A page's actions as a row of buttons in the body | The screen's `actions`, with icons; the client places them by role on every system. |
| Text in a tone's bright colour on a white card | The client darkens toned words for you; do not pick a colour to stand for a tone yourself. |
| The same name twice in a row (a picture's name and the words) | The client says it once; keep the words short and the picture's `name` whole. |
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
| Signing in inside the app's web view | The system's sign-in session, handed over to the app. |
| Listing `passkey`, `confirm`, `platform_sign_in` or `push` in an unsigned build | Each is offered only when the Info.plist says the build is signed for it. |

## Docs

- https://navree.com/docs/ui/native/
- Apple's pages are listed in `references/sources.md`.

References: [`references/availability.md`](references/availability.md) (what
the systems offer by OS version, and what Navree has of it),
[`references/sources.md`](references/sources.md) (the guideline pages used,
with the date read).
