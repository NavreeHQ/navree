# What Android offers, and what Navree has of it

Read on 2026-11-12 (see `sources.md`). Each row says what the system offers
and from which API level, then marks Navree's state plainly:

- **in Navree**: the client's code does it. Nothing here has been run on a
  physical device; the client was run in a cloud emulator.
- **by the system**: Navree gets it by using Material 3's own components.
- **not yet**: the system offers it and Navree does not.

"Account" says what the developer needs: **free**, **Play** (a Google Play
Console account; a new personal account needs 12 testers for 14 days before
production), **GCP** (a Google Cloud or Firebase project).

## Where Android stands (2026-11-12)

| What | State | Navree |
|---|---|---|
| Android 17 (API 37) | Stable since 16 Jun 2026 | not targeted yet: the client compiles against 36 |
| Android 16 (API 36) | Edge-to-edge cannot be opted out; predictive back on by default | targeted (compile SDK 36) |
| Play's target level | API 36 for new apps and updates since 31 Aug 2026; API 37 about August 2027 (an estimate, not confirmed) | the demo targets 36 |
| Floor | the client's minimum is Android 8.0 (API 26) | passkeys need 28, dynamic colour 31, themed icons 33, per-app language 33 |
| Material 3 | stable 1.4.0 has no Expressive components; the 1.5.0 line has them (1.5.0-rc01 on 2026-11-04) and needs platform 37 | `1.5.0-alpha18`, in one file |
| Toolchain | AGP 9.5.1, Gradle 9.6.0, JDK 17, Kotlin 2.4.21, Compose BOM 2026.06.01 (the newest that compiles on platform 36) | what the client's build uses; `navree native:doctor` checks the JDK, the SDK and platform 36 |

## The design language, by version

| What | From | Navree |
|---|---|---|
| Material 3 Expressive: shape morphing, springs, emphasized type | Material 3 1.5.0 line | in Navree: the theme and motion scheme |
| Navigation bar, rail, expanded rail; the drawer retired | Material 3 | in Navree: all three, by window width |
| Floating action button, extended; the FAB menu | Material 3 (menu: Expressive) | FAB in Navree; the FAB menu not yet |
| Floating toolbar for 3 or more contextual actions | Expressive | in Navree, for a page's `toolbar` |
| Connected button group (segmented buttons are no longer recommended) | Expressive | in Navree |
| Loading indicator, wavy progress | Expressive | the loading indicator in Navree; wavy progress not yet |
| Search bar and app-bar search | Material 3 (app-bar search: Expressive) | in Navree |
| List-detail and supporting-pane layouts | Material 3 Adaptive | the two-pane list and detail in Navree; supporting pane not yet |
| Dynamic colour from the wallpaper | Android 12 (API 31) | in Navree (when the app sets no accent or opts in) |
| Edge-to-edge, enforced | targeting 35 (no opt-out at 36) | in Navree |
| Predictive back | on by default at 36 | in Navree |
| Per-app language | Android 13 (API 33) | not yet |
| Themed (monochrome) app icon | Android 13 | an app's own concern; not yet in the demo |
| Adaptive app quality tiers 3, 2, 1 (large screens, foldable postures, drag and drop, stylus) | updated 2026-10-21 | tier 2 work in Navree (rail, two panes, capped widths); tier 1 not yet |
| Large screens ignore orientation and resizability locks | target 36 (no opt-out at 37) | in Navree: windows resize and nothing is locked |

## Device features (the catalogue's first tier)

Navree has no page-side way to start a device feature yet (`navree-native`,
"What does not exist yet"). The features below are what a client does on its
own or what a server reply may start.

| Feature | API | Needs | Account | Navree |
|---|---|---|---|---|
| Haptics | `View.performHapticFeedback` (`CONFIRM`, `REJECT` from 30) | API 26; no permission for a `View` | free | in Navree |
| Copy text | `ClipboardManager`; secrets marked with `EXTRA_IS_SENSITIVE` (33) | API 26 | free | in Navree, with the sensitive flag |
| Share | `Intent.ACTION_SEND` with a chooser | API 26 | free | in Navree |
| Open a URL; the sign-in session | Custom Tabs; Auth Tab (Chrome 137 or later, falls back to a Custom Tab) with an App Link callback | API 23 (the library) | free | in Navree (Custom Tabs; Auth Tab not used) |
| Passkeys | Credential Manager `createCredential`, `getCredential`; origin `android:apk-key-hash:` | API 28 | free (asset links) | in Navree |
| Confirm one action | a passkey assertion over the action's details | API 28 | free | in Navree |
| Sign in with Google | Credential Manager `GetGoogleIdOption` with the web client id | API 23 (the library) | GCP | in Navree |
| Push | Firebase Cloud Messaging; `POST_NOTIFICATIONS` (33); notification channels (26) | API 26 | GCP | in Navree (module `navree-push-fcm`; no sign-out token deletion yet) |
| TalkBack announcements | `announceForAccessibility`-style live regions | all | free | in Navree |
| App lock | `BiometricPrompt` with `BIOMETRIC_WEAK or DEVICE_CREDENTIAL` | API 23 (the library) | free | not yet |
| Local notifications and the badge | `NotificationManagerCompat`; WorkManager or an exact alarm; dots only, no numeric badge | API 26; `POST_NOTIFICATIONS` (33) | free | not yet |
| Photos | `PickVisualMedia` (no permission; never request `READ_MEDIA_*`) | API 26 (Play services backport below 30) | free | not yet (the file field is not drawn) |
| Files | `OpenDocument` | API 26 | free | not yet |
| Camera | the capture intent into a `FileProvider` URI, or CameraX | API 26 | free | not yet |
| Scan a code | Google code scanner (`GmsBarcodeScanning`) | API 23 | free | not yet |
| One location fix | `FusedLocationProviderClient.getCurrentLocation` | API 26; coarse or fine | free | not yet |
| Ask for a review | Play In-App Review | API 21 | Play | not yet |
| Keep the screen on | `FLAG_KEEP_SCREEN_ON` | API 1 | free | not yet |
| Open this app's settings | `Settings.ACTION_APPLICATION_DETAILS_SETTINGS` | API 26 | free | not yet |
| Sign in with Apple | the server's web flow in a browser tab | n/a | n/a | not offered; the client refuses it as unsupported |

## Surfaces outside the app

None of these is lowered from a page; each is written for its surface.

| Surface | API | From | Navree |
|---|---|---|---|
| App widgets | Jetpack Glance (Android 17 caps widget bitmap memory) | API 23 (the library) | not yet |
| Live Updates | `Notification.ProgressStyle` with a promoted ongoing notification | Android 16 (API 36) | not yet |
| App shortcuts, App Functions | shortcuts, AppFunctions | API 25 onward | not yet |
| Wear OS tiles, complications, watch apps | Wear Compose Material 3, Tiles | Wear OS 7 | not built |
| Android Auto | by category and approval | | not built |

## Not confirmed

The Android 17 public release date was not on a primary page; Play's next
target-level deadline; the mapping of Android 14's contrast setting to
Navree's contrast levels; the exact numbers of the 2025 and 2026 tonal-spot
palettes; runtime behaviour of Material 3 `1.5.0-alpha18` beyond what the
emulator runs showed.
