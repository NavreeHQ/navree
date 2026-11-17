# Navree Native for Apple platforms

The Apple client of Navree Native, for iPhone, iPad and Mac (iOS 26, iPadOS 26 and macOS 26 or later): a Swift package of `NavreeProtocol` (the protocol's models, written from its schema), `NavreeCoreBridge` (the runtime core, through its C interface), `NavreeRuntime` (the loop between the core and the app), `NavreeDesign` (the drawing, in SwiftUI) and `NavreeShell` (the app: its windows, the network, the web view, the session). `Demo` is an app that is the library plus its settings, and the template for one.

It is preview code. What it does not do yet is at the end of this page.

An app makes a `ShellConfiguration` with its origin and shows `AppShell`. How each part of the protocol is drawn is in `skills/navree-native-apple/SKILL.md`; what the runtime core decides, and what it asks of a client, is in `crates/navree-native-core/README.md`.

## What the library does by itself

These hold for every app built on the library, with nothing to configure.

### One session for the app

An app has one session at its origin, whatever the number of its windows: one cookie jar and its one Keychain item, one web view store, one place for the pages the cores keep, one for what is kept of each window, and one number, which goes up each time the session ends. Every request, every answer's cookies, every page kept, every snapshot and every picture carries the number it was begun under, and one begun under an older number is refused.

**Each window runs a runtime core of its own, over that one session.** The core's own README speaks of one core for an app. Here a window is a stack of screens with its own place, its own sheets and its own back, which is what a core holds, so a second window is a second core; what the cores share is the person. The consequences are these, and they are the whole difference:

- Every core is signed in as the same person or as nobody. Whichever window's core says the person signed out, the session ends for every window at once, and every other window's core is told (`signed_out`) and starts again as nobody's.
- The cores write the pages they keep into one store. Each core lists what it keeps for itself, so a second window can cost the first some of the pages it would have been shown at once at the next launch, and nothing else. The core of a window beside the main ones (the settings window on a Mac) keeps nothing between launches.
- What is kept of a window for a launch the system may end is written under the window's own name.

Whether an app should run one core for all its windows instead is an open question for the Mac client; this page says what the code does today.

### The end of a session

When a core says the person signed out, the session ends for the app, in this order:

1. The session's number goes up.
2. A mark is written to disk and put there by the system before anything is deleted: a sign-out has begun.
3. The requests and streams under way are dropped, unanswered.
4. The cookies go, from memory and from the Keychain. The web view's store of the session is written down as one that ended, and is never given to a web view again; the next session's store is a new one (`WKWebsiteDataStore(forIdentifier:)`, a new identifier for each session), so nothing a page of the last person stored is there for a page of the next.
5. Everything else kept of the person goes, for every window: the pages, what was kept of each window, the files picked for forms, the pictures in memory, the app's notifications and the count on its icon.
6. Every other window's core is told.
7. The mark is taken away, only when all of the above was done and the core has said the session is forgotten.

**The mark does not wait for the system to remove the store that ended.** Removing a web view store (`WKWebsiteDataStore.remove(forIdentifier:)`) needs every web view that used it gone, which the system decides. So the store is first emptied, which works while a web view still shows a page from it; its identifier is written down in a list of stores that ended, on disk, before the mark goes; no web view is ever made over it again; and its removal is tried a few times as the windows let go, and again at every start until it is gone. The mark says "what could bring the session back, or show the last person's data, may still be here"; a store that is emptied, listed and never handed out is neither.

A core with a push token to take back says the sign-out in two steps: it asks the server to forget the token, says `session_ending` in the same update, and says `forget_session` when that request is answered, which may take seconds. Everything above is done at `session_ending`. Only the requests of the window whose core is signing out are left to finish, the token's removal among them: that request was made before the effect was carried out, so it left with the session's cookie, and the cookies its answer sets are refused like those of any answer of the session that ended. The mark stays until `forget_session`. An app ended in between has already taken everything away, and its next start, finding the mark, does it once more.

A core says `forget_session` once for each time it said `session_ending`: when the token's removal is answered, or in answer to being told that the session ended meanwhile in another window. That word belongs to the sign-out its core began, which was carried out when the core said the session was ending. So it is counted and passed over: nothing is taken away twice, and nothing of whoever signed in since. A core that was told the session ended and asks for something new without that word has said all it will of that sign-out; the next `forget_session` it says is a sign-out of its own.

**A start that finds the mark finishes the sign-out before anything else**: before the Keychain is read, before anything kept is opened. The Keychain item, the list's current store, the pages, what was kept of each window and the picked files go, the notifications too, and the jar of that run takes nothing from the Keychain whether or not the item could be deleted. The mark stays until all of it is gone.

**The mark names the session it ends, and never ends a later one.** The session's folder keeps a count of the sessions that have written to the device, and a mark holds the count of the session it is the sign-out of. While a mark stands, the session after it writes nothing to the device (no cookie in the Keychain, no page, nothing of a window, no web view store) until the count has moved past the mark, and the count moves only once everything of the session that ended is gone. So there are two cases at a start, and no third. A mark that names the session the device still holds: nothing of a later session was written, and finishing the sign-out takes nobody else's session with it. A mark the count has moved past (somebody signed in while the core was still waiting for the token's removal, and the app was ended before the core said `forget_session`): it is an earlier session's, it is removed, and nothing is taken away. When a sign-out could not take everything away (the Keychain refused, a folder could not be deleted), it is tried again when the next session first has something to write; until it succeeds that session is held in memory only.

**A change of person with no sign-out.** A session can run out, and somebody else sign in, with no sign-out on this device; the core then lets go of the person before, has its pages emptied, and tells a client nothing more. So the session remembers who the cores last named, written in the session's folder: the key the core gives for the person (`app.user.key`: an opaque text, the same for one person and another for another account), and a mark made from the name and the address the core names them by (a digest; neither can be read from it). Two are the same person when both have a key and the keys are equal; when either has none, when the marks are equal. When a core names somebody else with no sign-out between, in the same run or at a later start, what was the last person's goes, before anything the new person's update asks for is carried out:

- the session's number moves on, so what was begun for the person before is refused;
- every other window's requests are dropped and its core is told, as at a sign-out: it lets go of the last person's screens and of what they typed, and starts again from the shell, so no window sends what one person typed as another;
- the web view's store is written down as one that ended, and the new person's pages get a new one;
- what was kept of each window, the files picked for forms, the pictures in memory, the app's notifications and the count on its icon go.

The cookies stay: they are the new person's already. Who is written down is replaced only when all of that finished, so an app ended in the middle does it over at its next start. A sign-out forgets who was written down, so whoever signs in after one is compared with nobody; a core that is signing out, and one that names nobody, say nothing of who comes next. The first time a key arrives for a person written down without one (the app was updated to a core that gives keys), the same name and address is the same person: the key is written down and nothing is taken away.

**The limit of that, with a core that gives no key.** The name and the address are then all a core tells a client of who is signed in. So a person who changes their name or their address is taken for another person once (what is kept for them goes, as it would for somebody else), and two people with the same name and the same address, or the same name and no address, are one person to the client. With a key neither happens.

### The push token

A build signed for push asks the system for its token only while somebody is signed in: when a window's core first names a person, each window once, so each window's core is handed the token. A start with nobody signed in asks for nothing. A sign-out gives the token up (`unregisterForRemoteNotifications`) at its first word, and it stays given up until somebody signs in. A start that finishes a sign-out the run before began gives the token up again, since nothing says the server was ever asked to forget it: until the next person signs in and their core registers the token as theirs, no push of the person who left reaches the device.

### The network

- **Addresses.** A request, a stream and a picture go only to the app's origin, whole; a picture also to a host the app names in `pictureHosts`, with no cookie.
- **Cookies.** The jar sends a cookie only where its domain, path and security allow. A cookie that cannot go on the wire as it is (a value with a letter of another script, a control character) is left out of the request rather than failing it. The same holds for the token a write carries in `x-xsrf-token`, which is the `XSRF-TOKEN` cookie decoded: one that does not decode to something a header can hold is left out, the nearest one that does is sent, and with none the write goes without the header for the server to answer.
- **How large an answer may be.** An answer's body is read up to the protocol's limit (8 MiB) and no further: by the length the answer announces, and by the bytes as they arrive. A longer one is answered to the core as a request that failed. A picture is read up to 10 MiB.
- **What the core is told when no answer came.** `offline` only when nothing of the request was sent (no connection, no route, the host not found, the connection refused, data not allowed). A connection lost after the request left, a timeout and everything else are `timeout` or `other`, which the core treats as a write the server may have taken.
- **Headers.** The answer's `x-navree-*` headers and its `cache-control` are passed to the core.
- **Event streams.** A stream is read away from the main actor, and at most 64 KiB of one line is held.

### The web view

A page shown in the web view is always on the app's origin, in the session's own store, with no script bridge to the app, no pop-up and no file access.

- **Links.** The core is told of a link (`web_link`) only when the system says the person activated one (`WKNavigationType.linkActivated`). What a page does by itself, a script that sets the address or a redirect or a form sent, stays on the origin or goes nowhere, and is never told to the core as a tap: a page cannot open the dialler, messages, mail, maps or another site that way. One case is left, and is at the end of this page.
- **What a page loads.** Scripts, styles, fonts, pictures, frames and a page's own requests come only from the origin and from the hosts the app names in `webHosts`. Two layers say it: content rules the web engine applies to every load, and a script at the start of every document, in every frame, that empties a document of another origin. A page that cannot be held to the rules is not loaded.

### What is kept on the device

- **The session's cookies** are one Keychain item in the data protection keychain, readable after the first unlock, on this device only, never synchronised.
- **Pages** the server lets a client keep are files in the app's caches, each named by a digest of its key.
- **What is kept of a window** for a launch the system may end is what the core gives (`snapshot`): where the person was and what they had typed and not sent, never a secret. It is not stored when the core says it does not know whose it is (`person_unknown`), so a person's place is never written over with nothing. What was kept and does not read starts the app plainly.
- **Files picked for a form** are copies in the app's temporary folder until the session ends.

### Files a form sends

**A photo is sent without the place it was taken**, unless its field asks for the place (`keep_location`). The place is taken out as the picked file is copied for the form, so the bytes the app keeps to send never hold it:

- A picture that names no place is copied as it is, byte for byte. So is a file that is not a picture.
- A picture that names one is written again without it: with its pixels untouched where the system can do that for its format, else drawn into a new file of the same format. What was written is read back, and a copy that still names a place is thrown away.
- A picture whose place cannot be taken out (a format the system reads and cannot write, a file that is a picture by its name and does not read as one) is not kept for the form at all. It is never sent as it was.

What goes is what the system calls the picture's GPS data, in its EXIF and in its XMP: where it was taken, how high, which way the camera looked. What a camera's maker writes in a part of its own, and the names of places somebody typed into a picture's caption, are not taken out. A film is sent as it is, with whatever it says of where it was made.

A file whose bytes the app no longer holds (the person signed out in another window, the system emptied the folder) is never sent empty under its name: the request does not go, and the core is told it failed.

### When something goes wrong

- **A runtime core that stopped.** A failure inside the core stops it for good, and every answer it gives from then on says so (`"stopped": true`). The window then sends it nothing more, tells the person the app must be started again, and offers to. Starting again makes a new core in the same window, started plainly; what was kept of the window is not deleted and not given to the new core, but set aside for that one start, since it may be what brought the stop.
- **Notices.** The core's `saved_not_refreshed` is worded as what it is: saved, and the screen could not be brought up to date. A notice this build has no words for is worded by its tone and never tells the person to do it again.

### The clipboard

A copy the server marks as a secret stays on this device and goes after a minute. On iPhone and iPad the system does both. On a Mac the system keeps it on that Mac; there is no expiry of the system's, so the app clears the pasteboard itself after the minute if nothing else was copied since, which holds only while the app runs.

## What an app must do itself

- **Hosts a page loads from.** A page the web view shows loads from another host only when the app names it in `webHosts` (`checkout.pay.example`, `*.cdn.example`; `https` only). A page whose own Content-Security-Policy lets in a payment frame or a captcha names the same host there once more. Never name a host anyone can publish on.
- **Covering the app in the app switcher.** `coversWhenInactive` covers the app's content while its scene is not active, on iPhone and iPad. It is off unless the app turns it on.
- **A Mac build keeps the session only when it is signed with an application identifier** a provisioning profile authorises: the data protection keychain is given to no other Mac app. Without one nothing is kept, the session lasts as long as the app runs, and it is never put in the login keychain instead.
- **Photos and films.** See "Files a form sends": a field that needs the place a photo was taken says so (`keep_location`); a film's place is not taken out.
- **Sharing.** The share sheet is offered on iPhone and iPad. A Mac build does not list `share` among what it can do, so the server draws what it wrote to stand in for it.

## What is not done yet

- The repository's workflow builds the drawing (`NavreeDesign`) for a Mac, and everything else for an iPhone simulator only, where the tests run. The Mac's side of the shell and of the demo (what is behind `#if os(macOS)` in `NavreeShell`, `NavreeRuntime` and `Demo`) is compiled by no job, and no test runs on a Mac.
- A form a web page posts to another site (a hosted payment page) is stopped and goes nowhere.
- A link a page's own script activates (`click()` on a link it made) is told to the core as a link the person activated: the system gives both the same type, and says nothing of whether a touch stood behind it. What a page may load is held to the origin and the hosts the app names, so only a script from one of those can do it.
- With a core that gives no key for the person signed in, people are told apart by name and address (see "A change of person with no sign-out").
- A long list costs more with every page of it, and a list inside a designed screen is not laid out lazily.
- A chosen tab, an open disclosure and the words of a search are held by the drawing, by place, and can outlive the page they were for.
- A picture is made ready to draw on the main thread, though never larger than a screen needs (4,096 pixels along its longer side).
- Picked files are removed only when the session ends, and are read whole into memory when sent.
- A Keychain item that cannot be read yet (before the device's first unlock) is taken for none.
