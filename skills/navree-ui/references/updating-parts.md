# Parts of a page that update

Regions that load after the page paints, poll or follow a stream; the `Cmd` and `Reply` that
actions use; and long work followed from a page. Each block compiles on its own.

## Regions: parts of a page that load or update

A `Region` is a named part of a page with its own source. Each answer
morphs in and keeps focus and what was typed. The handler of a source
returns a `View` of just that part.

```rust
use std::time::Duration;

use navree::ui::{Card, Region, Skeleton, SkeletonShape, Stack};

pub fn dashboard() -> Stack {
    Stack::gap(4).children((
        // Fetched after the page paints, with skeleton lines until then.
        Card::titled("Revenue by month").body(
            Region::new("revenue")
                .src("/fragments/revenue")
                .lazy()
                .children((Skeleton::new(SkeletonShape::Line), Skeleton::new(SkeletonShape::Short)))
                .when_offline(),
        ),
        // Fetched again every 5 seconds while the page is visible.
        Card::titled("Activity").body(
            Region::new("activity").src("/fragments/activity").poll(Duration::from_secs(5)),
        ),
        // Fetched again when a server-sent event stream says something. The event
        // is a nudge: its payload is never read.
        Card::titled("New leads").body(
            Region::new("leads")
                .src("/fragments/leads")
                .live_from("/events")
                .live_event("leads"),
        ),
    ))
}
```

- `.feed(into, after, empty)` asks the source only for rows newer than a
  cursor; `.feed_limit(n)` keeps the newest `n`.
- A region that gets a 401, 403, 404, 410, 429, 500 or 503 shows the state
  the server drew, keeps its card and offers Try again. A polling region
  stops on a 401, 403, 404 or 410 and backs off on a 429 or 5xx.
- A stream is any endpoint that answers `text/event-stream`. One connection
  serves every region and table of the page that names it. Wiring a stream
  to `navree-realtime` is coming: today you write the stream yourself.
- A `DataTable` has its own `.live(every)` and `.live_from(url)`.

## Actions: what runs in the browser and what the server sends back

`Cmd` is a closed list of things an element does with no request: show,
hide, toggle, open or close a dialog, focus, submit, copy, set text, count
characters, fill a dialog from a row. `Reply` is what an action handler
answers with: the same handler serves JavaScript on (ops) and off (a
redirect or a page).

```rust
use navree::ui::{Button, Cmd, Heading, Reply, ToastTone};

pub fn details_button() -> Button {
    Button::outline("Show details").on_click(Cmd::toggle("contact-details"))
}

pub fn saved(name: String) -> Reply {
    Reply::ops_or_redirect("/contacts")
        .morph("contact-title", Heading::h1(name))
        .toast(ToastTone::Success, "Saved")
}
```

`Reply::redirect(href)` also moves the page; `Reply::ops(ops)` answers `204`
with JavaScript off; `Reply::view(view)` shows a page. The ops are `morph`,
`replace`, `append`, `prepend`, `remove`, `errors`, `toast`, `open`,
`close`, `focus`, `url`, `navigate`, `reload` and `invalidate` (refetch a
region). A redirect that would leave the site is refused unless it is
`Reply::redirect_external`. Nothing is fetched ahead of a click unless a
link says `.prefetch()`: only mark a link whose `GET` changes nothing.

## Long work

An import, an export or a report that users wait for runs as an operation in
the queue (`navree-queues-and-scheduling`: start the operation, dispatch a
batch, call `ctx.progress(done, total, note)` in the job). Navree UI has no
component tied to an operation. A page follows one with a polling `Region`
whose source draws a `Progress` from the operation's state. The operation's
own JSON routes and its event stream (`/api/operations/{id}`) are for front
ends in React or Vue.

```rust
use std::time::Duration;

use navree::ui::{Progress, Region, Stack, Text, View};

/// Where the page waits for the work: a region that asks for the progress again every 2 seconds.
pub fn import_region() -> Region {
    Region::new("import")
        .src("/contacts/import/progress")
        .poll(Duration::from_secs(2))
        .children(Progress::indeterminate("Importing contacts"))
}

/// What that route answers: the figures the operation holds now.
pub fn import_progress(done: u32, total: u32) -> View {
    View::new("Import").body(Stack::gap(2).children((
        Progress::new(done, total, "Importing contacts"),
        Text::muted(format!("{done} of {total} rows")),
    )))
}
```
