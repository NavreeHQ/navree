# Navree Native for Windows

The Windows client of Navree Native, written in C# on .NET 10 and WinUI 3. The runtime core (a Rust library with a C interface of five functions) decides everything that is not drawing or talking to the system; this client draws with the system's own controls and carries out what the core asks.

| Folder | What |
|---|---|
| `src/Navree.Core` | the bridge to the core: the five functions through `LibraryImport`, a safe handle, bytes given back with the core's own function |
| `src/Navree.Protocol` | the protocol's models |
| `src/Navree.Runtime` | the view state, events and effects, and the client that drives the core |
| `src/Navree.Design` | the rules as plain functions, with no UI types: what is the app's own address, the window's width classes, the words and icons, the tree walked |
| `src/Navree.Platform` | what the client does with the system that needs no window, as plain .NET: the connections (no redirect followed, only the app's origins), the cookies, what is kept on the device (protected for this person), the wipe |
| `src/Navree.Presence` | the app's place in Windows as plain rules and the few calls of the system that need no window: what a build offers with and without package identity, the count on the taskbar, when a notification is due, the addresses of a jump list and of a sign-in's return, the notification-area icon |
| `src/Navree.Controls` | our own controls over WinUI 3 |
| `src/Navree.Shell` | the app: window, title bar, navigation, pages, the tree drawn with the system's own controls, every effect carried out, the web view held to the app's origins |
| `demo/Navree.Demo` | the demo app, which starts with no package identity and no registration |
| `demo/Navree.Demo.Packaged` | the same demo as an installable package, so the features that need package identity can be tried |
| `demo/Navree.Gallery` | each control in every state |
| `tests/*` | MSTest projects beside each library |

The product depends on the Windows App SDK and on what .NET itself ships. The test projects add MSTest. Every version is set in `Directory.Packages.props`.

## Building

1. Build the core as a DLL and put it where the projects find it:

   ```
   cargo rustc -p navree-native-core --locked --release --target x86_64-pc-windows-msvc --crate-type cdylib
   mkdir packages/windows/native/win-x64
   copy target\x86_64-pc-windows-msvc\release\navree_native_core.dll packages\windows\native\win-x64\
   ```

2. From this folder, with the .NET 10 SDK (`global.json` picks it):

   ```
   dotnet build Navree.Native.sln -c Release
   dotnet test Navree.Native.sln -c Release --no-build
   ```

The DLL is copied next to every program that uses `Navree.Core`. The "Native clients" workflow does both on a Windows runner, and starts the demo from its build folder.

## The demo

`Navree.Demo` runs the client on the store demo's recorded answers (`packages/android/fixtures/store-demo`, found above the program), or on a server with `--server https://...`. `--dark` and `--light` choose the theme, `--rtl` the direction. `--walk <folder>` walks it through its screens at three widths and writes a picture of the window's content and of each page whole, and `walk.txt` saying what was done. `scripts/take-pictures.ps1` runs the demo on a Windows machine with a desktop, pictures its window in each way there is, and walks it in light, dark and right to left.


## What is not drawn

The client tells the core which kinds it has no drawing for (`ShellOptions.Undrawable`, from the one list in `Navree.Design/Undrawable.cs`), and the core shows a page that holds one in the web view, whole, never with a hole: a web region and the range field. A test reads the renderer's sources against the list, so a kind gains a drawing and leaves the list in the same change, and a kind that loses its drawing cannot be forgotten.

## The colour field and the file field

A colour field is a field-shaped button: a swatch, the value in words (`#16a34a`), and for a screen reader its label, the colour's name and the value one character at a time. It opens a flyout with the system's own `ColorPicker` and OK and Cancel; only OK sends the colour, as `#rrggbb`.

A file field is a row: the label, a Choose button, and each file chosen with its name, its size and a button to take it out. Files come from the system's file picker (`Microsoft.Windows.Storage.Pickers.FileOpenPicker`, made with the window's `WindowId`; a run that must not open it gives `ShellHost.FilePicker` its own) or are dropped on the field. Each is checked against the field's `accept` and against the most bytes of the field together (`ShellOptions.MostFileBytes`, 100 MiB; the protocol carries none), and a file that does not fit is refused with words at the field. The core is told a token, the name, the type and the size, never a path; the request's body is written from the file as it is then, streamed, and a file that can no longer be read is left out of it (never sent empty) and shown as gone. What a file is comes from its first bytes, never from its name: a JPEG saved as `.jfif`, with no extension or with another one is a JPEG, and a file with no known extension is sent with the type its bytes say. A photograph in a field that does not keep the place it was taken (`keep_location`) has the GPS tags emptied in a copy kept in the app's own folder: JPEG, PNG, WebP and TIFF (the EXIF block and the XMP packet; an XMP packet that holds any property about a place, by its name in any namespace and any letter case, such as `exif:GPSLatitude`, `drone-dji:GpsLatitude`, `photoshop:City` or `Iptc4xmpCore:Location`, goes whole and is replaced by an empty one of the same length); GIF, BMP and SVG are checked for GPS text; HEIC, HEIF, AVIF, JPEG XL and raw pictures, and any picture that cannot be read to the end, are refused with words. Not done: the place in a video, a place written in a maker's note, and the camera (`capture` opens the file picker, as it does in the Apple client).

## The app's place in Windows

Each feature is offered only where the build can do it, and the client's capability list and the About page say which. With a package: the taskbar badge, the jump list, the startup task. Without: an overlay icon on the taskbar button, the person's own list of programs to run at sign-in. Either way: the icon in the notification area with a small window by it, app notifications (not to an app that runs as an administrator), the share sheet, several windows over the one core, one running app (a second start hands what it was asked to the first), and a sign-in in the system's browser that comes back on the app's own scheme. `--system-walk <folder>` walks these and writes what each answered; `--probe <file>` runs the app for a while so a second start and an address on its scheme can be tried on it; `scripts/take-system-pictures.ps1` does both with and without a package.

## The data table's columns

A column first gets the room its own words need, measured from the rows held (its heading too): never less than its least and never more than its most. A name asks for at most 240, so one very long name does not push the others out. A table whose columns all fit what they hold cuts nothing. When there is room left over it goes to the name column first. When there is too little, columns give way in this order, and only then is a word cut: the secondary columns first (the last one first), then the other columns of words, numbers and states, each in proportion to what it can give, down to its least; the name last, down to its least; then the table scrolls sideways, keeping the frozen columns in place. A width a person gave a column, a column of controls and a picture are not taken back. Rows that join later and hold wider words make a column grow, never narrow. A table too narrow for its least widths is a list.
