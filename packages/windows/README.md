# Navree Native for Windows

The Windows client of Navree Native, written in C# on .NET 10 and WinUI 3. The runtime core (a Rust library with a C interface of five functions) decides everything that is not drawing or talking to the system; this client draws with the system's own controls and carries out what the core asks.

| Folder | What |
|---|---|
| `src/Navree.Core` | the bridge to the core: the five functions through `LibraryImport`, a safe handle, bytes given back with the core's own function |
| `src/Navree.Protocol` | the protocol's models |
| `src/Navree.Runtime` | the view state, events and effects, and the client that drives the core |
| `src/Navree.Design` | the rules as plain functions, with no UI types: what is the app's own address, the window's width classes, the words and icons, the tree walked |
| `src/Navree.Platform` | what the client does with the system that needs no window, as plain .NET: the connections (no redirect followed, only the app's origins), the cookies, what is kept on the device (protected for this person), the wipe |
| `src/Navree.Controls` | our own controls over WinUI 3 |
| `src/Navree.Shell` | the app: window, title bar, navigation, pages, the tree drawn with the system's own controls, every effect carried out, the web view held to the app's origins |
| `demo/Navree.Demo` | the demo app, which starts with no package identity and no registration |
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
