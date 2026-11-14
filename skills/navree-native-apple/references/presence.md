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
- `content: .screen("/today")`, a small window showing one destination, is
  **not built yet**: it needs one session shared by the app's cores. Such an
  item stands nowhere and the app stays in the Dock.

The item is the system's own `NSStatusItem`, made only while the app stands
in the menu bar. An app that never asks for one has no item at all, not even
a hidden one.

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
`reopen`, `close`, `about:`, `login:`): the activation policy and the type the
system lists the app as (`Foreground` or `UIElement`) in each place and as
windows open and close, the item's button and its menu, the Dock tile's
count, the Dock menu, the About panel, opening at login. What only a person
at the screen sees: the Dock icon at launch, the Dock's own drawing of the
count and the menu, the item in the real menu bar, and the login item after
logging out and in.
