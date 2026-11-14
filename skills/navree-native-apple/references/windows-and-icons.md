# Windows, the toolbar and the app's icon

What an Apple app built on the client sets about its windows, their toolbar
and its icon, and how. All of it is the app's, at build time: a page says
nothing about it. The window's frame, its title bar, its materials and Liquid
Glass stay the system's; an app chooses among the options the system offers
and never draws one of its own.

## Declaring the scenes

The app's `body` is one line:

```swift
@main
struct ShopApp: App {
    #if os(macOS)
    @NSApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
    #else
    @UIApplicationDelegateAdaptor(NavreeAppDelegate.self) private var delegate
    #endif

    var body: some Scene {
        NavreeScenes(configuration: .shop)
    }
}
```

`NavreeScenes` holds the main window group (with the menu bar's commands),
and on a Mac the Settings window and the app's item in the menu bar
([`presence.md`](presence.md)). Two optional parts:

- `toolbarStyle:` (Mac), the window toolbar's style, below.
- A closure that wraps each window's shell, for what the app adds around it:
  `NavreeScenes(configuration: .shop) { $0.environment(\.myValue, 1) }`.

An app that needs scenes of its own (a document window, a utility window)
declares the parts beside them: `NavreeMainWindow(configuration:)`,
`SettingsShell(configuration:)` in a `Settings` scene, and
`NavreeStatusItem(configuration:)`.

## The window (Mac)

`ShellConfiguration.windows` is a `WindowLook`:

| Field | Default | What it does |
|---|---|---|
| `idealSize` | nil: the system's | The size a new main window opens at. A window the person sized before, and one the system restores, keeps its own size. |
| `minimumSize` | nil: the content's own | The smallest the content under the toolbar may become; the system keeps the window to it when the person resizes. |
| `toolbarLabels` | `.automatic`: the system's | Whether toolbar items show their symbols (`.iconOnly`), their words (`.titleOnly`) or both (`.titleAndIcon`) as each window's toolbar first shows. |
| `tabs` | `true` | Whether main windows may be merged as tabs. When they may, the person's own setting (System Settings > Desktop & Dock) decides; `false` takes the window tabs away for the whole app. |

```swift
extension ShellConfiguration {
    static let shop = ShellConfiguration(
        origin: "https://shop.example.in", appVersion: "1.0",
        windows: WindowLook(
            idealSize: CGSize(width: 1180, height: 760),
            minimumSize: CGSize(width: 760, height: 480),
            toolbarLabels: .iconOnly
        )
    )
}
```

The person may still change the labels from the toolbar's own menu
(Control-click the toolbar) for as long as the window is open. Where an item
cannot stand as words alone (a search field, a group of buttons), AppKit
keeps showing its symbol, so `.titleOnly` may show symbols; prefer
`.titleAndIcon` when words matter. With labels shown, a group of buttons
carries one label.

Nothing of the window is drawn by the app: the sizes are the system's own
(`defaultWindowPlacement` for the first size, the content's smallest size for
the minimum). Keep the minimum small enough for the sidebar, a list and a
detail of a few lines: a Mac window is never compact.

### The toolbar's style

The style is a type, chosen in code, so it is a parameter of the scenes, not
a field:

| `toolbarStyle:` | The window |
|---|---|
| none, or `.automatic` | As the system chooses: the title and the toolbar in one row. |
| `.unified` | The title and the toolbar in one row. |
| `.unifiedCompact` | The same row, shorter. |
| `.expanded` | The title above the toolbar, as Mail's message window. |
| `.unified(showsTitle: false)` or `.unifiedCompact(showsTitle: false)` | The toolbar alone. Only where the window's content names what it shows. |

```swift
var body: some Scene {
    NavreeScenes(configuration: .shop, toolbarStyle: .unifiedCompact)
}
```

### Never offered

These would take the window away from the system, so the client has no
setting for them, and an app should not add them around it:

- `.windowStyle(.hiddenTitleBar)` and `.windowStyle(.plain)`: every window
  carries a toolbar with its screen's title and actions.
- A painted window or bar background (`containerBackground(_:for: .window)`,
  `toolbarBackgroundVisibility`): bars are never painted under Liquid Glass;
  the brand belongs in the content's accent.
- Changing the Dock icon while the app runs (`applicationIconImage`): Finder
  and the app switcher keep the bundle's icon, so the app would show two
  icons. A drawn Dock tile: the badge is the system's.
- Alternate icons on a Mac: the system has none for Mac apps.

## The app's icon

The icon is the app's build, the only way on Apple's systems. The library
ships none.

1. Make the icon in Icon Composer (Xcode > Open Developer Tool > Icon
   Composer): a background fill and up to four groups of layers, each layer
   an SVG or PNG on a 1024-point canvas. Check the default, dark, clear and
   tinted looks in its preview: the system draws the clear and tinted ones
   from the layers, and the dark one from the dark fills the file gives.
2. Save it as `AppIcon.icon` (a folder: `icon.json` and `Assets/`) in the app
   target, and set the build setting `ASSETCATALOG_COMPILER_APPICON_NAME` to
   `AppIcon`. It replaces any `AppIcon` set in an asset catalog; Xcode makes
   the images older systems need at build time.
3. Give the bundle its version from the project: `CFBundleShortVersionString`
   `$(MARKETING_VERSION)` and `CFBundleVersion` `$(CURRENT_PROJECT_VERSION)`,
   which the Mac's About window shows beside the icon.

With XcodeGen, list the file on its own so the folder stays one file:

```yaml
sources:
  - path: Shop
    excludes: ["*.icon"]
  - path: Shop/AppIcon.icon
    type: file
    buildPhase: resources
settings:
  base:
    ASSETCATALOG_COMPILER_APPICON_NAME: AppIcon
```

### Icons the person may choose (iPhone, iPad)

On iPhone and iPad a person may choose another icon the app was built with;
the app never changes its icon by itself.

1. Add each alternate as an Icon Composer file of its own (`AppIconIndigo.icon`),
   with its own dark fills, and name it in the build setting
   `ASSETCATALOG_COMPILER_ALTERNATE_APPICON_NAMES` (space between names). Xcode
   writes `CFBundleAlternateIcons` into the Info.plist.
2. Put a picture of each icon, the primary one included, in the app's asset
   catalog, with a dark variant: the system draws no icon but the current one.
3. Name them in `ShellConfiguration.icons`, the primary icon (`name: nil`)
   first:

```swift
icons: [
    AppIcon(name: nil, label: "Saffron", preview: "AppIconPreview"),
    AppIcon(name: "AppIconIndigo", label: "Indigo", preview: "AppIconIndigoPreview"),
]
```

The account menu then holds "App Icon", which opens the icons as a list in a
sheet, the current one checked. Choosing one changes it at once, and the
system says so in its own alert. The row shows only where there is a choice:
the primary icon and at least one alternate the Info.plist declares, on a
system that can change icons. An alternate the build does not declare is left
out. A Mac has no alternate icons, so `icons` is not read there.

## The demo

`packages/apple/Demo` shows each of these. Its icon is `AppIcon.icon`, a
charkha on a saffron ground, with an indigo alternate on iPhone and iPad. On
a Mac its launch arguments set the window:

| Argument | Sets |
|---|---|
| `-NavreeWindowSize 1180x760` | `windows.idealSize` |
| `-NavreeMinimumSize 760x480` | `windows.minimumSize` |
| `-NavreeToolbarLabels iconOnly\|titleAndIcon\|titleOnly` | `windows.toolbarLabels` |
| `-NavreeTabs NO` | `windows.tabs` |
| `-NavreeToolbarStyle unified\|compact\|expanded\|untitled` | `toolbarStyle:` |
