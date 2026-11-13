# Sources for the Apple skill

Every page below was **read on 2026-11-12**. "Describes" is the OS generation
the page documents; the Human Interface Guidelines are a living document, so
a page's last change date is given where it was shown. The guidelines are
summarised and linked in `../SKILL.md`, never copied.

When a new OS version ships, this list is stale: read the pages again, update
the date and the "describes" column, and re-check `../SKILL.md` and
`availability.md` against them. A skill whose date is older than the newest
shipping OS is a task to refresh, not a source of truth.

## Release status and tooling

| Page | Used for | Describes |
|---|---|---|
| https://developer.apple.com/news/releases/rss/releases.rss | Shipping versions: 27.2.1 (9 Nov 2026), 27.3 betas (3 Nov), Xcode 27.2 (29 Oct) | 27.x |
| https://developer.apple.com/documentation/ios-ipados-release-notes/ios-ipados-27-release-notes | What changed in iOS and iPadOS 27 | iOS 27.0 |
| https://developer.apple.com/documentation/ios-ipados-release-notes/ios-ipados-27_2-release-notes and `27_3` | 27.2 and the 27.3 beta | iOS 27.2, 27.3 |
| https://developer.apple.com/documentation/macos-release-notes/macos-27-release-notes | AppKit and SwiftUI changes | macOS 27.0 |
| https://developer.apple.com/documentation/watchos-release-notes/watchos-27-release-notes , `tvos-release-notes/tvos-27-release-notes`, `visionos-release-notes/visionos-27-release-notes` | The other systems | 27.0 |
| https://developer.apple.com/documentation/xcode-release-notes/xcode-27-release-notes and `xcode-27_2-release-notes` | Swift 6.4, Icon Composer 2 | Xcode 27.x |
| https://developer.apple.com/support/xcode/ | Xcode and OS support matrix | Xcode 27.x |
| https://developer.apple.com/news/upcoming-requirements/ | App Store requirements | November 2026 |
| https://developer.apple.com/design/whats-new/ | Design changes, entries to 6 Nov 2026 | 27 |

## Human Interface Guidelines

All under `https://developer.apple.com/design/human-interface-guidelines/`;
the 27-era state unless noted.

| Page | Used for | Last changed |
|---|---|---|
| `tab-bars` | Top-level navigation, badges, minimising | 8 Jun 2026 |
| `sidebars` | The sidebar, its levels, 27's edge-to-edge sidebars | 8 Jun 2026 |
| `toolbars` | Title, back, groups, prominent action, overflow | 16 Dec 2025 |
| `sheets` | Cancel and Done placement, detents, grabber | 24 Mar 2026 |
| `alerts`, `action-sheets` | When to alert, destructive choices | 2 Feb 2024 |
| `buttons` | Roles, prominence, "don't make a destructive action primary" | 16 Dec 2025 |
| `menus`, `context-menus` | Menu content, hidden icons, destructive last | menus 8 Jun 2026; context 5 Dec 2023 |
| `lists-and-tables` | List and table styles, swipe, info buttons | 21 Jun 2023 |
| `search-fields`, `searching` | Search placement | 8 Jun 2026 |
| `progress-indicators`, `loading`, `feedback`, `modality` | Progress, states, status, modal rules | current |
| `toggles`, `pickers`, `segmented-controls`, `text-fields`, `entering-data` | Controls and forms | current |
| `tab-views`, `split-views`, `disclosure-controls`, `scroll-views` | Structure inside a screen | scroll-views 8 Jun 2026 |
| `layout` | Layout by size class, not by device | 9 Sep 2026 |
| `materials`, `color`, `dark-mode`, `typography` | Liquid Glass, colour, appearance, type | materials 9 Sep 2025; color and typography 16 Dec 2025 |
| `sf-symbols`, `icons`, `app-icons` | Symbols and standard icons, app icons | app-icons 28 Oct 2026 |
| `branding` | Brand in the content layer, the accent used judiciously | 9 Sep 2026 |
| `accessibility`, `charts`, `charting-data` | Accessibility, charts | current |
| `widgets`, `live-activities`, `complications`, `controls`, `notifications`, `app-shortcuts` | Surfaces outside the app | widgets and live-activities 13 Oct 2026 |
| `designing-for-ios`, `designing-for-ipados`, `designing-for-macos`, `designing-for-watchos`, `designing-for-tvos`, `designing-for-visionos` | Per-system notes | macOS 28 Oct 2026 |
| `designing-for-iphone-duo` | The folding iPhone | new, 9 Sep 2026 |
| `the-menu-bar`, `settings`, `windows`, `multitasking`, `pointing-devices`, `gestures`, `always-on`, `web-views`, `writing`, `images` | Mac and cross-system details | pointing-devices and gestures 28 Oct 2026 |

Pages that do not exist (404 on 2026-11-12): navigation bars (folded into
toolbars), refresh content controls (folded into progress indicators),
inspectors, and any toast, banner, snackbar or empty-state page. That is why
the client defines its own mapping for those (a status line, an alert, a
content-unavailable layout).

## Developer documentation

Under `https://developer.apple.com/documentation/`; availability read from
each page's metadata.

| Pages | Used for |
|---|---|
| `technologyoverviews/adopting-liquid-glass`, `technologyoverviews/liquid-glass`, `technologyoverviews/preparing-your-app-for-iphone-duo` | What the system applies, what to adopt |
| `bundleresources/information-property-list/uidesignrequirescompatibility` | The old-design opt-out, ignored from the 27 SDKs |
| `swiftui/applying-liquid-glass-to-custom-views`, `swiftui/glass`, `swiftui/glasseffectcontainer` | The glass APIs the client avoids on content |
| `swiftui/tab`, `swiftui/tabrole`, `swiftui/tabviewstyle/sidebaradaptable`, `swiftui/tabbarminimizebehavior` | Tabs by width |
| `swiftui/toolbaritemplacement` and its members, `swiftui/toolbarspacer`, `swiftui/toolbaroverflowmenu` | Where actions go |
| `swiftui/buttonrole`, `swiftui/buttonrole/close`, `swiftui/buttonrole/confirm` | Button roles |
| `swiftui/navigationstack`, `swiftui/navigationsplitview`, `swiftui/table`, `swiftui/liststyle`, `swiftui/formstyle`, `swiftui/contentunavailableview` | Structure, tables, empty states |
| `swiftui/view/swipeactions(edge:allowsfullswipe:content:)`, `swiftui/view/contextmenu(menuitems:)`, `swiftui/view/refreshable(action:)`, `swiftui/view/searchable(text:placement:prompt:)` | Row and search behaviour |
| `swiftui/presentationdetent`, `swiftui/view/sheet(ispresented:ondismiss:content:)`, `swiftui/view/alert(_:ispresented:actions:message:)`, `swiftui/view/confirmationdialog(_:ispresented:titlevisibility:presenting:actions:)` | Presentation |
| `charts`, `charts/chart` | Swift Charts |
| `authenticationservices/aswebauthenticationsession`, `authenticationservices/asauthorizationplatformpublickeycredentialprovider`, `authenticationservices/asauthorizationappleidprovider` | Sign-in and passkeys |
| `usernotifications/unusernotificationcenter`, `widgetkit`, `activitykit`, `appintents` | Push and surfaces |

## WWDC sessions (transcripts read)

| Session | Used for |
|---|---|
| WWDC26 102 Platforms State of the Union, https://developer.apple.com/videos/play/wwdc2026/102/ | The 27 refinements, resizable iPhone apps, no old-design opt-out |
| WWDC26 269 What's new in SwiftUI, https://developer.apple.com/videos/play/wwdc2026/269/ | Prominent tab, toolbar priority and overflow, pinned trailing |
| WWDC26 251 Communicate your brand identity on iOS, https://developer.apple.com/videos/play/wwdc2026/251/ | The brand in the content layer |
| WWDC26 292 Design intuitive search experiences, https://developer.apple.com/videos/play/wwdc2026/292/ | Search placement |
| WWDC26 278 Modernize your UIKit app, 289 Modernize your AppKit app, 221 Prepare your tvOS apps for Dynamic Type | The other toolkits |
| WWDC25 323 Build a SwiftUI app with the new design, https://developer.apple.com/videos/play/wwdc2025/323/ | The 26 tab bar, sheets, toolbars, search |

## Not confirmed on 2026-11-12

The public release date of iOS 27.1; which Mac models have a touch display;
whether the 26 and 27 releases draw `confirm` and `close` toolbar buttons
automatically; how a `TabSection` shows in a compact tab bar; whether
`.refreshable` shows pull to refresh on the Mac; whether an app-wide tint
recolours switches; the date the App Store will require the 27 SDKs.
