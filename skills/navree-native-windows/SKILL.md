---
name: navree-native-windows
description: "What the Windows client of Navree Native does with each part of a page on Windows 10 and Windows 11 (.NET 10, Windows App SDK 2.6.5, WinUI 3), as built: NavigationView modes by window width (minimal pane button, compact icon rail and open left pane), Mica title bar with Back, search and the person's account menu, secondary windows (Ctrl+N), where a screen's actions go by role, two-pane list and detail, side panes vs ContentDialogs vs anchored flyouts, full-window opaque locked sign-in covers that blank secondary windows and close covered web views, InfoBar notices, the virtualized DataTable with column roles and frozen columns, Fluent forms and fields with JPEG/PNG/WebP/TIFF and MP4/MOV/3GP location stripping, charts and health kinds, Segoe Fluent Icons, Narrator and UI Automation announcements, the origin-locked WebView2 fallback with file picking and drops turned off, and Windows presence with and without MSIX package identity (taskbar badge or overlay, jump list, startup, notification-area icon, app notifications, reminder silence checks, DPAPI storage); what an author should and should not do; what a Windows app needs from the server; a yes/no screenshot checklist; common mistakes with the fix; and dated sources. Use when building, checking or debugging a Navree app on Windows, when a native screen looks wrong in WinUI 3, or when asked what Fluent 2 or Windows App SDK rules mean for a Navree page. Load `navree-native` and `navree-native-design` first."
---

# Navree Native on Windows

The Windows client (`packages/windows`, C# on .NET 10 and WinUI 3 over Windows
App SDK `2.6.5`: `Navree.Core`, `Navree.Protocol`, `Navree.Runtime`,
`Navree.Design`, `Navree.Platform`, `Navree.Presence`, `Navree.Controls` and
`Navree.Shell`) draws a Navree page with WinUI 3's own controls so Windows
decides the brushes, type ramp, Mica backdrop and contrast themes. Everything
that is not drawing, window placement or talking to Windows (navigation, forms,
sign-in state, caching) belongs to the shared Rust runtime core (`Navree.Core`
over `LibraryImport`). This skill states what the client does with each part,
what that means for a server page, and how to check the result. It summarises
Microsoft's WinUI 3 and Fluent 2 guidance and links the pages
(`references/sources.md`, read on 2026-11-18); it never copies them.

The words an author has are in [`navree-native`](../navree-native/SKILL.md);
the rules that hold on every system are in
[`navree-native-design`](../navree-native-design/SKILL.md).

## What is built, and what is not

| Target | State |
|---|---|
| Unpackaged desktop app (`Navree.Demo`) | Built. Runs directly from its build folder with no registration and no MSIX identity: Mica window, three width classes, secondary windows, tray icon, taskbar overlay count, startup Run entry, app notifications (unless elevated), recorded store demo and `--walk` screenshot passes in light, dark and right to left. |
| Packaged desktop app (`Navree.Demo.Packaged`) | Built as an MSIX package so features that require package identity (the numeric taskbar badge, the taskbar jump list and the packaged startup task) run through Windows' own APIs. |
| Control gallery (`Navree.Gallery`) | Built: renders every protocol node and control state side by side. |
| ARM64 native core DLL, Xbox, HoloLens, desktop widgets | Not built. Managed projects list `win-x86;win-x64;win-arm64`, while the Rust core DLL build script targets `x86_64-pc-windows-msvc`; a missing or mismatched `navree_native_core.dll` opens `StartFailureWindow` naming the file and folder it looked in. |

Every protocol node is drawn natively except two in `Undrawable.Kinds`: `web`
and `input:range`. A page containing either is shown whole in WebView2 inside
the native window chrome, never with a hole in a native screen. A test checks
`NodeRenderer`'s sources against `Undrawable.Kinds` so a kind cannot gain or
lose a drawing without updating the list in the same change.

The target framework is `net10.0-windows10.0.26100.0` (Windows 11 24H2 SDK)
with a floor of `10.0.17763.0` (Windows 10 version 1809). Central package
versions live in `packages/windows/Directory.Packages.props` with locked
restores (`packages.lock.json` and `packages.win-x64.lock.json`).

## What the client does with each part

Placement follows the **window width** in effective pixels (`WindowRules`),
never the machine kind: **compact** up to `640` epx, **medium** below `1008`
epx, and **expanded** from `1008` epx upward. One step of protocol spacing is
`4` epx, prose stops at `ReadingMeasure` (`760` epx), and snap-layout minimum
sizes convert through `WindowRules.Raw` at the monitor's rasterization scale.
Pure layout, table, colour and file rules have unit tests in
`tests/Navree.Design.Tests`, `tests/Navree.Platform.Tests` and
`tests/Navree.Presence.Tests`.

| Your page says | The client draws |
|---|---|
| The shell's destinations (`NavItem`s) | WinUI `NavigationView`: `LeftMinimal` (a pane toggle button) in a compact window (`<= 640` epx), `LeftCompact` (`48` epx icon rail) in a medium window (`< 1008` epx), and `Left` (`320` epx open pane with group headers) in an expanded window (`>= 1008` epx). With zero or one destination the pane hides. Settings sits in the footer (`Ctrl+,`). |
| Title bar and window chrome | `TitleBar` over `MicaBackdrop` (`MicaKind.Base`) extended into the non-client area (`ExtendsContentIntoTitleBar = true`): Back (`Alt+Left` and mouse back button), pane toggle when `LeftMinimal`, the app title and subtitle, a centred `AutoSuggestBox` when the shell offers search, and the signed-in person's `PersonPicture` menu at the trailing end before the caption buttons. `F5` refreshes, `Ctrl+N` opens a secondary `ScreenWindow`, and `F11` toggles full screen. |
| The account menu (`app.user`) | Shown on the right of the title bar only while the core names a signed-in person (`PersonPicture` + `MenuFlyout` with name, email and server menu items). When the app has an authentication scheme and the server's user menu carries no sign-out action, the client appends its own `Sign out` item (`ShellHost.OffersOwnSignOut`). From a `401` or external session end until the shell answers again, `app.user` is `null` and the menu disappears. |
| A screen's title and header actions | A `TitleTextBlockStyle` heading (`AutomationHeadingLevel.Level1`) above the screen; if the body's first heading repeats the exact title, `NodeRenderer` skips the duplicate. Screen actions stand in the header bar: `Role::Primary`, `Role::Create` and `Role::Confirm` take `AccentButtonStyle`; `Prominence::Low` takes `TextBlockButtonStyle`; `Role::Destructive` takes `SystemFillColorCriticalBrush` text and never the accent fill; overflow actions move into a `More` `MenuFlyout`. |
| Two panes (`list` / `table` opening a detail, or `split`) | When the window is wide enough (`>= 860` epx of content: `ListPane` `380` epx + `DetailLeast` `480` epx), a list or table whose rows open a screen stands beside its detail pane with the open row marked active; narrower windows push onto the stack. A `split` node arranges panes side by side above compact width and stacks them in compact windows. |
| A sheet, a dialog, a popover | A non-locked modal (`Models.Modal`) opens as a WinUI `ContentDialog` (`DialogWidth` `520` epx) with primary, secondary and close buttons mapped by role; a destructive question asks first unless `undoable` is set. A sheet (`Models.Sheet`) opens in `SidePane` (`440` epx docked at the trailing edge on wide windows, full-width overlay on narrow ones) and shows an indeterminate progress bar while waiting for its body (`PaneRules.WaitsForItsBody`). A `Popover` opens an anchored `Flyout` whose `Escape` dismisses it and returns focus to its trigger. |
| A locked cover (`modal.Locked`: sign-in and waiting covers) | Rendered as a **full-window opaque cover** over `ShellWindow` (`CoverRules.BlocksWindow`), never as a `520` epx `ContentDialog` with a Close button. While up, `NavigationView`, the title-bar search box, the pane toggle and the account button collapse, `Escape` does not dismiss it, every secondary `ScreenWindow` hides its content behind an opaque blank message (`CoverRules.SecondaryWindowNotice`), and every live `WebPage` is closed so background timers, streams and workers stop immediately. |
| A toast or notice | WinUI `InfoBar` docked at the foot of the window (`Severity` mapped from `Tone`), announced through a polite UI Automation live peer. Good news (`Neutral`, `Info`, `Success`) closes itself after 5 seconds unless it carries an action button; `Warning` and `Danger` stay up until dismissed. |
| A table's records (`DataTable`) | Above compact width, `Navree.Controls.DataTable` virtualizes rows and headers: columns size from content and roles, `secondary` columns fold away first as the window narrows, frozen columns stay pinned with `StickyCoordinator`, arrow keys move cell by cell (`GridNavigation`), sortable headers re-query, and bulk selection raises `DataTableBulkBar`. At compact width (`<= 640` epx), records fold into list items (`RowFolding`) using `title`, `subtitle`, `status` and `trailing` column roles so no horizontal scroll bar is needed on narrow windows. |
| Search, filters, sort on a table | Search sits in the query bar (`AutoSuggestBox` / `TextBox` debounced), single filters in `ComboBox`es or menus, sort on column headers and the query bar, and page-size controls stay out when cursor paging drives the list. |
| Forms and fields | Stacked Fluent controls with visible labels above each input, help and inline error text in `SystemFillColorCriticalBrush` below, and focus moved to the first field in error: `TextBox` (multiline `AcceptsReturn` for textareas), `PasswordBox`, `NumberBox`, `ToggleSwitch`, `CheckBox`, `RadioButtons`, `ComboBox`, `CalendarDatePicker`, `TimePicker`, `ColorPicker` in a flyout (`ColourField`), and `FileField` (`FileOpenPicker` bound to the window's `HWND` plus drag-and-drop). While a submit is pending, the button swaps its icon for a `16` epx `ProgressRing` in place without losing keyboard focus. |
| Photo and video uploads in `FileField` | Content kind is read from magic bytes first (`PlaceKinds`), capped at `ShellOptions.MostFileBytes` (`100` MiB), and streamed by token without exposing local paths to the core. Unless the field sets `keep_location`, photos (JPEG, PNG, WebP, TIFF) and ISO base media / QuickTime videos (MP4, M4V, MOV, 3GP via `PlaceVideo`) are copied into the app's temp folder with location metadata stripped (`©xyz`, `loci`, `gps `, GoPro `GPMF`, DJI `djmd`/`dbgi`, Canon EXIF `uuid`, XMP place packets, QuickTime `mdta` location keys with surviving keys renumbered, iTunes `mdir` items and geotagged cover art, with `stco`/`co64` chunk offsets shifted). Videos with nothing of a place are sent byte for byte; containers or tracks that cannot be cleanly stripped (timed telemetry tracks, `iloc`/`sidx` offsets, WebM/MKV/AVI/WMV, motion photos, raw camera files) are refused with inline field words. |
| Charts and health kinds | Drawn in `Navree.Controls` (`ChartView`, `Bars`, `Heatmaps`, `Rings`, `Gauges`, `DayClock`, `Figure`, `FlagView`) using system theme brushes, contrast-checked palettes (`ContrastLooks`) and `AutomationProperties.Name` summaries; numbers format in the client's culture (`₹1,00,000` in `en-IN`). |
| Icons | Mapped to `Segoe Fluent Icons` codepoints (`SystemGlyphs`) with standard actions always taking Windows' own glyph; custom vector paths render at matching stroke weight, and unknown icon names fall back to a quiet geometric dot rather than raw ligature text. |
| Narrator and UI Automation | Every interactive control, heading (`AutomationHeadingLevel`), settings card (`SettingsCardPeer`), chart and table (`DataTableAutomationPeer`) exposes a clean `AutomationProperties.Name`. Left-to-right marks (`\u200E`) added around phone numbers (`+91 98765 43210`) or currency (`₹1,00,000`) on right-to-left screens and non-breaking spaces used to prevent orphan words are stripped from `AutomationProperties.Name` (`Look.Label`), so Narrator speaks the plain text once. |
| A page in the web view (`WebPage`) | Hosted in `WebView2` inside the native shell, locked to the app's origins (`Origins`) across documents, frames, service workers and shared workers (`CoreWebView2WebResourceRequestedContext`; runtimes too old for worker filtering show an update message instead of the page). External links open in the default browser; downloads are cancelled with a notice; each sign-in gets its own profile folder (`WebProfiles`) wiped on sign-out (`WebWipe`). Because WebView2 provides no callback to inspect or strip a file before script reads it, `<input type="file">`, `showOpenFilePicker` / `showDirectoryPicker` / `showSaveFilePicker`, file drops, file pastes and `file:` navigations are disabled (`WebData.DocumentGuardScript`), and the page explains why and points to native screens for uploads. |

## Fluent 2, Mica and Windows themes

- **Let Windows own the brushes and type ramp.** `Look.Resource` and `Look.Ramp`
  pull `TextFillColorPrimaryBrush`, `CardBackgroundFillColorDefaultBrush`,
  `SystemFillColorCriticalBrush` and `BodyTextBlockStyle` from
  `Application.Current.Resources`. Light, dark and Windows' Contrast Themes
  (Aquatic, Desert, Dusk, Night Sky) work automatically because no control
  paints fixed hex backgrounds.
- **Mica belongs to the base window, never to cards.** `ShellWindow`,
  `ScreenWindow` and `StartFailureWindow` set `SystemBackdrop = new MicaBackdrop()`
  once on the window. Cards and sections sit on `CardBackgroundFillColorDefaultBrush`
  with `1` epx `CardStrokeColorDefaultBrush` borders and `4` epx corners.
- **Keep left-to-right numbers intact in right-to-left layouts.** `FlowDirection`
  flips the entire window for RTL scripts (`--rtl`), while `Look.Directed` keeps
  amounts and phone numbers reading left to right.

## What an author should and should not do

Do:
- Declare button roles (`Role::Primary`, `Role::Create`, `Role::Destructive`,
  `Role::Cancel`, `Role::Confirm`) and column roles (`title`, `subtitle`,
  `status`, `trailing`, `secondary`) in `navree-native` so the header bar and
  `DataTable` adapt across compact, medium and expanded widths.
- Keep destination labels concise (one or two words) with an icon on each so
  both `LeftCompact` (`48` epx rail) and `Left` (`320` epx pane) read cleanly.
- Set `keep_location` on a file field only when the workflow genuinely needs GPS
  coordinates or raw camera / editor files; leave it off everywhere else so
  photos and MP4/MOV videos are cleaned before upload.
- Build upload screens with native forms (`#[derive(Form)]`) rather than relying
  on `native::web_only` pages, since `<input type="file">` is turned off inside
  the Windows web view.

Do not:
- Put a second page title row of buttons in the body when `AppPage::actions`
  can carry them: header actions map cleanly to Fluent accent, standard and
  overflow controls.
- Expect `input:range` to draw natively yet: it is on `Undrawable.Kinds` and
  sends the whole screen to the web view; prefer `NumberBox` or discrete choices
  when the rest of the page is native.
- Assume a desktop window is always wide: a snapped Windows 11 pane can be under
  `640` epx wide, folding `NavigationView` to `LeftMinimal` and `DataTable` rows
  into list items.
- Rely on non-401 redirects to end a session when `app.user` has no `id`: end
  expired sessions with `401` and a sign-in cover (or change `app.user.id`) so
  held reminders, cookies and web profiles clear immediately.

## Presence in Windows, packaging and the server

`Navree.Presence` (`Capabilities.For`) decides at startup what the current run
offers from two runtime facts — `PackageIdentity.Has()` and
`PackageIdentity.Elevated()` — and reports both in Settings > About and in the
protocol's `caps` list (`clipboard`, `open_url`, `open_settings`, `share`, and
`local_notification` when not elevated):

| Capability | Unpackaged (`Navree.Demo`) | Packaged MSIX (`Navree.Demo.Packaged`) |
|---|---|---|
| Notification-area (tray) icon & `TrayWindow` | Offered (`Shell_NotifyIconW`) | Offered |
| Open at Windows sign-in | Current user's `Run` registry key | Package `StartupTask` |
| Count on the taskbar button | Drawn overlay icon (`ITaskbarList3`) | Numeric `BadgeNotification` |
| App notifications & scheduled reminders | Offered unless running elevated as Administrator | Offered unless running elevated |
| Taskbar jump list | Not offered (Settings states it needs package identity) | Offered (`Windows.UI.StartScreen.JumpList`) |
| Kept session & reminder store | Protected with `CryptProtectData` (`DataProtectionScope.CurrentUser`) | Same (`CryptProtectData`) |

Reminders (`Notifications.cs` and `SystemPlace.cs`) obey two extra guards on
Windows:
1. A reminder scheduled while nobody is named (`app.user == null`) is never
   handed to the person who was signed in before, even if a newcomer uses the
   app without ever being named.
2. When a reminder comes due after the client has heard nothing from the server
   for longer than `PlaceOptions.ReminderSilence` (15 minutes by default), the
   client asks the shell endpoint first: it shows the reminder if the same
   person is still named or if the server cannot be reached, and holds it back
   if nobody or another person is named.

### Server pieces and what is not turned on yet

| Need | State on Windows | Server skill |
|---|---|---|
| In-app sign-in forms, locked sign-in covers, session cookies, CSRF and sign-out wipe | Built and active | `navree-auth` |
| Browser sign-in hand-over (`BrowserSignIn.Offered`) and custom URI scheme return (`ProtocolReturn`) | Gated off (`BrowserSignIn.Offered == false`): browser cookies are separate from the app's, and the server does not yet configure a Windows app entry (`auth.apps.windows`) for one-time code + PKCE hand-over. Incoming scheme activations are dropped while off. | `navree-auth` |
| Windows Hello and passkeys (`WindowsHelloAuthenticator`) | Interface stubbed (`KeyCredentialManager.IsSupportedAsync` checked, `ConfirmAsync` returns `Unsupported`); `Passkeys` is not advertised in `caps` yet. | `navree-auth` |
| Push notifications (WNS) | Not built; scheduled local notifications (`local_notification`) work while the app or tray icon runs. | `navree-notification-channels` |

## Screenshot checklist

Run `--walk <folder>` on `Navree.Demo` (or `scripts/take-pictures.ps1` on a
Windows desktop) across compact (`<= 640` epx), medium (`< 1008` epx) and
expanded (`>= 1008` epx) widths, in light, dark, a Windows Contrast Theme, 200%
display scale and `--rtl`. Answer yes or no; a "no" is a defect.

**All widths**
1. The window uses the system `TitleBar` with Mica on the base layer only; cards
   use subtle surface fills and `1` epx strokes, never Mica or acrylic on rows.
2. The screen's title appears once at the top of the page (not repeated as the
   first heading inside the scroll region) and is never the app's own name.
3. At most one header button uses `AccentButtonStyle`; destructive actions use
   critical text, never the accent fill, and ask before an irreversible write.
4. Locked modals (`modal.Locked`) cover the entire `ShellWindow` opaquely with
   `NavigationView`, search and the account button hidden, no Close button, no
   `Escape` dismissal, secondary `ScreenWindow`s blanked and covered web views
   taken down.
5. Non-locked modals open as `520` epx `ContentDialog`s; sheets open in
   `SidePane`; popovers close on `Escape` and restore focus to their trigger.
6. Notices appear as `InfoBar`s at the bottom of the window, never as modal
   boxes for routine confirmations.
7. Every field has a visible label above it; validation errors appear in words
   below the field and focus moves to the first invalid control.
8. While a form submits, its button shows an inline `ProgressRing` without
   dropping keyboard focus.
9. Icons come from `Segoe Fluent Icons` at consistent weight with no raw glyph
   names or clipping; numbers and currency (`₹1,00,000`) stay left to right
   under `--rtl`.
10. Light, dark and Contrast Themes keep at least `4.5:1` text contrast with
    visible keyboard focus rects (`Tab`, `Shift+Tab`, arrow keys in `DataTable`)
    and single-utterance Narrator names.

**Compact (`<= 640` epx)**
1. `NavigationView` collapses to `LeftMinimal` with no clipped title-bar items.
2. `DataTable` folds records into multi-line list items with no horizontal
   scroll bar needed to read titles, statuses or trailing amounts.
3. Split panes stack cleanly into a single column.

**Medium and expanded (`641` epx and wider)**
1. `NavigationView` shows the `48` epx icon rail at medium width and the `320`
   epx open pane with group labels from `1008` epx upward.
2. A list or table that opens a detail shows two panes from `860` epx of content
   width, with the open row highlighted.
3. Prose and forms cap their measure instead of stretching text boxes across a
   `1920` epx monitor.

## Common mistakes

| Mistake | Fix |
|---|---|
| Expecting `<input type="file">` to work inside a `native::web_only` page on Windows | Build the upload form natively (`#[derive(Form)]` with a file field); WebView2 cannot intercept file pickers before script reads the file, so web file inputs and drops are disabled. |
| Using `input:range` on a screen that should stay native | `input:range` is in `Undrawable.Kinds` and sends the whole page to WebView2; use a numeric input or discrete choice field until range is drawn natively. |
| Putting action buttons in a body row instead of `AppPage::actions` | Declare screen `actions` with `Button::role` so Windows places them in the page header and overflow menu. |
| Omitting column roles on a wide `DataTable` | Annotate `title`, `subtitle`, `status`, `trailing` and `secondary` so narrow snapped windows fold columns cleanly instead of clipping cells. |
| Leaving `keep_location` on for ordinary photo or video attachments | Keep `keep_location` off by default so JPEG/PNG/WebP/TIFF and MP4/MOV/3GP uploads strip location metadata automatically. |
| Ending a session with a `200` page when `app.user` has no `id` | Answer expired sessions with `401` and a sign-in cover (or give signed-in users stable `id`s) so held reminders, cookies and web profiles never carry across people. |
| Hard-coding colours, custom title-bar buttons or custom scroll bars | Let WinUI theme resources and `TitleBar` draw chrome so dark mode, Contrast Themes and Windows 11 snap flyouts keep working. |

## Docs

- https://navree.com/docs/ui/native/
- Microsoft's pages are listed in `references/sources.md`.

References: [`references/availability.md`](references/availability.md) (what
Windows and the Windows App SDK offer, and what Navree has of it),
[`references/sources.md`](references/sources.md) (the documentation pages used,
with the date read).
