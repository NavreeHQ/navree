# Sources for the Windows skill

Every page and source below was **checked on 2026-11-18** against Microsoft's
published WinUI 3 / Windows App SDK documentation and the shipping code in
`packages/windows`. Guidance is summarised and linked in `../SKILL.md`, never
copied.

When a new Windows App SDK or .NET major release ships, refresh this list,
update the date and re-check `../SKILL.md` and `availability.md`.

## Toolchain, Windows App SDK and WinUI 3

| Page | Used for | Describes |
|---|---|---|
| https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/ | Windows App SDK overview, unpackaged vs MSIX packaged deployment | Windows App SDK 1.x / 2.x |
| https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/Deployment-architecture | Framework-dependent vs self-contained, unpackaged capability matrix | Windows App SDK |
| https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/desktop-to-uwp-supported-api | Which WinRT and Windows App SDK APIs work without package identity vs require MSIX (`JumpList`, `StartupTask`, `BadgeNotification`) | Windows 10 / 11 |
| https://learn.microsoft.com/en-us/dotnet/core/whats-new/dotnet-10/overview | .NET 10 target framework and `LibraryImport` P/Invoke | .NET 10 |

## Fluent 2 design, layout and controls

| Page | Used for | Describes |
|---|---|---|
| https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/design-principles | Fluent 2 principles on Windows 11 | Windows 11 |
| https://learn.microsoft.com/en-us/windows/apps/design/layout/screen-sizes-and-breakpoints-for-responsive-design | Responsive breakpoints (`640` epx compact/medium, `1008` epx medium/expanded) | WinUI 3 |
| https://learn.microsoft.com/en-us/windows/apps/design/basics/navigation-basics and `.../controls/navigationview` | `NavigationView` display modes (`LeftMinimal`, `LeftCompact`, `Left`), pane hierarchy and Settings footer item | WinUI 3 |
| https://learn.microsoft.com/en-us/windows/apps/develop/title-bar | WinUI `TitleBar` control, `ExtendsContentIntoTitleBar`, Back button, search box, `RightHeader` and Windows 11 snap layouts | Windows App SDK |
| https://learn.microsoft.com/en-us/windows/apps/design/style/mica | `MicaBackdrop` (`MicaKind.Base`) on base windows only and Windows 10 fallback | Windows 11 |
| https://learn.microsoft.com/en-us/windows/apps/design/style/color and `.../style/typography` | Theme brushes (`TextFillColor*`, `CardBackgroundFillColor*`, `SystemFillColor*`), Contrast Themes and type ramp styles | WinUI 3 |
| https://learn.microsoft.com/en-us/windows/apps/design/style/segoe-fluent-icons-font | `Segoe Fluent Icons` glyph codepoints | Windows 11 |
| https://learn.microsoft.com/en-us/windows/apps/design/controls/dialogs-and-flyouts/dialogs , `.../flyouts`, `.../infobar` | `ContentDialog` sizing and button roles, `Flyout` light-dismiss and `Escape`, `InfoBar` severities | WinUI 3 |
| https://learn.microsoft.com/en-us/windows/apps/design/accessibility/accessibility-overview and `.../custom-automation-peers` | `AutomationProperties`, heading levels, live regions and custom `AutomationPeer`s for Narrator | WinUI 3 |

## Notifications, pickers, WebView2 and data protection

| Page | Used for | Describes |
|---|---|---|
| https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-quickstart | `AppNotificationManager` packaged and unpackaged behaviour (including elevation restriction) | Windows App SDK |
| https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.windows.storage.pickers.fileopenpicker | `Microsoft.Windows.Storage.Pickers.FileOpenPicker` bound to `WindowId` in unpackaged apps | Windows App SDK |
| https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security and `.../reference/winrt/microsoft_web_webview2_core/corewebview2` | `WebView2` `WebResourceRequested` (including service/shared worker contexts), `DownloadStarting`, `NavigationStarting`, `AddScriptToExecuteOnDocumentCreatedAsync`, and absence of a file-picker interception callback | WebView2 |
| https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata | `CryptProtectData` / `CryptUnprotectData` per-Windows-user local encryption | Win32 (`crypt32.dll`) |
| ISO/IEC 14496-12 (ISO Base Media File Format) & Apple QuickTime File Format Specification | `udta` (`©xyz`, `loci`, `gps `, `GPMF`, `djmd`, `dbgi`), QuickTime `meta` (`mdta` `keys` + `ilst`), iTunes `mdir` `ilst`, XMP `uuid` boxes and `stco`/`co64` chunk-offset shifting in `PlaceVideo.cs` | ISO BMFF / QuickTime |
