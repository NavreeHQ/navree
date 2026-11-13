//! The example envelopes in `corpus/`: written from the types, read back, and
//! held to what the types write now. Every client decodes all of them in its
//! own tests, so a change here is a change to the protocol.
//!
//! `NAVREE_NATIVE_CORPUS=write cargo test -p navree-native` writes the files
//! again after a change that is meant.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use navree_native::action::{Action, Command, Pair, Present, State};
use navree_native::charts::{Curve, Hint, HintMark, Hue, Measure, Reference, ReferenceKind, Stacking};
use navree_native::device::{
    Accuracy, Alt, AppLock, AuthSession, CancelNotification, CodeFormat, Confirm, ConfirmMethod,
    CopyText, DeviceCall, Haptic, HapticKind, HttpsUrl, LocateOnce, Notify, OpenSettings, OpenUrl,
    OriginPath, Passkey, PasskeyMode, PlatformSignIn, RegisterPush, ReplyCall, ScanCode, SetBadge,
    SettingsPage, Share, SignInProvider, Then, Ticket, When,
};
use navree_native::envelope::{Back, Envelope, Screen, Upgrade, Web};
use navree_native::input::{
    ChoiceCreate, ChoiceInput, ChoiceKind, ChoiceOption, ChoiceSearch, ChoicesInput, ColourInput,
    DateInput, DateKind, FileInput, Input, PhoneCountry, RangeEnd, RangeInput, SliderInput, TextInput, TextKind,
    ToggleInput, ToggleKind, Weekday,
};
use navree_native::node::{
    Across, Align, Avatar, Axis, Badge, Banner, Block, BlockKind, Button, CalendarHeatmap, Category,
    Centre, Chart, ChartKind, ClockMoment, Code, ColorStrip, Colour, Column, ColumnRole, DayClock,
    DayPager, DayPick, HeatDay,
    Delivery, Dialog,
    Direction, Disclosure, Divider, Empty, Fact, Facts, FeedRow,
    Field, Figure, Fit, Form, Gauge, Grid, Hidden, Hour, Icon, Image, Justify, Link, List, Live,
    MatrixHeatmap, Menu, Message, Meter, Method, MiniColumn, MiniColumns, Moment, NativeView,
    Node, Pane, PaneRole, Point, PointSeries, Popover, Progress, Prominence,
    PagerWords, Purpose, Question, Region, Reorder, Rich, Ring, RingValue, Rings, RingsCentre,
    RangeScale, Relation, Role, Row, RowState, Run, Scatter, Section, SectionStyle, Segment,
    StripCell,
    Selection, Series, Sheet, SheetSize, Sort, SortDir, Spacer, Split, Stack, Standard, Tab, Table,
    TableRow, Tabs, Text, TextStyle, Timeline, Tone, Toolbar, ToolbarItem, Trend, Unknown,
    WebRegion, Zone,
};
use navree_native::op::{Edge, Op, Toast, ToastAction, UrlMode};
use navree_native::shell::{
    Device, Nav, NavGroup, NavItem, Push, Route, Shell, SignIn, Theme, User,
};
use navree_native::{Kind, VOCAB};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A health app's day, written with the builders of `navree_native::ui`.
#[path = "../examples/health_today.rs"]
mod health_today;
/// A shop's shelves, composed freely with the same builders.
#[path = "../examples/shop_shelves.rs"]
mod shop_shelves;

const BUILD: &str = "3f9a1c07b2de";

fn s(text: &str) -> String {
    text.to_owned()
}

fn text(value: &str) -> Node {
    Node::text(value)
}

fn muted(value: &str) -> Node {
    Node::Text(Text {
        value: s(value),
        muted: true,
        ..Text::default()
    })
}

fn icon(name: &str, path: &str) -> Icon {
    Icon {
        name: Some(s(name)),
        paths: vec![s(path)],
        ..Icon::default()
    }
}

fn badge(text: &str, tone: Tone) -> Badge {
    Badge {
        text: s(text),
        tone: Some(tone),
        dot: true,
        ..Badge::default()
    }
}

fn pair(name: &str, value: &str) -> Pair {
    Pair {
        name: s(name),
        value: s(value),
    }
}

fn option(value: &str, label: &str) -> ChoiceOption {
    ChoiceOption {
        value: s(value),
        label: s(label),
        ..ChoiceOption::default()
    }
}

fn counted(value: &str, label: &str, count: &str) -> ChoiceOption {
    ChoiceOption {
        detail: Some(s(count)),
        ..option(value, label)
    }
}

/// The orders of a shop: a table a narrow screen draws as a list, from the
/// columns' roles.
fn screen_list() -> Envelope {
    let order =
        |id: &str, number: &str, who: &str, status: Badge, total: &str, when: &str| TableRow {
            id: Some(s(id)),
            cells: vec![
                Node::Stack(Stack {
                    children: vec![
                        Node::Button(Button {
                            label: s(number),
                            prominence: Prominence::Low,
                            action: Some(Action::Submit {
                                form: None,
                                href: None,
                                with: None,
                            }),
                            ..Button::default()
                        }),
                        muted(who),
                    ],
                    ..Stack::default()
                }),
                Node::Badge(status),
                text(total),
                text(when),
            ],
            action: Some(Action::navigate(format!("/orders/{id}"))),
            ..TableRow::default()
        };
    // A form in a cell is the row's inline edit: a client draws an action
    // named by the form's submit, which opens the form as a task.
    let note = |id: &str, said: &str| {
        Node::Form(Form {
            id: Some(format!("note-{id}")),
            method: Method::Post,
            action: format!("/orders/{id}/note"),
            hidden: vec![Hidden {
                name: s("_csrf"),
                value: s("tok"),
                id: None,
            }],
            children: vec![
                Node::Field(Field {
                    name: s("note"),
                    label: Some(s("Delivery note")),
                    input: Input::Text(TextInput {
                        value: s(said),
                        max_length: Some(120),
                        ..TextInput::default()
                    }),
                    ..Field::default()
                }),
                Node::Button(Button {
                    label: s("Update note"),
                    role: Role::Secondary,
                    prominence: Prominence::Low,
                    action: Some(Action::submit()),
                    ..Button::default()
                }),
            ],
        })
    };
    let field = |name: &str, label: &str, purpose: Purpose, input: Input| Field {
        name: s(name),
        label: Some(s(label)),
        purpose: Some(purpose),
        input,
        ..Field::default()
    };
    let mut rows = vec![
        order(
            "1042",
            "CH-1042",
            "Harpreet Sandhu",
            badge("Open", Tone::Info),
            "₹1,24,050",
            "Today, 09:41",
        ),
        order(
            "1041",
            "CH-1041",
            "Simran Bhatia",
            badge("Delivered", Tone::Success),
            "₹2,499",
            "Yesterday, 18:02",
        ),
    ];
    rows[0].cells.push(note("1042", ""));
    // Only a delivered order can be archived with the others, so an order still
    // on its way can't be chosen.
    rows[0].unchoosable = true;
    rows[1].cells.push(note("1041", "Left with the neighbour"));
    rows[1].selected = true;
    rows[1].open = true;
    rows[1].detail = vec![muted("2 units of Phulkari dupatta, to Patiala")];
    let table = Table {
        id: Some(s("orders")),
        caption: Some(s("Orders")),
        columns: vec![
            Column {
                key: Some(s("number")),
                label: s("Order"),
                role: Some(ColumnRole::Title),
                sort: Some(Sort {
                    href: s("/orders?sort=number"),
                    dir: None,
                }),
                ..Column::default()
            },
            Column {
                key: Some(s("status")),
                label: s("Status"),
                role: Some(ColumnRole::Status),
                ..Column::default()
            },
            Column {
                key: Some(s("total")),
                label: s("Total"),
                numeric: true,
                align: Some(Align::End),
                role: Some(ColumnRole::Trailing),
                sort: Some(Sort {
                    href: s("/orders?sort=total"),
                    dir: None,
                }),
                ..Column::default()
            },
            Column {
                key: Some(s("placed_on")),
                label: s("Placed"),
                secondary: true,
                sort: Some(Sort {
                    href: s("/orders?sort=placed_on"),
                    dir: Some(SortDir::Desc),
                }),
                ..Column::default()
            },
            Column {
                key: Some(s("note")),
                label: s("Delivery note"),
                secondary: true,
                ..Column::default()
            },
        ],
        rows,
        query: Some(Form {
            id: Some(s("orders-f")),
            method: Method::Get,
            action: s("/orders"),
            hidden: Vec::new(),
            children: vec![
                // Answers as it is typed.
                Node::Field(Field {
                    changed: Some(Action::submit()),
                    ..field(
                        "search",
                        "Search",
                        Purpose::Search,
                        Input::Text(TextInput {
                            kind: TextKind::Search,
                            placeholder: Some(s("Search orders")),
                            ..TextInput::default()
                        }),
                    )
                }),
                Node::Field(field(
                    "filter[status][in]",
                    "Status",
                    Purpose::Filter,
                    Input::Choices(ChoicesInput {
                        options: vec![
                            counted("open", "Open", "212"),
                            counted("cancelled", "Cancelled", "9"),
                        ],
                        values: vec![s("open")],
                        ..ChoicesInput::default()
                    }),
                )),
                Node::Field(field(
                    "placed_on",
                    "Placed",
                    Purpose::Filter,
                    Input::Range(RangeInput {
                        from: RangeEnd {
                            name: s("filter[placed_on][gte]"),
                            value: s("2026-11-01"),
                        },
                        to: RangeEnd {
                            name: s("filter[placed_on][lte]"),
                            value: String::new(),
                        },
                        max: Some(s("2026-11-13")),
                        ..RangeInput::default()
                    }),
                )),
                Node::Field(field(
                    "sort",
                    "Order by",
                    Purpose::Sort,
                    Input::Choice(ChoiceInput {
                        options: vec![
                            option("-placed_on", "Newest first"),
                            option("-total", "Largest first"),
                        ],
                        value: Some(s("-placed_on")),
                        ..ChoiceInput::default()
                    }),
                )),
                Node::Field(field(
                    "per_page",
                    "Rows a page",
                    Purpose::PageSize,
                    Input::Choice(ChoiceInput {
                        options: vec![option("25", "25"), option("100", "100")],
                        value: Some(s("25")),
                        ..ChoiceInput::default()
                    }),
                )),
            ],
        }),
        select: Some(Selection {
            name: s("ids[]"),
            form: Some(s("orders-bulk")),
            // The page holds 25 of the 1,204 orders the query matches.
            across: Some(Across {
                name: s("all_matching"),
                value: s("eyJzdGF0dXMiOiJvcGVuIn0"),
                total: Some(1204),
            }),
        }),
        more: Some(s("/orders?after=eyJpZCI6MTA0MX0")),
        prev: None,
        summary: Some(s("1–25 of 1,204")),
        empty: None,
    };
    let mut screen = Screen::new(
        "Orders",
        Node::Region(Region {
            id: s("orders-region"),
            src: s("/orders"),
            lazy: false,
            live: Some(Live {
                src: s("/broadcasting/stream"),
                event: Some(s("orders")),
            }),
            children: vec![Node::Table(table)],
        }),
    );
    screen.back = Some(Back {
        title: s("Dashboard"),
        href: s("/"),
    });
    screen.nav = Some(s("/orders"));
    screen.refresh = true;
    screen.actions = vec![Node::Button(Button {
        label: s("New order"),
        icon: Some(icon("plus", "M5 12h14M12 5v14")),
        role: Role::Create,
        prominence: Prominence::High,
        action: Some(Action::Navigate {
            href: s("/orders/new"),
            present: Present::Sheet,
        }),
        ..Button::default()
    })];
    Envelope::screen(BUILD, screen)
}

/// One order: facts, the steps it has taken, what staff may do, and the
/// dialogs that ask first.
fn screen_detail() -> Envelope {
    let step = |title: &str, detail: &str, when: &str, state: RowState, tone: Tone| {
        Node::Row(Row {
            leading: Some(Box::new(Node::Icon(Icon {
                tone: Some(tone),
                ..icon("package", "M3 7l9-4 9 4v10l-9 4-9-4z")
            }))),
            title: s(title),
            subtitle: Some(s(detail)),
            trailing: Some(Box::new(muted(when))),
            state: Some(state),
            ..Row::default()
        })
    };
    let body = Node::Stack(Stack {
        id: Some(s("order-page")),
        gap: 4,
        children: vec![
            Node::Stack(Stack {
                axis: Axis::Horizontal,
                gap: 2,
                align: Some(Align::Center),
                justify: Some(Justify::Between),
                wrap: true,
                children: vec![
                    Node::Badge(badge("Open", Tone::Info)),
                    Node::Badge(badge("Packed", Tone::Warning)),
                    Node::Spacer(Spacer::default()),
                    muted("Placed today, 09:41 · ₹1,24,050"),
                ],
                ..Stack::default()
            }),
            Node::Stack(Stack {
                axis: Axis::Horizontal,
                gap: 2,
                wrap: true,
                children: vec![
                    Node::Button(Button {
                        label: s("Refund"),
                        role: Role::Secondary,
                        action: Some(Action::command(Command::Open { id: s("refund") })),
                        ..Button::default()
                    }),
                    Node::Button(Button {
                        label: s("Cancel order"),
                        role: Role::Destructive,
                        prominence: Prominence::High,
                        action: Some(Action::command(Command::Open { id: s("cancel") })),
                        ..Button::default()
                    }),
                    Node::Button(Button {
                        label: s("Resend confirmation"),
                        role: Role::Secondary,
                        disabled: true,
                        reason: Some(s("It went out at 09:41. It can go again after 09:46.")),
                        ..Button::default()
                    }),
                ],
                ..Stack::default()
            }),
            Node::Facts(Facts {
                id: None,
                items: vec![
                    Fact {
                        label: s("Customer"),
                        value: vec![text("Harpreet Sandhu")],
                        icon: Some(icon("user", "M4 21a8 8 0 0 1 16 0")),
                        tone: Some(Tone::Info),
                        details: vec![
                            s("harpreet.sandhu@example.com"),
                            s("+91 98140 12345"),
                            s("7 orders since Mar 2024"),
                        ],
                    },
                    Fact {
                        label: s("Ships to"),
                        value: vec![
                            Node::Image(Image {
                                src: s("/_ui/flags/in.svg"),
                                alt: String::new(),
                                width: Some(16),
                                height: Some(12),
                                fit: Some(Fit::Contain),
                                ..Image::default()
                            }),
                            text("Amritsar"),
                        ],
                        icon: None,
                        tone: None,
                        details: vec![s("Punjab, PIN 143001")],
                    },
                ],
            }),
            Node::Section(Section {
                title: Some(s("Progress")),
                subtitle: Some(s("Packed so far")),
                style: SectionStyle::Card,
                children: vec![Node::List(List {
                    id: Some(s("order-steps")),
                    children: vec![
                        step(
                            "Placed",
                            "3 units for ₹1,24,050",
                            "09:41",
                            RowState::Done,
                            Tone::Info,
                        ),
                        step(
                            "Packed",
                            "At the Ludhiana warehouse",
                            "11:05",
                            RowState::Done,
                            Tone::Warning,
                        ),
                        step(
                            "Shipping",
                            "Handed to the carrier",
                            "Next",
                            RowState::Current,
                            Tone::Neutral,
                        ),
                        step(
                            "Delivery",
                            "At the door",
                            "Later",
                            RowState::Upcoming,
                            Tone::Neutral,
                        ),
                    ],
                    ..List::default()
                })],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("More from Harpreet Sandhu")),
                style: SectionStyle::Card,
                children: vec![Node::Region(Region {
                    id: s("order-others"),
                    src: s("/orders/1042/others"),
                    lazy: true,
                    live: None,
                    children: Vec::new(),
                })],
                ..Section::default()
            }),
            Node::Dialog(Dialog {
                id: s("cancel"),
                title: Some(s("Cancel CH-1042?")),
                text: Some(s(
                    "₹1,24,050 goes back to the customer's UPI account, and 3 units go back in stock. This can't be undone.",
                )),
                alert: true,
                actions: vec![
                    Node::Button(Button {
                        label: s("Keep the order"),
                        role: Role::Cancel,
                        action: Some(Action::Dismiss),
                        ..Button::default()
                    }),
                    Node::Button(Button {
                        label: s("Cancel order"),
                        role: Role::Destructive,
                        prominence: Prominence::High,
                        action: Some(Action::Request {
                            method: Method::Post,
                            href: s("/orders/1042/cancel"),
                            fields: vec![pair("_csrf", "tok")],
                        }),
                        ..Button::default()
                    }),
                ],
                ..Dialog::default()
            }),
            Node::Dialog(Dialog {
                id: s("refund"),
                title: Some(s("Refund CH-1042")),
                text: Some(s(
                    "Up to ₹1,24,050 can go back to the customer's UPI account.",
                )),
                children: vec![refund_form()],
                ..Dialog::default()
            }),
        ],
        ..Stack::default()
    });
    let mut screen = Screen::new("Order CH-1042", body);
    screen.id = Some(s("order-page"));
    screen.back = Some(Back {
        title: s("Orders"),
        href: s("/orders"),
    });
    screen.trail = vec![
        Back {
            title: s("Dashboard"),
            href: s("/"),
        },
        Back {
            title: s("Orders"),
            href: s("/orders"),
        },
    ];
    screen.nav = Some(s("/orders"));
    Envelope::screen(BUILD, screen)
}

fn refund_form() -> Node {
    Node::Form(Form {
        id: Some(s("refund-form")),
        method: Method::Post,
        action: s("/orders/1042/refund"),
        hidden: vec![Hidden {
            name: s("_csrf"),
            value: s("tok"),
            id: None,
        }],
        children: vec![
            Node::Field(Field {
                id: Some(s("refund-amount")),
                name: s("amount"),
                label: Some(s("Amount")),
                help: Some(s(
                    "Up to what is left to refund. All of it refunds the order in full.",
                )),
                error: None,
                required: true,
                disabled: false,
                purpose: None,
                changed: None,
                input: Input::Text(TextInput {
                    kind: TextKind::Decimal,
                    value: s("1240.50"),
                    ..TextInput::default()
                }),
            }),
            Node::Field(Field {
                id: Some(s("refund-reason")),
                name: s("reason"),
                label: Some(s("Reason")),
                help: None,
                error: None,
                required: false,
                disabled: false,
                purpose: None,
                changed: None,
                input: Input::Choice(ChoiceInput {
                    kind: ChoiceKind::Menu,
                    options: vec![
                        option("returned", "Returned by the customer"),
                        option("damaged", "Arrived damaged"),
                        option("late", "Arrived late"),
                        option("goodwill", "A goodwill gesture"),
                    ],
                    value: Some(s("returned")),
                    placeholder: None,
                    ..ChoiceInput::default()
                }),
            }),
            Node::Button(Button {
                label: s("Refund"),
                role: Role::Primary,
                prominence: Prominence::High,
                action: Some(Action::submit()),
                ..Button::default()
            }),
        ],
    })
}

/// A form with every kind of control.
fn screen_form() -> Envelope {
    let field = |name: &str, label: &str, input: Input| {
        Node::Field(Field {
            id: Some(format!("f-{name}")),
            name: s(name),
            label: Some(s(label)),
            help: None,
            error: None,
            required: false,
            disabled: false,
            purpose: None,
            changed: None,
            input,
        })
    };
    let typed = |kind: TextKind| {
        Input::Text(TextInput {
            kind,
            ..TextInput::default()
        })
    };
    let form = Form {
        id: Some(s("customer")),
        method: Method::Post,
        action: s("/customers"),
        hidden: vec![
            Hidden {
                name: s("_csrf"),
                value: s("tok"),
                id: None,
            },
            Hidden {
                name: s("plan"),
                value: s("free"),
                id: Some(s("plan")),
            },
        ],
        children: vec![
            Node::Field(Field {
                id: Some(s("f-name")),
                name: s("name"),
                label: Some(s("Name")),
                help: Some(s("As it is written on their invoices.")),
                error: Some(s("Enter a name.")),
                required: true,
                disabled: false,
                purpose: None,
                changed: None,
                input: Input::Text(TextInput {
                    value: String::new(),
                    placeholder: Some(s("Gurpreet Kaur")),
                    max_length: Some(80),
                    autofill: Some(s("name")),
                    ..TextInput::default()
                }),
            }),
            field("email", "Email", typed(TextKind::Email)),
            field("password", "Password", typed(TextKind::Password)),
            field(
                "phone",
                "Phone",
                Input::Text(TextInput {
                    kind: TextKind::Phone,
                    value: s("98140 12345"),
                    // The number is read in this country, which is posted beside it.
                    country: Some(PhoneCountry {
                        name: s("phone_country"),
                        value: s("IN"),
                    }),
                    ..TextInput::default()
                }),
            ),
            field("site", "Website", typed(TextKind::Url)),
            field("q", "Search", typed(TextKind::Search)),
            field("code", "Code", typed(TextKind::OneTimeCode)),
            field(
                "seats",
                "Seats",
                Input::Text(TextInput {
                    kind: TextKind::Number,
                    value: s("5"),
                    min: Some(s("1")),
                    max: Some(s("500")),
                    step: Some(s("1")),
                    ..TextInput::default()
                }),
            ),
            // The value is plain digits, whatever the person's locale writes
            // (₹1,00,000.50); the prefix carries the unit.
            field(
                "credit",
                "Credit limit",
                Input::Text(TextInput {
                    kind: TextKind::Decimal,
                    value: s("100000.50"),
                    prefix: Some(s("₹")),
                    ..TextInput::default()
                }),
            ),
            field(
                "notes",
                "Notes",
                Input::Text(TextInput {
                    kind: TextKind::Multiline,
                    readonly: true,
                    value: s("Prefers delivery before noon."),
                    ..TextInput::default()
                }),
            ),
            field(
                "newsletter",
                "Send the newsletter",
                Input::Toggle(ToggleInput {
                    kind: ToggleKind::Switch,
                    on: true,
                    value: None,
                }),
            ),
            field(
                "terms",
                "Agrees to the terms",
                Input::Toggle(ToggleInput {
                    kind: ToggleKind::Checkbox,
                    on: false,
                    value: Some(s("yes")),
                }),
            ),
            field(
                "city",
                "City",
                Input::Choice(ChoiceInput {
                    kind: ChoiceKind::List,
                    options: vec![
                        option("ludhiana", "Ludhiana"),
                        option("amritsar", "Amritsar"),
                        option("chandigarh", "Chandigarh"),
                        ChoiceOption {
                            disabled: true,
                            ..option("shimla", "Shimla")
                        },
                    ],
                    value: None,
                    placeholder: Some(s("Choose a city")),
                    ..ChoiceInput::default()
                }),
            ),
            field(
                "tier",
                "Tier",
                Input::Choice(ChoiceInput {
                    kind: ChoiceKind::Segmented,
                    options: vec![option("retail", "Retail"), option("trade", "Trade")],
                    value: Some(s("retail")),
                    placeholder: None,
                    ..ChoiceInput::default()
                }),
            ),
            field(
                "pays",
                "Pays by",
                Input::Choice(ChoiceInput {
                    kind: ChoiceKind::Radio,
                    options: vec![
                        option("upi", "UPI"),
                        option("card", "Card"),
                        option("cod", "Cash on delivery"),
                    ],
                    value: Some(s("upi")),
                    placeholder: None,
                    ..ChoiceInput::default()
                }),
            ),
            field(
                "tags",
                "Tags",
                Input::Choices(ChoicesInput {
                    options: vec![
                        option("festive", "Festive"),
                        option("bulk", "Bulk"),
                        option("gift", "Gift"),
                    ],
                    values: vec![s("festive"), s("gift")],
                    ..ChoicesInput::default()
                }),
            ),
            field(
                "since",
                "Customer since",
                Input::Date(DateInput {
                    kind: DateKind::Date,
                    value: s("2024-03-18"),
                    min: Some(s("2020-01-01")),
                    max: Some(s("2026-11-12")),
                    ..DateInput::default()
                }),
            ),
            field(
                "call_at",
                "Call at",
                Input::Date(DateInput {
                    kind: DateKind::Time,
                    value: s("09:41"),
                    ..DateInput::default()
                }),
            ),
            field(
                "visit",
                "Next visit",
                Input::Date(DateInput {
                    kind: DateKind::Datetime,
                    value: s("2026-11-14T16:30"),
                    ..DateInput::default()
                }),
            ),
            field(
                "discount",
                "Discount",
                Input::Slider(SliderInput {
                    min: s("0"),
                    max: s("30"),
                    step: Some(s("2.5")),
                    value: s("7.5"),
                }),
            ),
            field(
                "label",
                "Label colour",
                Input::Colour(ColourInput {
                    value: s("#16a34a"),
                }),
            ),
            field(
                "papers",
                "Papers",
                Input::File(FileInput {
                    accept: Some(s("image/*,.pdf")),
                    multiple: true,
                    capture: Some(s("environment")),
                    keep_location: false,
                }),
            ),
            Node::Stack(Stack {
                axis: Axis::Horizontal,
                gap: 2,
                justify: Some(Justify::End),
                children: vec![
                    Node::Button(Button {
                        label: s("Cancel"),
                        role: Role::Cancel,
                        action: Some(Action::Back),
                        ..Button::default()
                    }),
                    Node::Button(Button {
                        label: s("Save and add another"),
                        role: Role::Secondary,
                        action: Some(Action::Submit {
                            form: Some(s("customer")),
                            href: Some(s("/customers?again=1")),
                            with: Some(pair("then", "again")),
                        }),
                        ..Button::default()
                    }),
                    Node::Button(Button {
                        label: s("Save"),
                        role: Role::Primary,
                        prominence: Prominence::High,
                        action: Some(Action::submit()),
                        ..Button::default()
                    }),
                ],
                ..Stack::default()
            }),
        ],
    };
    let mut screen = Screen::new("New customer", Node::Form(form));
    screen.subtitle = Some(s("Charkha Supply Co."));
    screen.back = Some(Back {
        title: s("Customers"),
        href: s("/customers"),
    });
    Envelope::screen(BUILD, screen)
}

/// Every kind of node, each at least once, so a client's decoder meets them all.
fn screen_kinds() -> Envelope {
    let body = Node::Stack(Stack {
        gap: 4,
        children: vec![
            Node::Banner(Banner {
                id: Some(s("notice")),
                tone: Some(Tone::Warning),
                title: Some(s("Three shelves are running low")),
                text: Some(s("Restock them before the weekend.")),
                icon: Some(icon("triangle-alert", "M12 9v4M12 17h.01M3 20h18L12 4z")),
                actions: vec![Node::Link(Link {
                    id: None,
                    label: s("See the inventory"),
                    action: Action::navigate("/inventory"),
                    children: Vec::new(),
                })],
            }),
            Node::Grid(Grid {
                cols: Some(2),
                min: Some(160),
                gap: 3,
                children: vec![
                    Node::Section(Section {
                        title: Some(s("Revenue today")),
                        style: SectionStyle::Card,
                        actions: vec![Node::Menu(Menu {
                            id: Some(s("revenue-menu")),
                            label: s("More"),
                            icon: Some(icon("ellipsis", "M5 12h.01M12 12h.01M19 12h.01")),
                            items: vec![
                                Node::Button(Button {
                                    label: s("Copy the figure"),
                                    action: Some(Action::command(Command::Copy {
                                        text: s("₹1,24,050"),
                                    })),
                                    ..Button::default()
                                }),
                                Node::Divider(Divider::default()),
                                Node::Button(Button {
                                    label: s("Hide this card"),
                                    role: Role::Destructive,
                                    action: Some(Action::Commands {
                                        list: vec![
                                            Command::Set {
                                                id: s("revenue"),
                                                state: State::Hidden,
                                                on: true,
                                            },
                                            Command::Toast {
                                                tone: Tone::Neutral,
                                                text: s("Hidden"),
                                            },
                                        ],
                                    }),
                                    ..Button::default()
                                }),
                            ],
                        })],
                        children: vec![
                            Node::Text(Text {
                                id: Some(s("revenue")),
                                value: s("₹1,24,050"),
                                style: TextStyle::LargeTitle,
                                tone: Some(Tone::Success),
                                lines: Some(1),
                                heading: None,
                                muted: false,
                                ..Text::default()
                            }),
                            Node::Progress(Progress {
                                id: None,
                                value: Some(620),
                                label: Some(s("62% of the day's goal")),
                                tone: None,
                                colour: Some(Colour::new(Hue::Green)),
                            }),
                            Node::Progress(Progress {
                                id: None,
                                value: Some(910),
                                label: Some(s("Stock room: 91% full")),
                                tone: Some(Tone::Warning),
                                colour: None,
                            }),
                        ],
                        footer: vec![muted("Up 12% on yesterday")],
                        ..Section::default()
                    }),
                    Node::Chart(Chart {
                        id: Some(s("sales")),
                        kind: ChartKind::Bar,
                        title: Some(s("Orders this week")),
                        labels: vec![s("Mon"), s("Tue"), s("Wed")],
                        series: vec![
                            Series {
                                label: s("UPI"),
                                values: vec![Some(41.into()), Some(38.into()), None],
                                shown: Vec::new(),
                                ..Series::default()
                            },
                            Series {
                                label: s("Card"),
                                values: vec![
                                    Some(12.into()),
                                    serde_json::Number::from_f64(9.5),
                                    Some(17.into()),
                                ],
                                shown: vec![s("12"), s("9.5"), s("17")],
                                ..Series::default()
                            },
                        ],
                        ..Chart::default()
                    }),
                ],
                ..Grid::default()
            }),
            Node::Tabs(Tabs {
                id: Some(s("views")),
                items: vec![
                    Tab {
                        label: s("Activity"),
                        icon: Some(icon("activity", "M22 12h-4l-3 9L9 3l-3 9H2")),
                        selected: true,
                        href: None,
                        children: vec![Node::List(List {
                            id: Some(s("feed")),
                            more: Some(s("/feed?after=981")),
                            before: Some(s("/feed?before=981")),
                            ordered: false,
                            since: Some(s("0004410")),
                            changes: Some(s("/feed?dir=live")),
                            live: Some(Live {
                                src: s("/broadcasting/stream"),
                                event: Some(s("feed")),
                            }),
                            reorder: None,
                            children: vec![Node::Row(Row {
                                id: Some(s("event-981")),
                                key: None,
                                leading: Some(Box::new(Node::Avatar(Avatar {
                                    id: None,
                                    name: s("Ravneet Kaur"),
                                    src: None,
                                    initials: Some(s("RK")),
                                }))),
                                title: s("CH-1042 was packed"),
                                subtitle: Some(s("At the Ludhiana warehouse")),
                                detail: vec![Node::Rich(Rich {
                                    id: None,
                                    runs: vec![
                                        Run {
                                            text: s("Packed by "),
                                            ..Run::default()
                                        },
                                        Run {
                                            text: s("Ravneet"),
                                            bold: true,
                                            ..Run::default()
                                        },
                                        Run {
                                            text: s(", see "),
                                            italic: true,
                                            ..Run::default()
                                        },
                                        Run {
                                            text: s("the order"),
                                            href: Some(s("/orders/1042")),
                                            ..Run::default()
                                        },
                                        Run {
                                            text: s(" LDH-7"),
                                            code: true,
                                            ..Run::default()
                                        },
                                    ],
                                    style: TextStyle::Caption,
                                    blocks: Vec::new(),
                                    muted: true,
                                })],
                                trailing: Some(Box::new(muted("11:05"))),
                                badge: Some(badge("New", Tone::Accent)),
                                state: None,
                                action: Some(Action::navigate("/orders/1042")),
                                swipe: vec![Button {
                                    label: s("Archive"),
                                    role: Role::Destructive,
                                    action: Some(Action::Request {
                                        method: Method::Post,
                                        href: s("/feed/981/archive"),
                                        fields: Vec::new(),
                                    }),
                                    ..Button::default()
                                }],
                                menu: vec![Node::Button(Button {
                                    label: s("Copy the order number"),
                                    action: Some(Action::command(Command::Copy {
                                        text: s("CH-1042"),
                                    })),
                                    ..Button::default()
                                })],
                                muted: false,
                            })],
                        })],
                    },
                    // A view held here, still to be fetched: its region is asked for
                    // when the tab is shown, not before.
                    Tab {
                        label: s("Stock"),
                        icon: None,
                        selected: false,
                        href: None,
                        children: vec![Node::Region(Region {
                            id: s("stock-figures"),
                            src: s("/inventory/figures"),
                            lazy: true,
                            live: None,
                            children: Vec::new(),
                        })],
                    },
                    Tab {
                        label: s("Reports"),
                        icon: None,
                        selected: false,
                        href: Some(s("/reports")),
                        children: Vec::new(),
                    },
                ],
            }),
            Node::Split(Split {
                id: Some(s("panes")),
                primary: Box::new(Node::Empty(Empty {
                    id: None,
                    icon: Some(icon("inbox", "M22 12h-6l-2 3h-4l-2-3H2")),
                    title: s("No conversation is open"),
                    text: Some(s("Choose one from the list.")),
                    actions: vec![Node::Button(Button {
                        label: s("Refresh"),
                        action: Some(Action::Refresh),
                        ..Button::default()
                    })],
                })),
                detail: Box::new(Node::Disclosure(Disclosure {
                    id: Some(s("about")),
                    label: s("About this customer"),
                    open: true,
                    children: vec![
                        Node::Image(Image {
                            id: None,
                            src: s("/files/shop.jpg"),
                            alt: s("The shop's front on Mall Road"),
                            width: Some(320),
                            height: Some(180),
                            fit: Some(Fit::Cover),
                        }),
                        Node::Code(Code {
                            id: None,
                            text: s("GSTIN 03ABCDE1234F1Z5"),
                            lang: None,
                        }),
                    ],
                })),
            }),
            Node::Popover(Popover {
                id: s("hint"),
                children: vec![text("Totals leave out cancelled orders.")],
            }),
            Node::Sheet(Sheet {
                id: s("order"),
                title: Some(s("Order CH-1042")),
                subtitle: Some(s("3 units for Harpreet Sandhu in Amritsar, Punjab")),
                size: SheetSize::Large,
                task: false,
                open: false,
                children: vec![text("…")],
                actions: vec![Node::Button(Button {
                    label: s("Close"),
                    role: Role::Cancel,
                    action: Some(Action::Dismiss),
                    ..Button::default()
                })],
            }),
            Node::Native(NativeView {
                id: Some(s("map")),
                name: s("map"),
                props: serde_json::json!({"lat": 30.901, "lng": 75.8573, "zoom": 12}),
                alt: Some(Box::new(text("Ludhiana warehouse, Punjab"))),
            }),
            Node::Web(WebRegion {
                id: Some(s("terms")),
                html: s("<p>Goods once sold are <em>not</em> taken back after 7 days.</p>"),
            }),
        ],
        ..Stack::default()
    });
    Envelope::screen(BUILD, Screen::new("Every part", body))
}

/// What a post answers: the dialog closes, the order is drawn again, a toast
/// says what happened.
fn ops_reply() -> Envelope {
    Envelope::ops(
        BUILD,
        vec![
            Op::Close {
                target: s("refund"),
            },
            Op::Morph {
                target: s("order-page"),
                node: Node::Stack(Stack {
                    id: Some(s("order-page")),
                    children: vec![Node::Badge(badge("Refunded", Tone::Neutral))],
                    ..Stack::default()
                }),
            },
            Op::Replace {
                target: s("refund"),
                node: Node::Dialog(Dialog {
                    id: s("refund"),
                    title: Some(s("Refund CH-1042")),
                    text: Some(s("Nothing is left to refund.")),
                    ..Dialog::default()
                }),
            },
            Op::Append {
                target: s("feed"),
                nodes: vec![Node::Row(Row {
                    id: Some(s("event-982")),
                    title: s("CH-1042 was refunded"),
                    ..Row::default()
                })],
                more: None,
            },
            Op::Prepend {
                target: s("feed"),
                nodes: vec![Node::Row(Row {
                    id: Some(s("event-983")),
                    title: s("CH-1043 was placed"),
                    ..Row::default()
                })],
                before: Some(s("/feed?before=983")),
            },
            Op::Remove {
                target: s("event-12"),
            },
            Op::Errors {
                form: s("refund-form"),
                fields: BTreeMap::new(),
            },
            Op::Toast(Toast {
                tone: Tone::Success,
                text: s("₹1,240.50 went back to the customer's UPI account."),
                title: Some(s("Done")),
                id: Some(s("refund-1042")),
                action: Some(ToastAction {
                    label: s("See the order"),
                    href: s("/orders/1042"),
                    post: false,
                    fields: Vec::new(),
                }),
            }),
            Op::Focus {
                target: s("refund-amount"),
            },
            Op::Open { target: s("order") },
            Op::Url {
                href: s("/orders/1042"),
                mode: UrlMode::Replace,
            },
            Op::Invalidate {
                target: s("order-others"),
            },
        ],
    )
}

/// A form that was refused: messages at its fields.
fn ops_errors() -> Envelope {
    Envelope::ops(
        BUILD,
        vec![
            Op::Errors {
                form: s("refund-form"),
                fields: BTreeMap::from([
                    (
                        s("amount"),
                        vec![s("Enter an amount above zero, like 24.00.")],
                    ),
                    (s("reason"), vec![s("Choose a reason.")]),
                ]),
            },
            Op::Focus {
                target: s("refund-amount"),
            },
        ],
    )
}

/// A post that ends somewhere else, in or out of the app.
fn ops_navigate() -> Envelope {
    Envelope::ops(
        BUILD,
        vec![
            Op::Toast(Toast {
                tone: Tone::Info,
                text: s("This demo has no accounts to sign out of."),
                ..Toast::default()
            }),
            Op::Toast(Toast {
                tone: Tone::Success,
                text: s("CH-1042 was archived."),
                action: Some(ToastAction {
                    label: s("Undo"),
                    href: s("/orders/1042/unarchive"),
                    post: true,
                    fields: vec![pair("_csrf", "tok")],
                }),
                ..Toast::default()
            }),
            Op::Navigate {
                href: s("/"),
                present: Present::Root,
            },
            Op::NavigateExternal {
                href: s("https://pay.example/session/7"),
            },
            Op::Reload,
        ],
    )
}

/// What the reply to a sign-out post says: the person is signed out, a line
/// about it, and where they land.
fn ops_sign_out() -> Envelope {
    Envelope::ops(
        BUILD,
        vec![
            Op::SignedOut,
            Op::Toast(Toast {
                tone: Tone::Info,
                text: s("You have signed out of Charkha Supply Co."),
                ..Toast::default()
            }),
            Op::Navigate {
                href: s("/sign-in"),
                present: Present::Root,
            },
        ],
    )
}

fn web_fallback() -> Envelope {
    Envelope::web(
        BUILD,
        Web {
            url: s("/board"),
            title: Some(s("Board")),
            why: vec![s("slot:sortable"), s("tag:canvas")],
        },
    )
}

fn upgrade() -> Envelope {
    Envelope::upgrade(
        BUILD,
        Upgrade {
            min_app: s("1.2.0"),
            url: s("https://example.com/app"),
        },
    )
}

/// A screen with a toast left for it by the post before.
fn screen_with_toast() -> Envelope {
    let mut envelope = Envelope::screen(
        BUILD,
        Screen::new(
            "Inventory",
            Node::Empty(Empty {
                title: s("No product matches"),
                text: Some(s("Clear a filter, or search for part of a name or a SKU.")),
                ..Empty::default()
            }),
        ),
    );
    envelope.ops = vec![Op::Toast(Toast {
        tone: Tone::Success,
        text: s("24 units of Brass Kadhai added. 31 on the shelf now."),
        title: Some(s("Restocked")),
        ..Toast::default()
    })];
    envelope
}

fn shell() -> Shell {
    let mut shell = Shell::new(BUILD, "Charkha Supply Co.");
    shell.context = Some(s("Store operations"));
    shell.theme = Some(Theme {
        name: Some(s("ocean")),
        accent: Some(s("#0b6bcb")),
        accent_dark: Some(s("#5aa9f0")),
    });
    shell.nav = Nav {
        groups: vec![
            NavGroup {
                label: Some(s("Store")),
                items: vec![
                    NavItem {
                        label: s("Dashboard"),
                        icon: Some(icon("layout-dashboard", "M3 3h7v9H3zM14 3h7v5h-7z")),
                        href: s("/"),
                        ..NavItem::default()
                    },
                    NavItem {
                        label: s("Orders"),
                        icon: Some(icon("shopping-bag", "M6 2 3 6v14h18V6l-3-4z")),
                        href: s("/orders"),
                        badge: Some(s("12")),
                        items: vec![NavItem {
                            label: s("Returns"),
                            href: s("/orders/returns"),
                            ..NavItem::default()
                        }],
                    },
                ],
            },
            NavGroup {
                label: None,
                items: vec![NavItem {
                    label: s("Inventory"),
                    href: s("/inventory"),
                    ..NavItem::default()
                }],
            },
        ],
    };
    shell.user = Some(User {
        name: s("Ravneet Kaur"),
        email: Some(s("ravneet.kaur@example.com")),
        menu: vec![
            Node::Link(Link {
                id: None,
                label: s("Profile"),
                action: Action::navigate("/profile"),
                children: Vec::new(),
            }),
            Node::Button(Button {
                label: s("Sign out"),
                action: Some(Action::Request {
                    method: Method::Post,
                    href: s("/sign-out"),
                    fields: vec![pair("_csrf", "tok")],
                }),
                ..Button::default()
            }),
        ],
    });
    shell.routes = vec![
        Route {
            matches: s("/orders/new"),
            present: Some(Present::Sheet),
            chrome: true,
        },
        Route {
            matches: s("/sign-in"),
            present: Some(Present::Full),
            chrome: false,
        },
    ];
    shell.device = Some(Device {
        caps: vec![s("confirm"), s("push"), s("share"), s("scan_code")],
        push: Some(Push {
            register: path("/api/notifications/push-devices"),
        }),
        lock: Some(AppLock { after_secs: 300 }),
    });
    shell.sign_in = Some(SignIn {
        start: path("/auth/app/sign-in"),
        exchange: path("/api/auth/app/exchange"),
        callback: Some(s("https://charkha.example/app/signed-in")),
        second_step: Some(path("/sign-in/two-factor")),
    });
    shell
}

fn path(text: &str) -> OriginPath {
    OriginPath::parse(text).unwrap()
}

fn then(href: &str, ticket: &str) -> Then {
    Then {
        href: path(href),
        ticket: Ticket::from(s(ticket)),
        exp: 1_794_000_000,
    }
}

/// Buttons for every device call a tap may start, each with where its result
/// goes and what stands in its place where it can't start.
fn screen_device() -> Envelope {
    let button = |label: &str, role: Role, call: DeviceCall| {
        Node::Button(Button {
            label: s(label),
            role,
            action: Some(Action::Device(call)),
            ..Button::default()
        })
    };
    let calls = vec![
        button(
            "Pick a size",
            Role::None,
            DeviceCall::new(Haptic {
                kind: HapticKind::Selection,
            }),
        ),
        button(
            "Copy the code",
            Role::None,
            DeviceCall::new(CopyText {
                text: s("482913"),
                sensitive: true,
            }),
        ),
        button(
            "Share the invoice",
            Role::Secondary,
            DeviceCall::new(Share {
                title: Some(s("Invoice CH-1042")),
                text: Some(s("₹1,24,050 from Charkha Supply Co.")),
                href: Some(path("/orders/1042")),
                url: None,
                file: Some(path("/orders/1042/invoice.pdf")),
            })
            .then(then("/orders/1042/shared", "tk-share"))
            .alt(Alt::Text {
                text: s("Copy the link from the address bar"),
            }),
        ),
        button(
            "Track the parcel",
            Role::None,
            DeviceCall::new(OpenUrl {
                url: HttpsUrl::parse("https://track.example/LDH-7731").unwrap(),
                inside: true,
            }),
        ),
        button(
            "Continue with Google",
            Role::Primary,
            DeviceCall::new(AuthSession {
                start: path("/auth/google/redirect"),
                ephemeral: false,
            }),
        ),
        button(
            "Turn on order updates",
            Role::None,
            DeviceCall::new(RegisterPush {
                categories: vec![s("orders"), s("stock")],
            })
            .then(then("/settings/push", "tk-push")),
        ),
        button(
            "Remind me at 9",
            Role::None,
            DeviceCall::new(Notify {
                id: s("restock"),
                title: s("Restock the shelves"),
                body: Some(s("Brass Kadhai is down to 9 in Amritsar.")),
                at: When::Daily { hour: 9, minute: 0 },
                href: Some(path("/inventory?filter[stock][lt]=10")),
                thread: Some(s("stock")),
            }),
        ),
        button(
            "Stop the reminder",
            Role::None,
            DeviceCall::new(CancelNotification { id: s("restock") }),
        ),
        button(
            "Show 3 on the icon",
            Role::None,
            DeviceCall::new(SetBadge { count: Some(3) }),
        ),
        button(
            "Allow notifications in Settings",
            Role::None,
            DeviceCall::new(OpenSettings {
                page: SettingsPage::Notifications,
            }),
        ),
        button(
            "Sign in with a passkey",
            Role::Primary,
            DeviceCall::new(Passkey {
                mode: PasskeyMode::SignIn,
                options: path("/auth/passkeys/login/options"),
                conditional: false,
            })
            .then(then("/auth/passkeys/login", "tk-passkey")),
        ),
        button(
            "Approve",
            Role::Confirm,
            DeviceCall::new(Confirm {
                reason: s("Approve payment"),
                challenge: path("/api/auth/confirm/options"),
                detail: Some(s("₹1,00,000 to Gurpreet Kaur, HDFC ••4821")),
                method: ConfirmMethod::Either,
            })
            // Two requests: the ticket goes to `challenge`, which spends it and
            // answers the passkey request's options; the signature then goes to
            // the continuation, with no ticket.
            .then(then("/payments/42/approve", "tk-confirm"))
            .alt(Alt::Navigate {
                href: path("/payments/42/approve-with-code"),
            }),
        ),
        button(
            "Find the nearest store",
            Role::None,
            DeviceCall::new(LocateOnce {
                accuracy: Accuracy::Approximate,
                max_age_secs: 600,
                timeout_secs: 10,
            })
            .then(then("/stores/nearest", "tk-locate")),
        ),
        button(
            "Scan a product",
            Role::None,
            DeviceCall::new(ScanCode {
                formats: vec![CodeFormat::Ean13, CodeFormat::Code128],
                prompt: Some(s("Scan the SKU on the box")),
            })
            .then(then("/inventory/scanned", "tk-scan"))
            .alt(Alt::Hide),
        ),
        button(
            "Sign in with Apple",
            Role::Primary,
            DeviceCall::new(PlatformSignIn {
                provider: SignInProvider::Apple,
                nonce: path("/api/auth/social/apple/native/nonce"),
                sign_in: path("/api/auth/social/apple/native/session"),
                web: Some(path("/api/auth/social/apple/redirect")),
                client_id: None,
                second_step: Some(path("/sign-in/two-factor")),
            }),
        ),
        Node::Field(Field {
            id: Some(s("receipt")),
            name: s("receipt"),
            label: Some(s("Photo of the receipt")),
            help: None,
            error: None,
            required: false,
            disabled: false,
            purpose: None,
            changed: None,
            input: Input::File(FileInput {
                accept: Some(s("image/*")),
                capture: Some(s("environment")),
                keep_location: true,
                ..FileInput::default()
            }),
        }),
    ];
    Envelope::screen(
        BUILD,
        Screen::new(
            "Pay and share",
            Node::Stack(Stack {
                gap: 2,
                children: calls,
                ..Stack::default()
            }),
        ),
    )
}

/// What a reply may start with no tap.
fn ops_device() -> Envelope {
    Envelope::ops(
        BUILD,
        vec![
            Op::Device {
                call: ReplyCall::Haptic(Haptic::success()),
            },
            Op::Device {
                call: ReplyCall::SetBadge(SetBadge { count: None }),
            },
            Op::Device {
                call: ReplyCall::CancelNotification(CancelNotification { id: s("restock") }),
            },
            Op::Device {
                call: ReplyCall::RequestReview,
            },
            Op::Toast(Toast {
                tone: Tone::Success,
                text: s("Order CH-1042 delivered to Amritsar."),
                ..Toast::default()
            }),
        ],
    )
}

fn run(text: &str) -> Run {
    Run {
        text: s(text),
        ..Run::default()
    }
}

fn bold(text: &str) -> Run {
    Run {
        bold: true,
        ..run(text)
    }
}

fn number(value: i64) -> serde_json::Number {
    value.into()
}

/// A number written without a fraction when it has none (`2`, not `2.0`).
fn decimal(value: f64) -> serde_json::Number {
    if value.fract() == 0.0 {
        number(value as i64)
    } else {
        serde_json::Number::from_f64(value).unwrap()
    }
}

/// A shop's day at a glance: figures with their trends, a gauge, a timeline,
/// an activity feed, the screen's toolbar and a note in rich blocks.
fn screen_dashboard() -> Envelope {
    let figure = |label: &str, value: &str, direction: Direction, by: &str, tone: Tone| {
        Node::Figure(Figure {
            id: None,
            label: s(label),
            value: s(value),
            trend: Some(Trend {
                direction,
                text: Some(s(by)),
                tone: Some(tone),
            }),
            detail: Some(s("against yesterday")),
            tone: None,
            ..Figure::default()
        })
    };
    let moment = |title: &str, time: &str, state: RowState, icon_name: &str| Moment {
        title: s(title),
        time: Some(s(time)),
        state: Some(state),
        icon: Some(icon(icon_name, "M3 7l9-4 9 4v10l-9 4-9-4z")),
        ..Moment::default()
    };
    let toolbar = Node::Toolbar(Toolbar {
        id: Some(s("dashboard-bar")),
        items: vec![
            ToolbarItem {
                title: s("Share"),
                icon: icon("share", "M4 12v8h16v-8M12 3v13M8 7l4-4 4 4"),
                standard: Some(Standard::Share),
                action: Some(Action::navigate("/reports/today/share")),
                ..ToolbarItem::default()
            },
            ToolbarItem {
                id: Some(s("new-order")),
                title: s("New order"),
                icon: icon("plus", "M5 12h14M12 5v14"),
                role: Role::Create,
                prominence: Prominence::High,
                action: Some(Action::Navigate {
                    href: s("/orders/new"),
                    present: Present::Sheet,
                }),
                ..ToolbarItem::default()
            },
            ToolbarItem {
                title: s("Close the day"),
                icon: icon("lock", "M6 11h12v9H6zM8 11V8a4 4 0 0 1 8 0v3"),
                role: Role::Secondary,
                disabled: true,
                reason: Some(s("3 orders are still to be packed.")),
                action: Some(Action::Request {
                    method: Method::Post,
                    href: s("/day/close"),
                    fields: vec![pair("_csrf", "tok")],
                }),
                ..ToolbarItem::default()
            },
            ToolbarItem {
                title: s("More"),
                icon: icon("ellipsis", "M5 12h.01M12 12h.01M19 12h.01"),
                standard: Some(Standard::More),
                role: Role::Secondary,
                menu: vec![
                    Node::Button(Button {
                        label: s("Edit the dashboard"),
                        standard: Some(Standard::Edit),
                        action: Some(Action::navigate("/dashboard/edit")),
                        ..Button::default()
                    }),
                    Node::Button(Button {
                        label: s("Search orders"),
                        standard: Some(Standard::Search),
                        action: Some(Action::navigate("/orders?focus=search")),
                        ..Button::default()
                    }),
                ],
                ..ToolbarItem::default()
            },
        ],
    });
    let body = Node::Stack(Stack {
        gap: 4,
        children: vec![
            Node::Grid(Grid {
                cols: Some(3),
                min: Some(140),
                gap: 3,
                children: vec![
                    figure(
                        "Revenue today",
                        "₹1,24,050",
                        Direction::Up,
                        "12%",
                        Tone::Success,
                    ),
                    figure("Returns", "3", Direction::Down, "2", Tone::Success),
                    figure("Orders", "212", Direction::Flat, "0", Tone::Neutral),
                ],
                ..Grid::default()
            }),
            Node::Gauge(Gauge {
                id: Some(s("space")),
                value: number(72),
                min: number(0),
                max: number(100),
                label: Some(s("Ludhiana warehouse space")),
                shown: Some(s("72%")),
                tone: Some(Tone::Warning),
                ..Gauge::default()
            }),
            Node::Section(Section {
                title: Some(s("CH-1042 today")),
                style: SectionStyle::Card,
                children: vec![Node::Timeline(Timeline {
                    id: Some(s("order-day")),
                    items: vec![
                        Moment {
                            subtitle: Some(s("3 units for ₹1,24,050")),
                            detail: vec![muted("Paid by UPI")],
                            ..moment("Placed", "09:41", RowState::Done, "receipt")
                        },
                        moment("Packed", "11:05", RowState::Done, "package"),
                        // What a machine did, among what people did.
                        Moment {
                            muted: true,
                            ..moment("Reminder sent to the carrier", "11:30", RowState::Done, "bell")
                        },
                        Moment {
                            tone: Some(Tone::Info),
                            ..moment("Shipping", "Next", RowState::Current, "truck")
                        },
                        moment("Delivered", "Later", RowState::Upcoming, "house"),
                    ],
                })],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("Activity")),
                children: vec![Node::List(List {
                    id: Some(s("activity")),
                    more: Some(s("/activity?after=4410")),
                    children: vec![
                        Node::FeedRow(FeedRow {
                            id: Some(s("activity-4412")),
                            actor: Some(Avatar {
                                id: None,
                                name: s("Ravneet Kaur"),
                                src: None,
                                initials: Some(s("RK")),
                            }),
                            runs: vec![
                                bold("Ravneet Kaur"),
                                run(" packed "),
                                Run {
                                    href: Some(s("/orders/1042")),
                                    ..run("CH-1042")
                                },
                            ],
                            time: Some(s("11:05")),
                            action: Some(Action::navigate("/orders/1042")),
                            ..FeedRow::default()
                        }),
                        Node::FeedRow(FeedRow {
                            id: Some(s("activity-4411")),
                            icon: Some(icon("triangle-alert", "M12 9v4M12 17h.01M3 20h18L12 4z")),
                            runs: vec![
                                run("Stock of "),
                                bold("Brass Kadhai"),
                                run(" fell below 10"),
                            ],
                            time: Some(s("10:30")),
                            tone: Some(Tone::Warning),
                            detail: vec![muted("9 left on the shelf in Amritsar")],
                            ..FeedRow::default()
                        }),
                    ],
                    ..List::default()
                })],
                ..Section::default()
            }),
            Node::Rich(Rich {
                id: Some(s("note")),
                blocks: vec![
                    Block {
                        kind: BlockKind::Heading,
                        runs: vec![run("Before Diwali")],
                        level: 2,
                    },
                    Block {
                        kind: BlockKind::Paragraph,
                        runs: vec![run("Orders double in the week before. "), bold("Restock early.")],
                        level: 0,
                    },
                    Block {
                        kind: BlockKind::Bullet,
                        runs: vec![run("Brass Kadhai: 40 more")],
                        level: 0,
                    },
                    Block {
                        kind: BlockKind::Bullet,
                        runs: vec![run("Diyas, a set of 12: 200 more")],
                        level: 1,
                    },
                    Block {
                        kind: BlockKind::Numbered,
                        runs: vec![run("Call the Jalandhar supplier")],
                        level: 0,
                    },
                    Block {
                        kind: BlockKind::Quote,
                        runs: vec![run("Ship nothing after the 28th that can't arrive by the 31st.")],
                        level: 0,
                    },
                    Block {
                        kind: BlockKind::Code,
                        runs: vec![run("SKU BK-12 x 40")],
                        level: 0,
                    },
                ],
                ..Rich::default()
            }),
            Node::Button(Button {
                label: s("Delete this draft"),
                role: Role::Destructive,
                standard: Some(Standard::Delete),
                undoable: true,
                action: Some(Action::Request {
                    method: Method::Post,
                    href: s("/drafts/31/delete"),
                    fields: vec![pair("_csrf", "tok")],
                }),
                ..Button::default()
            }),
            Node::Stack(Stack {
                axis: Axis::Horizontal,
                gap: 2,
                justify: Some(Justify::End),
                children: vec![
                    Node::Button(Button {
                        label: s("Add a product"),
                        standard: Some(Standard::Add),
                        action: Some(Action::navigate("/products/new")),
                        ..Button::default()
                    }),
                    Node::Button(Button {
                        label: s("Cancel"),
                        standard: Some(Standard::Cancel),
                        role: Role::Cancel,
                        action: Some(Action::Back),
                        ..Button::default()
                    }),
                    Node::Button(Button {
                        label: s("Done"),
                        standard: Some(Standard::Done),
                        role: Role::Confirm,
                        action: Some(Action::submit()),
                        ..Button::default()
                    }),
                ],
                ..Stack::default()
            }),
            Node::Split(Split {
                id: Some(s("sales-split")),
                primary: Box::new(Node::Pane(Pane {
                    role: PaneRole::Content,
                    title: Some(s("Sales")),
                    current: true,
                    children: vec![text("₹8,40,300 this week")],
                    ..Pane::default()
                })),
                detail: Box::new(Node::Pane(Pane {
                    role: PaneRole::Supporting,
                    title: Some(s("Filters")),
                    children: vec![text("Punjab, all channels")],
                    ..Pane::default()
                })),
            }),
        ],
        ..Stack::default()
    });
    let mut screen = Screen::new("Today", body);
    screen.nav = Some(s("/"));
    screen.refresh = true;
    screen.actions = vec![toolbar];
    // Stamped as a server stamps every answer, with its icon shapes sent once.
    let mut envelope =
        Envelope::screen(BUILD, screen).stamped("app-7f3a91c2", shell().stamped().stamp);
    envelope.share_icons();
    envelope
}

/// A person's day, read at a glance: the health score and the five pillars it
/// is made of.
fn screen_today() -> Envelope {
    let pillar = |name: &str, key: &str, score: Option<i64>, hue: Hue, weight: &str| {
        let summary = match score {
            Some(score) => format!("{name} score: {score} of 100, {score}%."),
            None => format!("{name} score: no reading."),
        };
        Node::Link(Link {
            id: Some(format!("pillar-{key}")),
            label: format!("{name}, {weight}"),
            action: Action::navigate(format!("/{key}")),
            children: vec![Node::Stack(Stack {
                gap: 1,
                align: Some(Align::Center),
                children: vec![
                    Node::Ring(Ring {
                        id: Some(format!("{key}-score")),
                        value: score.map(number),
                        max: number(100),
                        shown: score.map(|score| score.to_string()),
                        colour: Some(Colour::new(hue)),
                        // A day with no reading is an empty ring with a mark in its middle.
                        centre: if score.is_some() { Centre::Value } else { Centre::Icon },
                        icon: score.is_none().then(|| {
                            icon("brain", "M12 5a3 3 0 1 0-5.9 1A4 4 0 0 0 6 14a4 4 0 0 0 6 3zM12 5a3 3 0 1 1 5.9 1A4 4 0 0 1 18 14a4 4 0 0 1-6 3z")
                        }),
                        summary: Some(summary),
                        ..Ring::default()
                    }),
                    Node::Text(Text {
                        value: s(name),
                        style: TextStyle::Headline,
                        ..Text::default()
                    }),
                    muted(weight),
                ],
                ..Stack::default()
            })],
        })
    };
    let score = Node::Ring(Ring {
        id: Some(s("health-score")),
        value: Some(number(83)),
        max: number(100),
        shown: Some(s("83")),
        label: Some(s("Health score")),
        colour: Some(Colour::blend(Hue::Cyan, Hue::Violet)),
        prominence: Prominence::High,
        summary: Some(s("Health score: 83 of 100, 83%.")),
        hint: Some(Hint {
            about: s("the health score"),
            title: Some(s("How the score is made")),
            text: s("Your health score blends five pillars, each scored from 0 to 100 from your own readings. A pillar with no data that day is left out and the rest are weighed up. It describes a pattern in your readings; it is not advice."),
            mark: HintMark::Question,
        }),
        ..Ring::default()
    });
    // Three values on rings inside each other, the first outside. The second
    // is past its goal.
    let move_ring = |name: &str, value: i64, goal: i64, unit: &str, hue: Hue| RingValue {
        name: s(name),
        value: Some(number(value)),
        max: number(goal),
        unit: Some(s(unit)),
        shown: Some(value.to_string()),
        colour: Some(Colour::new(hue)),
    };
    let activity = Node::Rings(Rings {
        id: Some(s("activity-rings")),
        title: Some(s("Activity rings")),
        rings: vec![
            move_ring("Move", 540, 600, "kcal", Hue::Red),
            move_ring("Exercise", 38, 30, "min", Hue::Green),
            move_ring("Stand", 9, 12, "hr", Hue::Cyan),
        ],
        centre: RingsCentre::Values,
        legend: true,
        summary: Some(s("Activity rings: Move 540 of 600 kcal, 90%. Exercise 38 of 30 min, 127%. Stand 9 of 12 hr, 75%.")),
        ..Rings::default()
    });
    // The day's figures: what the extended tile carries (an icon in its hue,
    // a unit, a status, a caption, a small ring beside it, the page it leads to).
    let small_ring = |value: i64, goal: i64, hue: Hue, summary: &str| {
        Node::Ring(Ring {
            value: Some(number(value)),
            max: number(goal),
            colour: Some(Colour::new(hue)),
            centre: Centre::Nothing,
            prominence: Prominence::Low,
            summary: Some(s(summary)),
            ..Ring::default()
        })
    };
    let tile = |label: &str, value: &str, unit: Option<&str>, glyph: Icon, hue: Hue, caption: &str, href: &str| Figure {
        id: Some(format!("figure-{}", label.to_lowercase().replace(' ', "-"))),
        label: s(label),
        value: s(value),
        unit: unit.map(s),
        icon: Some(glyph),
        detail: Some(s(caption)),
        colour: Some(Colour::new(hue)),
        action: Some(Action::navigate(href)),
        ..Figure::default()
    };
    let figures = Node::Grid(Grid {
        id: Some(s("activity-figures")),
        cols: Some(2),
        min: Some(150),
        gap: 3,
        children: vec![
            Node::Figure(Figure {
                aside: Some(Box::new(small_ring(9412, 10_000, Hue::Green, "Steps: 9,412 of 10,000, 94%."))),
                ..tile(
                    "Steps",
                    "9,412",
                    None,
                    icon("footprints", "M4 16v-2.4C4 11 5 9 7 9s3 2 3 4.6V16zM14 12v-2.4C14 7 15 5 17 5s3 2 3 4.6V12z"),
                    Hue::Green,
                    "94% of 10,000",
                    "/fitness/steps",
                )
            }),
            Node::Figure(tile(
                "Active energy",
                "540",
                Some("kcal"),
                icon("flame", "M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.4-.5-2-1-3-1.1-2.3.1-4.4 2-5.5.3 2.4 2 4 3.6 5.4 1.4 1.3 2.4 3 2.4 5.1a6 6 0 0 1-12 0c0-.9.3-1.8.5-2.5"),
                Hue::Orange,
                "90% of 600 kcal",
                "/fitness/energy",
            )),
            Node::Figure(tile(
                "Distance",
                "6.4",
                Some("km"),
                icon("route", "M6 19a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM9 16h6a3 3 0 0 0 0-6H9"),
                Hue::Cyan,
                "Across Ludhiana and back",
                "/fitness/distance",
            )),
            Node::Figure(Figure {
                status: Some(Badge {
                    text: s("Goal reached"),
                    tone: Some(Tone::Success),
                    ..Badge::default()
                }),
                trend: Some(Trend {
                    direction: Direction::Up,
                    text: Some(s("8 min")),
                    tone: Some(Tone::Success),
                }),
                ..tile(
                    "Exercise",
                    "38",
                    Some("min"),
                    icon("timer", "M10 2h4M12 14l3-3M12 22a8 8 0 1 0 0-16 8 8 0 0 0 0 16z"),
                    Hue::Green,
                    "against your goal of 30",
                    "/fitness/exercise",
                )
            }),
        ],
    });
    // The day as 24 wedges round a clock: steps an hour, what each hour was,
    // and the moments that stand out. It is 8 in the evening; the hours to come
    // have no value yet.
    let steps = [
        0, 0, 0, 0, 0, 0, 120, 1850, 940, 520, 380, 610, 760, 300, 280, 350, 600, 1200, 940, 420,
        142,
    ];
    let levels = [
        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.2, 0.9, 0.5, 0.3, 0.25, 0.4, 0.5, 0.2, 0.2, 0.25, 0.4,
        0.8, 0.55, 0.3, 0.1,
    ];
    // 0 active, 1 calm, 2 rest: the hours of the night are rest.
    let kinds = [2, 2, 2, 2, 2, 2, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1];
    let hours: Vec<Hour> = (0..24)
        .map(|hour| match steps.get(hour) {
            Some(count) => Hour {
                value: Some(number(*count)),
                level: Some(decimal(levels[hour])),
                category: Some(kinds[hour]),
            },
            None => Hour::default(),
        })
        .collect();
    let moment = |at: f64, name: &str, glyph: Icon, hue: Hue| ClockMoment {
        at: decimal(at),
        name: s(name),
        icon: Some(glyph),
        colour: Some(Colour::new(hue)),
    };
    let day = Node::DayClock(DayClock {
        id: Some(s("day-clock")),
        title: Some(s("Steps by hour")),
        hours,
        key: vec![
            Category {
                name: s("Active"),
                colour: Some(Colour::new(Hue::Green)),
                detail: None,
            },
            Category {
                name: s("Calm"),
                colour: Some(Colour::new(Hue::Cyan)),
                detail: None,
            },
            Category {
                name: s("Rest"),
                colour: Some(Colour::new(Hue::Violet)),
                detail: None,
            },
        ],
        moments: vec![
            moment(
                6.5,
                "Woke up, 6:30 am",
                icon("sunrise", "M12 2v6M4.9 10.9l1.4 1.4M2 18h2M20 18h2M17.7 12.3l1.4-1.4M22 22H2M16 18a4 4 0 0 0-8 0"),
                Hue::Amber,
            ),
            moment(
                7.25,
                "Morning run, 3.2 km",
                icon("footprints", "M4 16v-2.4C4 11 5 9 7 9s3 2 3 4.6V16zM14 12v-2.4C14 7 15 5 17 5s3 2 3 4.6V12z"),
                Hue::Green,
            ),
            moment(
                18.4,
                "Highest heart rate, 158 bpm",
                icon("heart-pulse", "M19.5 12.6 12 20l-7.5-7.4a5 5 0 0 1 7.5-6.6 5 5 0 0 1 7.5 6.6zM3 12h4l2-3 3 6 2-3h5"),
                Hue::Rose,
            ),
        ],
        now: Some(20),
        unit: Some(s("steps")),
        shown: Some(s("9,412")),
        label: Some(s("Steps")),
        icon: Some(icon("footprints", "M4 16v-2.4C4 11 5 9 7 9s3 2 3 4.6V16zM14 12v-2.4C14 7 15 5 17 5s3 2 3 4.6V12z")),
        colour: Some(Colour::new(Hue::Blue)),
        hub: Some(Colour::new(Hue::Violet)),
        summary: Some(s("9,412 steps in the day so far. Busiest at 7 am: 1,850. 3 moments: Woke up, 6:30 am, Morning run, 3.2 km, Highest heart rate, 158 bpm.")),
        ..DayClock::default()
    });
    // The control that steps the screen from day to day, with the day's three
    // scores as small rings before the date.
    let pager = Node::DayPager(DayPager {
        id: Some(s("day")),
        day: s("Today"),
        date: s("2026-11-13"),
        previous: Some(s("/?date=2026-11-12")),
        next: None,
        today: None,
        first: Some(s("2026-01-02")),
        last: Some(s("2026-11-13")),
        pick: Some(DayPick {
            href: s("/"),
            name: s("date"),
        }),
        scores: Some(Box::new(Node::Rings(Rings {
            id: Some(s("day-scores")),
            rings: vec![
                RingValue {
                    name: s("Movement"),
                    value: Some(number(35)),
                    shown: Some(s("35")),
                    colour: Some(Colour::new(Hue::Orange)),
                    ..RingValue::default()
                },
                RingValue {
                    name: s("Recovery"),
                    value: Some(number(72)),
                    shown: Some(s("72")),
                    colour: Some(Colour::new(Hue::Green)),
                    ..RingValue::default()
                },
                RingValue {
                    name: s("Sleep"),
                    value: Some(number(78)),
                    shown: Some(s("78")),
                    colour: Some(Colour::new(Hue::Violet)),
                    ..RingValue::default()
                },
            ],
            prominence: Prominence::Low,
            summary: Some(s("Today's scores: Movement 35, Recovery 72, Sleep 78.")),
            ..Rings::default()
        }))),
        words: Some(PagerWords {
            previous: s("Previous day"),
            next: s("Next day"),
            choose: s("Choose a day"),
        }),
    });
    let body = Node::Stack(Stack {
        gap: 4,
        children: vec![
            pager,
            Node::Text(Text {
                value: s("Good evening, Amandeep"),
                style: TextStyle::LargeTitle,
                heading: Some(1),
                ..Text::default()
            }),
            muted("Your Friday, 13 November, from your own readings."),
            Node::Section(Section {
                style: SectionStyle::Card,
                children: vec![
                    Node::Stack(Stack {
                        gap: 3,
                        align: Some(Align::Center),
                        children: vec![
                            score,
                            Node::Stack(Stack {
                                axis: Axis::Horizontal,
                                gap: 2,
                                align: Some(Align::Center),
                                children: vec![
                                    Node::Badge(Badge {
                                        text: s("Excellent"),
                                        tone: Some(Tone::Success),
                                        ..Badge::default()
                                    }),
                                    muted("2.4% lower than your last 7 days"),
                                ],
                                ..Stack::default()
                            }),
                        ],
                        ..Stack::default()
                    }),
                    Node::Grid(Grid {
                        id: Some(s("pillars")),
                        cols: Some(3),
                        min: Some(96),
                        gap: 3,
                        children: vec![
                            pillar("Sleep", "sleep", Some(87), Hue::Violet, "25% of score"),
                            pillar("Heart", "heart", Some(90), Hue::Rose, "20% of score"),
                            pillar("Recovery", "recovery", Some(60), Hue::Green, "20% of score"),
                            pillar("Movement", "fitness", Some(78), Hue::Orange, "20% of score"),
                            pillar("Mind", "mind", None, Hue::Cyan, "No data"),
                        ],
                    }),
                ],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("Your day")),
                style: SectionStyle::Card,
                children: vec![day],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("Activity")),
                style: SectionStyle::Card,
                children: vec![activity, figures],
                hint: Some(Hint {
                    about: s("how activity is counted"),
                    text: s("Move counts the energy your body spent moving, Exercise the minutes of brisk activity and Stand the hours in which you stood and moved for a minute."),
                    mark: HintMark::Info,
                    ..Hint::default()
                }),
                ..Section::default()
            }),
        ],
        ..Stack::default()
    });
    let mut screen = Screen::new("Today", body);
    screen.nav = Some(s("/"));
    screen.refresh = true;
    Envelope::screen(BUILD, screen)
}

/// A night's sleep: how long, what the night was made of, and, as the kinds are
/// added, its stages through the hours, its cycles and the week it sits in.
fn screen_sleep() -> Envelope {
    // The control that steps from night to night, with the night's score as a
    // small ring before the date. A night that is not the last has a day after
    // it, and a way back to today.
    let pager = Node::DayPager(DayPager {
        id: Some(s("night")),
        day: s("Thursday night"),
        date: s("2026-11-12"),
        previous: Some(s("/sleep?date=2026-11-11")),
        next: Some(s("/sleep?date=2026-11-13")),
        today: Some(s("/sleep")),
        first: Some(s("2026-01-02")),
        last: Some(s("2026-11-13")),
        pick: Some(DayPick {
            href: s("/sleep"),
            name: s("date"),
        }),
        scores: Some(Box::new(Node::Ring(Ring {
            value: Some(number(78)),
            max: number(100),
            shown: Some(s("78")),
            colour: Some(Colour::new(Hue::Violet)),
            centre: Centre::Nothing,
            prominence: Prominence::Low,
            summary: Some(s("Sleep score: 78 of 100, 78%.")),
            ..Ring::default()
        }))),
        words: Some(PagerWords {
            previous: s("Previous night"),
            next: s("Next night"),
            choose: s("Choose a night"),
        }),
    });
    let asleep = Node::Figure(Figure {
        id: Some(s("asleep")),
        label: s("Time asleep"),
        value: s("7 h 12 min"),
        icon: Some(icon("moon", "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z")),
        trend: Some(Trend {
            direction: Direction::Up,
            text: Some(s("18 min")),
            tone: Some(Tone::Success),
        }),
        detail: Some(s("against the week before")),
        colour: Some(Colour::new(Hue::Violet)),
        action: Some(Action::navigate("/sleep/week")),
        ..Figure::default()
    });
    // One bar split into the stages of the night: minutes, written as the page
    // writes durations.
    let stage = |name: &str, minutes: i64, shown: &str, hue: Hue, detail: &str| Segment {
        name: s(name),
        value: Some(number(minutes)),
        shown: Some(s(shown)),
        detail: Some(s(detail)),
        colour: Some(Colour::new(hue)),
    };
    let stages = Node::Meter(Meter {
        id: Some(s("sleep-stages")),
        title: Some(s("Sleep stages, last night")),
        segments: vec![
            stage("Deep", 83, "1 h 23 min", Hue::Violet, "19% of the night"),
            stage("REM", 96, "1 h 36 min", Hue::Cyan, "22% of the night"),
            stage("Core", 253, "4 h 13 min", Hue::Blue, "56% of the night"),
            stage("Awake", 21, "21 min", Hue::Amber, "5% of the night"),
        ],
        summary: Some(s("Sleep stages, last night: Deep 1 h 23 min, REM 1 h 36 min, Core 4 h 13 min, Awake 21 min.")),
        ..Meter::default()
    });
    // The cycles of the night, each split into the same three stages and drawn
    // as a whole, so they compare what they were made of. The third, with the
    // most REM, stands out.
    // Each part says its time over the night beside its name, as the stages'
    // meter writes it.
    let part = |name: &str, hue: Hue, total: &str| Category {
        name: s(name),
        colour: Some(Colour::new(hue)),
        detail: Some(s(total)),
    };
    let cycle = |label: &str, minutes: [i64; 3], total: &str| MiniColumn {
        label: s(label),
        values: minutes.iter().map(|value| Some(number(*value))).collect(),
        shown: Some(s(total)),
    };
    let cycles = Node::Columns(MiniColumns {
        id: Some(s("sleep-cycles")),
        title: Some(s("Sleep cycles")),
        parts: vec![
            part("Deep", Hue::Violet, "1 h 23 min"),
            part("REM", Hue::Cyan, "1 h 36 min"),
            part("Core", Hue::Blue, "4 h 13 min"),
        ],
        columns: vec![
            cycle("Cycle 1", [36, 18, 40], "1 h 34 min"),
            cycle("Cycle 2", [22, 30, 41], "1 h 33 min"),
            cycle("Cycle 3", [14, 36, 38], "1 h 28 min"),
            cycle("Cycle 4", [11, 12, 34], "57 min"),
        ],
        highlight: Some(2),
        summary: Some(s("Sleep cycles: 4 cycles of Deep, REM and Core. The longest, Cycle 1, was 1 h 34 min.")),
        ..MiniColumns::default()
    });
    // The night in half hours: every cell is the stage that half hour was in,
    // so the strip is in proportion to time.
    // Each stage says its share of the night in the page's own words, so no
    // client works one out from the cells.
    let stage = |name: &str, hue: Hue, share: &str| Category {
        name: s(name),
        colour: Some(Colour::new(hue)),
        detail: Some(s(share)),
    };
    let night = [
        ("11:00 pm", 3), ("11:30 pm", 2), ("12:00 am", 2), ("12:30 am", 0), ("1:00 am", 0),
        ("1:30 am", 2), ("2:00 am", 1), ("2:30 am", 2), ("3:00 am", 2), ("3:30 am", 0),
        ("4:00 am", 2), ("4:30 am", 1), ("5:00 am", 1), ("5:30 am", 2), ("6:00 am", 3),
    ];
    let strip = Node::Strip(ColorStrip {
        id: Some(s("sleep-strip")),
        title: Some(s("The night in half hours")),
        key: vec![
            stage("Deep", Hue::Violet, "19% of the night"),
            stage("REM", Hue::Cyan, "22% of the night"),
            stage("Core", Hue::Blue, "56% of the night"),
            stage("Awake", Hue::Amber, "5% of the night"),
        ],
        cells: night
            .iter()
            .map(|(label, category)| StripCell {
                label: s(label),
                category: Some(*category),
                value: None,
            })
            .collect(),
        shares: true,
        summary: Some(s("The night in half hours: 15 cells from 11:00 pm to 6:00 am. Core was the most common stage, 7 of 15.")),
        ..ColorStrip::default()
    });
    // Where one value sits between a low and a high end, on its zones: the heart
    // rate while asleep.
    let rate_zone = |from: i64, to: i64, label: &str, hue: Hue| Zone {
        from: number(from),
        to: number(to),
        label: Some(s(label)),
        colour: Some(Colour::new(hue)),
    };
    let resting = Node::Range(RangeScale {
        id: Some(s("resting-rate")),
        title: Some(s("Resting heart rate overnight")),
        value: Some(number(54)),
        min: number(40),
        max: number(120),
        unit: Some(s("bpm")),
        shown: Some(s("54")),
        low: Some(s("Slower")),
        high: Some(s("Faster")),
        zones: vec![
            rate_zone(40, 50, "Lower", Hue::Cyan),
            rate_zone(50, 70, "Usual", Hue::Success),
            rate_zone(70, 90, "Higher", Hue::Warning),
            rate_zone(90, 120, "Much higher", Hue::Danger),
        ],
        summary: Some(s("Resting heart rate overnight: 54 bpm, on a scale from 40 to 120 bpm. Usual: 50 to 70 bpm.")),
        hint: Some(Hint {
            about: s("the resting heart rate"),
            text: s("The lowest heart rate you held while still. It describes your own nights and moves with sleep, training and the weather; it is not advice."),
            mark: HintMark::Info,
            ..Hint::default()
        }),
    });
    // The week's nights as bars, with what the person needs drawn across them
    // and the usual range behind them.
    let week = Node::Chart(Chart {
        id: Some(s("sleep-week")),
        kind: ChartKind::Bar,
        title: Some(s("Hours asleep, last 7 nights")),
        labels: ["Fri", "Sat", "Sun", "Mon", "Tue", "Wed", "Thu"].map(s).to_vec(),
        series: vec![Series {
            label: s("Asleep"),
            values: [6.8, 7.4, 7.0, 6.2, 7.9, 8.1, 7.2].map(|hours| Some(decimal(hours))).to_vec(),
            shown: ["6 h 48 min", "7 h 24 min", "7 h", "6 h 12 min", "7 h 54 min", "8 h 6 min", "7 h 12 min"]
                .map(s)
                .to_vec(),
            colour: Some(Colour::new(Hue::Violet)),
            ..Series::default()
        }],
        x: Some(Measure {
            name: Some(s("Night")),
            ..Measure::default()
        }),
        y: Some(Measure {
            unit: Some(s("h")),
            min: Some(number(0)),
            max: Some(number(10)),
            ..Measure::default()
        }),
        references: vec![Reference {
            kind: ReferenceKind::Level,
            value: Some(number(8)),
            label: Some(s("Your need: 8 h")),
            colour: Some(Colour::new(Hue::Amber)),
        }],
        zones: vec![Zone {
            from: number(7),
            to: number(9),
            label: Some(s("Usual range")),
            colour: Some(Colour::new(Hue::Green)),
        }],
        summary: Some(s("Hours asleep, last 7 nights: from 6 h 12 min on Monday to 8 h 6 min on Wednesday. Your need is 8 h.")),
        hint: Some(Hint {
            about: s("hours asleep"),
            text: s("Time asleep each night, against the hours you said you need. It describes your own nights and is not advice."),
            ..Hint::default()
        }),
        ..Chart::default()
    });
    // The night's stages through the hours: each holds until the next, on lanes
    // named for the stages.
    let hypnogram = Node::Chart(Chart {
        id: Some(s("sleep-hypnogram")),
        kind: ChartKind::Area,
        title: Some(s("Sleep stages through the night")),
        labels: [
            "11:00 pm", "11:30 pm", "12:00 am", "12:30 am", "1:00 am", "1:30 am", "2:00 am",
            "2:30 am", "3:00 am", "3:30 am", "4:00 am", "4:30 am",
        ]
        .map(s)
        .to_vec(),
        series: vec![Series {
            label: s("Stage"),
            values: [3, 1, 1, 0, 0, 1, 2, 1, 1, 0, 1, 2].map(|stage| Some(number(stage))).to_vec(),
            colour: Some(Colour::new(Hue::Violet)),
            ..Series::default()
        }],
        levels: ["Deep", "Core", "REM", "Awake"].map(s).to_vec(),
        curve: Curve::Step,
        summary: Some(s("Sleep stages through the night: awake at 11:00 pm, then Core, Deep from 12:30 am to 1:30 am, and REM at 2:00 am and 4:30 am.")),
        ..Chart::default()
    });
    let oxygen = Node::Figure(Figure {
        id: Some(s("oxygen")),
        label: s("Blood oxygen"),
        value: s("96"),
        unit: Some(s("%")),
        icon: Some(icon("droplet", "M12 22a7 7 0 0 0 7-7c0-4-7-13-7-13S5 11 5 15a7 7 0 0 0 7 7z")),
        detail: Some(s("overnight average")),
        colour: Some(Colour::new(Hue::Info)),
        ..Figure::default()
    });
    let body = Node::Stack(Stack {
        gap: 4,
        children: vec![
            pager,
            Node::Grid(Grid {
                cols: Some(2),
                min: Some(150),
                gap: 3,
                children: vec![asleep, oxygen],
                ..Grid::default()
            }),
            Node::Section(Section {
                title: Some(s("Stages")),
                style: SectionStyle::Card,
                children: vec![stages],
                hint: Some(Hint {
                    about: s("the estimated stages"),
                    title: Some(s("An estimate")),
                    text: s("Stages are estimated from movement and heart rate while you sleep. They show a pattern in your nights, not a measurement of your brain."),
                    mark: HintMark::Info,
                }),
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("Cycles")),
                style: SectionStyle::Card,
                children: vec![cycles],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("Through the night")),
                style: SectionStyle::Card,
                children: vec![strip],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("Overnight heart rate")),
                style: SectionStyle::Card,
                children: vec![resting],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("The week")),
                style: SectionStyle::Card,
                children: vec![hypnogram, week],
                ..Section::default()
            }),
        ],
        ..Stack::default()
    });
    let mut screen = Screen::new("Sleep", body);
    screen.nav = Some(s("/sleep"));
    screen.refresh = true;
    Envelope::screen(BUILD, screen)
}

/// A week of training: how hard it was against the weeks before, and, as the
/// kinds are added, the calendar, the links between measures and the beats.
fn screen_fitness() -> Envelope {
    let zone = |from: f64, to: f64, label: &str, hue: Hue| Zone {
        from: decimal(from),
        to: decimal(to),
        label: Some(s(label)),
        colour: Some(Colour::new(hue)),
    };
    let load = Node::Gauge(Gauge {
        id: Some(s("load-ratio")),
        value: decimal(1.12),
        min: number(0),
        max: number(2),
        label: Some(s("Acute to chronic load ratio")),
        shown: Some(s("1.12")),
        caption: Some(s("Optimal")),
        zones: vec![
            zone(0.0, 0.8, "Undertraining", Hue::Violet),
            zone(0.8, 1.3, "Optimal", Hue::Green),
            zone(1.3, 1.5, "Overreaching", Hue::Amber),
            zone(1.5, 2.0, "Overuse", Hue::Red),
        ],
        summary: Some(s("Acute to chronic load ratio: 1.12, on a scale from 0 to 2. Optimal: from 0.8 to 1.3.")),
        hint: Some(Hint {
            about: s("the load ratio"),
            title: Some(s("What the ratio compares")),
            text: s("This week's training load against the average of the four weeks before it. It describes your own pattern of training and is not advice."),
            ..Hint::default()
        }),
        ..Gauge::default()
    });
    let vo2 = Node::Gauge(Gauge {
        id: Some(s("vo2-max")),
        value: decimal(44.2),
        min: number(20),
        max: number(70),
        label: Some(s("VO2 max")),
        shown: Some(s("44.2 mL/kg/min")),
        colour: Some(Colour::blend(Hue::Green, Hue::Cyan)),
        summary: Some(s("VO2 max: 44.2 mL/kg/min, on a scale from 20 to 70.")),
        ..Gauge::default()
    });
    // A heart rate every minute of a run, each cell the zone the reading fell in.
    // A minute with no reading has no zone.
    let zones = ["Easy", "Steady", "Hard"];
    let readings: [Option<i64>; 12] = [
        Some(92), Some(104), Some(118), Some(131), Some(142), Some(149), None, Some(151),
        Some(147), Some(138), Some(126), Some(112),
    ];
    let run = Node::Strip(ColorStrip {
        id: Some(s("run-zones")),
        title: Some(s("Heart rate zones, morning run")),
        key: vec![
            Category {
                name: s(zones[0]),
                colour: Some(Colour::new(Hue::Blue)),
                detail: None,
            },
            Category {
                name: s(zones[1]),
                colour: Some(Colour::new(Hue::Green)),
                detail: None,
            },
            Category {
                name: s(zones[2]),
                colour: Some(Colour::new(Hue::Amber)),
                detail: None,
            },
        ],
        cells: readings
            .iter()
            .enumerate()
            .map(|(minute, reading)| StripCell {
                label: format!("7:{:02} am", 15 + minute),
                category: reading.map(|bpm| if bpm < 110 { 0 } else if bpm < 140 { 1 } else { 2 }),
                value: reading.map(number),
            })
            .collect(),
        label: Some(s("Heart rate")),
        unit: Some(s("bpm")),
        summary: Some(s("Heart rate zones, morning run: 12 minutes. Steady for 5, Hard for 4, Easy for 2; one minute has no reading.")),
        ..ColorStrip::default()
    });
    // Ten weeks of training, a day a cell: a rest day is a day of nothing, and a
    // day with no reading is not a day of nothing.
    let mut loads: Vec<(String, Option<i64>)> = Vec::new();
    for (month, first, last) in [(9, 5, 30), (10, 1, 31), (11, 1, 13)] {
        for day in first..=last {
            let at = loads.len() as i64;
            let load = at * 37 % 130;
            let value = if at % 9 == 4 {
                None
            } else if load < 20 {
                Some(0)
            } else {
                Some(load)
            };
            loads.push((format!("2026-{month:02}-{day:02}"), value));
        }
    }
    let active = loads.iter().filter(|(_, load)| load.is_some_and(|load| load > 0)).count();
    let missing = loads.iter().filter(|(_, load)| load.is_none()).count();
    let highest = loads.iter().filter_map(|(_, load)| *load).max().unwrap_or(0);
    let calendar = Node::CalendarHeatmap(CalendarHeatmap {
        id: Some(s("training-calendar")),
        title: Some(s("Training calendar")),
        days: loads
            .iter()
            .map(|(date, load)| HeatDay {
                date: s(date),
                value: load.map(number),
            })
            .collect(),
        first: Some(s("2026-09-05")),
        last: Some(s("2026-11-13")),
        colour: Some(Colour::new(Hue::Accent)),
        steps: vec![number(30), number(60), number(90)],
        label: Some(s("Training load")),
        unit: Some(s("load")),
        summary: Some(format!(
            "Training calendar: {} days from 5 Sep to 13 Nov, {active} with training and {missing} with no reading. Highest load {highest}.",
            loads.len()
        )),
        ..CalendarHeatmap::default()
    });
    // Every measure against every other, each pair once: which move together
    // (blue) and which apart (orange), how strongly, and which are chance.
    let pair = |a: u32, b: u32, value: f64, note: &str, significant: bool| Relation {
        a,
        b,
        value: Some(decimal(value)),
        significant,
        note: Some(s(note)),
    };
    let links = Node::MatrixHeatmap(MatrixHeatmap {
        id: Some(s("what-moves-together")),
        title: Some(s("What moves together")),
        measures: ["Sleep", "Stress", "Steps", "HRV"].map(s).to_vec(),
        relations: vec![
            pair(0, 1, -0.58, "30 days, p < 0.001", true),
            pair(0, 3, 0.71, "30 days, p < 0.001", true),
            pair(1, 3, -0.34, "30 days, p = 0.07", false),
            pair(0, 2, 0.22, "30 days, p = 0.03", true),
            pair(2, 3, 0.18, "30 days, p = 0.2", false),
            // A pair never tested has no value.
            Relation {
                a: 1,
                b: 2,
                value: None,
                significant: false,
                note: Some(s("Too few days with both readings")),
            },
        ],
        lower: true,
        positive: Some(Colour::new(Hue::Blue)),
        negative: Some(Colour::new(Hue::Orange)),
        peak: Some(number(1)),
        label: Some(s("Correlation")),
        summary: Some(s("What moves together: 4 measures, 6 pairs, 5 tested, 3 significant. Strongest: Sleep and HRV, 0.71.")),
        hint: Some(Hint {
            about: s("what moves together"),
            title: Some(s("How to read it")),
            text: s("Each cell compares two measures over your last 30 days: blue where they rise and fall together, orange where one rises as the other falls. A grey cell is a pattern that may be chance. It shows a pattern, never a cause."),
            ..Hint::default()
        }),
    });
    // The time between one heartbeat and the next, each against the one before:
    // a cloud of points, on axes of one range so that the diagonal is "no
    // change". One cloud is the resting beats and one the beats after a run.
    let beats = [812, 820, 805, 830, 841, 823, 818, 836, 829, 811, 827, 845, 838, 822, 815, 833, 826];
    let after = [684, 691, 678, 702, 695, 688, 710, 699, 693];
    let cloud = |intervals: &[i64]| -> Vec<(i64, i64)> {
        intervals.windows(2).map(|pair| (pair[0], pair[1])).collect()
    };
    let resting_beats = cloud(&beats);
    let run_beats = cloud(&after);
    let poincare = Node::Scatter(Scatter {
        id: Some(s("beat-intervals")),
        title: Some(s("Successive heartbeat intervals")),
        series: vec![
            PointSeries {
                name: s("Resting"),
                points: resting_beats
                    .iter()
                    .enumerate()
                    .map(|(at, (x, y))| Point {
                        x: number(*x),
                        y: number(*y),
                        label: Some(format!("Beat {}", at + 1)),
                    })
                    .collect(),
                colour: Some(Colour::new(Hue::Rose)),
            },
            PointSeries {
                name: s("After the run"),
                points: run_beats
                    .iter()
                    .map(|(x, y)| Point {
                        x: number(*x),
                        y: number(*y),
                        label: None,
                    })
                    .collect(),
                colour: Some(Colour::new(Hue::Slate)),
            },
        ],
        x: Some(Measure {
            name: Some(s("Interval n")),
            unit: Some(s("ms")),
            ..Measure::default()
        }),
        y: Some(Measure {
            name: Some(s("Interval n+1")),
            unit: Some(s("ms")),
            ..Measure::default()
        }),
        equal_axes: true,
        references: vec![Reference {
            kind: ReferenceKind::Diagonal,
            label: Some(s("No change")),
            ..Reference::default()
        }],
        summary: Some(s("Successive heartbeat intervals: 24 points in 2 series. Resting: 16 points, from 805 to 845 ms. After the run: 8 points, from 678 to 710 ms.")),
        ..Scatter::default()
    });
    let days = || ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].map(s).to_vec();
    let minutes = |name: &str, hue: Hue, values: [i64; 7]| Series {
        label: s(name),
        values: values.map(|minutes| Some(number(minutes))).to_vec(),
        colour: Some(Colour::new(hue)),
        ..Series::default()
    };
    // Active minutes by intensity, one on another.
    let intensity = Node::Chart(Chart {
        id: Some(s("minutes-by-intensity")),
        kind: ChartKind::Bar,
        title: Some(s("Active minutes by intensity, last 7 days")),
        labels: days(),
        series: vec![
            minutes("Easy", Hue::Blue, [22, 30, 18, 25, 20, 41, 35]),
            minutes("Steady", Hue::Green, [12, 25, 0, 18, 22, 30, 10]),
            minutes("Hard", Hue::Amber, [0, 8, 0, 12, 0, 15, 0]),
        ],
        y: Some(Measure {
            unit: Some(s("min")),
            ..Measure::default()
        }),
        stacking: Stacking::Values,
        summary: Some(s("Active minutes by intensity, last 7 days: most on Saturday, 86 minutes.")),
        ..Chart::default()
    });
    // The same minutes as shares of each day, along rows.
    let shares = Node::Chart(Chart {
        id: Some(s("minutes-shares")),
        kind: ChartKind::Bar,
        title: Some(s("What each day was made of")),
        labels: days(),
        series: vec![
            minutes("Easy", Hue::Blue, [22, 30, 18, 25, 20, 41, 35]),
            minutes("Steady", Hue::Green, [12, 25, 0, 18, 22, 30, 10]),
            minutes("Hard", Hue::Amber, [0, 8, 0, 12, 0, 15, 0]),
        ],
        stacking: Stacking::Shares,
        horizontal: true,
        summary: Some(s("What each day was made of: Thursday had the largest share of Hard minutes, 22%.")),
        ..Chart::default()
    });
    // The load of six weeks as a smooth line, with its average across it.
    let weekly = Node::Chart(Chart {
        id: Some(s("load-weeks")),
        kind: ChartKind::Line,
        title: Some(s("Training load by week")),
        labels: ["W41", "W42", "W43", "W44", "W45", "W46"].map(s).to_vec(),
        series: vec![Series {
            label: s("Load"),
            values: [310, 355, 402, 388, 441, 462].map(|load| Some(number(load))).to_vec(),
            colour: Some(Colour::new(Hue::Rose)),
            unit: Some(s("load")),
            ..Series::default()
        },
        // The six weeks before, in the quiet colour: not the subject.
        Series {
            label: s("The six weeks before"),
            values: [285, 301, 296, 322, 340, 331].map(|load| Some(number(load))).to_vec(),
            colour: Some(Colour::new(Hue::Neutral)),
            unit: Some(s("load")),
            ..Series::default()
        }],
        references: vec![Reference {
            value: Some(number(393)),
            label: Some(s("Average: 393")),
            ..Reference::default()
        }],
        curve: Curve::Smooth,
        summary: Some(s("Training load by week: from 310 in week 41 to 462 in week 46, above the average of 393.")),
        ..Chart::default()
    });
    // A tile with its small chart under it: a sparkline of the last week.
    let resting = Node::Figure(Figure {
        id: Some(s("resting-heart-rate")),
        label: s("Resting heart rate"),
        value: s("58"),
        unit: Some(s("bpm")),
        icon: Some(icon("heart-pulse", "M19.5 12.6 12 20l-7.5-7.4a5 5 0 0 1 7.5-6.6 5 5 0 0 1 7.5 6.6zM3 12h4l2-3 3 6 2-3h5")),
        trend: Some(Trend {
            direction: Direction::Down,
            text: Some(s("3.3%")),
            tone: Some(Tone::Success),
        }),
        detail: Some(s("vs last week")),
        status: Some(Badge {
            text: s("Athletic"),
            tone: Some(Tone::Success),
            ..Badge::default()
        }),
        colour: Some(Colour::new(Hue::Rose)),
        chart: Some(Box::new(Node::Chart(Chart {
            id: Some(s("resting-heart-rate-days")),
            kind: ChartKind::Sparkline,
            labels: days(),
            series: vec![Series {
                label: s("Resting heart rate"),
                values: [61, 60, 62, 59, 58, 59, 58].map(|bpm| Some(number(bpm))).to_vec(),
                colour: Some(Colour::new(Hue::Rose)),
                unit: Some(s("bpm")),
                ..Series::default()
            }],
            summary: Some(s("Resting heart rate, 7 days: 61 bpm falling to 58 bpm.")),
            ..Chart::default()
        }))),
        action: Some(Action::navigate("/heart")),
        ..Figure::default()
    });
    let body = Node::Stack(Stack {
        gap: 4,
        children: vec![
            resting,
            Node::Section(Section {
                title: Some(s("Training load")),
                style: SectionStyle::Card,
                children: vec![load],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("Fitness")),
                style: SectionStyle::Card,
                children: vec![vo2],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("Morning run")),
                style: SectionStyle::Card,
                children: vec![run],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("Consistency")),
                style: SectionStyle::Card,
                children: vec![calendar],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("Patterns")),
                style: SectionStyle::Card,
                children: vec![links],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("Heartbeats")),
                style: SectionStyle::Card,
                children: vec![poincare],
                ..Section::default()
            }),
            Node::Section(Section {
                title: Some(s("The week in minutes")),
                style: SectionStyle::Card,
                children: vec![intensity, shares, weekly],
                ..Section::default()
            }),
        ],
        ..Stack::default()
    });
    let mut screen = Screen::new("Fitness", body);
    screen.nav = Some(s("/fitness"));
    screen.refresh = true;
    Envelope::screen(BUILD, screen)
}

/// A support inbox: a list of conversations beside the one open, its
/// messages, the customer in an inspector, a reply form with searched
/// choices and a follow-up date, and the customer's orders with row actions.
fn screen_inbox() -> Envelope {
    let message = |author: &str, mine: bool, said: &str, time: &str| Message {
        id: None,
        author: s(author),
        avatar: None,
        mine,
        note: false,
        body: vec![text(said)],
        time: Some(s(time)),
        delivery: None,
    };
    let sent = |said: &str, time: &str, delivery: Delivery| {
        Node::Message(Message {
            delivery: Some(delivery),
            ..message("Amandeep Gill", true, said, time)
        })
    };
    let conversation = |id: &str, who: &str, last: &str| {
        Node::Row(Row {
            id: Some(format!("c-{id}")),
            leading: Some(Box::new(Node::Avatar(Avatar {
                id: None,
                name: s(who),
                src: None,
                initials: None,
            }))),
            title: s(who),
            subtitle: Some(s(last)),
            action: Some(Action::navigate(format!("/inbox/{id}"))),
            ..Row::default()
        })
    };
    let reply = Node::Form(Form {
        id: Some(s("reply")),
        method: Method::Post,
        action: s("/inbox/88/reply"),
        hidden: vec![Hidden {
            name: s("_csrf"),
            value: s("tok"),
            id: None,
        }],
        children: vec![
            Node::Field(Field {
                id: Some(s("reply-assignee")),
                name: s("assignee"),
                label: Some(s("Assign to")),
                help: None,
                error: None,
                required: false,
                disabled: false,
                purpose: None,
                changed: None,
                input: Input::Choice(ChoiceInput {
                    kind: ChoiceKind::List,
                    options: vec![option("12", "Amandeep Gill")],
                    value: Some(s("12")),
                    search: Some(ChoiceSearch {
                        href: s("/inbox/assignees/options"),
                        name: s("q"),
                        min: 2,
                    }),
                    ..ChoiceInput::default()
                }),
            }),
            Node::Field(Field {
                id: Some(s("reply-tags")),
                name: s("tags"),
                label: Some(s("Tags")),
                help: None,
                error: None,
                required: false,
                disabled: false,
                purpose: None,
                changed: None,
                input: Input::Choices(ChoicesInput {
                    options: vec![option("refund", "Refund"), option("late", "Late delivery")],
                    values: vec![s("late")],
                    search: Some(ChoiceSearch {
                        href: s("/inbox/tags/options"),
                        name: s("q"),
                        min: 0,
                    }),
                    create: Some(ChoiceCreate {
                        label: s("Add a tag"),
                        href: Some(s("/inbox/tags")),
                    }),
                    more: Some(s("/inbox/tags/options?after=late")),
                }),
            }),
            Node::Field(Field {
                id: Some(s("reply-follow-up")),
                name: s("follow_up"),
                label: Some(s("Follow up on")),
                help: Some(s("The shop is shut on Sundays and on Gurpurab.")),
                error: None,
                required: false,
                disabled: false,
                purpose: None,
                changed: None,
                input: Input::Date(DateInput {
                    kind: DateKind::Date,
                    value: s("2026-11-16"),
                    min: Some(s("2026-11-13")),
                    max: None,
                    off_days: vec![s("2026-11-15")],
                    off_weekdays: vec![Weekday::Sunday],
                }),
            }),
            Node::Field(Field {
                id: Some(s("reply-city")),
                name: s("city"),
                label: Some(s("City")),
                help: None,
                error: None,
                required: false,
                disabled: false,
                purpose: None,
                changed: None,
                input: Input::Choice(ChoiceInput {
                    kind: ChoiceKind::Menu,
                    options: vec![option("ludhiana", "Ludhiana"), option("amritsar", "Amritsar")],
                    create: Some(ChoiceCreate {
                        label: s("Another city"),
                        href: None,
                    }),
                    ..ChoiceInput::default()
                }),
            }),
            Node::Button(Button {
                label: s("Send"),
                role: Role::Primary,
                prominence: Prominence::High,
                action: Some(Action::submit()),
                ..Button::default()
            }),
        ],
    });
    let orders = Node::Table(Table {
        id: Some(s("customer-orders")),
        caption: Some(s("Orders")),
        columns: vec![
            Column {
                key: Some(s("who")),
                label: s("Customer"),
                role: Some(ColumnRole::Leading),
                ..Column::default()
            },
            Column {
                key: Some(s("number")),
                label: s("Order"),
                role: Some(ColumnRole::Title),
                ..Column::default()
            },
            Column {
                key: Some(s("placed_on")),
                label: s("Placed"),
                role: Some(ColumnRole::Subtitle),
                ..Column::default()
            },
            Column {
                key: Some(s("total")),
                label: s("Total"),
                numeric: true,
                role: Some(ColumnRole::Trailing),
                ..Column::default()
            },
            Column {
                label: s("Actions"),
                role: Some(ColumnRole::Actions),
                ..Column::default()
            },
        ],
        rows: vec![TableRow {
            id: Some(s("1042")),
            cells: vec![
                Node::Avatar(Avatar {
                    id: None,
                    name: s("Harpreet Sandhu"),
                    src: None,
                    initials: Some(s("HS")),
                }),
                text("CH-1042"),
                text("Today, 09:41"),
                text("₹1,24,050"),
                Node::Spacer(Spacer::default()),
            ],
            action: Some(Action::navigate("/orders/1042")),
            actions: vec![Node::Menu(Menu {
                id: None,
                label: s("Order actions"),
                icon: None,
                items: vec![
                    Node::Button(Button {
                        label: s("Refund"),
                        action: Some(Action::command(Command::Open { id: s("refund") })),
                        ..Button::default()
                    }),
                    Node::Button(Button {
                        label: s("Cancel order"),
                        role: Role::Destructive,
                        ask: Some(Question {
                            title: s("Cancel order CH-1042?"),
                            text: Some(s("₹1,24,050 goes back to Harpreet Sandhu's UPI account.")),
                            confirm: s("Cancel order"),
                            cancel: Some(s("Keep it")),
                        }),
                        action: Some(Action::Request {
                            method: Method::Post,
                            href: s("/orders/1042/cancel"),
                            fields: vec![pair("_csrf", "tok")],
                        }),
                        ..Button::default()
                    }),
                ],
            })],
            ..TableRow::default()
        }],
        ..Table::default()
    });
    let thread = Node::Pane(Pane {
        id: Some(s("thread")),
        role: PaneRole::Detail,
        title: Some(s("Harpreet Sandhu")),
        current: true,
        children: vec![
            Node::Message(message(
                "Harpreet Sandhu",
                false,
                "My order CH-1042 has not come yet. It was due on Tuesday.",
                "Yesterday, 18:02",
            )),
            Node::Message(Message {
                id: Some(s("m-3")),
                note: true,
                ..message(
                    "Amandeep Gill",
                    true,
                    "The courier lost a day in Ambala.",
                    "09:12",
                )
            }),
            Node::Message(Message {
                id: Some(s("m-4")),
                avatar: Some(Avatar {
                    id: None,
                    name: s("Amandeep Gill"),
                    src: None,
                    initials: Some(s("AG")),
                }),
                delivery: Some(Delivery::Read),
                ..message(
                    "Amandeep Gill",
                    true,
                    "Sorry for the wait. It reaches Amritsar by Friday.",
                    "09:20",
                )
            }),
            sent("Tracking: LDH-7731", "09:21", Delivery::Failed),
            sent("I've added ₹100 credit.", "09:22", Delivery::Sending),
            sent("Anything else?", "09:23", Delivery::Sent),
            sent("Have a good day.", "09:24", Delivery::Delivered),
            reply,
        ],
    });
    let customer = Node::Pane(Pane {
        id: Some(s("customer")),
        role: PaneRole::Inspector,
        title: Some(s("Customer")),
        current: false,
        children: vec![
            Node::Facts(Facts {
                id: None,
                items: vec![Fact {
                    label: s("Phone"),
                    value: vec![text("+91 98140 12345")],
                    icon: None,
                    tone: None,
                    details: Vec::new(),
                }],
            }),
            orders,
        ],
    });
    let body = Node::Split(Split {
        id: Some(s("inbox")),
        primary: Box::new(Node::Pane(Pane {
            id: Some(s("conversations")),
            role: PaneRole::List,
            title: Some(s("Inbox")),
            current: false,
            children: vec![Node::List(List {
                id: Some(s("conversation-list")),
                more: Some(s("/inbox/rows?after=88")),
                children: vec![
                    conversation("88", "Harpreet Sandhu", "My order CH-1042 has not come yet."),
                    conversation("87", "Simran Bhatia", "Thank you, it arrived."),
                ],
                ..List::default()
            })],
        })),
        detail: Box::new(Node::Split(Split {
            id: Some(s("conversation")),
            primary: Box::new(thread),
            detail: Box::new(customer),
        })),
    });
    let mut screen = Screen::new("Inbox", body);
    screen.nav = Some(s("/inbox"));
    Envelope::screen(BUILD, screen)
}

/// A board of cards: lists whose rows move by dragging between them, a card
/// made in a task, a menu of ticked items and a choice of one, fields with a
/// prefix and a suffix and fields that act as they change, quiet rows, and
/// icons with their systems' symbols.
fn screen_board() -> Envelope {
    // A card's id names it on the screen; its key is the record the app moves.
    let card = |id: &str, title: &str, who: &str| Row {
        id: Some(s(id)),
        key: id.strip_prefix("c-").map(s),
        title: s(title),
        subtitle: Some(s(who)),
        action: Some(Action::Navigate {
            href: format!("/cards/{id}"),
            present: Present::Push,
        }),
        ..Row::default()
    };
    let column = |id: &str, title: &str, limit: Option<u32>, cards: Vec<Node>| {
        Node::Section(Section {
            title: Some(s(title)),
            children: vec![Node::List(List {
                id: Some(s(id)),
                children: cards,
                reorder: Some(Reorder {
                    href: s("/cards/move"),
                    // A column's key is the status a card in it has.
                    key: id.strip_prefix("board-").map(s),
                    group: Some(s("board")),
                    limit,
                    fields: vec![pair("_csrf", "tok")],
                }),
                ..List::default()
            })],
            ..Section::default()
        })
    };
    let plus = Icon {
        name: Some(s("plus")),
        paths: vec![s("M5 12h14M12 5v14")],
        ..Icon::default()
    }
    .with_symbol();
    let new_card = Node::Sheet(Sheet {
        id: s("new-card"),
        title: Some(s("New card")),
        task: true,
        children: vec![Node::Form(Form {
            id: Some(s("card-form")),
            method: Method::Post,
            action: s("/cards"),
            hidden: vec![Hidden {
                name: s("_csrf"),
                value: s("tok"),
                id: None,
            }],
            children: vec![
                Node::Field(Field {
                    name: s("title"),
                    label: Some(s("Title")),
                    required: true,
                    ..Field::default()
                }),
                Node::Field(Field {
                    name: s("budget"),
                    label: Some(s("Budget")),
                    input: Input::Text(TextInput {
                        kind: TextKind::Decimal,
                        prefix: Some(s("₹")),
                        ..TextInput::default()
                    }),
                    ..Field::default()
                }),
                Node::Field(Field {
                    name: s("estimate"),
                    label: Some(s("Estimate")),
                    input: Input::Text(TextInput {
                        kind: TextKind::Number,
                        suffix: Some(s("hours")),
                        ..TextInput::default()
                    }),
                    ..Field::default()
                }),
                Node::Field(Field {
                    id: Some(s("card-notes")),
                    name: s("notes"),
                    label: Some(s("Notes")),
                    help: Some(s("0/160")),
                    input: Input::Text(TextInput {
                        kind: TextKind::Multiline,
                        max_length: Some(160),
                        ..TextInput::default()
                    }),
                    changed: Some(Action::command(Command::Count {
                        id: s("notes-count"),
                        max: Some(160),
                    })),
                    ..Field::default()
                }),
                Node::Text(Text {
                    id: Some(s("notes-count")),
                    value: s("0/160"),
                    muted: true,
                    ..Text::default()
                }),
            ],
        })],
        actions: vec![Node::Button(Button {
            label: s("Add card"),
            role: Role::Confirm,
            action: Some(Action::Submit {
                form: Some(s("card-form")),
                href: None,
                with: None,
            }),
            ..Button::default()
        })],
        ..Sheet::default()
    });
    let view = Node::Menu(Menu {
        id: Some(s("board-view")),
        label: s("View"),
        icon: None,
        items: vec![
            Node::Button(Button {
                label: s("Show archived cards"),
                checked: Some(false),
                action: Some(Action::navigate("/board?archived=1")),
                ..Button::default()
            }),
            Node::Divider(Divider::default()),
            Node::Button(Button {
                label: s("By due date"),
                checked: Some(true),
                one_of: Some(s("order")),
                action: Some(Action::navigate("/board?order=due")),
                ..Button::default()
            }),
            Node::Button(Button {
                label: s("By who it is for"),
                checked: Some(false),
                one_of: Some(s("order")),
                action: Some(Action::navigate("/board?order=owner")),
                ..Button::default()
            }),
        ],
    });
    let watch = Node::Form(Form {
        id: Some(s("watch")),
        method: Method::Post,
        action: s("/board/watch"),
        hidden: vec![Hidden {
            name: s("_csrf"),
            value: s("tok"),
            id: None,
        }],
        children: vec![Node::Field(Field {
            name: s("watching"),
            label: Some(s("Tell me when cards move")),
            input: Input::Toggle(ToggleInput {
                on: true,
                ..ToggleInput::default()
            }),
            // Saved as it flips.
            changed: Some(Action::submit()),
            ..Field::default()
        })],
    });
    let body = Node::Stack(Stack {
        gap: 3,
        children: vec![
            view,
            watch,
            Node::Grid(Grid {
                cols: Some(3),
                min: Some(220),
                gap: 3,
                children: vec![
                    column(
                        "board-todo",
                        "To do",
                        None,
                        vec![
                            Node::Row(card("c-7", "Restock Brass Kadhai", "Ravneet Kaur")),
                            Node::Row(card(
                                "c-8",
                                "Photograph the Diwali range",
                                "Jaspreet Singh",
                            )),
                        ],
                    ),
                    column(
                        "board-doing",
                        "Doing",
                        Some(3),
                        vec![Node::Row(card(
                            "c-5",
                            "Call the Jalandhar supplier",
                            "Amandeep Gill",
                        ))],
                    ),
                    column(
                        "board-done",
                        "Done",
                        None,
                        vec![Node::Row(Row {
                            muted: true,
                            ..card("c-2", "Archived 14 old cards", "The nightly clean-up")
                        })],
                    ),
                ],
                ..Grid::default()
            }),
            Node::FeedRow(FeedRow {
                id: Some(s("board-event-77")),
                icon: Some(Icon {
                    name: Some(s("refresh-cw")),
                    ..Icon::default()
                }
                .with_symbol()),
                runs: vec![run("The board was tidied up overnight")],
                time: Some(s("03:00")),
                muted: true,
                ..FeedRow::default()
            }),
            Node::Button(Button {
                label: s("New card"),
                icon: Some(plus.clone()),
                role: Role::Create,
                action: Some(Action::Navigate {
                    href: s("/cards/new"),
                    present: Present::Task,
                }),
                ..Button::default()
            }),
            new_card,
        ],
        ..Stack::default()
    });
    let mut screen = Screen::new("Board", body);
    screen.nav = Some(s("/board"));
    screen.actions = vec![Node::Toolbar(Toolbar {
        id: None,
        items: vec![ToolbarItem {
            title: s("Clear done"),
            icon: Icon {
                name: Some(s("trash-2")),
                paths: vec![s("M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14")],
                ..Icon::default()
            }
            .with_symbol(),
            standard: Some(Standard::Delete),
            role: Role::Destructive,
            ask: Some(Question {
                title: s("Clear 1 done card?"),
                text: None,
                confirm: s("Clear"),
                cancel: None,
            }),
            action: Some(Action::Request {
                method: Method::Post,
                href: s("/board/clear-done"),
                fields: vec![pair("_csrf", "tok")],
            }),
            ..ToolbarItem::default()
        }],
    })];
    Envelope::screen(BUILD, screen)
}

/// What changes a list and a table row by row: a live list's changes since
/// its revision, a table's next records, one record drawn again and one gone,
/// and a post that leads to a task.
fn ops_lists() -> Envelope {
    let record = |id: &str, number: &str, total: &str| TableRow {
        id: Some(s(id)),
        cells: vec![text(number), text(total)],
        action: Some(Action::navigate(format!("/orders/{id}"))),
        ..TableRow::default()
    };
    Envelope::ops(
        BUILD,
        vec![
            Op::Merge {
                target: s("board-doing"),
                nodes: vec![Node::Row(Row {
                    id: Some(s("c-8")),
                    key: Some(s("8")),
                    title: s("Photograph the Diwali range"),
                    subtitle: Some(s("Jaspreet Singh")),
                    ..Row::default()
                })],
                gone: vec![s("c-5")],
                since: s("0000512"),
                at: Edge::Start,
                more: false,
            },
            Op::Rows {
                target: s("orders"),
                rows: vec![
                    record("1040", "CH-1040", "₹8,900"),
                    record("1039", "CH-1039", "₹12,450"),
                ],
                at: Edge::End,
                more: Some(s("/orders?after=1039")),
                prev: None,
            },
            // The records before the first, which move the table's cursor back.
            Op::Rows {
                target: s("orders"),
                rows: vec![
                    record("1045", "CH-1045", "₹3,150"),
                    record("1044", "CH-1044", "₹54,000"),
                ],
                at: Edge::Start,
                more: None,
                prev: Some(s("/orders?before=1045")),
            },
            Op::Row {
                target: s("1042"),
                row: TableRow {
                    open: true,
                    detail: vec![muted("Refunded ₹1,240.50 to UPI")],
                    ..record("1042", "CH-1042", "₹1,22,809.50")
                },
            },
            Op::Remove { target: s("1041") },
            Op::Navigate {
                href: s("/orders/1042/refund"),
                present: Present::Task,
            },
        ],
    )
}

fn envelopes() -> Vec<(&'static str, Envelope)> {
    vec![
        ("screen-list", screen_list()),
        ("screen-board", screen_board()),
        ("ops-lists", ops_lists()),
        ("screen-detail", screen_detail()),
        ("screen-form", screen_form()),
        ("screen-kinds", screen_kinds()),
        ("screen-dashboard", screen_dashboard()),
        ("screen-inbox", screen_inbox()),
        ("screen-today", screen_today()),
        ("screen-sleep", screen_sleep()),
        ("screen-fitness", screen_fitness()),
        ("screen-designed-health", health_today::envelope(BUILD)),
        ("screen-designed-store", shop_shelves::envelope(BUILD)),
        ("screen-device", screen_device()),
        ("ops-device", ops_device()),
        ("screen-with-toast", screen_with_toast()),
        ("ops-reply", ops_reply()),
        ("ops-errors", ops_errors()),
        ("ops-navigate", ops_navigate()),
        ("ops-sign-out", ops_sign_out()),
        ("web-fallback", web_fallback()),
        ("upgrade", upgrade()),
    ]
}

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

/// Holds `corpus/<name>.json` to what `value` writes, writing the file when it
/// is missing or when asked, then reads the file back into the type.
fn held<T>(name: &str, value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let path = corpus().join(format!("{name}.json"));
    let mut written = serde_json::to_string_pretty(value).unwrap();
    written.push('\n');
    let asked = std::env::var("NAVREE_NATIVE_CORPUS").is_ok_and(|mode| mode == "write");
    if asked || !path.exists() {
        std::fs::create_dir_all(corpus()).unwrap();
        std::fs::write(&path, &written).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(
        on_disk, written,
        "corpus/{name}.json is not what the types write now; if the change is meant, write \
         the corpus again with NAVREE_NATIVE_CORPUS=write"
    );
    let read: T = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(&read, value, "corpus/{name}.json");
}

#[test]
fn every_example_is_written_and_reads_back_as_it_was() {
    for (name, envelope) in envelopes() {
        held(name, &envelope);
    }
    held("shell", &shell().stamped());
}

/// Whether any icon in `value` still carries its own shapes, outside the
/// envelope's icon table.
fn shapes_outside_the_table(value: &serde_json::Value, table: bool) -> bool {
    match value {
        serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
            (!table && (key == "paths" || key == "fills"))
                || shapes_outside_the_table(value, table || key == "icons")
        }),
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| shapes_outside_the_table(item, table)),
        _ => false,
    }
}

#[test]
fn every_icon_shape_is_sent_once_and_put_back_where_it_was() {
    for (name, envelope) in envelopes() {
        let mut whole = envelope.clone();
        whole.inline_icons();
        assert!(whole.icons.is_empty(), "{name}");
        let mut shared = whole.clone();
        shared.share_icons();
        // Every icon of the corpus has a name, and each name one shape.
        let json = serde_json::to_value(&shared).unwrap();
        assert!(!shapes_outside_the_table(&json, false), "{name}: {json}");
        shared.inline_icons();
        assert_eq!(shared, whole, "{name}");
    }
}

/// The examples no type can write, because they hold what this build does not
/// know. They are written by hand and only read.
const BY_HAND: &[&str] = &["from-the-future.json"];

/// An answer from a server newer than the app: a kind, an action, a command, a
/// control, a device call and its stand-in, and an op this build has never
/// heard of, and fields it does not know at every level.
#[test]
fn the_future_reads_as_unknown_parts_that_keep_their_stand_ins() {
    let text = std::fs::read_to_string(corpus().join("from-the-future.json")).unwrap();
    let envelope: Envelope = serde_json::from_str(&text).unwrap();
    let screen = envelope.screen.as_ref().unwrap();
    assert_eq!(screen.title, "Warehouse");

    // A known kind keeps what it knows and passes over the rest.
    assert_eq!(
        screen.body.find("stock-heading"),
        Some(&Node::Text(Text {
            id: Some(s("stock-heading")),
            value: s("Stock across Punjab"),
            ..Text::default()
        }))
    );
    // An unknown kind keeps its id and its stand-in.
    let Some(Node::Unknown(hologram)) = screen.body.find("shelves-3d") else {
        panic!("the hologram is read as an unknown part");
    };
    assert_eq!(hologram.kind, "hologram");
    let Some(Node::Image(image)) = hologram.alt.as_deref() else {
        panic!("the hologram keeps the picture that stands in for it");
    };
    assert_eq!(image.alt, "The shelves at the Ludhiana warehouse");
    assert_eq!(
        screen.body.find("no-stand-in"),
        Some(&Node::Unknown(Unknown {
            kind: s("hologram"),
            id: Some(s("no-stand-in")),
            alt: None,
        }))
    );

    // An unknown action, command, control and device call.
    let Node::Button(teleport) = &screen.actions[0] else {
        panic!("the screen's action is a button");
    };
    assert_eq!(teleport.action, Some(Action::Unknown));
    let Some(Node::Form(form)) = screen.body.find("move") else {
        panic!("the form is read");
    };
    let Node::Field(signature) = &form.children[0] else {
        panic!("the signature is a field");
    };
    assert_eq!(signature.input, Input::Unknown);
    let Node::Field(units) = &form.children[1] else {
        panic!("the units are a field");
    };
    assert!(matches!(&units.input, Input::Text(text) if text.value == "24"));
    let Some(Node::Button(celebrate)) = screen.body.find("celebrate") else {
        panic!("the button is read");
    };
    let Some(Action::Commands { list }) = &celebrate.action else {
        panic!("its commands are read");
    };
    assert_eq!(list[0], Command::Unknown);
    assert!(matches!(&list[1], Command::Toast { text, .. } if text == "Moved"));
    let Some(Node::Button(measure)) = screen.body.find("measure") else {
        panic!("the button is read");
    };
    let Some(Action::Device(call)) = &measure.action else {
        panic!("its device call is read");
    };
    assert_eq!(call.call, navree_native::device::Call::Unknown);
    assert_eq!(call.alt, Some(Alt::Unknown));

    // An unknown op is skipped; a tap-only call in an op is never started.
    assert_eq!(envelope.ops.len(), 3);
    assert_eq!(envelope.ops[0], Op::Unknown);
    assert_eq!(
        envelope.ops[1],
        Op::Device {
            call: ReplyCall::Unknown
        }
    );
    assert!(matches!(&envelope.ops[2], Op::Toast(toast) if toast.text == "Synced"));

    // What a client draws: the stand-in in the hologram's place, and the page
    // in the web view for the part with none.
    assert_eq!(
        navree_native::vocab::downgrade(screen.body.clone(), VOCAB),
        navree_native::vocab::Downgraded::Web {
            why: vec![s("vocab:hologram")]
        }
    );
    let Some(Node::Stack(body)) = Some(&screen.body) else {
        panic!("the body is a stack");
    };
    let mut drawn = body.clone();
    drawn.children.retain(|node| node.id() != Some("no-stand-in"));
    let navree_native::vocab::Downgraded::Node(Node::Stack(drawn)) =
        navree_native::vocab::downgrade(Node::Stack(drawn), VOCAB)
    else {
        panic!("with a stand-in for every unknown part, the page is drawn");
    };
    assert!(matches!(&drawn.children[1], Node::Image(image) if image.src == "/files/shelves.jpg"));
}

#[test]
fn the_corpus_holds_nothing_the_tests_do_not_write() {
    let mut expected: BTreeSet<String> = envelopes()
        .into_iter()
        .map(|(name, _)| format!("{name}.json"))
        .collect();
    expected.insert("shell.json".to_owned());
    expected.extend(BY_HAND.iter().map(|name| (*name).to_owned()));
    let Ok(entries) = std::fs::read_dir(corpus()) else {
        return;
    };
    for entry in entries {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(expected.contains(&name), "corpus/{name} has no example");
    }
}

/// The values of the closed lists a node holds, by list, as `Debug` names them.
type Values = BTreeMap<&'static str, BTreeSet<String>>;

/// The words of the closed lists of the charts that the JSON of an example
/// holds, by list. A word left at its default is left out of the JSON, so the
/// default is counted where the part that holds it is.
fn words_in(value: &serde_json::Value, found: &mut BTreeMap<&'static str, BTreeSet<String>>) {
    match value {
        serde_json::Value::Object(map) => {
            let word = |key: &str, default: &str| {
                map.get(key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(default)
                    .to_owned()
            };
            let mut see = |list: &'static str, word: String| {
                found.entry(list).or_default().insert(word);
            };
            match map.get("t").and_then(serde_json::Value::as_str) {
                Some("ring") => {
                    see("centre", word("centre", "value"));
                    see("prominence", word("prominence", "medium"));
                }
                Some("rings") => {
                    see("rings_centre", word("centre", "nothing"));
                    see("prominence", word("prominence", "medium"));
                }
                Some("chart") => {
                    see("curve", word("curve", "linear"));
                    see("stacking", word("stacking", "apart"));
                }
                Some("text") => {
                    see("text_style", word("style", "body"));
                    for list in ["weight", "change"] {
                        if map.contains_key(list) {
                            see(list, word(list, ""));
                        }
                    }
                }
                Some("icon") if map.contains_key("scale") => see("icon_scale", word("scale", "")),
                Some("shape") => see("form", word("form", "rounded")),
                Some("layers") => see("place", word("place", "center")),
                Some("stack") if map.contains_key("wide") => see("stack_wide", word("wide", "")),
                _ => {}
            }
            // A part that arrives, and one that leads into what it opens, say how.
            for list in ["appear", "transition"] {
                if let Some(how) = map.get(list).and_then(serde_json::Value::as_str) {
                    see(list, how.to_owned());
                }
            }
            // A hint says what it is about, in words.
            if map.contains_key("about") && map.contains_key("text") {
                see("hint", word("mark", "question"));
            }
            // A colour names its hue, and the hue it blends into.
            if let Some(hue) = map.get("hue").and_then(serde_json::Value::as_str) {
                see("hue", hue.to_owned());
                if let Some(to) = map.get("to").and_then(serde_json::Value::as_str) {
                    see("hue", to.to_owned());
                }
            }
            if let Some(serde_json::Value::Array(lines)) = map.get("references") {
                for line in lines {
                    let kind = line
                        .get("kind")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("level");
                    found.entry("reference").or_default().insert(kind.to_owned());
                }
            }
            for inner in map.values() {
                words_in(inner, found);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                words_in(item, found);
            }
        }
        _ => {}
    }
}

fn kinds_in(
    node: &Node,
    kinds: &mut BTreeSet<&'static str>,
    inputs: &mut BTreeSet<String>,
    values: &mut Values,
) {
    kinds.insert(node.kind().as_str());
    let mut seen = |list: &'static str, value: &dyn std::fmt::Debug| {
        values
            .entry(list)
            .or_default()
            .insert(format!("{value:?}"));
    };
    match node {
        Node::Field(field) => {
            let json = serde_json::to_value(&field.input).unwrap();
            let mut name = json["t"].as_str().unwrap().to_owned();
            if let Some(kind) = json.get("kind").and_then(|kind| kind.as_str()) {
                name = format!("{name}:{kind}");
            }
            inputs.insert(name);
            if let Some(purpose) = &field.purpose {
                seen("purpose", purpose);
            }
        }
        Node::Button(button) => {
            if let Some(standard) = &button.standard {
                seen("standard", standard);
            }
            if let Some(Action::Device(call)) = &button.action {
                let tag = serde_json::to_value(&call.call).unwrap()["f"].clone();
                seen("call", &tag.as_str().unwrap());
            }
        }
        Node::Toolbar(toolbar) => {
            for standard in toolbar.items.iter().filter_map(|item| item.standard.as_ref()) {
                seen("standard", standard);
            }
        }
        Node::Rich(rich) => {
            for block in &rich.blocks {
                seen("block", &block.kind);
            }
        }
        Node::Pane(pane) => seen("pane", &pane.role),
        Node::Message(message) => {
            if let Some(delivery) = &message.delivery {
                seen("delivery", delivery);
            }
        }
        Node::Figure(figure) => {
            if let Some(trend) = &figure.trend {
                seen("direction", &trend.direction);
            }
        }
        Node::Table(table) => {
            for role in table.columns.iter().filter_map(|column| column.role.as_ref()) {
                seen("column", role);
            }
        }
        _ => {}
    }
    for child in node.children() {
        kinds_in(child, kinds, inputs, values);
    }
}

#[test]
fn the_corpus_holds_every_kind_of_node_and_every_control() {
    let mut kinds = BTreeSet::new();
    let mut inputs = BTreeSet::new();
    let mut values = Values::new();
    for (_, envelope) in envelopes() {
        let mut roots: Vec<&Node> = Vec::new();
        if let Some(screen) = &envelope.screen {
            roots.push(&screen.body);
            roots.extend(&screen.actions);
        }
        for op in &envelope.ops {
            let tag = serde_json::to_value(op).unwrap()["op"].clone();
            values
                .entry("op")
                .or_default()
                .insert(format!("{:?}", tag.as_str().unwrap()));
            match op {
                Op::Morph { node, .. } | Op::Replace { node, .. } => roots.push(node),
                Op::Append { nodes, .. } | Op::Prepend { nodes, .. } => roots.extend(nodes),
                Op::Merge { nodes, at, .. } => {
                    roots.extend(nodes);
                    values.entry("edge").or_default().insert(format!("{at:?}"));
                }
                Op::Rows { rows, at, .. } => {
                    roots.extend(rows.iter().flat_map(TableRow::nodes));
                    values.entry("edge").or_default().insert(format!("{at:?}"));
                }
                Op::Row { row, .. } => roots.extend(row.nodes()),
                Op::Device { call } => {
                    let tag = serde_json::to_value(call).unwrap()["f"].clone();
                    values
                        .entry("reply")
                        .or_default()
                        .insert(format!("{:?}", tag.as_str().unwrap()));
                }
                _ => {}
            }
        }
        for root in roots {
            kinds_in(root, &mut kinds, &mut inputs, &mut values);
        }
    }
    for kind in Kind::ALL {
        assert!(kinds.contains(kind.as_str()), "no example holds a {kind:?}");
    }
    // The closed lists of the charts: every hue of the protocol, and every word
    // of the others that an example can hold.
    let mut found = BTreeMap::new();
    for (_, envelope) in envelopes() {
        words_in(&serde_json::to_value(&envelope).unwrap(), &mut found);
    }
    let hues: Vec<String> = navree_native::schema::document()["$defs"]["Hue"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hue| hue.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(hues.len(), 15, "a hue was added or taken away: {hues:?}");
    let owned = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
    for (list, names) in [
        ("hue", hues),
        ("centre", owned(&["value", "icon", "nothing"])),
        ("rings_centre", owned(&["nothing", "values"])),
        ("prominence", owned(&["high", "medium", "low"])),
        ("hint", owned(&["question", "info"])),
        ("curve", owned(&["linear", "smooth", "step"])),
        ("stacking", owned(&["apart", "values", "shares"])),
        ("reference", owned(&["level", "diagonal"])),
        // What a composed screen is laid out with. A text style, a place and a
        // stack's other axis have more words than these: the examples hold the
        // ones a composed screen adds, and the places a mark sits at.
        ("text_style", owned(&["display", "title2", "body", "headline", "caption", "callout"])),
        ("weight", owned(&["regular", "medium", "semibold", "bold"])),
        ("change", owned(&["count", "fade"])),
        ("appear", owned(&["fade", "rise", "scale"])),
        ("transition", owned(&["zoom"])),
        ("form", owned(&["rounded", "circle", "capsule", "rect"])),
        ("place", owned(&["center", "bottom_end"])),
        ("icon_scale", owned(&["title2"])),
        ("stack_wide", owned(&["horizontal"])),
    ] {
        for name in names {
            assert!(
                found.get(list).is_some_and(|seen| seen.contains(&name)),
                "no example holds the {list} {name}"
            );
        }
    }
    for (list, names) in [
        (
            "standard",
            &["Share", "Delete", "Add", "Edit", "Search", "Done", "Cancel", "More"][..],
        ),
        (
            "block",
            &["Paragraph", "Heading", "Quote", "Bullet", "Numbered", "Code"],
        ),
        (
            "pane",
            &["List", "Content", "Detail", "Supporting", "Inspector"],
        ),
        (
            "delivery",
            &["Sending", "Sent", "Delivered", "Read", "Failed"],
        ),
        ("direction", &["Up", "Down", "Flat"]),
        ("purpose", &["Search", "Filter", "Sort", "PageSize"]),
        ("edge", &["Start", "End"]),
        (
            "column",
            &["Leading", "Title", "Subtitle", "Status", "Trailing", "Actions"],
        ),
        (
            "call",
            &[
                "\"haptic\"",
                "\"copy\"",
                "\"share\"",
                "\"open_url\"",
                "\"auth_session\"",
                "\"register_push\"",
                "\"notify\"",
                "\"cancel_notification\"",
                "\"set_badge\"",
                "\"open_settings\"",
                "\"passkey\"",
                "\"confirm\"",
                "\"locate_once\"",
                "\"scan_code\"",
                "\"platform_sign_in\"",
            ],
        ),
        (
            "op",
            &[
                "\"morph\"",
                "\"replace\"",
                "\"append\"",
                "\"prepend\"",
                "\"remove\"",
                "\"merge\"",
                "\"rows\"",
                "\"row\"",
                "\"errors\"",
                "\"toast\"",
                "\"open\"",
                "\"close\"",
                "\"focus\"",
                "\"url\"",
                "\"navigate\"",
                "\"navigate_external\"",
                "\"reload\"",
                "\"invalidate\"",
                "\"device\"",
                "\"signed_out\"",
            ],
        ),
        (
            "reply",
            &[
                "\"haptic\"",
                "\"set_badge\"",
                "\"cancel_notification\"",
                "\"request_review\"",
            ],
        ),
    ] {
        for name in names {
            assert!(
                values.get(list).is_some_and(|seen| seen.contains(*name)),
                "no example holds the {list} {name}"
            );
        }
    }
    for input in [
        "text",
        "text:email",
        "text:password",
        "text:number",
        "text:decimal",
        "text:phone",
        "text:url",
        "text:search",
        "text:multiline",
        "text:one_time_code",
        "toggle",
        "toggle:checkbox",
        "choice",
        "choice:segmented",
        "choice:radio",
        "choice:list",
        "choices",
        "date",
        "date:time",
        "date:datetime",
        "slider",
        "colour",
        "file",
        "range",
    ] {
        assert!(inputs.contains(input), "no example holds a {input} control");
    }
}

#[test]
fn every_example_is_written_in_the_current_vocabulary() {
    for (name, envelope) in envelopes() {
        assert_eq!(envelope.navree, navree_native::MAJOR, "{name}");
        assert_eq!(envelope.vocab, VOCAB, "{name}");
        if let Some(screen) = &envelope.screen {
            assert_eq!(
                navree_native::vocab::newer_than(&screen.body, VOCAB),
                None,
                "{name}"
            );
        }
    }
}
