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

**What a client must guarantee.** Events are sent in the order things happened, from one thread at a time: the simplest way is to call only from the thread that draws. A lock inside the handle keeps two calls made at once from running together, so a mistake costs order, never memory. Bytes passed in stay valid for the call. Every answer is given back once. The calling thread needs about 1 MiB of stack. No call panics across the boundary.

`src/ffi.rs` is the only file with `unsafe` in it.

## Config

```json
{"origin": "https://shop.example",
 "also": ["https://*.shop.example"],
 "client": {"platform": "ios", "os": "26.1", "form": "phone", "app": "1.4.0", "vocab": 1,
            "scheme": "light", "contrast": "normal", "text_percent": 100,
            "locale": "en-IN", "tz": "Asia/Kolkata", "caps": ["push"]},
 "lib": "0.1.0",
 "back_to_first": false,
 "deep_links": "routes",
 "cache": {"pages": 24, "persist": true},
 "undrawable": ["chart", "input:colour"],
 "views": ["shelf-map"],
 "sign_in": {"paths": ["/auth/*/redirect"], "callback": "shop"}}
```

Only `origin` and `client` are required. `origin` must be `https` (`loopback_http: true` allows plain `http` for the machine itself, for development). `also` lists other origins the app treats as its own. `back_to_first` is Android's back, which leads to the first destination before it leaves the app. `undrawable` names the kinds this build of the client can't draw yet: a page that holds one is shown in the web view.

## Events

Every event is an object whose `t` names it. A field shown with `?` may be left out.

| `t` | Fields | When |
|---|---|---|
| `start` | | The app started, or the person asked to try again after it failed to. |
| `response` | `request`, `status?`, `url?`, `content_type?`, `body?`, `error?`, `left?` | A `fetch` was answered. `url` is where the answer came from in the end, when a redirect was followed. `error` (`offline`, `timeout`, `other`) when no answer came. `left` when a redirect led to another site, which was not fetched. |
| `select` | `destination` | The person chose a top-level destination. |
| `act` | `screen?`, `action`, `form?`, `overlay?`, `text?` | The person tapped something that has an action. `action` is the node's own. `form` and `overlay` are the ids of the form and of the sheet, dialog or popover the control stands in. |
| `field` | `screen`, `form?`, `field`, `value` | The person changed a field. `value` is `{"t":"text","text":"…"}`, `{"t":"flag","on":true}`, `{"t":"many","values":[…]}`, `{"t":"files","files":[{"token","name","type","size"}]}` or `{"t":"none"}`. |
| `back` | | The system's back. |
| `pop_to` | `screen` | The person went back to a screen further down the stack. |
| `dismissed` | `screen` | The person took a sheet or cover away. |
| `overlay` | `screen`, `id`, `open` | The person opened or closed a sheet, dialog, popover or disclosure of the tree themselves. |
| `refresh` | `screen` | Pull to refresh, or "try again". |
| `region_retry` | `screen`, `region` | The person asked for a region that failed again. |
| `more` | `screen`, `id` | The end of a list or table is near. |
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
| `device_result` | `call`, `ok`, `fields?` | A device call finished. |
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
             "headers":[["x-navree-native","1"],["x-navree-client","platform=ios; form=phone; vocab=1"],
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
| `rows` (`screen`, `id`, `from`, `count`) | `count` rows were added at the end of a list or table, starting at `from`, and nothing else of the screen changed. The part holds those rows, the state of the forms in them, and the list's `more`, `loading` and `failed`. A `count` of 0 says only that state changed. A client that does not read rows by themselves reads the screen. |
| `messages` | The messages waiting to be shown. |

## Effects

| `t` | Fields | What the client does |
|---|---|---|
| `fetch` | `request`, `method`, `url`, `headers`, `body?`, `timeout_ms`, `xsrf?` | Sends the request with its cookies, follows redirects that stay on the app's origins, and answers with a `response` event carrying `request`. `body` is `{"t":"form","text":"a=1&b=2"}` (send `text` as it is) or `{"t":"multipart","parts":[…]}` (write the parts, putting in the bytes kept under each `token`). With `xsrf`, the `XSRF-TOKEN` cookie, decoded, also goes in `x-xsrf-token`. |
| `cancel` | `request` | May drop the request: its answer is no longer wanted. |
| `open_url` | `url` | Opens the address outside the app. |
| `sign_in` | `url`, `callback?` | Opens the system's sign-in session, and answers with `sign_in_done`. |
| `message` | `id`, `tone`, `text?`, `title?`, `notice?`, `action?` | Shows a short message, and answers with `message_shown` when it has gone. `notice` is a message of the core's own, for the client to word. |
| `haptic` | `kind` | Plays a haptic. |
| `device` | `call`, `name`, `args?` | Runs a device call the server's reply started, and answers with `device_result`. |
| `cache_read` | `key` | Reads what is kept under the key, and answers with `cache`. |
| `cache_write` | `key`, `value?` | Keeps the value under the key; without a value, takes the key away. |
| `cache_clear` | | Throws away everything kept. |
| `announce` | `text?`, `notice?`, `count?` | Says it to assistive technology. |
| `copy` | `text` | Puts the text on the clipboard. |
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
- `modals[].locked`: the person can't take it away (the sign-in a session that ended was sent to).

The whole of it, with every event, effect and query, is in `schema/navree-native-core.schema.json`.

## The conversations

`transcripts/` holds each conversation with a server as a sequence of events in and the updates expected out, to be read by a person and replayed by the tests: opening the app, pages, every way a link is presented, route rules, back, refresh, forms (messages at fields, one send at a time, redirects, files), every op and command, regions, live regions, lists by cursor, the table's query, the web view's page, an app too old, kinds a build has never heard of, a session that ends, failures, answers nobody waits for any more, links from outside, a change of person, and what is kept between launches. The bodies in `transcripts/fixtures/store` are what the store demo answered; those in `fixtures/made` are written by hand.

`NAVREE_CORE_TRANSCRIPTS=write cargo test -p navree-native-core --test transcripts` writes the expected updates and the schema file again after a change that is meant.
