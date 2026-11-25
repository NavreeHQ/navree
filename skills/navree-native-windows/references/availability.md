# What Windows offers, and what Navree has of it

Read on 2026-11-18 (see `sources.md`). Each row says what Windows and the
Windows App SDK offer and from which version, then marks Navree's state plainly:

- **in Navree**: the client's C# code in `packages/windows` implements it.
- **by the system**: Navree gets it from WinUI 3 and Windows App SDK controls
  with no custom drawing code.
- **not yet**: Windows offers it and Navree does not advertise or wire it yet.

"Identity" states whether Windows requires **MSIX package identity** (`packaged`)
or allows a plain `.exe` (`unpackaged`). `Navree.Presence/Capabilities.cs`
checks `GetCurrentPackageFullName` and `Environment.IsPrivilegedProcess` at
startup so no API requiring package identity is called without it.

## Where the platform stands (2026-11-18)

| Piece | Target in `packages/windows` | Notes |
|---|---|---|
| .NET | `.NET 10` (`10.0.100` in `global.json`, `rollForward: latestFeature`) | Nullable enabled, analyzers at `latest`, warnings as errors |
| Target Windows SDK | `net10.0-windows10.0.26100.0` (Windows 11 24H2 SDK) | Compiles unpackaged (`Navree.Demo`) and MSIX (`Navree.Demo.Packaged`) |
| Minimum OS floor | `10.0.17763.0` (Windows 10 version 1809) | Mica falls back to solid theme ground on Windows 10 automatically |
| Windows App SDK | `2.6.5` (`Directory.Packages.props`) | Ships WinUI 3, `TitleBar`, `FileOpenPicker` with `WindowId`, `AppNotificationManager`, WebView2 |
| Native Rust core DLL | `x86_64-pc-windows-msvc` (`navree_native_core.dll`) | Loaded via `LibraryImport` in `Navree.Core`; ARM64 DLL build is not wired yet |

## Fluent 2 and WinUI 3 shell features

| What | From | Navree |
|---|---|---|
| `MicaBackdrop` (`MicaKind.Base`) on top-level windows | Windows 11 22000 (solid fallback on Windows 10) | in Navree on `ShellWindow`, `ScreenWindow` and `StartFailureWindow`; never on cards |
| WinUI `TitleBar` (`ExtendsContentIntoTitleBar`, Back, pane button, search `AutoSuggestBox`, `RightHeader`, drag regions and snap layout menu on maximize) | Windows App SDK 1.5+ (`2.6.5` here) | in Navree |
| `NavigationView` adaptive modes (`LeftMinimal`, `LeftCompact`, `Left`) | WinUI 3 | in Navree, switched at `640` and `1008` epx (`WindowRules`) |
| Light, dark and Windows Contrast Themes (`SystemFillColor*`, `TextFillColor*`, `CardBackgroundFillColor*`) | Windows 10 / 11 | by the system through `Look.Resource` and `ContrastLooks` |
| Multiple windows of one process (`Ctrl+N` `ScreenWindow`, blanked when a locked cover stands on `ShellWindow`) | WinUI 3 | in Navree (`ScreenPresenter`, `CoverRules`) |
| Full-screen window mode (`AppWindowPresenterKind.FullScreen` on `F11`) | Windows App SDK | in Navree |
| Single-instance activation redirect (`AppInstance.FindOrRegisterForKey`) | Windows App SDK | in Navree (`Activation.cs`) |
| Virtualized multi-column data grid with frozen columns and compact row folding | Custom over WinUI 3 (`Navree.Controls.DataTable`) | in Navree (WinUI 3 ships no built-in data table control) |
| `InfoBar` status notices, `ContentDialog` modals, `Flyout` popovers, `SidePane` sheets | WinUI 3 + `Navree.Shell` | in Navree; `modal.Locked` renders as a full-window opaque cover, never a `ContentDialog` |
| `Segoe Fluent Icons` (Windows 11) / `Segoe MDL2 Assets` fallback | Windows 10 / 11 | in Navree (`SystemGlyphs`) |
| UI Automation peers, heading levels, live announcements and Narrator reading order | WinUI 3 Automation | in Navree (`DataTableAutomationPeer`, `SettingsCardPeer`, `Look.Label`) |

## Device and desktop features

| Feature | Windows API | Identity | Navree |
|---|---|---|---|
| Copy text to clipboard | `Windows.ApplicationModel.DataTransfer.Clipboard` | unpackaged | in Navree (`clipboard` in `caps`) |
| Open external URL | `Windows.System.Launcher.LaunchUriAsync` (`http`/`https` only) | unpackaged | in Navree (`open_url` in `caps`) |
| Open app settings | In-app `SettingsPage` + `ms-settings:` URIs | unpackaged | in Navree (`open_settings` in `caps`) |
| System share sheet | `IDataTransferManagerInterop.GetForWindow` (`HWND`) | unpackaged | in Navree (`share` in `caps`) |
| Local notifications & scheduled reminders | `AppNotificationManager` (`Microsoft.Windows.AppNotifications`) | unpackaged (disabled when process runs elevated as Administrator) | in Navree (`local_notification` in `caps`), with unnamed-session guard and `ReminderSilence` (15 min) shell check |
| Taskbar count | `BadgeNotificationManager` when packaged; `ITaskbarList3.SetOverlayIcon` when unpackaged | both | in Navree (`Badge.cs`) |
| Notification-area (tray) icon & flyout window | `Shell_NotifyIconW` + `TrayWindow` | unpackaged | in Navree (`Tray.cs`, `TrayWindow.cs`) |
| Open at Windows sign-in | `StartupTask` when packaged; `HKCU\...\Run` when unpackaged | both | in Navree (`Startup.cs`) |
| Taskbar jump list | `Windows.UI.StartScreen.JumpList` | packaged only | in Navree when packaged; cleanly reported unavailable when unpackaged |
| File picking & drag-drop upload with photo/video location stripping | `Microsoft.Windows.Storage.Pickers.FileOpenPicker` (`WindowId`) + `DragOver`/`Drop` + `PlaceRules.TakeOut` (`PlaceJpeg.cs`, `PlacePng.cs`, `PlaceWebp.cs`, `PlaceTiff.cs`, `PlaceVideo.cs`, `PlaceXmp.cs`) | unpackaged | in Navree on native `FileField`; disabled inside `WebView2` (`WebData.DocumentGuardScript` + `Page.setInterceptFileChooserDialog`) because WebView2 offers no file-picker interception hook |
| Protected local storage for cookies, cache and reminders | `CryptProtectData` / `CryptUnprotectData` (`crypt32.dll`, current Windows user) | unpackaged | in Navree (`Dpapi.cs`, `KeptStore.cs`) |
| Browser sign-in hand-over & custom URI scheme (`ProtocolReturn`) | Default browser + URI activation on the app's own scheme | unpackaged (scheme registered for the user at start) or packaged (`windows.protocol` in the manifest) | in Navree (`BrowserSignIn.cs`, `SignIn.cs`; `auth_session` in `caps` when the app has a scheme), against a server that lists the app under `auth.apps.windows`; the loopback listener (`LoopbackReturn.cs`) is written and never started |
| Windows Hello & passkeys | `KeyCredentialManager` / `webauthn.dll` | unpackaged or packaged | not yet (`WindowsHelloAuthenticator` checks availability and returns `Unsupported`; not in `caps`) |
| Push notifications (WNS) | `PushNotificationManager` (`Microsoft.Windows.PushNotifications`) | Azure / Partner Center app identity | not yet |
| Camera capture, barcode scanning, geolocation fix | `CameraCaptureUI`, `MediaCapture`, `Geolocator` | unpackaged / packaged | not yet (`capture` on a file field opens `FileOpenPicker`) |
| Desktop widgets | Windows Widgets Board (`IWidgetProvider`) | packaged | not built |

## Only checkable on a Windows desktop session

Unit tests cover all pure rules, container stripping, transcript replays and
state machines headlessly, while `Navree.Demo --walk` captures live WinUI 3
renders on a desktop runner. Visual Mica tinting over a user's wallpaper,
narrated speech output in Windows Narrator, `ITaskbarList3` overlay sharpness at
125%–200% DPI, Windows Hello hardware prompts and MSIX jump-list clicks require
an interactive Windows 11 desktop session.
