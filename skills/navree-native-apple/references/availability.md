# What Apple systems offer, and what Navree has of it

Read on 2026-11-12 (see `sources.md`). Each row says what the system offers
and from which OS version, then marks Navree's state plainly:

- **in Navree**: the client's code does it. Nothing here has been run on a
  physical device; the client was run in cloud simulators.
- **by the system**: Navree gets it by drawing with the system's own
  components, with no code of its own.
- **not yet**: the system offers it and Navree does not.

"Account" says what the app's developer needs: **free** (no paid account),
**ADP** (the paid Apple Developer Program), **ADP and a form** (ADP plus
Apple's approval of a request).

## Where the systems stand (2026-11-12)

| System | Shipping | Navree |
|---|---|---|
| iOS | 27.2.1 (27.0 on 14 Sep 2026; the 26 line is the one before) | iPhone client built, floor 26 |
| iPadOS | 27.2.1 | iPad client built (tab bar to sidebar), floor 26 |
| macOS | 27.2.1 | package targets 26; not run; no menu bar yet |
| watchOS, tvOS, visionOS | 27.2.1, 27.2, 27.2.1 | not built |
| Xcode | 27.2 (Swift 6.4), needs macOS Tahoe 26.6 on Apple silicon | the cloud build uses Xcode 26.6 and 27.2 |

App Store: apps must be built with the 26 SDKs or later; apps built with the
27 SDKs cannot keep the old design and must use the scene life cycle and a
launch screen.

## The design language, by version

| What | From | Navree |
|---|---|---|
| Liquid Glass on bars, tab bars, sidebars, sheets, menus, alerts, search in a toolbar, controls while touched | 26 | by the system. The client never calls glass on content. |
| Floating tab bar; the search tab apart at the trailing end; a tab bar that can minimise on scroll | 26 | by the system (tab bar); minimising: not yet |
| Toolbar items grouped on shared glass, one prominent tinted item; `ToolbarSpacer` between groups | 26 | in Navree (a fixed spacer sets the account apart) |
| Search in the bottom toolbar on iPhone | 26 | not yet: search is a field under the title |
| Button roles confirm and close for toolbar buttons | 26 | in Navree through the toolbar placements |
| `.glass` and `.glassProminent` button styles for floating controls | 26 | not yet, by design: no floating controls from the server |
| Title-case section headers (no capitals) | 26 | by the system |
| A user setting from clear to tinted glass; more diffusion over busy content | 27 | by the system, with no recompile |
| Sidebars extend to the window edge; sidebar icons regain the accent | 27 | by the system |
| Menu item images hidden by default in the iPadOS and macOS menu bars | 27 | not applicable (no menu bar yet) |
| iPhone apps resize (iPad, Mirroring): layout by size class | 27 | in Navree: placement is by size class |
| Toolbar visibility priority, a declared overflow menu, a pinned trailing slot; bars that slide away on scroll | 27 | not yet (they need Xcode 27's compiler) |
| A prominent tab always visible when the tab bar collapses | 27 | not yet |
| Swipe actions on any view; item-bound alerts and dialogs | 27 (dialogs back-deployed to 15) | swipe actions on rows: in Navree; the rest not yet |
| `Tab`, `TabSection`, `.sidebarAdaptable`, `presentationSizing`, zoom transitions | 18 | `.sidebarAdaptable` in Navree; the rest not yet |
| `ContentUnavailableView`, `inspector`, `sensoryFeedback`, Observation | 17 | `ContentUnavailableView` in Navree; the rest not yet |
| Dynamic Type on tvOS | 27 | not built (tvOS) |
| iPhone Duo (a folding iPhone): bars on a vertical axis, an inner regular-width display | 27.1 | not yet |
| A Mac with a touch display | 27 | not yet |

## Device features (the catalogue's first tier)

Navree has no page-side way to start a device feature yet (`navree-native`,
"What does not exist yet"). The features below are what a client does on its
own or what a server reply may start.

| Feature | API | Needs (OS) | Account | Navree |
|---|---|---|---|---|
| Haptics | `UINotificationFeedbackGenerator`, `.sensoryFeedback` | iOS 10 (17); iPad is mostly silent; macOS 10.11 trackpad; watchOS 2 | free | in Navree: a haptic with each message's tone |
| Copy text | `UIPasteboard` (write only; a paste goes through the system paste control) | iOS 3 | free | in Navree (`Cmd::Copy`) |
| Share | `ShareLink`, `UIActivityViewController` | iOS 16 (6) | free | in Navree for a page shown in the web view; not as a page call |
| Open a URL; the sign-in session | `openURL`, `ASWebAuthenticationSession` with an https callback | iOS 12; macOS 10.15 | free | in Navree: other sites open in Safari; sign-in uses the session |
| Passkeys | `ASAuthorizationPlatformPublicKeyCredentialProvider` | iOS 15; macOS 12; not watchOS | ADP (associated domains) | not yet in the client; the server side is done (`navree-auth`) |
| Confirm one action (a passkey assertion over the action's details) | the same API | iOS 15 | ADP | not yet in the client; the server side is done |
| App lock | `LAContext` | iOS 8 | free (`NSFaceIDUsageDescription`) | not yet |
| Push | APNs: `UNUserNotificationCenter`, `registerForRemoteNotifications` | iOS 10; macOS 10.14; watchOS 3 | ADP | not yet in the client; the server side is done (`notify.push.apns`) |
| Local notifications and the badge | `UNNotificationRequest`, `setBadgeCount` | iOS 10 (badge 16) | free | not yet |
| Photos | `PhotosPicker` (no library permission) | iOS 16 (picker 14) | free | not yet |
| Files | `fileImporter` | iOS 14 | free | in Navree (the file field) |
| Camera | `UIImagePickerController`, AVFoundation | iOS | free (`NSCameraUsageDescription`) | not yet |
| Scan a code | VisionKit `DataScannerViewController` | iOS 16 | free | not yet |
| One location fix | `CLLocationManager.requestLocation` | iOS 9; approximate may be all the person grants | free (`NSLocationWhenInUseUsageDescription`) | not yet |
| Ask for a review | `RequestReviewAction` | iOS 16 | free | not yet |
| Keep the screen on | `isIdleTimerDisabled` | iOS 2 | free | not yet |
| Open this app's settings | `openSettingsURLString` | iOS 8 | free | not yet |
| Sign in with Apple | `ASAuthorizationAppleIDProvider` | iOS 13; watchOS 6 | ADP (capability) | not yet in the client; the server side is done (`navree-social`) |
| Sign in with Google on iPhone | the web flow in `ASWebAuthenticationSession` | iOS 12 | free (a Google Cloud project) | the server side is done; the client uses the sign-in session |

## Surfaces outside the app

None of these is lowered from a page; each is written for its surface.

| Surface | API | From | Account | Navree |
|---|---|---|---|---|
| Widgets (Home and Lock Screen) | WidgetKit | iOS 14 (Lock Screen 16) | ADP (App Group) | not yet |
| Control widgets | WidgetKit | iOS 18 | ADP | not yet |
| Live Activities and the Dynamic Island | ActivityKit | iOS 16.1 | ADP for push | not yet |
| Complications and the Smart Stack | WidgetKit accessory families | watchOS 9 | ADP | not yet |
| App Intents and Shortcuts | App Intents | iOS 16 | free | not yet |
| Spotlight | Core Spotlight | iOS | free | not yet |
| CarPlay | CarPlay | by category and approval | ADP and a form | not yet |

## Not confirmed

These could not be checked on an Apple page and are not relied on: whether the
27 releases draw a confirming toolbar button as prominent glass and a
cancelling one as a close symbol with no code, how a `TabSection` shows in a
compact tab bar, whether `.refreshable` shows pull to refresh on the Mac, and
whether an app-wide tint recolours switches. Check them in a simulator before
depending on them.
