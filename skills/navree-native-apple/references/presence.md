# Where an Apple app stands outside its windows

Where the app stands in the system is the app's, never a page's: it is set
once in the build, in `ShellConfiguration.presence` (a `Presence`). A server
reaches it only through what the protocol already has: the count on the icon
(`set_badge`), notifications the device schedules (`notify`,
`cancel_notification`), the destinations of the shell. Every default keeps an
ordinary app in the Dock, with the system's own About and Help, no item in the
menu bar, no quick actions and no Dock menu of its own.

```swift
@main
struct ShopApp: App {
    #if os(macOS)
    @NSApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
    #else
    @UIApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
    #endif

    var body: some Scene { NavreeScenes(configuration: .shop) }
}

extension ShellConfiguration {
    static let shop = ShellConfiguration(
        origin: "https://shop.example.in", appVersion: "1.4.0",
        presence: Presence(
            place: .both,                                  // Dock and menu bar
            choices: [.dock, .menuBar, .both],             // the person may change it in Settings
            statusItem: StatusItem(image: .symbol("shippingbox")),
            offersOpenAtLogin: true,                       // a checkbox in Settings, off until the person turns it on
            shortcuts: .on(limit: 4),                      // quick actions and the Dock menu
            badge: .destination("/orders"),                // the icon's count follows a destination's badge
            about: About(credits: "Made in Ludhiana. [Shop](https://shop.example.in)", copyright: "© 2026 Shop Traders"),
            help: .page("/help")
        )
    )
}
```

The delegate is adopted once, in the `App`, as SwiftUI asks. On a Mac it
gives the Dock its menu, opens the main window when an app in the menu bar
only is opened again with no window showing, and opens a clicked
notification's address. On iPhone and iPad it passes on the push token and a
tapped notification, and gives each window scene `NavreeSceneDelegate`, which
opens a Home Screen quick action.

## Where the app stands on a Mac

| `place` | What the person sees |
|---|---|
| `.dock` (default) | An ordinary app: a Dock icon, the menu bar's menus, a window at launch. |
| `.menuBar` | The app's item in the menu bar only: no Dock icon, no window at launch. While one of its main windows is open it is in the Dock with its menus, so every command of the window is in the menu bar too, and it leaves the Dock when the last window closes (`dockWhileWindowOpen: false` keeps it out of the Dock). |
| `.both` | A Dock app with its item in the menu bar as well. |

- The menu bar needs a `statusItem`: without one the app stands in the Dock,
  whatever `place` says, so it can always be reached.
- `choices` are the places the Settings window offers as a radio group in a
  General pane; fewer than two that can stand offer no choice and no pane.
  The person's choice is kept by the system's defaults (`navree.place`).
- **Info.plist.** An app that may ever start in the menu bar only says
  `LSUIElement` = YES in its Info.plist, so its Dock icon never shows at
  launch; if the person then chooses the Dock, the icon comes a moment after
  launch. The client logs a fault at launch when the Info.plist and the
  configuration disagree.
- The person may take the item out of the menu bar (⌘-drag): an app in both
  places then stays in the Dock; the system ends an app in the menu bar only,
  as it does for any such item.

## The item in the menu bar

`StatusItem(image:title:showsCount:content:)`:
- `image`: a system symbol (`.symbol("shippingbox")`), or a template image of
  the app's assets (`.asset(name)`), drawn in the menu bar's own colour. The
  menu bar is 24 points high; a symbol is what Apple suggests.
- `title`: words beside the image; none by default, since the system hides
  items when the menu bar is crowded.
- `showsCount`: the app's count beside the image while the app is in the menu
  bar only (in the Dock, the count is the Dock icon's).
- `content: .menu`: a menu of the app's destinations (each with its badge),
  the person's own menu when the app is in the menu bar only (there is no App
  menu then), Settings… (⌘,) and Quit (⌘Q). What is chosen opens in the main
  window in front, or in a new one. Before any window has shown the app's
  destinations, the menu offers "Open <App>".
- `content: .app(StatusWindow(...))`: **the whole app under the item**, for
  an app in the menu bar only (below).
- `content: .screen("/today")`, a small window showing one destination, is
  **not built yet**: it needs one session shared by the app's cores. Such an
  item stands nowhere and the app stays in the Dock.
- `tooltip`: what the item says when the pointer rests on it.

The item is the system's own `NSStatusItem`, made only while the app stands
in the menu bar. An app that never asks for one has no item at all, not even
a hidden one. (SwiftUI's `MenuBarExtra` is not used: a scene cannot be left
out by a value known at run time, a `MenuBarExtra` left out of the menu bar
still makes an item, and it gives no right-click menu.)

## The whole app under the item (an app in the menu bar only)

```swift
presence: Presence(
    place: .menuBar,
    statusItem: StatusItem(
        image: .symbol("shippingbox"), title: "Charkha", tooltip: "Orders and stock",
        content: .app(StatusWindow(size: CGSize(width: 400, height: 560), resizable: false, pinned: false, opensAtLaunch: false))
    )
)
```

The build says `LSUIElement`. Then:
- **A left click** opens a window under the item, centred on it and kept on
  the screen whose menu bar holds the item, as the system's own items open
  theirs: no arrow, no window buttons, the system's window material, the item
  shown pressed while it is open. A second click on the item, or Escape once
  nothing in the window takes it first (a field's words, a menu, a sheet's
  Cancel), closes it; so does a click anywhere else or the app going to the
  background, unless `pinned` (then it stays open above other windows). It
  takes the keyboard, so forms can be typed in, and holds the app's sheets,
  questions and menus as any window does. It is never in the Window menu nor
  among the windows ⌘` steps through. It opens again where the person left
  the app, and a window the person may resize at the size they left it.
- **Its sheets fit it.** A Mac sheet takes a form's width (470 points) within
  the room the window it stands on has under its bar, and is as tall as its
  content up to a form's height (400): in the 400-point window under the item
  a sheet is that window's width, its form whole, its content scrolling when
  taller, never hanging over the window's edges; a sheet up while the person
  resizes the window stays inside it, centred; carried into the full window
  with the shell, it takes a form's width there. Escape presses its Cancel on
  the first press, also while a field is being typed in.
- **It holds the whole app**: the app's one shell and session, with its
  navigation, sheets, forms, search, menus, notices, web screens and the
  person's menu. There is no second session: the window *is* the app's.
- **Laid out as a compact window**: the core is told the width is compact,
  so each destination is one column (a list pushes its detail, with the back
  button in the bar), and the destinations stand in the Mac's own sidebar,
  folded away behind the toolbar's sidebar button, as a narrow window of
  Notes or Mail folds its sidebar. Not the iPhone's tab bar at the foot (no
  Mac app has one), and not a segmented control in the bar: Apple's guidelines
  put navigation among an app's sections in a sidebar (Sidebars), and keep
  tab views and segmented controls for closely related content and choices
  (Tab views, Segmented controls).
- **The bar shows the screen's own title**, leading, with the screen's
  actions trailing, in the system's compact toolbar (the bar AppKit gives a
  narrow utility window: 24-point controls in a 40-point bar; the unified
  bar's 36-point circles are heavy in a window 400 points wide), the content
  starting 20 points under it as a grouped form's first section does: the window holds a whole
  app, with a back button on a pushed screen, so the bar says where the person
  is, as a narrow window of Mail or Notes titles what it shows (Windows: a
  window's title names what it shows) and as Apple's own menu bar windows
  (Control Center's Wi-Fi, Bluetooth and Sound) head themselves with the name
  of what they hold, their controls trailing. The window's title is that same
  title, so VoiceOver and the full window say what the bar says, and it is the
  app's name only while no screen has one. `MenuBarExtra`'s window style has no
  bar at all, so a screen's title, its back button and its actions would have
  nowhere to stand: that is the one way this window differs from it.
- **A right click** (or a click with Control held) opens the system's menu:
  the destinations, the person's menu, Open in Window, Settings…, About,
  Quit. `MenuBarExtra` has no right click, which is why the item is the
  system's `NSStatusItem` with a panel (`NSPanel` hosting the shell).
- **Open in Window** (the right-click menu; ⌘N and File > Open in Window
  while the app's menus show) moves the same shell, what it shows included,
  into a full window, laid out by that window's width as a Dock app's window
  is from its start (the sidebar beside the content, the screen's title over
  the content, the toolbar laid out again for the new window), with the app in
  the Dock and its menus while the window is open (`LSUIElement` apps come into
  the Dock only then); the window under the item closes. Closing the full
  window puts the app back under its item and out of the Dock. Opening the
  app again from the Finder, the Dock menu's New Window, and a notification
  clicked while nothing is open lead to the same place.
- `StatusWindow`: `size` (400 × 560 points by default), `minimumSize`,
  `resizable`, `pinned`, `opensAtLaunch`. No background or material of the
  app's own: the window keeps the system's.
- The item's `title` is its label, `tooltip` its tooltip; the count a server
  sets with `set_badge` stands beside the image while `showsCount` is on.
- From code: `PresenceModel.shared.showStatusWindow()`, `hideStatusWindow()`,
  `openInWindow()`, and `statusWindowShown` (observable) for whether it is open.
- In both places (`.both`) such an item would hold a second session beside
  the Dock app's windows; until the app's cores share one session, `.both`
  is not offered for it and stands as the menu bar only.
- **The app starts with the app**, hidden: its shell and core run from
  launch, so the server's `set_badge` count stands beside the item before the
  person first opens the window, and the window opens on the app already
  there. No window shows at launch and the app stays out of the Dock.
- **Signing in**: the system's sign-in sheet (and Sign in with Apple's and a
  passkey's) stands over the window under the item, opened for it if it was
  closed, or over the full window while one is open; the window under the item
  stays open while such a sheet is up, whatever is clicked.
- **Its buttons are words.** On a Mac every sheet's Cancel reads "Cancel" (or
  "Close" for a glance) and its confirming button the action's own words
  ("Refund", "Restock", "Apply"), as push buttons at the sheet's foot; the
  confirming one is the default button, which Return presses, and Escape
  presses Cancel. iPhone and iPad keep ✕ and ✓ in the sheet's bar.

## The count on the app's icon

- A server sets it with `set_badge` (`{"op":"device","call":{"f":"set_badge","count":3}}`),
  in any answer's ops, the home screen's answer at launch included, or from a
  tap; `count` left out clears it.
- `badge: .destination("/inbox")` makes the count follow one destination's
  badge, as Mail counts its inbox; a `set_badge` call still sets it until that
  badge changes. Only a badge written in digits counts.
- Where it shows: the app's icon on the Home Screen, once the person allows
  notifications (the system ties the two); the Dock icon on a Mac
  (`NSDockTile.badgeLabel`, which needs no permission); the item in the menu
  bar for an app in the menu bar only.
- Apple asks that the count be the number of unread notifications only.
- A destination's badge in the tab bar and the sidebar is drawn as it is
  written: digits as a count, a word ("New") as that word.

## Notifications the device schedules

The client lists `local_notification` among what it can do, so a server may
offer buttons that call `notify`; they are no longer replaced by their
stand-in on Apple systems.
- `notify` asks the person to allow notifications the first time, then
  schedules: `at` a moment (shown in the device's time zone; at once when it
  has passed), `after` some seconds (at least one), `daily` at an hour and
  minute of the device's zone. A time this version does not know is not
  scheduled, as the protocol says. `thread` groups them; `href` is the page a
  tap opens, as a link from outside does.
- `cancel_notification` takes back a scheduled or shown one by its id.
- None of the three posts a result.
- Signing out clears the count and every scheduled and shown notification.

## Quick ways in

`shortcuts: .on(limit: 4)`:
- **Home Screen quick actions** (iPhone, iPad): the app's first destinations,
  up to the limit (the Home Screen shows four), with their symbols, set
  whenever the window in front's destinations change. Choosing one opens its
  destination, at launch or while the app runs.
- **The Dock menu** (Mac): New Window, then the same destinations, above the
  system's own items.
- With `.off` (default) the app never touches either.
- An app may also declare static quick actions in its Info.plist
  (`UIApplicationShortcutItems`), which work before its first launch: type
  `navree.open` and the address in `UIApplicationShortcutItemUserInfo` under
  `href`. Only addresses of the app's own origin are opened.
- The protocol has no word yet for shortcuts other than the destinations
  (such as "New order"); until it has, the destinations stand in.

## Opening at login (Mac)

`offersOpenAtLogin: true` adds a checkbox "Open at login" to the General
pane. The person alone turns it on, never the app (App Review Guidelines
2.4.5(iii)); the system holds the state (`SMAppService.mainApp`), read each
time the pane shows. When the system waits for the person's approval, the
pane offers "Allow in System Settings…".

## About and Help (Mac; Help also on an iPad's menu bar)

- `about`: credits written as Markdown, links kept, and a copyright line,
  added to the system's own About panel. The app's name, icon and version stay
  the Info.plist's: set `CFBundleShortVersionString` from `MARKETING_VERSION`
  so the panel says the version the app ships. With nothing added, the
  system's own item and panel stand.
- `help`: `.system` (default: the system's item, which opens the app's help
  book or says there is none), `.page("/help")` ("<App> Help", ⌘?, opens that
  page in the window in front, as a link from outside does), `.site(url)`
  (an `https` site in the person's browser), `.hidden` (no item; the menu
  keeps the system's search field).

## What is not offered, and why

- A Dock tile drawn by the app, or an icon changed in the Dock only: the
  badge is the system's, and Finder, Launchpad and the app switcher keep the
  bundle's icon, so a Dock-only icon would disagree with them.
- Any window chrome of the app's own.
- Opening at login on by default.

## What was run

On a Mac (macOS 27.2), the demo pictures and logs itself
(`-NavreePictures <folder> -NavreeSteps …`, steps `presence`, `place:`,
`status:`, `statusmenu`, `statuschoose:`, `badge:`, `dockmenu`, `dockopen:`,
`reopen`, `close`, `panel:`, `openinwindow`, `closewindow`, `typetext:`,
`about:`, `login:`, `statusclick`, `keypress:`, `deactivate`, `panelsize:`):
the activation policy and the type the system lists the app as (`Foreground`
or `UIElement`) in each place and as windows open and close, the item's button
and its menus, the window under the item with the store's orders, a pushed
order and a sheet with words typed in it (pictured as the window server
composes it, alone and with the window it stands on), the app moved into a
full window and back, the Dock tile's count, the Dock menu, the About panel,
opening at login. Against the live store, with a count added to its home
answer: the count beside the item before the window first opened; a click on
the item and a second click, a right click and its menu, Escape, the app sent
to the background, pinned, resized and opened at launch; the refund, restock
and filter sheets inside the window under the item with their words, Return
submitting and Escape cancelling; ⌘N and File > Open in Window with the full
window open; the right-click menu's account items; Help opening a site. On an
iPhone simulator, a UI test chose a quick action on the Home Screen and found
its destination open, and the Home Screen showed the count a server set. Also run:
Escape with words typed in a sheet's field (one press cancels), a one-field
sheet fitting its content, a sheet carried into the full window, the full
window's toolbar and title right after Open in Window, the full window dragged
below its smallest size (it holds), and the window under the item keeping its
size for a screen with a size of its own. What only a person
at the screen sees: the Dock icon at launch, the Dock's own drawing of the
count and the menu, the item in the real menu bar, and the login item after
logging out and in. Started but not finished: signing in from the window under
the item; the core asked for the system's session and the window opened for
it, and the system's own prompt then waits for a person to answer it.
