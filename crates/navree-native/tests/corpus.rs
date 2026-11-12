//! The example envelopes in `corpus/`: written from the types, read back, and
//! held to what the types write now. Every client decodes all of them in its
//! own tests, so a change here is a change to the protocol.
//!
//! `NAVREE_NATIVE_CORPUS=write cargo test -p navree-native` writes the files
//! again after a change that is meant.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use navree_native::action::{Action, Command, Pair, Present, State};
use navree_native::envelope::{Back, Envelope, Screen, Upgrade, Web};
use navree_native::input::{
    ChoiceInput, ChoiceKind, ChoiceOption, ChoicesInput, ColourInput, DateInput, DateKind,
    FileInput, Input, SliderInput, TextInput, TextKind, ToggleInput, ToggleKind,
};
use navree_native::node::{
    Align, Avatar, Axis, Badge, Banner, Button, Chart, ChartKind, Code, Column, ColumnRole,
    Dialog, Disclosure, Divider, Empty, Fact, Facts, Field, Fit, Form, Grid, Hidden, Icon, Image,
    Justify, Link, List, Live, Menu, Method, NativeView, Node, Popover, Progress, Prominence,
    Region, Rich, Role, Row, RowState, Run, Section, SectionStyle, Series, Sheet, SheetSize, Sort,
    SortDir, Spacer, Split, Stack, Tab, Table, TableRow, Tabs, Text, TextStyle, Tone, WebRegion,
};
use navree_native::op::{Op, Toast, ToastAction, UrlMode};
use navree_native::shell::{Device, Nav, NavGroup, NavItem, Push, Route, Shell, Theme, User};
use navree_native::{Kind, VOCAB};
use serde::Serialize;
use serde::de::DeserializeOwned;

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
    let order = |id: &str, number: &str, who: &str, status: Badge, total: &str, when: &str| {
        TableRow {
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
        }
    };
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
        ],
        rows: vec![
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
        ],
        query: Some(Form {
            id: Some(s("orders-f")),
            method: Method::Get,
            action: s("/orders"),
            hidden: vec![Hidden {
                name: s("sort"),
                value: s("-placed_on"),
                id: None,
            }],
            children: vec![
                Node::Field(Field {
                    id: None,
                    name: s("search"),
                    label: Some(s("Search")),
                    help: None,
                    error: None,
                    required: false,
                    disabled: false,
                    input: Input::Text(TextInput {
                        kind: TextKind::Search,
                        placeholder: Some(s("Search orders")),
                        ..TextInput::default()
                    }),
                }),
                Node::Field(Field {
                    id: None,
                    name: s("filter[status][in]"),
                    label: Some(s("Status")),
                    help: None,
                    error: None,
                    required: false,
                    disabled: false,
                    input: Input::Choices(ChoicesInput {
                        options: vec![
                            counted("open", "Open", "212"),
                            counted("cancelled", "Cancelled", "9"),
                        ],
                        values: vec![s("open")],
                    }),
                }),
                Node::Field(Field {
                    id: None,
                    name: s("filter[placed_on][gte]"),
                    label: Some(s("Placed from")),
                    help: None,
                    error: None,
                    required: false,
                    disabled: false,
                    input: Input::Date(DateInput::default()),
                }),
            ],
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
                text: Some(s("Up to ₹1,24,050 can go back to the customer's UPI account.")),
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
                    value: s("+919814012345"),
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
            field("credit", "Credit limit", typed(TextKind::Decimal)),
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
                }),
            ),
            field(
                "pays",
                "Pays by",
                Input::Choice(ChoiceInput {
                    kind: ChoiceKind::Radio,
                    options: vec![option("upi", "UPI"), option("card", "Card"), option("cod", "Cash on delivery")],
                    value: Some(s("upi")),
                    placeholder: None,
                }),
            ),
            field(
                "tags",
                "Tags",
                Input::Choices(ChoicesInput {
                    options: vec![option("festive", "Festive"), option("bulk", "Bulk"), option("gift", "Gift")],
                    values: vec![s("festive"), s("gift")],
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
                Input::Colour(ColourInput { value: s("#16a34a") }),
            ),
            field(
                "papers",
                "Papers",
                Input::File(FileInput {
                    accept: Some(s("image/*,.pdf")),
                    multiple: true,
                    capture: Some(s("environment")),
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
                            }),
                            Node::Progress(Progress {
                                id: None,
                                value: Some(620),
                                label: Some(s("62% of the day's goal")),
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
                            },
                            Series {
                                label: s("Card"),
                                values: vec![
                                    Some(12.into()),
                                    serde_json::Number::from_f64(9.5),
                                    Some(17.into()),
                                ],
                                shown: vec![s("12"), s("9.5"), s("17")],
                            },
                        ],
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
                            ordered: false,
                            children: vec![Node::Row(Row {
                                id: Some(s("event-981")),
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
                            })],
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
            Op::Navigate { href: s("/") },
            Op::NavigateExternal {
                href: s("https://pay.example/session/7"),
            },
            Op::Reload,
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
        caps: vec![s("biometrics"), s("push")],
        push: Some(Push {
            register: s("/_navree/native/push"),
        }),
    });
    shell
}

fn envelopes() -> Vec<(&'static str, Envelope)> {
    vec![
        ("screen-list", screen_list()),
        ("screen-detail", screen_detail()),
        ("screen-form", screen_form()),
        ("screen-kinds", screen_kinds()),
        ("screen-with-toast", screen_with_toast()),
        ("ops-reply", ops_reply()),
        ("ops-errors", ops_errors()),
        ("ops-navigate", ops_navigate()),
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
    let on_disk = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
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
    held("shell", &shell());
}

#[test]
fn the_corpus_holds_nothing_the_tests_do_not_write() {
    let mut expected: BTreeSet<String> = envelopes()
        .into_iter()
        .map(|(name, _)| format!("{name}.json"))
        .collect();
    expected.insert("shell.json".to_owned());
    let Ok(entries) = std::fs::read_dir(corpus()) else {
        return;
    };
    for entry in entries {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(expected.contains(&name), "corpus/{name} has no example");
    }
}

fn kinds_in(node: &Node, kinds: &mut BTreeSet<&'static str>, inputs: &mut BTreeSet<String>) {
    kinds.insert(node.kind().as_str());
    if let Node::Field(field) = node {
        let json = serde_json::to_value(&field.input).unwrap();
        let mut name = json["t"].as_str().unwrap().to_owned();
        if let Some(kind) = json.get("kind").and_then(|kind| kind.as_str()) {
            name = format!("{name}:{kind}");
        }
        inputs.insert(name);
    }
    for child in node.children() {
        kinds_in(child, kinds, inputs);
    }
}

#[test]
fn the_corpus_holds_every_kind_of_node_and_every_control() {
    let mut kinds = BTreeSet::new();
    let mut inputs = BTreeSet::new();
    for (_, envelope) in envelopes() {
        let mut roots: Vec<&Node> = Vec::new();
        if let Some(screen) = &envelope.screen {
            roots.push(&screen.body);
            roots.extend(&screen.actions);
        }
        for op in &envelope.ops {
            match op {
                Op::Morph { node, .. } | Op::Replace { node, .. } => roots.push(node),
                Op::Append { nodes, .. } | Op::Prepend { nodes, .. } => roots.extend(nodes),
                _ => {}
            }
        }
        for root in roots {
            kinds_in(root, &mut kinds, &mut inputs);
        }
    }
    for kind in Kind::ALL {
        assert!(kinds.contains(kind.as_str()), "no example holds a {kind:?}");
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
