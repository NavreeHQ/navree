# Lists, panes, history, drag and drop, and steps

Components for long lists, two-pane screens, history, boards and multi-step flows. Each block compiles
on its own.

## Lists, panes and history

**LazyList** is a stream read in one order (an inbox, a conversation, a
feed): the page draws the first page and the runtime asks for the next by
cursor. Use a `DataTable` instead when people compare, sort and act on rows.

**SplitPane** puts panes side by side with draggable separators, sizes kept on
the server, and one pane at a time on a phone.

**Timeline** and **ActivityFeed** show what happened, with times in a zone
you name.

```rust
use navree::ui::{
    ActivityFeed, ActivityItem, FeedHeight, LazyItem, LazyList, Meta, Pane, SplitHeight, SplitPane,
    SplitWidth, Text, Timeline, TimelineEvent, Tone, icons,
};

/// The first page of a conversation list. `more_after` is the link to the next page.
pub fn conversations(rows: Vec<(String, String)>) -> LazyList {
    LazyList::new("desk-rows", "Conversations", "/desk/rows")
        .items(rows.into_iter().map(|(key, title)| LazyItem::new(key.clone(), key, Text::new(title))))
        .more_after("/desk?after=000000000995")
}

pub fn desk() -> SplitPane {
    SplitPane::new("desk")
        .height(SplitHeight::Md)
        .stack_below(SplitWidth::Md)
        .save_to("/desk/panes")
        .pane(Pane::new("desk-list", "Conversations").size(30).min(22).max(45).body(Text::new("List")))
        .pane(Pane::fill("desk-thread", "Conversation").min(30).body(Text::new("Thread")))
        .current("desk-thread")
}

pub fn order_history() -> Timeline {
    Timeline::new("order", "Order 4821 for Harleen Kaur")
        .time_zone("Asia/Kolkata", 19_800)
        .event(TimelineEvent::new("Order placed").at(1_793_592_667).tone(Tone::Neutral))
        .event(
            TimelineEvent::new("Packed")
                .at(1_793_594_468)
                .tone(Tone::Info)
                .meta(Meta::icon(icons::PACKAGE, "2 boxes")),
        )
}

pub fn recent_activity() -> ActivityFeed {
    ActivityFeed::new("activity", "Recent activity")
        .summary("1 event")
        .height(FeedHeight::Md)
        .item(
            ActivityItem::new("Manpreet Gill", "replied to a ticket")
                .tone(Tone::Success)
                .icon(icons::MESSAGE_SQUARE_REPLY)
                .time_text("2 min ago"),
        )
}
```

`LazyQuery` is the extractor a list's source reads (`query.ask()` says the
first page, what is before or after a cursor, or what changed since a
revision) and `LazyPage::new(items, more)` is its answer. Page by keyset,
never by offset. `SplitSave` reads the sizes a person dragged to, the way a
form is read, and `SplitSizes` holds them to the layout's limits: keep them
for that person and draw the next page with `.sizes(..)`.

## Drag and drop

`Sortable` draws lists and boards whose items move by mouse, touch and keys.
The page moves the item at once and posts a `Move`: the item, the list the
browser saw it in, the list it goes to and the neighbours it landed beside.
The server checks the move against the lists as they are now and saves it, or
refuses it, and the page puts the item back with a toast.

```rust
use navree::ui::{ListState, Move, Reply, SortItem, SortList, Sortable};

pub fn board() -> Sortable {
    Sortable::new("launch", "Launch board")
        .board()
        .post("/launch/moves")
        .list(
            SortList::new("ready", "Ready")
                .item(SortItem::new("11", "Pick the launch date"))
                .item(SortItem::new("12", "Draft the press note")),
        )
        .list(SortList::new("doing", "In progress").limit(3).item(SortItem::new("13", "Fix the sign-up email")))
        .list(SortList::new("done", "Done").empty("Nothing done yet"))
}

/// Holds a move to the lists as the server has them now.
pub fn answer(mv: &Move, lists: &[ListState]) -> Reply {
    match mv.check(lists) {
        Ok(_landing) => mv.done("/launch"),
        Err(refusal) => mv.refused("/launch", refusal.message()),
    }
}
```

Run the check and the write inside `Move::transaction`, which locks the lists
the move touches before you read them, so two moves at once queue instead of
both taking the last free place. Keep a long list by rank (`Move::place`).
The keys of the lists come from the browser: make them unique per workspace,
or use `Move::transaction_scoped` with a `ListScope` naming the caller's
workspace or board (`sortable::lock_lists_scoped` and
`sortable::renew_ranks_scoped` take the same scope), so a key of another
scope is a list that isn't there.
The whole story, with the code, is in
[Drag and drop](https://navree.com/docs/ui/drag-and-drop/).

## Steps, ranges and trees

**Wizard** splits one long form into steps. Each step is a struct with
`#[derive(Form)]`, checked on the server before the next opens. The progress
lives in a `Draft` kept on the server, so a person can leave and come back
from any device. Every move is a plain post, and the last step checks every
step again. `MemoryDrafts` suits tests and an app with one process; a real
app implements `DraftStore` over a table.
**Stepper** is the step bar on its own.

**Slider** is one value or a range that posts without script; `SliderSpec`
holds a post to the same grid the browser drew.

**TreeView** (the `Tree` type) shows nested links or choices, with children
loaded from the server as a branch opens.

**Toolbar** is one tab stop with arrow keys and a More menu for what does not
fit.

```rust
use navree::ui::{
    Form, Slider, SliderLabels, SliderSpec, Step, Stepper, Toolbar, ToolbarItem, Tree, TreeNode,
    Validate, Wizard, WizardStep, icons,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, Validate, Form)]
pub struct DealBasics {
    /// Deal name
    #[validate(length(min = 1, max = 60))]
    pub title: String,
}

#[derive(Debug, Deserialize, Validate, Form)]
pub struct DealContact {
    /// Email
    #[validate(email)]
    #[form(widget = email)]
    pub email: String,
}

/// Built fresh for each request. `wizard.page(&draft, step)` draws a step and
/// `wizard.submit(&mut draft, step, &pairs)` reads a post and says where to go.
pub fn deal_wizard() -> Wizard {
    Wizard::new("deal")
        .base("/deals/new")
        .step(WizardStep::of::<DealBasics>("basics", "Basics").description("What the deal is"))
        .step(WizardStep::of::<DealContact>("contact", "Contact"))
        .review()
}

pub fn progress_bar() -> Stepper {
    Stepper::new("Checkout")
        .step(Step::new("Cart"))
        .step(Step::new("Address"))
        .step(Step::new("Payment"))
        .current(1)
}

pub fn discount() -> Slider {
    Slider::new("discount").label("Discount").max(100).step(5).value(10).suffix("%")
}

/// The grid the browser drew is the grid the server holds a post to.
pub fn read_discount(posted: &[(String, String)]) -> String {
    let grid = SliderSpec::new(0, 100).step(5);
    match grid.read(posted, "discount") {
        Ok(value) => format!("Discount {value}%"),
        Err(error) => grid.message(error, &SliderLabels::default()),
    }
}

pub fn folders() -> Tree {
    Tree::new("files", "Files")
        .node(
            TreeNode::branch(
                "docs",
                "Documents",
                [TreeNode::leaf("plans", "Plans").link("/files/plans").icon(icons::FILE)],
            )
            .icon(icons::FOLDER)
            .open(true),
        )
        .node(TreeNode::lazy("music", "Music", "/tree/children?parent=music").icon(icons::FOLDER))
}

pub fn formatting() -> Toolbar {
    Toolbar::new("format", "Text formatting")
        .item(ToolbarItem::icon_toggle("bold", icons::LETTER_B, "Bold"))
        .item(ToolbarItem::separator())
        .item(ToolbarItem::button("Copy").icon(icons::COPY))
}
```
