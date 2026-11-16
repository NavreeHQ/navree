# Navree Native for Windows

The Windows client of Navree Native, written in C# on .NET 10 and WinUI 3. The runtime core (a Rust library with a C interface of five functions) decides everything that is not drawing or talking to the system; this client draws with the system's own controls and carries out what the core asks.

| Folder | What |
|---|---|
| `src/Navree.Core` | the bridge to the core: the five functions through `LibraryImport`, a safe handle, bytes given back with the core's own function |
| `src/Navree.Protocol` | the protocol's models |
| `src/Navree.Runtime` | the view state, events and effects, and the client that drives the core |
| `src/Navree.Design` | the rules as plain functions, with no UI types: what is the app's own address, the window's width classes, the words and icons, the tree walked |
| `src/Navree.Platform` | what the client does with the system that needs no window, as plain .NET: the connections (no redirect followed, only the app's origins), the cookies, what is kept on the device (protected for the Windows user the app runs as: see "What is kept on the device"), the wipe |
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

A build without the DLL stops with an error that says where it was looked for. The DLL is built for x64 only. When the DLL is missing as the app starts, cannot be loaded, or is for another processor, making the `ShellHost` throws `CoreUnavailableException`, whose words name the file and the one folder it was looked for in (the program's own); the demo shows them in a `StartFailureWindow`, which ends the app when the person closes it, and an app does the same.

## The demo

`Navree.Demo` runs the client on the store demo's recorded answers (`packages/android/fixtures/store-demo`, found above the program), or on a server with `--server https://...`. `--dark` and `--light` choose the theme, `--rtl` the direction. `--walk <folder>` walks it through its screens at three widths and writes a picture of the window's content and of each page whole, and `walk.txt` saying what was done. `scripts/take-pictures.ps1` runs the demo on a Windows machine with a desktop, pictures its window in each way there is, and walks it in light, dark and right to left.


## What is not drawn

The client tells the core which kinds it has no drawing for (`ShellOptions.Undrawable`, from the one list in `Navree.Design/Undrawable.cs`), and the core shows a page that holds one in the web view, whole, never with a hole: a web region and the range field. A test reads the renderer's sources against the list, so a kind gains a drawing and leaves the list in the same change, and a kind that loses its drawing cannot be forgotten.

## The web view

A page of the app's own origins can be shown in the system's web view (WebView2). Every request it makes, from the page, its frames and its workers (service workers and shared workers too), is asked of the client first, and one that is not for the app's own origins is refused; a web view runtime too old to ask about the requests of workers shows no page, and says that updating the WebView2 Runtime lets it be shown. A WebSocket is not such a request, so the client cannot refuse one: the server's Content-Security-Policy `connect-src` is what keeps a page's WebSockets to the app's own origins. A download a page starts is refused, and the person is told it was not saved.

A file reaches a page in the web view only through the page's own file input, which opens the system's file picker: nothing dragged from outside is handed to a web page, and a page is never sent to a file of this computer (the file's path is told to nobody). A file a page takes through its own file input is sent by the page as it is: the client does not look into it, so the place a photo was taken is not taken out there. The promise about the place holds for the fields the client draws. An app that takes photos from people has the client draw that form (a file field in a form of the page, which the client now draws), and does not put it in a page that is shown in the web view.

## The colour field and the file field

A colour field is a field-shaped button: a swatch, the value in words (`#16a34a`), and for a screen reader its label, the colour's name and the value one character at a time. It opens a flyout with the system's own `ColorPicker` and OK and Cancel; only OK sends the colour, as `#rrggbb`.

A file field is a row: the label, a Choose button, and each file chosen with its name, its size and a button to take it out. Files come from the system's file picker (`Microsoft.Windows.Storage.Pickers.FileOpenPicker`, made with the window's `WindowId`; a run that must not open it gives `ShellHost.FilePicker` its own) or are dropped on the field. A dropped folder, or a dropped thing with no path on this computer (a file on a phone seen through Explorer, a link from a browser), is refused with words at the field, and so is a picker that fails; neither can end the app. Each is checked against the field's `accept` and against the most bytes of the field together (`ShellOptions.MostFileBytes`, 100 MiB; the protocol carries none), and a file that does not fit is refused with words at the field. The core is told a token, the name, the type and the size, never a path; the request's body is written from the file as it is then, streamed, and a file that can no longer be read is left out of it (never sent empty) and shown as gone. What a file is comes from its first bytes, never from its name: a JPEG saved as `.jfif`, with no extension or with another one is a JPEG, and a file with no known extension is sent with the type its bytes say. A picture in a field that does not keep the place it was taken (`keep_location`) leaves the device only if the client can say where every place in it could be and has emptied each; what it cannot be sure of is refused with words at the field, never sent as it is. The place is taken out of a copy kept in the app's own folder, where it is written and nowhere else, so the picture's own data (a JPEG's scans, a PNG's image chunks, a TIFF's strips and tiles) is never changed. JPEG: every segment up to the end of the picture is read, the ones between the scans of a progressive picture too; the EXIF block's GPS directory, maker's note and GeoTIFF tags are emptied and its thumbnail is cleaned as a picture of its own; Photoshop's resources are read (the IPTC city, sub-location, state and country are emptied); application segments the client does not read (a GoPro's, JUMBF, a maker's own) are emptied, and the colour profile, Adobe's colour transform and the multi-picture index are kept; every picture after the first (the further pictures of a multi-picture file, an Ultra HDR gain map) is cleaned as a picture of its own. A motion photo is refused with words, because its video can hold the place: save it as a still. Anything else after the pictures, and a picture cut short, is refused. TIFF: every directory, the XMP, IPTC and Photoshop tags and the thumbnail; a camera's raw picture is refused, by its extension or its bytes, and so is a file where emptying a place would touch the picture's own data. PNG: the `eXIf` chunk is cleaned; a text chunk holding an XMP packet with a place goes, compressed or not (expanded only up to 8 MiB, past which the file is refused), and so does a profile written as text and any ancillary chunk the client does not know; a file with something after `IEND` is refused. WebP: the `EXIF` and `XMP ` chunks, and an unknown chunk is emptied. An XMP packet anywhere that holds any property about a place, by its name in any namespace and any letter case, such as `exif:GPSLatitude`, `drone-dji:GpsLatitude`, `photoshop:City` or `Iptc4xmpCore:Location`, goes whole (replaced by an empty one of the same length); an extended XMP packet is read whole, its pieces joined in order first. GIF, BMP and SVG are searched for such a property and refused when one is found. HEIC, HEIF, AVIF, JPEG XL, JPEG 2000 and BigTIFF are known by their bytes and refused with words, and so are a Photoshop document (which keeps the EXIF and XMP of the photo it was made from) and a camera's raw picture whose first bytes say what it is (Fujifilm, Sigma, Minolta, Olympus, Panasonic, Canon's CRW and CR2), whatever the file is named; a raw picture that is a plain TIFF by its bytes is known by its name, and read as a TIFF under another name. Not looked into: words a person or a program wrote (a caption, a comment, a title), which are kept; the place in a video; the camera (`capture` opens the file picker, as it does in the Apple client).

## The app's place in Windows

Each feature is offered only where the build can do it, and the client's capability list and the About page say which. With a package: the taskbar badge, the jump list, the startup task. Without: an overlay icon on the taskbar button, the person's own list of programs to run at sign-in. Either way: the icon in the notification area with a small window by it, app notifications (not to an app that runs as an administrator), the share sheet, several windows over the one core, and one running app (a second start hands what it was asked to the first). `--system-walk <folder>` walks these and writes what each answered; `--probe <file>` runs the app for a while so a second start can be tried on it; `scripts/take-system-pictures.ps1` does both with and without a package.

## Signing in

Sign-in through the browser is not available on Windows yet; an app signs people in with its own sign-in page inside the app. That page comes to the client as any page of the app does: drawn by the client, its form sent with the client's own requests, so the session's cookie lands in the client's own cookie store. A sign-in page that holds a part the client does not draw is shown in the web view instead, and a sign-in there signs in the web view, not the app: the web view keeps its own cookies. A server that offers the hand-over to apps names its routes to every client, and on Windows that sign-in does not start, so such an app cannot sign people in on Windows yet.

Until the hand-over is offered for Windows, the client starts no sign-in in the browser, listens on no port for one, registers and claims no scheme of its own, and drops any address that arrives on its scheme, with or without a sign-in open: it is never opened as a page and never taken as a sign-in's return. Whether it is offered is decided in one place, `Navree.Presence/BrowserSignIn.cs`.

## What signing out does

Signing out ends the session on this device for good. From its first moment nothing more is written for the person who is leaving: the cookies go from memory, the requests, streams and timers under way are cancelled, and an answer to a request made before cannot put a cookie back. Then, in this order: the cookie file; what the web view keeps (its pages are emptied, and it asks nothing of the network meanwhile); the core is told and lets go of the person; everything else kept; the copies of chosen photographs; the notifications that wait and those shown, the count on the taskbar button and the jump list; the pictures of the pages. A mark is written to disk before any of it and taken away only when all of it has finished and the core has let go, so an app that is ended in the middle (killed, or a power cut) finishes the sign-out at its next start, before it reads anything it kept. A kept file that another program holds at that start does not stop the app from starting: the app starts signed out, the file is set aside, never read again, and deleted at a later start. Not reached: what the person copied to the clipboard and files they saved.

What the web view keeps is in a folder of the session's own, inside the app's web folder, and that folder is never used for another session: whoever signs in next gets a new one, which the profile of the person who left was never in. The new folder is in force from the first moment of a sign-out, before anything of the old one is emptied, so a web page asked for while the old folder is still being emptied is opened in the new one or not at all. At a sign-out every web view that was made for the session is emptied and closed, in the main window and in a window of its own, and what they kept is cleared through one of them or deleted with the folder; a web view left alive from an earlier session is closed too, and a profile cleared through it counts for nothing. A folder that was cleared and cannot be deleted yet (the browser behind it is still ending) is deleted at a later sign-out or start. A web page shown in a window or a sheet is closed with it. When what the web view kept can be neither cleared nor deleted, the sign-out is not finished: the person is told so in the window, with a button that starts the app again, no web page is shown for the rest of that run, and the next start finishes the sign-out and goes on in a new folder.

## What is kept on the device

What the app keeps between launches (the cookies, the pages the core may keep, the snapshot it starts again from, the widths of a table's columns, the notifications that wait) is in its own folder under the local application data, each value protected with Windows' data protection (`CryptProtectData`) for the Windows user the app runs as. It is kept in two parts. The pages the core may keep are a cache: the core has it emptied after every change a person sends, and emptying it touches nothing else. The cookies, the snapshot, the widths a person gave a table's columns and the notifications that wait are the person's, and stay until the person signs out, which takes both parts. That protection keeps what is kept from other people who sign in to the same computer. It does not keep it from other programs: any program that runs as the same Windows user can read it, and a user with a roaming profile can open it on another computer.

## The data table's columns

A column first gets the room its own words need, measured from the rows held (its heading too): never less than its least and never more than its most. A name asks for at most 240, so one very long name does not push the others out. A table whose columns all fit what they hold cuts nothing. When there is room left over it goes to the name column first. When there is too little, columns give way in this order, and only then is a word cut: the secondary columns first (the last one first), then the other columns of words, numbers and states, each in proportion to what it can give, down to its least; the name last, down to its least; then the table scrolls sideways, keeping the frozen columns in place. A width a person gave a column, a column of controls and a picture are not taken back. Rows that join later and hold wider words make a column grow, never narrow. A table too narrow for its least widths is a list.
