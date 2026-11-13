//! Builders for the rest of the parts: panes and tabs, words in runs,
//! pictures, tables, bars of actions, messages, and what the parts already
//! built take beside what they had.

use super::{IntoParts, list_of};
use crate::action::{Action, Command, Pair};
use crate::envelope::{Back, Screen, Search};
use crate::node::{
    Across, Align, Avatar, Banner, Block, BlockKind, Button, Code, Column, ColumnRole, Delivery,
    Dialog, Disclosure, Divider, Empty, Fact, Facts, FeedRow, Field, Fit, Form, Icon, Image, List,
    Message, Moment, NativeView, Node, Pane, PaneRole, Popover, Progress, Prominence, Purpose,
    Question, Reorder, Rich, Role, Row, Run, Selection, Sheet, SheetSize, Sort, SortDir, Spacer,
    Split, Standard, Tab, Table, TableRow, Tabs, TextStyle, Timeline, Tone, Toolbar, ToolbarItem,
    WebRegion,
};

impl Action {
    /// Sends the form `form`, from a control that stands outside it: the
    /// button at the foot of a dialog.
    pub fn submit_form(form: impl Into<String>) -> Self {
        Self::Submit {
            form: Some(form.into()),
            href: None,
            with: None,
        }
    }

    /// Closes the sheet, the dialog or the popover `id`.
    pub fn close(id: impl Into<String>) -> Self {
        Self::command(Command::Close { id: id.into() })
    }

    /// Opens the part `id` when it is shut and shuts it when it is open.
    pub fn toggle(id: impl Into<String>) -> Self {
        Self::command(Command::Toggle { id: id.into() })
    }

    /// Copies `text` for the person to paste.
    pub fn copy(text: impl Into<String>) -> Self {
        Self::command(Command::Copy { text: text.into() })
    }

    /// Says `text` in a toast, in the tone `tone`.
    pub fn toast(tone: Tone, text: impl Into<String>) -> Self {
        Self::command(Command::Toast {
            tone,
            text: text.into(),
        })
    }
}

impl Screen {
    /// Adds a screen above this one that is not the one a step back goes to:
    /// the way up, from the nearest.
    #[must_use]
    pub fn trail(mut self, title: impl Into<String>, href: impl Into<String>) -> Self {
        self.trail.push(Back {
            title: title.into(),
            href: href.into(),
        });
        self
    }

    /// The screen's search, which a client puts where its system puts one.
    #[must_use]
    pub fn search(mut self, search: Search) -> Self {
        self.search = Some(search);
        self
    }
}

impl Search {
    /// A search that asks `action` with the typed words as the query value
    /// `name`.
    pub fn new(name: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: String::new(),
            placeholder: None,
            action: action.into(),
            hidden: Vec::new(),
        }
    }

    /// The words searched for now.
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    /// What the empty box says: `Search orders`.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// A value that goes with every search: the filter already chosen.
    #[must_use]
    pub fn hidden(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.hidden.push(Pair {
            name: name.into(),
            value: value.into(),
        });
        self
    }
}

impl Split {
    /// The pane a reader chooses in and the pane that shows what was chosen:
    /// side by side where there is room, one over the other where there is
    /// not.
    pub fn new(primary: impl Into<Node>, detail: impl Into<Node>) -> Self {
        Self {
            id: None,
            primary: Box::new(primary.into()),
            detail: Box::new(detail.into()),
        }
    }
}

impl Pane {
    /// A pane of a split screen, by the part it plays.
    pub fn new(role: PaneRole) -> Self {
        Self {
            role,
            ..Self::default()
        }
    }

    /// Its title, where a system names panes.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// It is the one a narrow screen shows, when only one fits.
    #[must_use]
    pub fn current(mut self) -> Self {
        self.current = true;
        self
    }

    /// Adds what it holds.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl Tabs {
    /// Views a reader switches between, none yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a tab.
    #[must_use]
    pub fn tab(mut self, tab: Tab) -> Self {
        self.items.push(tab);
        self
    }
}

impl Tab {
    /// A tab named `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }

    /// Its icon.
    #[must_use]
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// It is the one shown.
    #[must_use]
    pub fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    /// The page it leads to, when its view is not held here.
    #[must_use]
    pub fn href(mut self, href: impl Into<String>) -> Self {
        self.href = Some(href.into());
        self
    }

    /// Adds to its view.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl Disclosure {
    /// A part that opens and closes under `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }

    /// It starts open.
    #[must_use]
    pub fn open(mut self) -> Self {
        self.open = true;
        self
    }

    /// Adds what it holds.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl Divider {
    /// A line between parts.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Spacer {
    /// Room that takes what is left over along a stack.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Run {
    /// A run of plain words.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// The run is bold.
    #[must_use]
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// The run is italic.
    #[must_use]
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// The run is code.
    #[must_use]
    pub fn code(mut self) -> Self {
        self.code = true;
        self
    }

    /// The run is a link to `href`.
    #[must_use]
    pub fn href(mut self, href: impl Into<String>) -> Self {
        self.href = Some(href.into());
        self
    }
}

impl From<&str> for Run {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for Run {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

/// The runs of `runs`, where each may be a run or plain words.
fn runs_of<R: Into<Run>>(runs: impl IntoIterator<Item = R>) -> Vec<Run> {
    runs.into_iter().map(Into::into).collect()
}

impl Block {
    /// A block of the kind `kind`, of runs or of plain words.
    pub fn new<R: Into<Run>>(kind: BlockKind, runs: impl IntoIterator<Item = R>) -> Self {
        Self {
            kind,
            runs: runs_of(runs),
            level: 0,
        }
    }

    /// A paragraph.
    pub fn paragraph<R: Into<Run>>(runs: impl IntoIterator<Item = R>) -> Self {
        Self::new(BlockKind::Paragraph, runs)
    }

    /// A heading of the level `level`, from 1.
    pub fn heading<R: Into<Run>>(level: u8, runs: impl IntoIterator<Item = R>) -> Self {
        Self {
            level,
            ..Self::new(BlockKind::Heading, runs)
        }
    }

    /// Words quoted from elsewhere.
    pub fn quote<R: Into<Run>>(runs: impl IntoIterator<Item = R>) -> Self {
        Self::new(BlockKind::Quote, runs)
    }

    /// An item of a list whose items are marked.
    pub fn bullet<R: Into<Run>>(runs: impl IntoIterator<Item = R>) -> Self {
        Self::new(BlockKind::Bullet, runs)
    }

    /// An item of a list whose items are counted.
    pub fn numbered<R: Into<Run>>(runs: impl IntoIterator<Item = R>) -> Self {
        Self::new(BlockKind::Numbered, runs)
    }

    /// Code, its lines kept as they are.
    pub fn code(text: impl Into<String>) -> Self {
        Self::new(BlockKind::Code, [Run::new(text)])
    }

    /// How deep an item of a list sits, from 0.
    #[must_use]
    pub fn level(mut self, level: u8) -> Self {
        self.level = level;
        self
    }
}

impl Rich {
    /// One paragraph of runs: each a run or plain words.
    pub fn runs<R: Into<Run>>(runs: impl IntoIterator<Item = R>) -> Self {
        Self {
            runs: runs_of(runs),
            ..Self::default()
        }
    }

    /// A longer text in blocks: paragraphs, headings, quotes, items of lists
    /// and code.
    pub fn blocks(blocks: impl IntoIterator<Item = Block>) -> Self {
        Self {
            blocks: blocks.into_iter().collect(),
            ..Self::default()
        }
    }

    /// What the text is for.
    #[must_use]
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }

    /// The text is secondary.
    #[must_use]
    pub fn muted(mut self) -> Self {
        self.muted = true;
        self
    }
}

impl Image {
    /// The picture at `src`, and what it shows for a reader who can't see
    /// it. A picture that is decoration says nothing: `""`.
    pub fn new(src: impl Into<String>, alt: impl Into<String>) -> Self {
        Self {
            src: src.into(),
            alt: alt.into(),
            ..Self::default()
        }
    }

    /// Its width and its height, in points.
    #[must_use]
    pub fn size(mut self, width: u16, height: u16) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// The box is filled, and the picture's edges may be cut.
    #[must_use]
    pub fn cover(mut self) -> Self {
        self.fit = Some(Fit::Cover);
        self
    }

    /// All of the picture shows in its box.
    #[must_use]
    pub fn contain(mut self) -> Self {
        self.fit = Some(Fit::Contain);
        self
    }
}

impl Avatar {
    /// The picture of the person or the thing named `name`: its initials
    /// where there is no picture.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Where the picture is.
    #[must_use]
    pub fn src(mut self, src: impl Into<String>) -> Self {
        self.src = Some(src.into());
        self
    }

    /// The initials to draw, when they are not the first letters of the name.
    #[must_use]
    pub fn initials(mut self, initials: impl Into<String>) -> Self {
        self.initials = Some(initials.into());
        self
    }
}

impl Banner {
    /// A message across the screen that says `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::default()
        }
    }

    /// Its heading.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// What it means.
    #[must_use]
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = Some(tone);
        self
    }

    /// Its icon.
    #[must_use]
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Adds what a reader may do about it.
    #[must_use]
    pub fn action(mut self, action: impl IntoParts) -> Self {
        action.push_into(&mut self.actions);
        self
    }
}

impl Empty {
    /// What stands where there is nothing yet: `No orders yet`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// A line under the title.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Its icon.
    #[must_use]
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Adds what a reader may do about it.
    #[must_use]
    pub fn action(mut self, action: impl IntoParts) -> Self {
        action.push_into(&mut self.actions);
        self
    }
}

impl Code {
    /// Code, its lines kept as they are.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// The language it is written in: `rust`.
    #[must_use]
    pub fn lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = Some(lang.into());
        self
    }
}

impl Popover {
    /// A small panel anchored to what opened it. It draws nothing until an
    /// action opens it by its id.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            children: Vec::new(),
        }
    }

    /// Adds what it holds.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl NativeView {
    /// A view an app's own client code draws, by its name.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// What the view is given.
    #[must_use]
    pub fn props(mut self, props: serde_json::Value) -> Self {
        self.props = props;
        self
    }

    /// What stands in its place on a client that has no view of this name.
    #[must_use]
    pub fn alt(mut self, part: impl Into<Node>) -> Self {
        self.alt = Some(Box::new(part.into()));
        self
    }
}

impl WebRegion {
    /// Markup for a small region, drawn in a web view the height of its
    /// content: for what no part can say. A screen that holds one is not
    /// drawn by the system alone.
    pub fn new(html: impl Into<String>) -> Self {
        Self {
            id: None,
            html: html.into(),
        }
    }
}

impl Toolbar {
    /// Actions on a bar, none yet. A client places them by role.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an action.
    #[must_use]
    pub fn item(mut self, item: ToolbarItem) -> Self {
        self.items.push(item);
        self
    }
}

impl ToolbarItem {
    /// An action that says `title` and shows `icon`: a system shows one, the
    /// other or both, by where the item ends up.
    pub fn new(title: impl Into<String>, icon: Icon) -> Self {
        Self {
            title: title.into(),
            icon,
            ..Self::default()
        }
    }

    /// The standard action it is: the system's own symbol and words take the
    /// place of its own.
    #[must_use]
    pub fn standard(mut self, standard: Standard) -> Self {
        self.standard = Some(standard);
        self
    }

    /// What it means, which is where a client places it.
    #[must_use]
    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    /// How loud it is.
    #[must_use]
    pub fn prominence(mut self, prominence: Prominence) -> Self {
        self.prominence = prominence;
        self
    }

    /// What it does.
    #[must_use]
    pub fn action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    /// It goes to `href`.
    #[must_use]
    pub fn to(self, href: impl Into<String>) -> Self {
        self.action(Action::navigate(href))
    }

    /// It opens a menu of these buttons, links and dividers.
    #[must_use]
    pub fn menu(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.menu);
        self
    }

    /// It asks before it acts, in the system's own confirmation: the question,
    /// a line under it, and the words of the answer that goes ahead.
    #[must_use]
    pub fn ask(
        mut self,
        title: impl Into<String>,
        text: impl Into<String>,
        confirm: impl Into<String>,
    ) -> Self {
        self.ask = Some(Question {
            title: title.into(),
            text: Some(text.into()),
            confirm: confirm.into(),
            cancel: None,
        });
        self
    }

    /// What it does can be undone.
    #[must_use]
    pub fn undoable(mut self) -> Self {
        self.undoable = true;
        self
    }

    /// It can't be used now, and why, in words a person reads.
    #[must_use]
    pub fn unavailable(mut self, reason: impl Into<String>) -> Self {
        self.disabled = true;
        self.reason = Some(reason.into());
        self
    }
}

impl Message {
    /// A message of a conversation, written by `author`.
    pub fn by(author: impl Into<String>) -> Self {
        Self {
            author: author.into(),
            ..Self::default()
        }
    }

    /// The author's picture or initials.
    #[must_use]
    pub fn avatar(mut self, avatar: Avatar) -> Self {
        self.avatar = Some(avatar);
        self
    }

    /// It comes from the reader's side of the conversation.
    #[must_use]
    pub fn mine(mut self) -> Self {
        self.mine = true;
        self
    }

    /// It is a note only the team sees.
    #[must_use]
    pub fn note(mut self) -> Self {
        self.note = true;
        self
    }

    /// Adds to what it says: text, rich text, pictures.
    #[must_use]
    pub fn body(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.body);
        self
    }

    /// When it was written, in the screen's words.
    #[must_use]
    pub fn time(mut self, time: impl Into<String>) -> Self {
        self.time = Some(time.into());
        self
    }

    /// How far it has got.
    #[must_use]
    pub fn delivery(mut self, delivery: Delivery) -> Self {
        self.delivery = Some(delivery);
        self
    }
}

impl Table {
    /// Records in columns, none yet. A wide place draws a table; a narrow
    /// one draws each record as a row, from the columns' roles.
    pub fn new() -> Self {
        Self::default()
    }

    /// What the table lists.
    #[must_use]
    pub fn caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// Adds a column.
    #[must_use]
    pub fn column(mut self, column: Column) -> Self {
        self.columns.push(column);
        self
    }

    /// Adds records.
    #[must_use]
    pub fn rows(mut self, rows: impl IntoIterator<Item = TableRow>) -> Self {
        self.rows.extend(rows);
        self
    }

    /// The form that asks for other records: each of its fields says what it
    /// is for by its [`purpose`](Field::purpose).
    #[must_use]
    pub fn query(mut self, form: Form) -> Self {
        self.query = Some(form);
        self
    }

    /// The rows can be chosen, and this is how the chosen ones are sent.
    #[must_use]
    pub fn select(mut self, selection: Selection) -> Self {
        self.select = Some(selection);
        self
    }

    /// Where the next records are, by cursor.
    #[must_use]
    pub fn more(mut self, href: impl Into<String>) -> Self {
        self.more = Some(href.into());
        self
    }

    /// Where the records before these are.
    #[must_use]
    pub fn prev(mut self, href: impl Into<String>) -> Self {
        self.prev = Some(href.into());
        self
    }

    /// How many there are, in the screen's words: `1–25 of 1,204`.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// What stands in place of the records when there are none.
    #[must_use]
    pub fn empty(mut self, part: impl Into<Node>) -> Self {
        self.empty = Some(Box::new(part.into()));
        self
    }
}

impl Column {
    /// A column headed `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }

    /// The column's key, as the app knows it.
    #[must_use]
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Its cells are numbers: set at the end, in figures of one width.
    #[must_use]
    pub fn numeric(mut self) -> Self {
        self.numeric = true;
        self
    }

    /// How its cells are set.
    #[must_use]
    pub fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    /// The records can be put in its order: where that is asked, and the
    /// order they are in now, when it is this column's.
    #[must_use]
    pub fn sort(mut self, href: impl Into<String>, now: Option<SortDir>) -> Self {
        self.sort = Some(Sort {
            href: href.into(),
            dir: now,
        });
        self
    }

    /// The part its cell plays in a record drawn as a row.
    #[must_use]
    pub fn role(mut self, role: ColumnRole) -> Self {
        self.role = Some(role);
        self
    }

    /// A narrow place may leave it out.
    #[must_use]
    pub fn secondary(mut self) -> Self {
        self.secondary = true;
        self
    }
}

impl TableRow {
    /// A record: one cell a column, in the columns' order.
    pub fn new(cells: impl IntoParts) -> Self {
        Self {
            cells: list_of(cells),
            ..Self::default()
        }
    }

    /// A tap on the row goes to `href`.
    #[must_use]
    pub fn to(self, href: impl Into<String>) -> Self {
        self.action(Action::navigate(href))
    }

    /// What a tap on the row does.
    #[must_use]
    pub fn action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    /// Adds what a reader may do with the record: buttons and menus.
    #[must_use]
    pub fn actions(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.actions);
        self
    }

    /// The row is chosen.
    #[must_use]
    pub fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    /// The row can't be chosen: the actions on the rows chosen do not apply
    /// to it.
    #[must_use]
    pub fn unchoosable(mut self) -> Self {
        self.unchoosable = true;
        self
    }

    /// Adds to what opens under the row.
    #[must_use]
    pub fn detail(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.detail);
        self
    }

    /// Its detail shows now.
    #[must_use]
    pub fn open(mut self) -> Self {
        self.open = true;
        self
    }
}

impl Selection {
    /// The id of each chosen row is sent under `name`.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// The form the ids go with: the one whose buttons act on the rows
    /// chosen.
    #[must_use]
    pub fn form(mut self, form: impl Into<String>) -> Self {
        self.form = Some(form.into());
        self
    }

    /// The person may choose every record that matches, and not only the
    /// rows loaded: `name` is then sent with `value`, the server's own words
    /// for them all, and `total` is how many there are when it is counted.
    #[must_use]
    pub fn across(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
        total: Option<u32>,
    ) -> Self {
        self.across = Some(Across {
            name: name.into(),
            value: value.into(),
            total,
        });
        self
    }
}

impl List {
    /// Where the rows before the first are, by cursor: newer ones above a
    /// list of the newest first.
    #[must_use]
    pub fn before(mut self, href: impl Into<String>) -> Self {
        self.before = Some(href.into());
        self
    }

    /// The rows are numbered.
    #[must_use]
    pub fn ordered(mut self) -> Self {
        self.ordered = true;
        self
    }

    /// The rows can be moved by dragging, and this is where a move is posted.
    #[must_use]
    pub fn reorder(mut self, reorder: Reorder) -> Self {
        self.reorder = Some(reorder);
        self
    }
}

impl Reorder {
    /// A move is posted to `href`.
    pub fn to(href: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            ..Self::default()
        }
    }

    /// The list's own key, as the app knows it, which a move posts in place
    /// of the list's id.
    #[must_use]
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// The lists a row may move between: those of the same group.
    #[must_use]
    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// The most rows the list takes.
    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// A value that goes with each move.
    #[must_use]
    pub fn field(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.push(Pair {
            name: name.into(),
            value: value.into(),
        });
        self
    }
}

impl Row {
    /// The row's key, as the app knows it: what a move posts in place of
    /// its id.
    #[must_use]
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }
}

impl Button {
    /// Only its icon shows; its label is what assistive technology says.
    #[must_use]
    pub fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }

    /// It sends the form `form`, which it stands outside: the button at the
    /// foot of a dialog.
    #[must_use]
    pub fn submits_form(self, form: impl Into<String>) -> Self {
        self.action(Action::submit_form(form))
    }

    /// It is an item of a menu that turns something on or off, ticked or not.
    #[must_use]
    pub fn checked(mut self, on: bool) -> Self {
        self.checked = Some(on);
        self
    }

    /// The choice of one it belongs to, among the ticked items of a menu.
    #[must_use]
    pub fn one_of(mut self, group: impl Into<String>) -> Self {
        self.one_of = Some(group.into());
        self
    }

    /// It is waiting for an answer.
    #[must_use]
    pub fn pending(mut self) -> Self {
        self.pending = true;
        self
    }
}

impl Progress {
    /// Something is going on, and how far it has got is not known.
    pub fn busy() -> Self {
        Self::default()
    }
}

impl Fact {
    /// A fact: its label and the parts that are its value.
    pub fn new(label: impl Into<String>, value: impl IntoParts) -> Self {
        Self {
            label: label.into(),
            value: list_of(value),
            ..Self::default()
        }
    }

    /// Its icon.
    #[must_use]
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// What its value means.
    #[must_use]
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = Some(tone);
        self
    }

    /// Adds a quieter line under its value.
    #[must_use]
    pub fn detail(mut self, line: impl Into<String>) -> Self {
        self.details.push(line.into());
        self
    }
}

impl Facts {
    /// Adds a fact built on its own: one with an icon, a tone or a value
    /// that is more than words.
    #[must_use]
    pub fn item(mut self, fact: Fact) -> Self {
        self.items.push(fact);
        self
    }
}

impl Timeline {
    /// Adds a moment.
    #[must_use]
    pub fn moment(mut self, moment: Moment) -> Self {
        self.items.push(moment);
        self
    }
}

impl Moment {
    /// Adds to what the moment holds under its words.
    #[must_use]
    pub fn detail(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.detail);
        self
    }

    /// The moment is quieter than the ones beside it.
    #[must_use]
    pub fn muted(mut self) -> Self {
        self.muted = true;
        self
    }
}

impl FeedRow {
    /// Who did it, by picture or initials.
    #[must_use]
    pub fn actor(mut self, actor: Avatar) -> Self {
        self.actor = Some(actor);
        self
    }

    /// The row is quieter than the ones beside it.
    #[must_use]
    pub fn muted(mut self) -> Self {
        self.muted = true;
        self
    }
}

impl Sheet {
    /// A line under the title.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// It takes all of the screen.
    #[must_use]
    pub fn full(mut self) -> Self {
        self.size = SheetSize::Full;
        self
    }

    /// It is presented as the screen arrives.
    #[must_use]
    pub fn open(mut self) -> Self {
        self.open = true;
        self
    }
}

impl Dialog {
    /// It is presented as the screen arrives.
    #[must_use]
    pub fn open(mut self) -> Self {
        self.open = true;
        self
    }
}

impl Field {
    /// What is wrong with its value.
    #[must_use]
    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// It can't be changed now.
    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// What the field is for, in a form that asks for records: a client puts
    /// it where its system puts that.
    #[must_use]
    pub fn purpose(mut self, purpose: Purpose) -> Self {
        self.purpose = Some(purpose);
        self
    }

    /// What the client does when the person changes the value, besides
    /// holding it.
    #[must_use]
    pub fn changed(mut self, action: Action) -> Self {
        self.changed = Some(action);
        self
    }

    /// The form is sent when the person changes the value: a box that saves
    /// as it is ticked, a search that answers as it is typed.
    #[must_use]
    pub fn sends(self) -> Self {
        self.changed(Action::submit())
    }
}
