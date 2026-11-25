# Navree Native for Windows

The Windows client of [Navree](https://navree.com) Native, in C# on .NET 10 and WinUI 3: an app for Windows whose screens a Navree server draws, with the system's own controls. Its guide, its tests and a demo app are in [`packages/windows`](https://github.com/NavreeHQ/navree/tree/main/packages/windows) of Navree's repository, where the client is changed.

## The packages

Each release of Navree makes these eight packages at the release's version. An app references `Navree.Shell`, and the others come with it.

| Package | What |
|---|---|
| `Navree.Shell` | the app: the window, the title bar, the navigation, the pages, every effect carried out, the web view held to the app's origins |
| `Navree.Controls` | the client's own controls over WinUI 3: the data table, the charts and the other drawn kinds |
| `Navree.Presence` | the app's place in Windows: the taskbar, notifications, the jump list, the notification area |
| `Navree.Platform` | the connections, the cookies, what is kept on the device and the wipe |
| `Navree.Design` | the design rules as plain functions |
| `Navree.Runtime` | the view state, the events and effects, and the client that drives the core |
| `Navree.Protocol` | the protocol's models |
| `Navree.Core` | the bridge to the runtime core, and the core's library |

## Adding it

In a WinUI 3 app for .NET 10 (`net10.0-windows10.0.26100.0`) that is built for `win-x64`:

```
dotnet add package Navree.Shell
```

The app makes a `ShellOptions` with the server's origin and the app's name, a `ShellHost` from it, and a `ShellWindow` for the host; then it starts the host and activates the window. The demo app in the repository (`demo/Navree.Demo`) does exactly that.

Keep the packages at the minor version of Navree the server runs: what a native client is answered may change in a minor release.

The runtime core is a native library, `navree_native_core.dll`, which comes in `Navree.Core` for 64-bit Windows on x64 (`win-x64`) only. A build for that runtime puts it next to the program, which is the one folder the client looks for it in; an app that starts without it says so in words (`CoreUnavailableException`).

## Licence

MIT or Apache 2.0, at your option, as Navree.
