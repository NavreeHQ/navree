# navree-native-core

The runtime core of Navree Native: everything a native client does that is not drawing, placement or talking to the system. Navigation, the tree store, forms, and every conversation with the server are written once, here, and every client runs them: SwiftUI on Apple's systems, Jetpack Compose on Android, and whatever comes after.

The core is a pure function of state and event.

```rust
use navree_native::{Client, FormFactor, Platform};
use navree_native_core::{Config, Core, Event};

let mut core = Core::new(Config::new("https://shop.example", Client::new(Platform::Ios, FormFactor::Phone)));
let update = core.send(Event::Start);   // what changed, and what to do about it
let view = core.view();                 // what a client draws from
```

- `Core::new(Config) -> Core`
- `Core::send(&mut self, Event) -> Update`, where `Update { changed: Vec<Change>, effects: Vec<Effect> }`
- `Core::view(&self) -> &ViewState`, and `Core::part(&Query)` for one part of it as JSON
- `Core::snapshot(&self) -> Snapshot`, what a client stores so the app starts again where the person was

The core never does I/O, reads no clock and rolls no dice: the random bytes a sign-in needs are asked of the client, like everything else. It asks for a request, a timer or a stream with an **effect**, and learns what came of it from a later **event**. So every rule in it is tested with plain values, and four clients cannot drift apart.

A client keeps no state of its own about anything in the view state. Scroll positions, carets and the text of a field while it is being typed are the toolkit's.

## The loop

1. Something happens: the app starts, the person taps, a request is answered, a timer runs out.
2. The client sends it as one event and gets an update.
3. For each entry of `changed`, the client reads that part of the view state again and draws from it.
4. The client carries out each effect, in order. What comes of an effect is a later event.

## The C interface

`include/navree_core.h`, five functions. Everything that crosses is UTF-8 JSON, as a pointer and a length.

| Function | Takes | Gives |
|---|---|---|
| `navree_core_new(config, len)` | a `Config` | a core, or `NULL` when the configuration does not read |
| `navree_core_send(core, event, len)` | an `Event` | an `Update`, or `{"error":"..."}` |
| `navree_core_view(core, query, len)` | a `Query`, or nothing for all of it | that part of the `ViewState`, or `{"error":"..."}` |
| `navree_bytes_free(bytes)` | bytes the core gave | |
| `navree_core_free(core)` | a core | |

With the feature `jni` the crate also exports the four native methods of the Kotlin class `com.navree.core.Core` (`nativeNew`, `nativeSend`, `nativeView`, `nativeFree`), which take and return `ByteArray`s of the same JSON and keep the handle as a `Long`.

**What a client must guarantee.** Events are sent in the order things happened, from one thread at a time: the simplest way is to call only from the thread that draws. A lock inside the handle keeps two calls made at once from running together, so a mistake costs order, never memory. Bytes passed in stay valid for the call. Every answer is given back once. No call panics across the boundary.

**Stack.** A call may be made from a thread with 512 KiB of stack, which is what a secondary thread has on Apple's systems. Reading a tree as deep as the protocol allows takes more than that, so the core reads each answer on a thread of its own with 4 MiB, and waits for it; everything else it does with such a tree fits the caller's thread. `tests/stack.rs` runs all of it on 512 KiB in a debug build. A tree deeper than the protocol allows is refused before it is read.

`src/ffi.rs` is the only file with `unsafe` in it.

## Config

```json
{"origin": "https://shop.example",
 "also": ["https://*.shop.example"],
 "app_id": "com.charkha.supply",
 "client": {"platform": "ios", "os": "26.1", "form": "phone", "app": "1.4.0", "lib": "0.1.0", "vocab": 0,
            "scheme": "light", "contrast": "normal", "text_percent": 100,
            "locale": "en-IN", "tz": "Asia/Kolkata", "caps": ["share", "clipboard", "open_url"]},
 "back_to_first": false,
 "deep_links": "routes",
 "cache": {"pages": 24, "persist": true},
 "undrawable": ["chart", "input:colour"],
 "views": ["shelf-map"],
 "sign_in": {"paths": ["/auth/*/redirect"], "callback": "shop", "native": ["apple"]}}
```

Only `origin` and `client` are required. `origin` must be `https` (`loopback_http: true` allows plain `http` for the machine itself, for development). `also` lists other origins the app treats as its own. `app_id` is the app's own name with its system (the bundle id, the package name): a sign-in is handed over to the app by it and the device's push token is registered for it, and without it neither is started. `client` is the protocol's own description of a client, sent with every request; its `caps` name the device features this build has, by the protocol's names (`share`, `clipboard`, `open_url`, `scan_code`, `location_once`, …). A control that calls a feature the build lacks is drawn as the call's stand-in says, or not at all. `sign_in.native` names the providers this build signs in with through the system's own sheet (`apple` on Apple's systems, `google` on Android); a sign-in with any other goes the web's way. `back_to_first` is Android's back, which leads to the first destination before it leaves the app. `undrawable` names the kinds this build of the client can't draw yet: a page that holds one is shown in the web view.

## Addresses

Every address the core is given, by a page, an answer, a redirect, a link from outside or the web view, is read before anything is done with it, and read as it is: nothing is trimmed and nothing is repaired. What does not read leads nowhere.

- Text that is empty, starts with `#`, has white space at either end, or holds anywhere a control character, a backslash, or a space other than the ASCII space does not read. An ASCII space inside an address is kept.
- A host is ASCII letters, digits, `-` and `.`, or an IPv6 literal in brackets. A name in another script is written in its ASCII form (`xn--…`). The origins in the configuration are held to the same rules.
- An address with a name before its host (`user@host`) does not read.
- A path that starts with `//` once its dot segments (`.`, `..`, `%2e`) are taken out does not read. An empty segment further inside stays, as a browser keeps it.
- A scheme is read by what an address starts with, never by `://` somewhere in it: `/x?a=b://c` is a path and a query on the app's origin.

**Every address a client is given to fetch, to stream from or to show in the web view is a whole address**, `scheme://host[:port]/path[?query]`, built from the parts that were read, on the app's own origin or one it allows (`also`). The text that came in never goes out as it came: the path, the query and a fragment are written as a browser sends them, percent-encoded by the URL standard's path, special-query and fragment sets for `http` and `https` (non-ASCII as its UTF-8, a space as `%20`; `[` and `]` in a query stay). An escape already written stays; any other `%` is written `%25`. The address in the view state (`screens[n].url`) is the form that was read, not encoded. A request whose address is not one of these is never sent: it fails as an answer of 502 does. A stream is never opened, and a page is never put in the web view (the screen fails).

**Pictures.** The core writes the source of every picture (`image.src`, `avatar.src`, a feed row's `actor`, a message's `avatar`) in a tree it takes in, from a page, a region, an op, a list's rows or the shell's user menu, before a client sees it; a relative source in a part of a page (a region, a list's rows) is read against the page, as a browser reads it against the document: a source on one of the app's origins becomes its whole address; a whole `https` address of another host is kept, written again from its parts (a client loads it or not by its own list of picture hosts); a picture written into the page itself (a source that starts with `data:image/`, as a QR code or a small chart is) stays exactly as it came, since it is no address and nothing is fetched for it; anything else (`http` to another host, another `data:` source, a broken address) becomes empty, which a client draws as its placeholder. A client never resolves a picture's source itself.

**A client asks again.** Before it fetches an address, streams from it or loads it in the web view, a client asks "is this ours?" of what **its platform's own parser** reads, because that parser is what connects: the text holds no white space, control character or backslash; the platform reads it as `http` or `https` with no user information; and its scheme, host and port are an origin of the app (`*.name` for one label or more under `name`, at a label's edge; plain `http` only to a loopback host when the build allows it). `vectors/addresses.json` holds the addresses every client must read the same way as the core: `resolve` (how an address found on a page reads, and the whole address that leaves; what turns on the path, such as `//` after dot segments, is decided here, by the core alone) and `ours` (only cases the client's rule above decides). The core's tests run them, and so must each client's.

**What opens outside.** An `open_url` effect holds only a whole `http` or `https` address that reads, written again from its parts (its fragment kept), or a `mailto`, `tel`, `sms` or `geo` one. A newer app (`upgrade`) is offered only at an `https` address; any other is dropped, and `upgrade.url` is empty.

## Events

Every event is an object whose `t` names it. A field shown with `?` may be left out.

| `t` | Fields | When |
|---|---|---|
| `start` | | The app started, or the person asked to try again after it failed to. |
| `response` | `request`, `status?`, `content_type?`, `location?`, `headers?`, `body?`, `url?`, `error?`, `left?` | A `fetch` was answered. `headers` holds the answer's headers whose names start with `x-navree-`, and its `cache-control`, each as `[name, value]`; a page is kept only when its `cache-control`, when passed, does not say `no-store` (`transcripts/keep-cache-control.json`). A redirect is handed over as it came, with its `status` and `location`: the core checks where it leads and asks again. `error` (`offline`, `timeout`, `other`) when no answer came. A transport that follows redirects itself gives the last address as `url`, and stops with `left` at one that leaves the app's origins. |
| `select` | `destination` | The person chose a top-level destination. |
| `act` | `screen?`, `action`, `form?`, `overlay?`, `text?`, `now?`, `ask?`, `from?` | The person tapped something that has an action. `action` is the node's own. `form` and `overlay` are the ids of the form and of the sheet, dialog or popover the control stands in. `now` is the time in seconds since 1970, given when the action is a device call: a call whose ticket has run out is not started. `ask` is the question the control asks first, as the node holds it: the core then holds the action and puts the question in the screen's `question`. `from` is the id of the part tapped, when it has one: a screen the action opens keeps it (see the view state). |
| `answer` | `screen`, `question`, `yes` | The person answered the screen's question. Yes runs what the control was about to do. |
| `field` | `screen`, `form?`, `field`, `value` | The person changed a field. `value` is `{"t":"text","text":"…"}`, `{"t":"flag","on":true}`, `{"t":"many","values":[…]}`, `{"t":"files","files":[{"token","name","type","size"}]}`, `{"t":"range","from":"…","to":"…"}` for the two ends of a range, `{"t":"phone","text":"…","country":"IN"}` for a phone number with the country the person picked, or `{"t":"none"}`. A field whose node says what happens as it changes (`changed`) needs nothing more from the client: the core sends the form or runs the commands. |
| `options` | `screen`, `form?`, `field`, `text?`, `more?` | The person typed in a choice whose options are searched on the server; or, with `more`, came near the end of the options it shows. |
| `choose` | `screen`, `table`, `row?`, `on`, `across?` | The person ticked or cleared a row of a table whose rows can be chosen; every row it holds when `row` is left out. With `across`, every record that matches the table's query, which a table offers with `select.across`. |
| `tab` | `screen`, `id`, `index` | The person chose another tab of a set of tabs. |
| `random` | `request`, `bytes` | The bytes a `random` effect asked for, as hex. |
| `push_token` | `token`, `environment?` | The system gave the device a push token, or changed it: sent at every launch and at every change. |
| `restore` | `snapshot` | The app starts again after the system ended it, from the snapshot stored before. Sent in place of `start`. |
| `move` | `screen`, `item`, `from`, `to`, `before?` | The person dragged the row `item` from the list `from` into the list `to`, before the row `before` or at the end. |
| `search` | `screen`, `text` | The person searched with the screen's own search (`screen.search`). |
| `back` | | The system's back. |
| `pop_to` | `screen` | The person went back to a screen further down the stack. |
| `dismissed` | `screen` | The person took a sheet or cover away. |
| `overlay` | `screen`, `id`, `open` | The person opened or closed a sheet, dialog, popover or disclosure of the tree themselves; or, with a table row's id, what is under that row. |
| `refresh` | `screen` | Pull to refresh, or "try again". |
| `region_retry` | `screen`, `region` | The person asked for a region that failed again. |
| `more` | `screen`, `id`, `before?` | The end of a list or table is near; with `before`, the start of a list that has rows before its first. |
| `region_go` | `screen`, `region`, `href` | A table goes to another address of itself: a sort, another page. |
| `message_shown`, `message_action` | `id` | A message went away; its button was tapped. |
| `web_link` | `screen`, `url` | A link was tapped in the web view. |
| `open_link` | `url` | The app was opened with an address. |
| `notification` | `href` | A notification was tapped. |
| `window` | `width` | The window's width changed class: `compact`, `medium`, `expanded`. |
| `environment` | `scheme?`, `contrast?`, `text_percent?`, `locale?`, `tz?` | The person's settings changed. |
| `foreground`, `background` | | The app came to the front, or left it. |
| `signed_out` | | The client ended the session itself. |
| `timer` | `timer` | A timer the core asked for ran out. |
| `stream` | `stream`, `state`, `name?`, `status?` | A stream opened (`open`), spoke (`event`, with the event's `name`) or ended (`closed`). |
| `cache` | `key`, `value?` | A `cache_read` finished. |
| `device_result` | `call`, `ok`, `refusal?`, `fields?` | A device call that posts its result finished. `fields` is the result as a form's fields; `refusal` says why there is none (`denied`, `cancelled`, `unsupported`; `failed` when left out). |
| `sign_in_done` | `url?` | The system's sign-in session ended, with the address it came back to. |
| `focused` | `screen`, `serial` | A focus the view state asked for was given. |
| `open_upgrade` | | The person agreed to get the newer app. |

```json
{"t":"act","screen":3,"action":{"t":"submit"},"form":"refund","overlay":"refund"}
{"t":"response","request":5,"status":422,"content_type":"application/vnd.navree.native+json","body":"{…}"}
```

## Updates

```json
{"changed":[{"t":"nav"},{"t":"screen","id":3}],
 "effects":[{"t":"fetch","request":4,"method":"get","url":"https://shop.example/orders/1",
             "headers":[["x-navree-native","1"],["x-navree-client","platform=ios; form=phone; vocab=0"],
                        ["accept","application/vnd.navree.native+json"]],
             "timeout_ms":30000}]}
```

`changed` names parts of the view state. Each is read with the query of the same shape (`"t"` becomes `"part"`).

| Change | The part |
|---|---|
| `app` | The app's name, theme and user. |
| `nav` | Where the person is: `stage`, `failure`, `upgrade`, `destinations` with their stacks, `selected`, `modals`, `front`, `can_go_back`, `window`. |
| `screen` (`id`) | Everything about one screen. `null` when no stack holds it any more. |
| `form` (`screen`, `form`) | The state of one form, and nothing else of the screen. |
| `rows` (`screen`, `id`, `from`, `count`) | `count` rows were added at the end of a list or table, starting at `from`, and nothing else of the screen changed. The part holds those rows, the state of the forms in them, and what the list says of itself: `more`, `loading` and `failed`; for a list also `before`, `loading_before` and `failed_before`; for a table whose rows can be chosen also `selected`, the ids of every row chosen. A `count` of 0 says only that this state changed: a tick, or a fetch that began or ended. A client that does not read rows by themselves reads the screen. |
| `redraw` (`screen`, `id`, `rows`, `gone`) | Some rows of a list or table changed, and nothing else of the screen but what the list says of itself. Each id in `rows` is a row that is new, has new content or stands in a new place; the ids in `gone` left. The part gives each named row as `{"id", "before", "node"}` (a table's as `"row"`), in the list's order, where `before` is the id of the row it stands before now, or `null` at the end. A client takes out the rows that left and the rows named, then puts the named ones in from the last to the first, each before its `before`. A client that does not redraw rows by themselves reads the screen. |
| `messages` | The messages waiting to be shown. |

One more part is read without a change to say so: `{"part":"snapshot"}`, below.

## Effects

| `t` | Fields | What the client does |
|---|---|---|
| `fetch` | `request`, `method`, `url`, `headers`, `body?`, `timeout_ms`, `xsrf?` | Sends the request with its cookies, follows no redirect, and answers with a `response` event carrying `request`. `method` is `get`, `post` or `delete`. `body` is `{"t":"form","text":"a=1&b=2"}` (send `text` as it is), `{"t":"multipart","parts":[…]}` (write the parts, putting in the bytes kept under each `token`) or `{"t":"json","text":"{…}"}` (send `text` as it is, which may be empty: the framework's own routes speak JSON, and the answer's body is handed back as any other). With `xsrf`, the `XSRF-TOKEN` cookie, decoded, also goes in `x-xsrf-token`. A request for one region or for a list's next rows carries `x-navree-region` with the region's id, percent-encoded: each byte of its UTF-8 outside `!` to `~`, and `%`, is written as `%` and two hex digits, so a plain ASCII id goes as it is. `url` is a whole address on one of the app's origins (see "Addresses"). |
| `cancel` | `request` | May drop the request: its answer is no longer wanted. |
| `open_url` | `url`, `inside?` | Opens the address outside the app; with `inside`, in the system's browser inside the app. Only a whole `http` or `https` address, or a `mailto`, `tel`, `sms` or `geo` one. |
| `sign_in` | `url`, `callback?`, `ephemeral?` | Opens the system's sign-in session, and answers with `sign_in_done`. With `ephemeral`, the session shares nothing with the system's browser. |
| `message` | `id`, `tone`, `text?`, `title?`, `notice?`, `action?` | Shows a short message, and answers with `message_shown` when it has gone. `notice` is a message of the core's own, for the client to word. |
| `haptic` | `kind` | Plays a haptic. |
| `device` | `call`, `device`, `posts?`, `options?` | Runs a device call: one a tap started, or one of the few a server's reply may start. `device` is the call as the protocol writes it (`{"f":"share","title":"…"}`). With `posts`, answers with `device_result`, and the core sends the result where it goes. `options` is what the server gave for the call to start with, which the core asked for: the passkey request for `confirm`, the nonce for `platform_sign_in`. The client asks the server for nothing itself. |
| `random` | `request`, `bytes` | Hands the core that many bytes from the system's secure random generator, with a `random` event. They are a secret: never logged, never used for anything else. |
| `forget_session` | | Forgets the session: every cookie of the app's origins, in the web view's store too. |
| `cache_read` | `key` | Reads what is kept under the key, and answers with `cache`. |
| `cache_write` | `key`, `value?` | Keeps the value under the key; without a value, takes the key away. |
| `cache_clear` | | Throws away everything kept. |
| `announce` | `text?`, `notice?`, `count?` | Says it to assistive technology. |
| `copy` | `text`, `sensitive?` | Puts the text on the clipboard. With `sensitive`, as a secret the system should not show or sync. |
| `timer`, `cancel_timer` | `timer`, `ms` | Answers with a `timer` event after that long; or no longer. |
| `stream_open`, `stream_close` | `stream`, `url`, `headers` | Opens a stream of server-sent events at `url`, a whole address on one of the app's origins, and answers with `stream` events; or closes it. Only an event's name is passed on. The client does not reconnect by itself: the core asks. |
| `exit` | | There is nothing to go back to: the system takes the back. |
| `report` | `what`, `detail` | Something the app's developers will want to know. `what` is `address` when an address an answer gave (a link, a redirect, a form's target, a region's or a list's source, a shell entry) does not read and was not followed: `detail` names the kind and the address's scheme and host (or `a path`), never its path, once for an event. |

## The view state

```json
{"stage": "ready",
 "app": {"name": "Charkha Supply Co.", "home": "/", "user": {…}},
 "destinations": [{"label": "Orders", "href": "/orders", "badge": "12", "items": [], "stack": [2, 3]}],
 "selected": 0,
 "modals": [{"present": "sheet", "locked": false, "stack": [4]}],
 "front": 4,
 "can_go_back": true,
 "window": {"width": "compact"},
 "screens": {"3": {
     "id": 3, "url": "/orders/1",
     "content": {"t": "native", "screen": {…}},
     "loading": false, "refreshing": false, "cached": false, "status": 200, "chrome": true,
     "overlays": ["refund"],
     "states": {"row-1": ["selected"]},
     "forms": {"refund": {"pending": false, "fields": {
         "refund-amount": {"name": "amount", "value": {"t": "text", "text": "10"}, "errors": []}}}},
     "regions": {"figures": {"loading": false}},
     "lists": {"feed": {"loading": true, "failed": false}}}},
 "messages": [{"id": 1, "tone": "success", "text": "Refunded ₹10.00"}]}
```

- `stage`: `starting`, `ready`, `failed` (with `failure`) or `upgrade` (with `upgrade`). An `upgrade` whose `url` is empty offers nothing to open.
- A screen's `content` is `{"t":"blank"}`, `{"t":"native","screen":…}` (a screen of the protocol) or `{"t":"web","url","title?","why","serial"}` (load the address in the web view, a whole address on one of the app's origins; load it again when `serial` moves on).
- **The tree is ready to draw.** What is open, hidden, disabled or waiting is already in it: a sheet's `open`, a stack's `hidden`, a button's `pending`. A state that has no field in the tree is in `states`, by the node's id.
- **Every form and every field has an id that is its own on its screen.** A form that came with none goes by `@` and its address. `forms` holds each form's state by that id, and each field's by the field's id: what it holds now, and what is wrong with it. A client draws a field's value and messages from here, never from the node.
- `overlays` lists the sheets, dialogs and popovers that are open, the last opened last.
- `modals[].present` is `sheet`, `full` or `task` (something the person finishes or cancels, presented as each system presents one). `modals[].locked`: the person can't take it away (the sign-in a session that ended was sent to).
- `question`: what a control asked before it acts, `{"id","title","text?","confirm","cancel?"}`, there until an `answer` event.
- A link to a top-level destination selects it. A link that itself says `present: push` is the exception: its screen is stacked on the one it stands on, in the stack the person is in, also when the address is a destination's own (a card on the first destination that opens another destination's page as a detail, with a way back). The destination's own stack is untouched. (`transcripts/push-destination.json`)
- `from`: the id of the part whose action opened the screen, on the screen it was opened from, as the `act` event named it. A screen that was pushed or presented has it, also when the action waited for a question; a screen changed in place (a day step, a replace) and a destination's first screen do not gain one. A client that grows a screen out of a card (`transition: zoom`) shrinks it back into the same card, and the snapshot keeps it for a launch that is restored. (`transcripts/opened-from.json`)
- A field's state may hold `options` (what a search on the server found, drawn in place of the control's own), `more` (there are options after these) and `searching`.

The whole of it, with every event, effect and query, is in `schema/navree-native-core.schema.json`.

## Signing in, signing out, and push

- **A sign-in in the system's session, handed over.** The session has cookies of its own, so a sign-in in it signs nobody in here. When the shell names the hand-over's routes (`sign_in.start`, `sign_in.exchange`), the core asks for 48 random bytes, makes from them a verifier (which stays in the core) and a `state`, and opens the session at the start route with the verifier's SHA-256 (`code_challenge`, `S256`), the `state`, the app's id and its system. The return is taken only while a session the core opened is open, and only with that `state`; a return that comes any other way is not used and shows nothing. The `callback` the shell names is handed to the system only when it is an `https` address on one of the app's origins or a link of the scheme the app was built with (`sign_in.callback`); otherwise the sign-in does not start, and the person is told it failed. A return that carries `error=access_denied` and this sign-in's `state` is the person saying no: it ends the sign-in as closing the sheet does, and nobody is told it failed. A return with an `error` and another `state`, or none, is not this sign-in's: nothing changes and the sign-in stays open. Any other `error` with this sign-in's `state` is a sign-in that failed. (`transcripts/sign-in-declined.json`) The code is posted to the exchange route with the verifier, as JSON, and only an answer of `200` is a sign-in: the shell is fetched again, and the shell says who is signed in. One at a time: one started while a session is open takes its place. (`transcripts/hand-over.json`)
- **A second step.** When the route of a sign-in the app carried out itself (`platform_sign_in`) says a second step is still to come, its page (the call's `second_step`, else the shell's) is opened inside the app, as a task over what is there, with `?next=` the address underneath. When a write from that page leads away, it goes and the shell is fetched again. (`transcripts/second-step.json`)
- **Push.** The client hands the core the device's token with `push_token`. The core posts it to the route the shell names (`device.push.register`), as JSON with the app's id, when somebody is signed in; again when the token changes; and for the next person who signs in. No client makes that request. Asking the person's permission is the `register_push` call, whose result says only what they answered. (`transcripts/push.json`)
- **Signing out on purpose.** A reply that carries `signed_out`: everything of the person's goes at once, the token is taken away at the same route with a `delete`, and when that is answered the client is told to forget the session's cookies (`forget_session`); then the app starts again from the shell and lands where the reply's `navigate` said. A session that ran out is not this: the way in is put up over the work, and the work stays. (`transcripts/signed-out.json`)

SHA-256 is the `sha2` crate, pure Rust. The core makes nothing random itself.

## A launch that is restored

`{"part":"snapshot"}` gives what a client stores when the app leaves the front: the stack of each destination and what is presented over them, each screen's address and title and the part it was opened from, what the person typed into forms and did not send, and which sheets of a page were open. It holds no tree and nothing secret: no token, no ticket, no verifier of a sign-in, nothing typed into a password field, no file. It names the person it was taken for by a hash.

When the system has ended the app, the client sends `restore` with the snapshot in place of `start`. The shell is fetched as at any start. If it names the same person, the stacks are put back, each screen with its address and title and nothing to show yet; the screen in front is fetched at once, the others as they come to the front, and what was typed goes back into a screen when its page arrives. A snapshot of another version (`version` is 1), another origin or another person, or one that does not read, is not used: the app starts as it does the first time, and says so with a `report`. (`transcripts/snapshot.json`, `transcripts/snapshot-refused.json`)

## Lists and tables that change

- **A list kept up to date.** A list that says the revision its rows are at (`since`), where to ask what changed (`changes`) and the stream that says when (`live`) is kept up to date by the core: a burst of messages costs one ask, and the answer is a `merge` op. A row that changed takes the place of the one with its id, new rows join at the end the op names, rows that left go, and what the person typed into a row that stays is kept. (`transcripts/live-list.json`)
- **Rows by cursor, both ways.** `more` loads the rows after the last, and with `before` the ones before the first. A table's next rows come as a `rows` op; one row is changed by `row`, which keeps whether it is chosen, and taken out by `remove`. (`transcripts/table-rows.json`)
- **Rows chosen.** The ids of the rows chosen go with the form the table's `select` names, under the name it gives, after the form's own fields and before the pair of the button that sent it. A row that can't be chosen (`unchoosable`) is passed over by "all" and never sent. When the person takes every record that matches (`across`), what stands for that is sent in place of the ids, until a row is cleared or the table's region is fetched again. (`transcripts/table-words.json`)
- **Changes that name their rows.** A merge, a row drawn again, a row taken out, rows that join the start, a move and what is under a row opening or shutting are each a `redraw` change that names the rows, so a client draws those again and not the screen.
- **A day pager.** A step to another day (the pager's `previous`, `next` or `today`, sent as a plain `navigate`) takes the day's place on the screen: the screen keeps its place in its stack, and back does not lead through the days. A day picked in the system's date control is the pager's `pick` address sent with the day as the action's `text` (`YYYY-MM-DD`); the core asks for it under the name the pager gives, and not at all for a day outside the pager's first and last. (`transcripts/health-charts.json`)
- **The pager in a screen's bar.** A screen may carry its pager as its own (`screen.pager`) and not among its parts. It steps and picks the same way, and when a screen has both, the one in the bar is asked first. Ops reach it as they reach a part: the scores it holds are found by their own id (they stand between the screen's actions and its body in document order), a `morph` or a `replace` that brings a `day_pager` for the pager's id takes its place, and a `remove` of that id takes it away with what it holds. (`transcripts/bar-pager.json`)
- **Parts added to a composed screen.** An `append` and a `prepend` add to a `card`, to `layers` and to a `carousel` as they add to a stack or a list: to the part's children, never to a card's menu. A part whose id the target already holds is passed over. A `shape` holds nothing, and the two rooms of an `adaptive` part are not one list, so neither is added to: name the stack inside. (`transcripts/composed-ops.json`)
- **Tabs.** Of a set of tabs with an id, the selected one shows. A lazy region in another tab is fetched when its tab is chosen (`tab`). A tab that is a link takes the screen to its page. (`transcripts/tabs.json`)
- **Rows moved.** A row the person dragged stands where they put it at once; the move is posted as `item`, `from`, `to`, `before` and `position`, with the list's own fields; the row and the lists are named by the app's own keys where the page gave them, and by their ids where it did not. A move the server refuses, or that fails on the way, puts the row back. A row moves within its list and between the lists of one `group`, never into a full one. (`transcripts/reorder.json`)

## What the core sends for the client

- **Every post is marked `xsrf`.** The client sends the session's token as the `x-xsrf-token` header, from the `XSRF-TOKEN` cookie, as a browser's runtime does with every post. A token the page wrote into a form or an action goes as it came, beside it. So a post from a menu that an answer brought, which has no page to take a token from, is sent like any other. (`transcripts/menu-post.json`)
- **A field's own doing.** A field that says `changed` sends its form as it changes (at once for a tick or a pick, when the typing pauses for text) or runs commands with what it holds. (`transcripts/field-changed.json`)
- **Searched options.** The search of a choice is asked when the typing pauses, never for fewer characters than the control asks for. The answer is the field's because the request that asked was for it, whatever name the field in the answer has, and an answer to an older search is dropped. (`transcripts/searched-choice.json`)
- **A confirmation.** The core posts the call's ticket to the challenge route the call names, as JSON, and hands the client what came back with the `device` effect. The client shows the system's passkey sheet and nothing else; the assertion is posted where the call said, as `credential`, without the ticket, which the challenge spent. (`transcripts/device-call.json`)
- **A phone number's country.** A phone field that names a country sends it as a second field beside the number: the one the person picked, or the one the page came with. (`transcripts/phone-country.json`)
- **A sign-in with a provider.** The core asks the nonce route, hands the client the nonce, and posts the token the provider's sheet gave to the app's sign-in route as JSON, with the state the nonce came with. Only an answer that is the person signed in counts as one: the shell and the screen are then fetched again. An answer that says a second step is still to come signs nobody in; the person is told (`second_step`) and the page is asked again. (`transcripts/platform-sign-in.json`)

## What a redirect means

The core follows redirects itself, and a redirect means one of three things.

- **The session ended.** An answer of 401 that carries a `navigate` op, or a redirect that lands on an address the shell's routes present with no chrome as a full cover or a sheet. The way in is put up over what the person was doing, as the route says, and can't be taken away. When a write from it leads somewhere else, it goes, and the person is back where they were. (`transcripts/session-ended.json`)
- **The page has moved.** Any other redirect of a plain request. The screen takes the address the answer came from, and nothing is presented over anything. (`transcripts/page-moved.json`)
- **A post was done.** A redirect, or a `navigate` op, in the answer to a write. Out of a presented screen and back to what is under it; back to a screen already in the stack; or in place of the screen that posted. A `navigate` op that says how to `present` its screen is obeyed instead. (`transcripts/post-landing.json`, `transcripts/form-redirect.json`)

A redirect that leaves the app's origins opens outside the app and is never fetched. One whose address does not read (see "Addresses") is not followed: the request failed, as a sixth redirect in a row does. (`transcripts/addresses.json`)

## What the core keeps current

- **The shell.** Every answer names the shell it was written beside, in its `shell` field or its `x-navree-shell` header. When that differs from the shell the core holds, the shell is fetched again, once for each new name.
- **The app.** An answer whose `app` (or `build`) differs from the last one empties the pages kept.
- **The person.** When the shell's user becomes nobody, every screen is emptied and what was kept is thrown away, but the addresses stay, so the same person signing in again is back where they were. When it becomes someone else, nothing of the person before stays, not even where they were.
- **A newer server.** A kind of node, action, command, control or op this build has never heard of costs that one thing: its stand-in is drawn, or the page is the web view's, or the screen is fetched again. A word a closed list lacks is read as the field's default.

## What is kept between launches

With `cache.persist`, the core hands the client things to keep for the next launch, with `cache_write`, under keys the client need not understand.

- **The shell**, always: the app opens with its bars drawn.
- **A page**, only when it was asked for with a `get` and its answer was a whole page (not a region or a list's rows), with success, carrying `x-navree-keep: 1`. The server sends the header when the page's `Cache-Control` does not say `no-store` (`View::cache("private, no-cache")`, or `native::Answer::cache` for a screen written for native clients lets a page be kept), and only on an answer to a `GET` or `HEAD`. Any other answer for an address (an error, a redirect, an answer to a post, a page for the web view, ops alone, an answer without the header) takes away what was kept for it. A page is kept under its whole address as it leaves the core, so two spellings of one address are one page. Pages kept in memory while the app runs are as before. (`transcripts/keep.json`, `transcripts/keep-answers.json`)
- **What is kept has an owner**: the person, or the visitor, its pages were answered for, written beside the list in `boot`. **A kept page is read, and a page is written, only when this launch's shell has answered over the network as a shell and named that owner**, whatever order the answers come in; every answer of the shell asks again. Before that, a kept page is not even read. A shell that names someone else empties everything kept first. An answer that is not a shell (a 404, another type, one that does not read) names nobody and makes nothing readable. A `boot` with no owner (written by an older build), or whose kept shell does not read, is emptied, not taken. A shell that can't be reached: no kept page is drawn, and the screen shows what a screen that could not load shows. The fresh answer always takes a kept page's place. (`transcripts/keep-owner.json`)
- **Taking away and emptying reach the device whatever the core holds.** A page answered without the header before the list was read is taken away, and stays away when the list is read. A write, a new build and a change of person empty the device's store even when nothing is listed. The build the pages were answered by is kept beside them, so a deploy between launches empties them. (`transcripts/keep-store.json`)
- **A kept page is drawn only for the read that asked for it.** A read that was on its way when what is kept was emptied (a change of person, a write, a new build), or that was never asked, is dropped when its answer comes. (`transcripts/keep-reads.json`)

**What a client must do with what it keeps** (the cache and the snapshot): store it so that it can't be read while the device is locked, where the system offers that; leave it out of backups and of a move to another device; and empty it on `forget_session` and `cache_clear`, in the order the effects arrive.

## The conversations

`transcripts/` holds each conversation with a server as a sequence of events in and the updates expected out, to be read by a person and replayed by the tests: opening the app, pages, every way a link is presented, route rules, back, refresh, forms (messages at fields, one send at a time, redirects, files), every op and command, regions, live regions, lists by cursor, the table's query, the web view's page, an app too old, kinds a build has never heard of, a session that ends, a page that moved, where a post leads, device calls, a sign-in with a provider, a sign-in handed over from the system's session, a second step, push, signing out on purpose, a launch that is restored, tabs, lists that change by revision, tables row by row, rows moved, fields that act as they change, ranges, searched options, ticked menu items, questions, a post from a menu an answer brought, tasks, the health charts and a day pager stepped in place, a pager in a screen's bar, parts added to a card and a carousel, the part a screen was opened from, a destination's page pushed as a detail, failures, answers nobody waits for any more, links from outside, addresses that do not read and an id in another script, a change of person, and what is kept between launches. The bodies in `transcripts/fixtures/store` are what the store demo answered; those in `fixtures/made` are written by hand.

`NAVREE_CORE_TRANSCRIPTS=write cargo test -p navree-native-core --test transcripts` writes the expected updates and the schema file again after a change that is meant.
