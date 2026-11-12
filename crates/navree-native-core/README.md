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
- `Core::view(&self) -> &ViewState`, and `ViewState::part(&Query)` for one part of it as JSON

The core never does I/O, reads no clock and rolls no dice. It asks for a request, a timer or a stream with an **effect**, and learns what came of it from a later **event**. So every rule in it is tested with plain values, and four clients cannot drift apart.

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

Only `origin` and `client` are required. `origin` must be `https` (`loopback_http: true` allows plain `http` for the machine itself, for development). `also` lists other origins the app treats as its own. `client` is the protocol's own description of a client, sent with every request; its `caps` name the device features this build has, by the protocol's names (`share`, `clipboard`, `open_url`, `scan_code`, `location_once`, …). A control that calls a feature the build lacks is drawn as the call's stand-in says, or not at all. `sign_in.native` names the providers this build signs in with through the system's own sheet (`apple` on Apple's systems, `google` on Android); a sign-in with any other goes the web's way. `back_to_first` is Android's back, which leads to the first destination before it leaves the app. `undrawable` names the kinds this build of the client can't draw yet: a page that holds one is shown in the web view.

## Events

Every event is an object whose `t` names it. A field shown with `?` may be left out.

| `t` | Fields | When |
|---|---|---|
| `start` | | The app started, or the person asked to try again after it failed to. |
| `response` | `request`, `status?`, `content_type?`, `location?`, `headers?`, `body?`, `url?`, `error?`, `left?` | A `fetch` was answered. `headers` holds the answer's headers whose names start with `x-navree-`, each as `[name, value]`. A redirect is handed over as it came, with its `status` and `location`: the core checks where it leads and asks again. `error` (`offline`, `timeout`, `other`) when no answer came. A transport that follows redirects itself gives the last address as `url`, and stops with `left` at one that leaves the app's origins. |
| `select` | `destination` | The person chose a top-level destination. |
| `act` | `screen?`, `action`, `form?`, `overlay?`, `text?`, `now?`, `ask?` | The person tapped something that has an action. `action` is the node's own. `form` and `overlay` are the ids of the form and of the sheet, dialog or popover the control stands in. `now` is the time in seconds since 1970, given when the action is a device call: a call whose ticket has run out is not started. `ask` is the question the control asks first, as the node holds it: the core then holds the action and puts the question in the screen's `question`. |
| `answer` | `screen`, `question`, `yes` | The person answered the screen's question. Yes runs what the control was about to do. |
| `field` | `screen`, `form?`, `field`, `value` | The person changed a field. `value` is `{"t":"text","text":"…"}`, `{"t":"flag","on":true}`, `{"t":"many","values":[…]}`, `{"t":"files","files":[{"token","name","type","size"}]}`, `{"t":"range","from":"…","to":"…"}` for the two ends of a range, or `{"t":"none"}`. A field whose node says what happens as it changes (`changed`) needs nothing more from the client: the core sends the form or runs the commands. |
| `options` | `screen`, `form?`, `field`, `text?`, `more?` | The person typed in a choice whose options are searched on the server; or, with `more`, came near the end of the options it shows. |
| `choose` | `screen`, `table`, `row?`, `on` | The person ticked or cleared a row of a table whose rows can be chosen; every row it holds when `row` is left out. |
| `move` | `screen`, `item`, `from`, `to`, `before?` | The person dragged the row `item` from the list `from` into the list `to`, before the row `before` or at the end. |
| `search` | `screen`, `text` | The person searched with the screen's own search (`screen.search`). |
| `back` | | The system's back. |
| `pop_to` | `screen` | The person went back to a screen further down the stack. |
| `dismissed` | `screen` | The person took a sheet or cover away. |
| `overlay` | `screen`, `id`, `open` | The person opened or closed a sheet, dialog, popover or disclosure of the tree themselves. |
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
| `messages` | The messages waiting to be shown. |

## Effects

| `t` | Fields | What the client does |
|---|---|---|
| `fetch` | `request`, `method`, `url`, `headers`, `body?`, `timeout_ms`, `xsrf?` | Sends the request with its cookies, follows no redirect, and answers with a `response` event carrying `request`. `body` is `{"t":"form","text":"a=1&b=2"}` (send `text` as it is), `{"t":"multipart","parts":[…]}` (write the parts, putting in the bytes kept under each `token`) or `{"t":"json","text":"{…}"}` (send `text` as it is, which may be empty: the framework's own routes speak JSON, and the answer's body is handed back as any other). With `xsrf`, the `XSRF-TOKEN` cookie, decoded, also goes in `x-xsrf-token`. A request for one region or for a list's next rows carries `x-navree-region` with the region's id. |
| `cancel` | `request` | May drop the request: its answer is no longer wanted. |
| `open_url` | `url`, `inside?` | Opens the address outside the app; with `inside`, in the system's browser inside the app. |
| `sign_in` | `url`, `callback?`, `ephemeral?` | Opens the system's sign-in session, and answers with `sign_in_done`. With `ephemeral`, the session shares nothing with the system's browser. |
| `message` | `id`, `tone`, `text?`, `title?`, `notice?`, `action?` | Shows a short message, and answers with `message_shown` when it has gone. `notice` is a message of the core's own, for the client to word. |
| `haptic` | `kind` | Plays a haptic. |
| `device` | `call`, `device`, `posts?`, `options?` | Runs a device call: one a tap started, or one of the few a server's reply may start. `device` is the call as the protocol writes it (`{"f":"share","title":"…"}`). With `posts`, answers with `device_result`, and the core sends the result where it goes. `options` is what the server gave for the call to start with, which the core asked for: the passkey request for `confirm`, the nonce for `platform_sign_in`. The client asks the server for nothing itself. |
| `cache_read` | `key` | Reads what is kept under the key, and answers with `cache`. |
| `cache_write` | `key`, `value?` | Keeps the value under the key; without a value, takes the key away. |
| `cache_clear` | | Throws away everything kept. |
| `announce` | `text?`, `notice?`, `count?` | Says it to assistive technology. |
| `copy` | `text`, `sensitive?` | Puts the text on the clipboard. With `sensitive`, as a secret the system should not show or sync. |
| `timer`, `cancel_timer` | `timer`, `ms` | Answers with a `timer` event after that long; or no longer. |
| `stream_open`, `stream_close` | `stream`, `url`, `headers` | Opens a stream of server-sent events and answers with `stream` events; or closes it. Only an event's name is passed on. The client does not reconnect by itself: the core asks. |
| `exit` | | There is nothing to go back to: the system takes the back. |
| `report` | `what`, `detail` | Something the app's developers will want to know. |

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

- `stage`: `starting`, `ready`, `failed` (with `failure`) or `upgrade` (with `upgrade`).
- A screen's `content` is `{"t":"blank"}`, `{"t":"native","screen":…}` (a screen of the protocol) or `{"t":"web","url","title?","why","serial"}` (load the address in the web view; load it again when `serial` moves on).
- **The tree is ready to draw.** What is open, hidden, disabled or waiting is already in it: a sheet's `open`, a stack's `hidden`, a button's `pending`. A state that has no field in the tree is in `states`, by the node's id.
- **Every form and every field has an id that is its own on its screen.** A form that came with none goes by `@` and its address. `forms` holds each form's state by that id, and each field's by the field's id: what it holds now, and what is wrong with it. A client draws a field's value and messages from here, never from the node.
- `overlays` lists the sheets, dialogs and popovers that are open, the last opened last.
- `modals[].present` is `sheet`, `full` or `task` (something the person finishes or cancels, presented as each system presents one). `modals[].locked`: the person can't take it away (the sign-in a session that ended was sent to).
- `question`: what a control asked before it acts, `{"id","title","text?","confirm","cancel?"}`, there until an `answer` event.
- A field's state may hold `options` (what a search on the server found, drawn in place of the control's own), `more` (there are options after these) and `searching`.

The whole of it, with every event, effect and query, is in `schema/navree-native-core.schema.json`.

## Lists and tables that change

- **A list kept up to date.** A list that says the revision its rows are at (`since`), where to ask what changed (`changes`) and the stream that says when (`live`) is kept up to date by the core: a burst of messages costs one ask, and the answer is a `merge` op. A row that changed takes the place of the one with its id, new rows join at the end the op names, rows that left go, and what the person typed into a row that stays is kept. (`transcripts/live-list.json`)
- **Rows by cursor, both ways.** `more` loads the rows after the last, and with `before` the ones before the first. A table's next rows come as a `rows` op; one row is changed by `row`, which keeps whether it is chosen, and taken out by `remove`. (`transcripts/table-rows.json`)
- **Rows chosen.** The ids of the rows chosen go with the form the table's `select` names, under the name it gives, after the form's own fields and before the pair of the button that sent it.
- **Rows moved.** A row the person dragged stands where they put it at once; the move is posted as `item`, `from`, `to`, `before` and `position`, with the list's own fields. A move the server refuses, or that fails on the way, puts the row back. A row moves within its list and between the lists of one `group`, never into a full one. (`transcripts/reorder.json`)

## What the core sends for the client

- **Every post is marked `xsrf`.** The client sends the session's token as the `x-xsrf-token` header, from the `XSRF-TOKEN` cookie, as a browser's runtime does with every post. A token the page wrote into a form or an action goes as it came, beside it. So a post from a menu that an answer brought, which has no page to take a token from, is sent like any other. (`transcripts/menu-post.json`)
- **A field's own doing.** A field that says `changed` sends its form as it changes (at once for a tick or a pick, when the typing pauses for text) or runs commands with what it holds. (`transcripts/field-changed.json`)
- **Searched options.** The search of a choice is asked when the typing pauses, never for fewer characters than the control asks for. The answer is the field's because the request that asked was for it, whatever name the field in the answer has, and an answer to an older search is dropped. (`transcripts/searched-choice.json`)
- **A confirmation.** The core posts the call's ticket to the challenge route the call names, as JSON, and hands the client what came back with the `device` effect. The client shows the system's passkey sheet and nothing else; the signature is posted where the call said, without the ticket, which the challenge spent. (`transcripts/device-call.json`)
- **A sign-in with a provider.** The core asks the nonce route, hands the client the nonce, and posts the token the provider's sheet gave to the app's sign-in route as JSON, with the state the nonce came with. Only an answer that is the person signed in counts as one: the shell and the screen are then fetched again. An answer that says a second step is still to come signs nobody in; the person is told (`second_step`) and the page is asked again. (`transcripts/platform-sign-in.json`)

## What a redirect means

The core follows redirects itself, and a redirect means one of three things.

- **The session ended.** An answer of 401 that carries a `navigate` op, or a redirect that lands on an address the shell's routes present with no chrome as a full cover or a sheet. The way in is put up over what the person was doing, as the route says, and can't be taken away. When a write from it leads somewhere else, it goes, and the person is back where they were. (`transcripts/session-ended.json`)
- **The page has moved.** Any other redirect of a plain request. The screen takes the address the answer came from, and nothing is presented over anything. (`transcripts/page-moved.json`)
- **A post was done.** A redirect, or a `navigate` op, in the answer to a write. Out of a presented screen and back to what is under it; back to a screen already in the stack; or in place of the screen that posted. A `navigate` op that says how to `present` its screen is obeyed instead. (`transcripts/post-landing.json`, `transcripts/form-redirect.json`)

A redirect that leaves the app's origins opens outside the app and is never fetched.

## What the core keeps current

- **The shell.** Every answer names the shell it was written beside, in its `shell` field or its `x-navree-shell` header. When that differs from the shell the core holds, the shell is fetched again, once for each new name.
- **The app.** An answer whose `app` (or `build`) differs from the last one empties the pages kept.
- **The person.** When the shell's user becomes nobody, every screen is emptied and what was kept is thrown away, but the addresses stay, so the same person signing in again is back where they were. When it becomes someone else, nothing of the person before stays, not even where they were.
- **A newer server.** A kind of node, action, command, control or op this build has never heard of costs that one thing: its stand-in is drawn, or the page is the web view's, or the screen is fetched again. A word a closed list lacks is read as the field's default.

## The conversations

`transcripts/` holds each conversation with a server as a sequence of events in and the updates expected out, to be read by a person and replayed by the tests: opening the app, pages, every way a link is presented, route rules, back, refresh, forms (messages at fields, one send at a time, redirects, files), every op and command, regions, live regions, lists by cursor, the table's query, the web view's page, an app too old, kinds a build has never heard of, a session that ends, a page that moved, where a post leads, device calls, a sign-in with a provider, lists that change by revision, tables row by row, rows moved, fields that act as they change, ranges, searched options, ticked menu items, questions, a post from a menu an answer brought, tasks, failures, answers nobody waits for any more, links from outside, a change of person, and what is kept between launches. The bodies in `transcripts/fixtures/store` are what the store demo answered; those in `fixtures/made` are written by hand.

`NAVREE_CORE_TRANSCRIPTS=write cargo test -p navree-native-core --test transcripts` writes the expected updates and the schema file again after a change that is meant.
