//! Navree Native UI: a screen for native clients, written once in Rust.
//!
//! A screen is written with the protocol's own parts. Each part is built with
//! a few plain methods, a component of an app is a function that returns a
//! part, and the screen that comes out is the [`Screen`] a client is sent:
//! SwiftUI draws it on Apple systems and Jetpack Compose on Android, each as
//! its system draws such a thing. Nothing here knows a request, a database or
//! HTML: a screen is a function of its data.
//!
//! There are three levels, and a screen mixes them freely:
//!
//! 1. **Structure the system places.** [`Screen`] with its title, its way
//!    back, its actions by [`Role`] and its day [`pager`](Screen::pager);
//!    [`Section`], [`List`], [`Row`], [`Form`], [`Field`], [`Sheet`],
//!    [`Dialog`], [`Region`]. A part of this level says what it is, and the
//!    client decides where it goes.
//! 2. **Content the system styles.** [`Figure`], [`Ring`], [`Rings`],
//!    [`Chart`], [`DayClock`], [`Timeline`], [`FeedRow`], [`Badge`],
//!    [`Progress`], [`Facts`]. A part of this level carries its data and its
//!    words, and each client draws it in its system's manner.
//! 3. **Free composition**, for a screen that deserves a design of its own:
//!    [`Stack`], [`Grid`], [`Card`], [`Layers`], [`Shape`], [`Carousel`],
//!    [`Adaptive`], [`Text`] in the system's styles, [`Icon`] as a symbol.
//!    See [`compose`](crate::compose) for what a composition may say.
//!
//! Reach for the third level only where the first two can't make the screen:
//! a list of records is a [`List`], whatever a design would like it to be.
//!
//! ```
//! use navree_native::ui::*;
//!
//! /// A tile of a dashboard: an app's own component is a function.
//! fn tile(name: &str, value: &str, hue: Hue) -> Card {
//!     Card::new().tint(hue).parts((
//!         Text::new(name).style(TextStyle::Caption).weight(Weight::Semibold).muted(),
//!         Text::new(value).style(TextStyle::Title2).tabular(),
//!     ))
//! }
//!
//! let screen = Screen::titled("Today")
//!     .refresh()
//!     .action(Button::new("Orders").role(Role::Secondary).to("/orders"))
//!     .body(Stack::down().gap(4).parts((
//!         Text::new("₹1,24,050").style(TextStyle::Display).change(Change::Count),
//!         Grid::fit(150).gap(3).parts([
//!             tile("To pack", "12", Hue::Amber),
//!             tile("To ship", "7", Hue::Blue),
//!         ]),
//!     )));
//! assert_eq!(screen.title, "Today");
//! let Node::Stack(body) = &screen.body else { panic!("the body is the stack") };
//! assert_eq!(body.children.len(), 2);
//! ```

use crate::action::Command;
use crate::node::{Hidden, Live, Method, Question, Run, SystemSymbol, Trend};

pub use crate::action::{Action, Pair, Present};
pub use crate::charts::{
    Category, Centre, ClockMoment, Colour, DayClock, DayPager, DayPick, Hint, HintMark, Hour, Hue,
    PagerWords, Ring, RingValue, Rings, RingsCentre,
};
pub use crate::compose::{
    Adaptive, Appear, Card, Carousel, Change, Layers, Place, Shape, ShapeForm, Size, Transition,
    Weight,
};
pub use crate::envelope::{Back, Screen, Search};
pub use crate::input::{ChoiceInput, ChoiceKind, ChoiceOption, Input, TextInput, TextKind};
pub use crate::node::{
    Align, Avatar, Axis, Badge, Banner, Button, Chart, ChartKind, Dialog, Direction, Divider,
    Empty, Fact, Facts, FeedRow, Field, Figure, Form, Grid, Icon, Justify, Link, List, Menu,
    Moment, Node, Progress, Prominence, Region, Role, Row, RowState, Section, SectionStyle, Series,
    Sheet, SheetSize, Spacer, Stack, Standard, Text, TextStyle, Timeline, Tone,
};
pub use crate::op::Op;

/// What can stand where parts are asked for: one part, a tuple of parts, a
/// list or an array of them, an `Option`, and what an iterator's `map`,
/// `filter_map`, `flat_map`, `chain` and `take` give. Any other iterator goes
/// through [`each`].
///
/// ```
/// use navree_native::ui::*;
///
/// let cities = ["Ludhiana", "Amritsar", "Jalandhar"];
/// let stack = Stack::down().parts((
///     Text::new("Ships to").style(TextStyle::Headline),
///     cities.iter().map(|city| Text::new(*city)),
///     cities.is_empty().then(|| Text::new("Nowhere yet").muted()),
/// ));
/// assert_eq!(stack.children.len(), 4);
/// ```
pub trait IntoParts {
    /// Adds the parts to `out`, in their order.
    fn push_into(self, out: &mut Vec<Node>);
}

impl<T: Into<Node>> IntoParts for T {
    #[inline]
    fn push_into(self, out: &mut Vec<Node>) {
        out.push(self.into());
    }
}

impl<T: IntoParts> IntoParts for Vec<T> {
    fn push_into(self, out: &mut Vec<Node>) {
        out.reserve(self.len());
        for item in self {
            item.push_into(out);
        }
    }
}

impl<T: IntoParts, const N: usize> IntoParts for [T; N] {
    fn push_into(self, out: &mut Vec<Node>) {
        out.reserve(N);
        for item in self {
            item.push_into(out);
        }
    }
}

impl<T: IntoParts> IntoParts for Option<T> {
    fn push_into(self, out: &mut Vec<Node>) {
        if let Some(item) = self {
            item.push_into(out);
        }
    }
}

/// The parts an iterator gives, where parts are asked for: see [`each`].
#[derive(Clone, Debug)]
pub struct Each<I>(I);

/// The parts `items` gives, one after another, for an iterator that is not
/// one of those [`IntoParts`] takes as it is.
///
/// ```
/// use navree_native::ui::*;
///
/// let rows = ["Placed", "Paid", "Packed"];
/// let list = List::new().parts(each(rows.iter().rev().map(|step| Row::new(*step))));
/// assert_eq!(list.children.len(), 3);
/// ```
pub fn each<I>(items: I) -> Each<I::IntoIter>
where
    I: IntoIterator,
    I::Item: IntoParts,
{
    Each(items.into_iter())
}

impl<I> IntoParts for Each<I>
where
    I: Iterator,
    I::Item: IntoParts,
{
    fn push_into(self, out: &mut Vec<Node>) {
        for item in self.0 {
            item.push_into(out);
        }
    }
}

macro_rules! iterators {
    ($(impl<$($param:ident),+> for $ty:ty { $($bound:tt)+ })+) => {
        $(
            impl<$($param),+> IntoParts for $ty
            where
                $($bound)+
            {
                fn push_into(self, out: &mut Vec<Node>) {
                    for item in self {
                        item.push_into(out);
                    }
                }
            }
        )+
    };
}

iterators! {
    impl<I, F, T> for std::iter::Map<I, F> { I: Iterator, F: FnMut(I::Item) -> T, T: IntoParts }
    impl<I, F, T> for std::iter::FilterMap<I, F> { I: Iterator, F: FnMut(I::Item) -> Option<T>, T: IntoParts }
    impl<I, U, F> for std::iter::FlatMap<I, U, F> { I: Iterator, U: IntoIterator, F: FnMut(I::Item) -> U, U::Item: IntoParts }
    impl<A, B> for std::iter::Chain<A, B> { A: Iterator, B: Iterator<Item = A::Item>, A::Item: IntoParts }
    impl<I> for std::iter::Take<I> { I: Iterator, I::Item: IntoParts }
}

macro_rules! tuples {
    ($($name:ident)+) => {
        impl<$($name: IntoParts),+> IntoParts for ($($name,)+) {
            #[allow(non_snake_case)]
            fn push_into(self, out: &mut Vec<Node>) {
                let ($($name,)+) = self;
                $($name.push_into(out);)+
            }
        }
    };
}

tuples!(A);
tuples!(A B);
tuples!(A B C);
tuples!(A B C D);
tuples!(A B C D E);
tuples!(A B C D E F);
tuples!(A B C D E F G);
tuples!(A B C D E F G H);
tuples!(A B C D E F G H I);
tuples!(A B C D E F G H I J);
tuples!(A B C D E F G H I J K);
tuples!(A B C D E F G H I J K L);

/// The parts of `parts`, as a list.
fn list_of(parts: impl IntoParts) -> Vec<Node> {
    let mut out = Vec::new();
    parts.push_into(&mut out);
    out
}

macro_rules! kinds {
    ($($variant:ident($ty:ty),)+) => {
        $(
            impl From<$ty> for Node {
                #[inline]
                fn from(part: $ty) -> Self {
                    Node::$variant(part)
                }
            }
        )+
    };
}

kinds! {
    Stack(Stack), Grid(Grid), Section(Section), List(List), Row(Row), Divider(Divider),
    Spacer(Spacer), Region(Region), Text(Text), Icon(Icon), Avatar(Avatar), Badge(Badge),
    Progress(Progress), Banner(Banner), Empty(Empty), Facts(Facts), Chart(Chart),
    Button(Button), Link(Link), Menu(Menu), Form(Form), Field(Field), Sheet(Sheet),
    Dialog(Dialog), Figure(Figure), Timeline(Timeline), FeedRow(FeedRow), Ring(Ring),
    Rings(Rings), DayClock(DayClock), DayPager(DayPager), Card(Card), Layers(Layers),
    Shape(Shape), Carousel(Carousel), Adaptive(Adaptive),
    Table(crate::node::Table), Tabs(crate::node::Tabs), Rich(crate::node::Rich),
    Image(crate::node::Image), Gauge(crate::node::Gauge), Toolbar(crate::node::Toolbar),
    Message(crate::node::Message), Pane(crate::node::Pane), Disclosure(crate::node::Disclosure),
    Code(crate::node::Code), Popover(crate::node::Popover), Meter(crate::charts::Meter),
    Columns(crate::charts::MiniColumns), Strip(crate::charts::ColorStrip),
    Range(crate::charts::RangeScale), CalendarHeatmap(crate::charts::CalendarHeatmap),
    MatrixHeatmap(crate::charts::MatrixHeatmap), Scatter(crate::charts::Scatter),
}

/// A number as a part holds it, from the number types a screen's data comes
/// in. A fraction that is not a number (infinite, or none at all) is 0.
pub trait IntoNumber {
    /// The number.
    fn into_number(self) -> serde_json::Number;
}

macro_rules! whole {
    ($($ty:ty),+) => {
        $(impl IntoNumber for $ty {
            fn into_number(self) -> serde_json::Number {
                self.into()
            }
        })+
    };
}

whole!(i8, i16, i32, i64, u8, u16, u32, u64, usize);

impl IntoNumber for f64 {
    fn into_number(self) -> serde_json::Number {
        // A fraction with nothing after the point is written whole: `7`, not `7.0`.
        if self.fract() == 0.0 && self.abs() < 9.0e15 {
            (self as i64).into()
        } else {
            serde_json::Number::from_f64(self).unwrap_or_else(|| 0.into())
        }
    }
}

impl IntoNumber for serde_json::Number {
    fn into_number(self) -> serde_json::Number {
        self
    }
}

macro_rules! named {
    ($($ty:ty),+) => {
        $(impl $ty {
            /// Names the part, so an answer can change it and a command can
            /// find it.
            #[must_use]
            pub fn id(mut self, id: impl Into<String>) -> Self {
                self.id = Some(id.into());
                self
            }
        })+
    };
}

named!(
    Stack, Grid, Section, List, Row, Text, Icon, Badge, Button, Menu, Form, Field, Figure,
    Timeline, FeedRow, Ring, Rings, DayClock, DayPager, Card, Layers, Shape, Carousel, Adaptive,
    Chart, Facts, Progress
);

impl Action {
    /// Asks for `href` and applies what it answers: the row that opens a
    /// record in a sheet the server fills.
    pub fn request(href: impl Into<String>) -> Self {
        Self::Request {
            method: Method::Get,
            href: href.into(),
            fields: Vec::new(),
        }
    }

    /// Posts `fields` to `href`: a button that acts at once.
    pub fn post<N, V>(href: impl Into<String>, fields: impl IntoIterator<Item = (N, V)>) -> Self
    where
        N: Into<String>,
        V: Into<String>,
    {
        Self::Request {
            method: Method::Post,
            href: href.into(),
            fields: fields
                .into_iter()
                .map(|(name, value)| Pair {
                    name: name.into(),
                    value: value.into(),
                })
                .collect(),
        }
    }

    /// Goes to `href`, presented as said and not as the shell's routes say.
    pub fn present(href: impl Into<String>, present: Present) -> Self {
        Self::Navigate {
            href: href.into(),
            present,
        }
    }

    /// Presents the sheet or the dialog `id` of this screen.
    pub fn open(id: impl Into<String>) -> Self {
        Self::command(Command::Open { id: id.into() })
    }
}

impl Screen {
    /// A screen named `title`, with nothing in it yet.
    pub fn titled(title: impl Into<String>) -> Self {
        Self::new(title, Node::Stack(Stack::default()))
    }

    /// A line under the screen's name.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// The screen before this one in the app's own order.
    #[must_use]
    pub fn back(mut self, title: impl Into<String>, href: impl Into<String>) -> Self {
        self.back = Some(Back {
            title: title.into(),
            href: href.into(),
        });
        self
    }

    /// The address of the app's destination the screen belongs to, when it is
    /// not the one its own address is under.
    #[must_use]
    pub fn nav(mut self, href: impl Into<String>) -> Self {
        self.nav = Some(href.into());
        self
    }

    /// Adds what a reader may do with the screen as a whole. Each has a
    /// [`Role`], and the client places it by that.
    #[must_use]
    pub fn action(mut self, action: impl IntoParts) -> Self {
        action.push_into(&mut self.actions);
        self
    }

    /// Pulling down fetches the screen again.
    #[must_use]
    pub fn refresh(mut self) -> Self {
        self.refresh = true;
        self
    }

    /// The control that steps the screen from day to day.
    #[must_use]
    pub fn pager(mut self, pager: DayPager) -> Self {
        self.pager = Some(pager);
        self
    }

    /// The screen's content: one part, or several one under another.
    #[must_use]
    pub fn body(mut self, parts: impl IntoParts) -> Self {
        let mut parts = list_of(parts);
        self.body = match parts.len() {
            1 => parts.remove(0),
            _ => Node::Stack(Stack {
                children: parts,
                ..Stack::default()
            }),
        };
        self.id = self.body.id().map(str::to_owned);
        self
    }
}

impl Stack {
    /// Parts one under another.
    pub fn down() -> Self {
        Self::default()
    }

    /// Parts side by side.
    pub fn across() -> Self {
        Self {
            axis: Axis::Horizontal,
            ..Self::default()
        }
    }

    /// The room between parts, in steps of the system's spacing scale.
    #[must_use]
    pub fn gap(mut self, steps: u8) -> Self {
        self.gap = steps;
        self
    }

    /// The room round the parts, in steps of the system's spacing scale.
    #[must_use]
    pub fn pad(mut self, steps: u8) -> Self {
        self.pad = steps;
        self
    }

    /// Where parts sit across the axis.
    #[must_use]
    pub fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    /// How the room along the axis is shared.
    #[must_use]
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = Some(justify);
        self
    }

    /// Parts that do not fit go to a new line.
    #[must_use]
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }

    /// It takes the room left along the stack it stands in.
    #[must_use]
    pub fn grow(mut self) -> Self {
        self.grow = true;
        self
    }

    /// The axis it runs along in a wide room.
    #[must_use]
    pub fn wide(mut self, axis: Axis) -> Self {
        self.wide = Some(axis);
        self
    }

    /// How it comes in when it joins a screen already drawn.
    #[must_use]
    pub fn appear(mut self, appear: Appear) -> Self {
        self.appear = Some(appear);
        self
    }

    /// Adds parts.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl Grid {
    /// As many columns as fit, each at least `min` points wide: two on a
    /// phone, more in a wider room.
    pub fn fit(min: u16) -> Self {
        Self {
            min: Some(min),
            ..Self::default()
        }
    }

    /// `cols` columns in the widest room, fewer in a narrower one.
    pub fn cols(cols: u8) -> Self {
        Self {
            cols: Some(cols),
            ..Self::default()
        }
    }

    /// The most columns, however wide the room.
    #[must_use]
    pub fn most(mut self, cols: u8) -> Self {
        self.cols = Some(cols);
        self
    }

    /// The room between parts, in steps of the system's spacing scale.
    #[must_use]
    pub fn gap(mut self, steps: u8) -> Self {
        self.gap = steps;
        self
    }

    /// Adds parts.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl Section {
    /// A group of parts with no heading.
    pub fn new() -> Self {
        Self::default()
    }

    /// A group of parts under a heading.
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::default()
        }
    }

    /// A line under the heading.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Set off as the system sets off a group of settings or of a record's
    /// facts.
    #[must_use]
    pub fn inset(mut self) -> Self {
        self.style = SectionStyle::Inset;
        self
    }

    /// Set off as a card.
    #[must_use]
    pub fn card(mut self) -> Self {
        self.style = SectionStyle::Card;
        self
    }

    /// Adds what a reader may do with the whole section, beside its heading.
    #[must_use]
    pub fn action(mut self, action: impl IntoParts) -> Self {
        action.push_into(&mut self.actions);
        self
    }

    /// Adds what stands under the parts.
    #[must_use]
    pub fn footer(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.footer);
        self
    }

    /// A note about what the section shows, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Adds parts.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl List {
    /// Rows, none yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds rows.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }

    /// Where the next rows are, by cursor: the list goes on, and the client
    /// asks when its end is near.
    #[must_use]
    pub fn more(mut self, href: impl Into<String>) -> Self {
        self.more = Some(href.into());
        self
    }

    /// Kept current: the rows are at the revision `since`, what changed is
    /// asked at `changes`, and the stream `stream` says when with its event
    /// `event`. Every row of such a list has an id.
    #[must_use]
    pub fn live(
        mut self,
        since: impl Into<String>,
        changes: impl Into<String>,
        stream: impl Into<String>,
        event: impl Into<String>,
    ) -> Self {
        self.since = Some(since.into());
        self.changes = Some(changes.into());
        self.live = Some(Live {
            src: stream.into(),
            event: Some(event.into()),
        });
        self
    }
}

impl Row {
    /// A row named `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// A line under the name.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// What stands before the words: an icon, an avatar, a picture.
    #[must_use]
    pub fn leading(mut self, part: impl Into<Node>) -> Self {
        self.leading = Some(Box::new(part.into()));
        self
    }

    /// What stands after the words: a value, a time.
    #[must_use]
    pub fn trailing(mut self, part: impl Into<Node>) -> Self {
        self.trailing = Some(Box::new(part.into()));
        self
    }

    /// A value after the words, as quiet text.
    #[must_use]
    pub fn value(self, value: impl Into<String>) -> Self {
        self.trailing(Text::new(value).muted())
    }

    /// A badge beside the name.
    #[must_use]
    pub fn badge(mut self, badge: Badge) -> Self {
        self.badge = Some(badge);
        self
    }

    /// Where the row stands in a sequence.
    #[must_use]
    pub fn state(mut self, state: RowState) -> Self {
        self.state = Some(state);
        self
    }

    /// A tap goes to `href`.
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

    /// Adds what a swipe on the row offers.
    #[must_use]
    pub fn swipe(mut self, button: Button) -> Self {
        self.swipe.push(button);
        self
    }

    /// Adds what a long press or a secondary click offers.
    #[must_use]
    pub fn menu(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.menu);
        self
    }

    /// Adds parts under the two lines.
    #[must_use]
    pub fn detail(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.detail);
        self
    }

    /// Drawn quieter than its neighbours.
    #[must_use]
    pub fn muted(mut self) -> Self {
        self.muted = true;
        self
    }
}

impl Region {
    /// A part named `id` that fetches itself from `src`.
    pub fn new(id: impl Into<String>, src: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            src: src.into(),
            ..Self::default()
        }
    }

    /// Fetched once the screen is drawn, because it came empty.
    #[must_use]
    pub fn lazy(mut self) -> Self {
        self.lazy = true;
        self
    }

    /// Fetched again whenever the stream `stream` sends its event `event`.
    #[must_use]
    pub fn live(mut self, stream: impl Into<String>, event: impl Into<String>) -> Self {
        self.live = Some(Live {
            src: stream.into(),
            event: Some(event.into()),
        });
        self
    }

    /// What it holds now.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl Text {
    /// Text that says `value`, in the body style.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            ..Self::default()
        }
    }

    /// What the text is for: its style in the system's own type.
    #[must_use]
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }

    /// What its colour means.
    #[must_use]
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = Some(tone);
        self
    }

    /// Secondary: drawn quieter than the text around it.
    #[must_use]
    pub fn muted(mut self) -> Self {
        self.muted = true;
        self
    }

    /// The most lines it takes before it is cut short.
    #[must_use]
    pub fn lines(mut self, lines: u8) -> Self {
        self.lines = Some(lines);
        self
    }

    /// It is a heading of this level, from 1, for assistive technology.
    #[must_use]
    pub fn heading(mut self, level: u8) -> Self {
        self.heading = Some(level);
        self
    }

    /// How heavy it is set.
    #[must_use]
    pub fn weight(mut self, weight: Weight) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Its colour by hue.
    #[must_use]
    pub fn colour(mut self, colour: impl Into<Colour>) -> Self {
        self.colour = Some(colour.into());
        self
    }

    /// Where its lines sit in the room it has.
    #[must_use]
    pub fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    /// Its figures are all one width.
    #[must_use]
    pub fn tabular(mut self) -> Self {
        self.tabular = true;
        self
    }

    /// How it shows that its words changed. A number that counts has figures
    /// of one width.
    #[must_use]
    pub fn change(mut self, change: Change) -> Self {
        self.tabular |= change == Change::Count;
        self.change = Some(change);
        self
    }
}

impl Icon {
    /// The icon `name` of the toolkit's set, with the system symbol that
    /// means the same where there is one. Its own shapes are added by the
    /// server that answers with it.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..Self::default()
        }
        .with_symbol()
    }

    /// The system's own symbols it is drawn with: its name in SF Symbols and
    /// its name in Material Symbols.
    #[must_use]
    pub fn system(mut self, apple: impl Into<String>, material: impl Into<String>) -> Self {
        self.symbol = Some(SystemSymbol {
            apple: apple.into(),
            apple_filled: None,
            material: material.into(),
        });
        self
    }

    /// Adds a shape a client strokes when it has no symbol for the icon, as
    /// SVG path data in a box of 24: for an icon that is not in the toolkit's
    /// set, whose shapes a server would not know.
    #[must_use]
    pub fn path(mut self, data: impl Into<String>) -> Self {
        self.paths.push(data.into());
        self
    }

    /// What the icon says, when it stands alone.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// What its colour means.
    #[must_use]
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = Some(tone);
        self
    }

    /// The text style it is as large as.
    #[must_use]
    pub fn scale(mut self, style: TextStyle) -> Self {
        self.scale = Some(style);
        self
    }

    /// Its colour by hue.
    #[must_use]
    pub fn colour(mut self, colour: impl Into<Colour>) -> Self {
        self.colour = Some(colour.into());
        self
    }
}

/// `icon` in the colour `hue` on a soft circle of the same hue: the mark at
/// the head of a row or a tile. It grows with the person's text size.
///
/// ```
/// use navree_native::ui::*;
///
/// let mark = chip(Icon::named("package"), Hue::Amber);
/// assert_eq!(mark.children.len(), 2);
/// assert!(matches!(&mark.children[0], Node::Shape(shape) if shape.soft));
/// ```
pub fn chip(icon: Icon, hue: Hue) -> Layers {
    Layers::new()
        .size(Size::square(40).scaled())
        .parts((Shape::circle().fill(hue).soft(), icon.colour(hue)))
}

impl Badge {
    /// A short label that says `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// What its colour means.
    #[must_use]
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = Some(tone);
        self
    }

    /// A dot in the tone's colour stands before the words.
    #[must_use]
    pub fn dot(mut self) -> Self {
        self.dot = true;
        self
    }
}

impl Progress {
    /// `done` of `whole`.
    pub fn of(done: f64, whole: f64) -> Self {
        let share = if whole > 0.0 {
            (done / whole).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Self {
            value: Some((share * 1000.0).round() as u16),
            ..Self::default()
        }
    }

    /// What is progressing.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl Button {
    /// A button that says `label`.
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

    /// What it means: the client places a screen's actions by this.
    #[must_use]
    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    /// The standard action it is: the system's own symbol and words are drawn
    /// for it.
    #[must_use]
    pub fn standard(mut self, standard: Standard) -> Self {
        self.standard = Some(standard);
        self
    }

    /// How loud it is among its neighbours.
    #[must_use]
    pub fn prominence(mut self, prominence: Prominence) -> Self {
        self.prominence = prominence;
        self
    }

    /// A tap goes to `href`.
    #[must_use]
    pub fn to(self, href: impl Into<String>) -> Self {
        self.action(Action::navigate(href))
    }

    /// A tap posts `fields` to `href`.
    #[must_use]
    pub fn post<N, V>(
        self,
        href: impl Into<String>,
        fields: impl IntoIterator<Item = (N, V)>,
    ) -> Self
    where
        N: Into<String>,
        V: Into<String>,
    {
        self.action(Action::post(href, fields))
    }

    /// A tap presents the sheet or the dialog `id` of this screen.
    #[must_use]
    pub fn opens(self, id: impl Into<String>) -> Self {
        self.action(Action::open(id))
    }

    /// A tap sends the form it stands in.
    #[must_use]
    pub fn submits(self) -> Self {
        self.action(Action::submit())
    }

    /// A tap closes the sheet or the dialog it stands in.
    #[must_use]
    pub fn dismisses(self) -> Self {
        self.action(Action::Dismiss)
    }

    /// What it does.
    #[must_use]
    pub fn action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    /// The question it asks before it acts: `title`, and the words of the
    /// button that goes ahead.
    #[must_use]
    pub fn ask(
        mut self,
        title: impl Into<String>,
        text: Option<String>,
        confirm: impl Into<String>,
    ) -> Self {
        self.ask = Some(Question {
            title: title.into(),
            text,
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

impl Link {
    /// A link that says `label` and does `action`.
    pub fn new(label: impl Into<String>, action: Action) -> Self {
        Self {
            id: None,
            label: label.into(),
            action,
            children: Vec::new(),
        }
    }

    /// A link that says `label` and goes to `href`.
    pub fn to(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self::new(label, Action::navigate(href))
    }

    /// What it holds, when that is more than its words: any parts, tapped as
    /// one.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl Menu {
    /// A button that says `label` and opens a list of actions.
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

    /// Adds items: buttons, links and dividers.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.items);
        self
    }
}

impl Form {
    /// A form posted to `action`: the answer is ops, and what is wrong with a
    /// field comes back at the field. A form with an id is the one an answer's
    /// errors name.
    pub fn post(action: impl Into<String>) -> Self {
        Self {
            method: Method::Post,
            action: action.into(),
            ..Self::default()
        }
    }

    /// A form sent as a query to `action`: the answer is a screen.
    pub fn get(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            ..Self::default()
        }
    }

    /// A value it sends that the reader never sees.
    #[must_use]
    pub fn hidden(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.hidden.push(Hidden {
            name: name.into(),
            value: value.into(),
            id: None,
        });
        self
    }

    /// Adds fields and whatever stands among them.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl Field {
    /// A field sent as `name`, with the control `input`.
    pub fn new(name: impl Into<String>, input: Input) -> Self {
        Self {
            name: name.into(),
            input,
            ..Self::default()
        }
    }

    /// A field of plain text.
    pub fn text(name: impl Into<String>) -> Self {
        Self::new(name, Input::Text(TextInput::default()))
    }

    /// A field for an amount: a number that may have a fraction, typed as
    /// plain digits. `prefix` stands before it (`₹`).
    pub fn amount(name: impl Into<String>, prefix: impl Into<String>) -> Self {
        Self::new(
            name,
            Input::Text(TextInput {
                kind: TextKind::Decimal,
                prefix: Some(prefix.into()),
                ..TextInput::default()
            }),
        )
    }

    /// A field that takes one of `options`, each a value and its words, drawn
    /// as the system draws a choice of `kind`.
    pub fn choice<V, L>(
        name: impl Into<String>,
        kind: ChoiceKind,
        options: impl IntoIterator<Item = (V, L)>,
    ) -> Self
    where
        V: Into<String>,
        L: Into<String>,
    {
        Self::new(
            name,
            Input::Choice(ChoiceInput {
                kind,
                options: options
                    .into_iter()
                    .map(|(value, label)| ChoiceOption {
                        value: value.into(),
                        label: label.into(),
                        ..ChoiceOption::default()
                    })
                    .collect(),
                ..ChoiceInput::default()
            }),
        )
    }

    /// Its label.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// A line of help under it.
    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// It must be filled.
    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// The value it starts with: the text of a text field, the chosen value
    /// of a choice.
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        match &mut self.input {
            Input::Text(text) => text.value = value.into(),
            Input::Choice(choice) => choice.value = Some(value.into()),
            _ => {}
        }
        self
    }
}

impl Dialog {
    /// A dialog named `id`. It draws nothing until it is opened.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Self::default()
        }
    }

    /// What it asks or says.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// More about it.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// It interrupts to ask before something that can't be undone.
    #[must_use]
    pub fn alert(mut self) -> Self {
        self.alert = true;
        self
    }

    /// What it holds besides its words: a form, most often.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }

    /// Adds its answers, each with its role.
    #[must_use]
    pub fn action(mut self, action: impl IntoParts) -> Self {
        action.push_into(&mut self.actions);
        self
    }
}

impl Sheet {
    /// A sheet named `id`. It draws nothing until it is opened.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Self::default()
        }
    }

    /// Its title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// It takes about half the screen.
    #[must_use]
    pub fn medium(mut self) -> Self {
        self.size = SheetSize::Medium;
        self
    }

    /// It is a task the person finishes or cancels, not a glance.
    #[must_use]
    pub fn task(mut self) -> Self {
        self.task = true;
        self
    }

    /// What it holds.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }

    /// Adds the actions at its foot, each with its role.
    #[must_use]
    pub fn action(mut self, action: impl IntoParts) -> Self {
        action.push_into(&mut self.actions);
        self
    }
}

impl Figure {
    /// A key figure: what it is, and its value in the screen's own words.
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            ..Self::default()
        }
    }

    /// What the value counts, written after it and smaller.
    #[must_use]
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// An icon before the label, in the hue `hue`.
    #[must_use]
    pub fn icon(mut self, icon: Icon, hue: Hue) -> Self {
        self.icon = Some(icon);
        self.colour = Some(Colour::new(hue));
        self
    }

    /// Which way it is going, by how much in words, and whether that is good.
    #[must_use]
    pub fn trend(mut self, direction: Direction, text: impl Into<String>, good: bool) -> Self {
        self.trend = Some(Trend {
            direction,
            text: Some(text.into()),
            tone: Some(if good { Tone::Success } else { Tone::Danger }),
        });
        self
    }

    /// A line under it: what it is measured against.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// A word about it at the end of its label's line, with its tone.
    #[must_use]
    pub fn status(mut self, status: Badge) -> Self {
        self.status = Some(status);
        self
    }

    /// A small graphic beside it: a ring.
    #[must_use]
    pub fn aside(mut self, part: impl Into<Node>) -> Self {
        self.aside = Some(Box::new(part.into()));
        self
    }

    /// A small chart under it: a sparkline of its last periods.
    #[must_use]
    pub fn chart(mut self, chart: Chart) -> Self {
        self.chart = Some(Box::new(Node::Chart(chart)));
        self
    }

    /// A tap goes to `href`: the page the figure is about.
    #[must_use]
    pub fn to(mut self, href: impl Into<String>) -> Self {
        self.action = Some(Action::navigate(href));
        self
    }
}

impl Chart {
    /// A chart of `kind` that shows `title`.
    pub fn new(kind: ChartKind, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: Some(title.into()),
            ..Self::default()
        }
    }

    /// A small line with no axes, named `title` for a reader who can't see
    /// it, through `values`.
    pub fn sparkline<V: IntoNumber>(
        title: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        let title = title.into();
        Self::new(ChartKind::Sparkline, title.clone()).series(title, values)
    }

    /// The labels along its axis, or of its shares.
    #[must_use]
    pub fn labels<L: Into<String>>(mut self, labels: impl IntoIterator<Item = L>) -> Self {
        self.labels = labels.into_iter().map(Into::into).collect();
        self
    }

    /// Adds a series named `label`, a value a label.
    #[must_use]
    pub fn series<V: IntoNumber>(
        mut self,
        label: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        self.series.push(Series {
            label: label.into(),
            values: values
                .into_iter()
                .map(|value| Some(value.into_number()))
                .collect(),
            ..Series::default()
        });
        self
    }

    /// The values of the series added last in the screen's own words
    /// (`₹1,24,050`), one a label.
    #[must_use]
    pub fn shown<S: Into<String>>(mut self, words: impl IntoIterator<Item = S>) -> Self {
        if let Some(series) = self.series.last_mut() {
            series.shown = words.into_iter().map(Into::into).collect();
        }
        self
    }

    /// The colour of the series added last.
    #[must_use]
    pub fn colour(mut self, colour: impl Into<Colour>) -> Self {
        if let Some(series) = self.series.last_mut() {
            series.colour = Some(colour.into());
        }
        self
    }

    /// One sentence that says the whole chart to a reader who can't see it.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }
}

impl Facts {
    /// Labels with their values, none yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a fact: what the value is, and the value in words.
    #[must_use]
    pub fn fact(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.items.push(Fact {
            label: label.into(),
            value: vec![Node::text(value)],
            ..Fact::default()
        });
        self
    }
}

impl Timeline {
    /// Moments in order, none yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds moments, the earliest first.
    #[must_use]
    pub fn moments(mut self, moments: impl IntoIterator<Item = Moment>) -> Self {
        self.items.extend(moments);
        self
    }
}

impl Moment {
    /// What happened, or will.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// A line under it.
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// When, in the screen's own words.
    #[must_use]
    pub fn time(mut self, time: impl Into<String>) -> Self {
        self.time = Some(time.into());
        self
    }

    /// Whether it has happened, is happening or is to come.
    #[must_use]
    pub fn state(mut self, state: RowState) -> Self {
        self.state = Some(state);
        self
    }

    /// Its icon, and what its colour means.
    #[must_use]
    pub fn icon(mut self, icon: Icon, tone: Tone) -> Self {
        self.icon = Some(icon);
        self.tone = Some(tone);
        self
    }
}

impl FeedRow {
    /// An entry of a feed that says `who` did `what` to `which`: the name and
    /// the thing stand out.
    pub fn new(who: impl Into<String>, what: impl Into<String>, which: impl Into<String>) -> Self {
        let bold = |text: String| Run {
            text,
            bold: true,
            ..Run::default()
        };
        let plain = |text: String| Run {
            text,
            ..Run::default()
        };
        let which = which.into();
        let mut runs = vec![bold(who.into()), plain(format!(" {}", what.into()))];
        if !which.is_empty() {
            runs.push(plain(" ".to_owned()));
            runs.push(bold(which));
        }
        Self {
            runs,
            ..Self::default()
        }
    }

    /// What kind of thing happened, and what its colour means.
    #[must_use]
    pub fn icon(mut self, icon: Icon, tone: Tone) -> Self {
        self.icon = Some(icon);
        self.tone = Some(tone);
        self
    }

    /// When, in the screen's own words.
    #[must_use]
    pub fn time(mut self, time: impl Into<String>) -> Self {
        self.time = Some(time.into());
        self
    }

    /// Adds parts under it: the facts that matter.
    #[must_use]
    pub fn detail(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.detail);
        self
    }

    /// A tap goes to `href`.
    #[must_use]
    pub fn to(mut self, href: impl Into<String>) -> Self {
        self.action = Some(Action::navigate(href));
        self
    }
}

impl Ring {
    /// `value` against `max`, as a ring.
    pub fn of(value: impl IntoNumber, max: impl IntoNumber) -> Self {
        let value = value.into_number();
        Self {
            shown: Some(value.to_string()),
            value: Some(value),
            max: max.into_number(),
            ..Self::default()
        }
    }

    /// A ring with no reading, against `max`.
    pub fn empty(max: impl IntoNumber) -> Self {
        Self {
            max: max.into_number(),
            ..Self::default()
        }
    }

    /// The value as the screen writes it, without its unit: `12,450`.
    #[must_use]
    pub fn shown(mut self, shown: impl Into<String>) -> Self {
        self.shown = Some(shown.into());
        self
    }

    /// What the value counts, written after it.
    #[must_use]
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// What it measures.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// A quieter line after the label.
    #[must_use]
    pub fn caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// Its colour.
    #[must_use]
    pub fn colour(mut self, colour: impl Into<Colour>) -> Self {
        self.colour = Some(colour.into());
        self
    }

    /// The ring the screen is about.
    #[must_use]
    pub fn hero(mut self) -> Self {
        self.prominence = Prominence::High;
        self
    }

    /// A small ring beside words or in a row, with nothing in its middle.
    #[must_use]
    pub fn small(mut self) -> Self {
        self.prominence = Prominence::Low;
        self.centre = Centre::Nothing;
        self
    }

    /// One sentence that says the whole ring to a reader who can't see it.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// A note about what the ring means, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl Rings {
    /// Rings inside each other, none yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a ring, the first outside: `value` against `max`, counted in
    /// `unit`, in the hue `hue`.
    #[must_use]
    pub fn ring(
        mut self,
        name: impl Into<String>,
        value: impl IntoNumber,
        max: impl IntoNumber,
        unit: impl Into<String>,
        hue: Hue,
    ) -> Self {
        let value = value.into_number();
        let unit = unit.into();
        self.rings.push(RingValue {
            name: name.into(),
            shown: Some(value.to_string()),
            value: Some(value),
            max: max.into_number(),
            // A score counts nothing but itself.
            unit: (!unit.is_empty()).then_some(unit),
            colour: Some(Colour::new(hue)),
        });
        self
    }

    /// Each ring's value over its goal stands in the middle, and a key names
    /// each ring.
    #[must_use]
    pub fn told(mut self) -> Self {
        self.centre = RingsCentre::Values;
        self.legend = true;
        self
    }

    /// Small rings beside words: the day's scores in a date control.
    #[must_use]
    pub fn small(mut self) -> Self {
        self.prominence = Prominence::Low;
        self
    }

    /// What the rings show.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// One sentence that says all the rings to a reader who can't see them.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }
}

impl DayPager {
    /// The control that steps a screen from day to day, on the day `day` (in
    /// words) which is `date` (`YYYY-MM-DD`).
    pub fn on(day: impl Into<String>, date: impl Into<String>) -> Self {
        Self {
            day: day.into(),
            date: date.into(),
            ..Self::default()
        }
    }

    /// The address of the day before, when there is one.
    #[must_use]
    pub fn previous(mut self, href: Option<String>) -> Self {
        self.previous = href;
        self
    }

    /// The address of the day after, when there is one.
    #[must_use]
    pub fn next(mut self, href: Option<String>) -> Self {
        self.next = href;
        self
    }

    /// The address of today, when the day shown is another.
    #[must_use]
    pub fn today(mut self, href: Option<String>) -> Self {
        self.today = href;
        self
    }

    /// A day can be chosen between `first` and `last` (`YYYY-MM-DD`): it is
    /// asked for at `href` with the query value `name`.
    #[must_use]
    pub fn pick(
        mut self,
        first: impl Into<String>,
        last: impl Into<String>,
        href: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        self.first = Some(first.into());
        self.last = Some(last.into());
        self.pick = Some(DayPick {
            href: href.into(),
            name: name.into(),
        });
        self
    }

    /// The day's scores, as small rings before the date.
    #[must_use]
    pub fn scores(mut self, rings: Rings) -> Self {
        self.scores = Some(Box::new(Node::Rings(rings)));
        self
    }

    /// The names of its two steppers and of its day chooser, in the screen's
    /// language.
    #[must_use]
    pub fn words(
        mut self,
        previous: impl Into<String>,
        next: impl Into<String>,
        choose: impl Into<String>,
    ) -> Self {
        self.words = Some(PagerWords {
            previous: previous.into(),
            next: next.into(),
            choose: choose.into(),
        });
        self
    }
}

impl Hour {
    /// An hour in which `value` happened, `level` strong (0 to 1), of the
    /// kind at the place `category` of the clock's key.
    pub fn of(value: impl IntoNumber, level: f64, category: u32) -> Self {
        Self {
            value: Some(value.into_number()),
            level: Some(level.into_number()),
            category: Some(category),
        }
    }
}

impl DayClock {
    /// A day as 24 hours round a dial, from midnight: an hour with no reading
    /// is `Hour::default()`.
    pub fn of(hours: impl IntoIterator<Item = Hour>) -> Self {
        Self {
            hours: hours.into_iter().collect(),
            ..Self::default()
        }
    }

    /// What the clock shows.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a kind of hour to the key, in its hue: the first is place 0.
    #[must_use]
    pub fn key(mut self, name: impl Into<String>, hue: Hue) -> Self {
        self.key.push(Category {
            name: name.into(),
            colour: Some(Colour::new(hue)),
        });
        self
    }

    /// Adds a moment that stands out, at the hour `at` of the day (`6.5` is
    /// half past six).
    #[must_use]
    pub fn moment(mut self, at: f64, name: impl Into<String>, icon: Icon, hue: Hue) -> Self {
        self.moments.push(ClockMoment {
            at: at.into_number(),
            name: name.into(),
            icon: Some(icon),
            colour: Some(Colour::new(hue)),
        });
        self
    }

    /// The hour it is now: the hours after it are still to come.
    #[must_use]
    pub fn now(mut self, hour: u8) -> Self {
        self.now = Some(hour);
        self
    }

    /// The day's total in the middle: the number as the screen writes it,
    /// what it counts, and what it is.
    #[must_use]
    pub fn total(
        mut self,
        shown: impl Into<String>,
        unit: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        self.shown = Some(shown.into());
        self.unit = Some(unit.into());
        self.label = Some(label.into());
        self
    }

    /// One sentence that says the whole day to a reader who can't see it.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }
}

impl Card {
    /// A card with nothing in it yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Its ground is tinted, softly, with a hue.
    #[must_use]
    pub fn tint(mut self, colour: impl Into<Colour>) -> Self {
        self.colour = Some(colour.into());
        self
    }

    /// The room between its edge and its parts, in steps of the system's
    /// spacing scale, when not the system's own.
    #[must_use]
    pub fn pad(mut self, steps: u8) -> Self {
        self.pad = Some(steps);
        self
    }

    /// A tap on the card does `action`; `label` names the card as a control.
    #[must_use]
    pub fn opens(mut self, label: impl Into<String>, action: Action) -> Self {
        self.label = Some(label.into());
        self.action = Some(action);
        self
    }

    /// The card grows into the screen its action opens.
    #[must_use]
    pub fn zoom(mut self) -> Self {
        self.transition = Some(Transition::Zoom);
        self
    }

    /// Adds what a long press or a secondary click offers.
    #[must_use]
    pub fn menu(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.menu);
        self
    }

    /// How it comes in when it joins a screen already drawn.
    #[must_use]
    pub fn appear(mut self, appear: Appear) -> Self {
        self.appear = Some(appear);
        self
    }

    /// Adds parts, one under another.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl Size {
    /// `side` points each way.
    pub const fn square(side: u16) -> Self {
        Self {
            width: Some(side),
            height: Some(side),
            scaled: false,
        }
    }

    /// `width` points wide and `height` high.
    pub const fn new(width: u16, height: u16) -> Self {
        Self {
            width: Some(width),
            height: Some(height),
            scaled: false,
        }
    }

    /// `width` points wide; as high as what it holds.
    pub const fn width(width: u16) -> Self {
        Self {
            width: Some(width),
            height: None,
            scaled: false,
        }
    }

    /// `height` points high; as wide as the room it has.
    pub const fn height(height: u16) -> Self {
        Self {
            width: None,
            height: Some(height),
            scaled: false,
        }
    }

    /// It follows the person's text size.
    #[must_use]
    pub const fn scaled(mut self) -> Self {
        self.scaled = true;
        self
    }
}

impl Layers {
    /// Parts over each other, none yet, each at the centre.
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the parts sit in the box.
    #[must_use]
    pub fn place(mut self, place: Place) -> Self {
        self.place = place;
        self
    }

    /// The size of the box.
    #[must_use]
    pub fn size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }

    /// The room round the parts, in steps of the system's spacing scale.
    #[must_use]
    pub fn pad(mut self, steps: u8) -> Self {
        self.pad = steps;
        self
    }

    /// It takes the room left along the stack it stands in.
    #[must_use]
    pub fn grow(mut self) -> Self {
        self.grow = true;
        self
    }

    /// Adds parts, the first at the back.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl Shape {
    fn of(form: ShapeForm) -> Self {
        Self {
            form,
            ..Self::default()
        }
    }

    /// A circle.
    pub fn circle() -> Self {
        Self::of(ShapeForm::Circle)
    }

    /// A rectangle whose short sides are half circles.
    pub fn capsule() -> Self {
        Self::of(ShapeForm::Capsule)
    }

    /// A rectangle with the system's own corner.
    pub fn rounded() -> Self {
        Self::of(ShapeForm::Rounded)
    }

    /// A rectangle with square corners.
    pub fn rect() -> Self {
        Self::of(ShapeForm::Rect)
    }

    /// The colour it is filled with.
    #[must_use]
    pub fn fill(mut self, colour: impl Into<Colour>) -> Self {
        self.fill = Some(colour.into());
        self
    }

    /// The fill is the soft tint of its hue: a ground for what stands on it.
    #[must_use]
    pub fn soft(mut self) -> Self {
        self.soft = true;
        self
    }

    /// The colour of its outline.
    #[must_use]
    pub fn stroke(mut self, colour: impl Into<Colour>) -> Self {
        self.stroke = Some(colour.into());
        self
    }

    /// Its size.
    #[must_use]
    pub fn size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }

    /// What it says to a reader who can't see it.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl Carousel {
    /// Parts side by side that scroll across, none yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The least a part may be wide, in points: as many as fit are shown.
    #[must_use]
    pub fn min(mut self, points: u16) -> Self {
        self.min = Some(points);
        self
    }

    /// The room between parts, in steps of the system's spacing scale.
    #[must_use]
    pub fn gap(mut self, steps: u8) -> Self {
        self.gap = steps;
        self
    }

    /// One part shows at a time, with the system's marks for which.
    #[must_use]
    pub fn paged(mut self) -> Self {
        self.paged = true;
        self
    }

    /// What the run is, for a reader who can't see it.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Adds parts.
    #[must_use]
    pub fn parts(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.children);
        self
    }
}

impl Adaptive {
    /// Parts for a compact room and parts for a wide one, none yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The parts for a compact room.
    #[must_use]
    pub fn compact(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.compact);
        self
    }

    /// The parts for a wide room.
    #[must_use]
    pub fn wide(mut self, parts: impl IntoParts) -> Self {
        parts.push_into(&mut self.wide);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_is_written_as_parts_and_is_the_screen_a_client_is_sent() {
        let screen = Screen::titled("Shelves")
            .back("Today", "/")
            .nav("/inventory")
            .refresh()
            .action(
                Button::new("Inventory")
                    .role(Role::Secondary)
                    .to("/inventory"),
            )
            .body(
                Stack::down().id("shelves").gap(4).parts((
                    Text::new("3")
                        .style(TextStyle::Display)
                        .change(Change::Count),
                    Card::new()
                        .id("shelf-7")
                        .tint(Hue::Amber)
                        .opens("Brass lota, 3 left", Action::request("/inventory/7/peek"))
                        .zoom()
                        .menu(
                            Button::new("Restock 24")
                                .post("/inventory/7/restock", [("quantity", "24")]),
                        )
                        .parts((
                            chip(Icon::named("package"), Hue::Amber),
                            Text::new("Brass lota").style(TextStyle::Headline).lines(2),
                        )),
                )),
            );
        assert_eq!(screen.id.as_deref(), Some("shelves"));
        assert_eq!(
            serde_json::to_string(&screen).unwrap(),
            concat!(
                r#"{"title":"Shelves","id":"shelves","back":{"title":"Today","href":"/"},"nav":"/inventory","#,
                r#""actions":[{"t":"button","label":"Inventory","role":"secondary","action":{"t":"navigate","href":"/inventory"}}],"#,
                r#""refresh":true,"body":{"t":"stack","id":"shelves","gap":4,"children":["#,
                r#"{"t":"text","value":"3","style":"display","tabular":true,"change":"count"},"#,
                r#"{"t":"card","id":"shelf-7","colour":{"hue":"amber"},"#,
                r#""action":{"t":"request","href":"/inventory/7/peek"},"label":"Brass lota, 3 left","#,
                r#""menu":[{"t":"button","label":"Restock 24","action":{"t":"request","method":"post","href":"/inventory/7/restock","fields":[{"name":"quantity","value":"24"}]}}],"#,
                r#""transition":"zoom","children":["#,
                r#"{"t":"layers","size":{"width":40,"height":40,"scaled":true},"children":["#,
                r#"{"t":"shape","form":"circle","fill":{"hue":"amber"},"soft":true},"#,
                r#"{"t":"icon","name":"package","symbol":{"apple":"shippingbox","apple_filled":"shippingbox.fill","material":"inventory_2"},"colour":{"hue":"amber"}}]},"#,
                r#"{"t":"text","value":"Brass lota","style":"headline","lines":2}]}]}}"#
            )
        );
        // What was written reads back as the same screen.
        let read: Screen = serde_json::from_str(&serde_json::to_string(&screen).unwrap()).unwrap();
        assert_eq!(read, screen);
    }

    #[test]
    fn a_body_of_several_parts_is_a_stack_and_of_one_is_that_part() {
        let one = Screen::titled("Orders").body(List::new().id("orders"));
        assert!(matches!(&one.body, Node::List(list) if list.id.as_deref() == Some("orders")));
        assert_eq!(one.id.as_deref(), Some("orders"));
        let two = Screen::titled("Orders").body((Text::new("a"), Text::new("b")));
        assert!(matches!(&two.body, Node::Stack(stack) if stack.children.len() == 2));
        assert_eq!(two.id, None);
    }

    #[test]
    fn parts_come_from_tuples_lists_options_and_iterators() {
        let low: Vec<&str> = vec!["Brass lota", "Khadi kurta"];
        let stack = Stack::across().parts((
            Text::new("Running low"),
            low.iter().map(|name| Badge::new(*name)),
            low.iter()
                .filter_map(|name| name.strip_suffix(" kurta").map(Text::new)),
            vec![Divider::default(), Divider::default()],
            [Spacer::default()],
            None::<Text>,
            each(low.iter().rev().map(|name| Text::new(*name))),
        ));
        let kinds: Vec<&str> = stack
            .children
            .iter()
            .map(|part| part.kind().as_str())
            .collect();
        assert_eq!(
            kinds,
            [
                "text", "badge", "badge", "text", "divider", "divider", "spacer", "text", "text"
            ]
        );
    }

    #[test]
    fn a_number_is_written_whole_when_it_is_whole() {
        assert_eq!(Ring::of(83, 100).value.unwrap().to_string(), "83");
        assert_eq!(Ring::of(7.0, 8.5).value.unwrap().to_string(), "7");
        assert_eq!(Ring::of(7.25, 8).shown.as_deref(), Some("7.25"));
        assert_eq!(Ring::of(f64::NAN, 8).value.unwrap().to_string(), "0");
        assert_eq!(Ring::empty(100).value, None);
    }

    #[test]
    fn a_form_and_a_dialog_are_written_with_their_fields_and_answers() {
        let dialog = Dialog::new("refund")
            .title("Refund")
            .parts(
                Form::post("/orders/1/refund").id("refund").parts((
                    Field::amount("amount", "₹")
                        .label("Amount")
                        .required()
                        .value("450.00"),
                    Field::choice(
                        "reason",
                        ChoiceKind::Menu,
                        [("late", "Arrived late"), ("damaged", "Damaged")],
                    )
                    .label("Reason")
                    .value("late"),
                )),
            )
            .action((
                Button::new("Keep").role(Role::Cancel).dismisses(),
                Button::new("Refund")
                    .role(Role::Confirm)
                    .action(Action::Submit {
                        form: Some("refund".to_owned()),
                        href: None,
                        with: None,
                    }),
            ));
        let Node::Form(form) = &dialog.children[0] else {
            panic!("the dialog holds the form");
        };
        assert_eq!(
            (form.method, form.action.as_str()),
            (Method::Post, "/orders/1/refund")
        );
        let Node::Field(amount) = &form.children[0] else {
            panic!("a field");
        };
        assert!(matches!(&amount.input, Input::Text(text)
            if text.kind == TextKind::Decimal && text.value == "450.00" && text.prefix.as_deref() == Some("₹")));
        let Node::Field(reason) = &form.children[1] else {
            panic!("a field");
        };
        assert!(matches!(&reason.input, Input::Choice(choice)
            if choice.value.as_deref() == Some("late") && choice.options.len() == 2));
        assert_eq!(dialog.actions.len(), 2);
    }
}
