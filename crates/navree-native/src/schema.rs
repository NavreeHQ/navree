//! The protocol's JSON Schema, `schema/navree-native.schema.json`: what every
//! client's models are generated from.
//!
//! Each type is described below by the names of its fields, required first,
//! then those a client may leave out; the types of the fields come from the
//! types themselves. The code each description writes takes every struct
//! apart field by field and matches every enum variant by variant, so a field
//! or a variant added to a type and not to its description does not compile.
//! The tests then hold each description to what serde does: the least value
//! the description allows is read, a required field left out is refused, and
//! each field a client may leave out has the default and the name serde gives
//! it.
//!
//! What the schema says beyond JSON Schema, for the model generator:
//! - `x-kind` on each definition: `object`, `enum`, `union` (one of several
//!   objects, told apart by the field `x-tag`) or `string` (text with a rule,
//!   named by `format`).
//! - `x-variants` on a union: each variant's tag and, unless it holds nothing,
//!   the definition of what it holds. A definition named `Union.Variant`
//!   belongs to that variant alone.
//! - `x-unknown` on a union: what a variant this build does not know keeps,
//!   besides its tag.
//! - `x-since` on every definition, variant and field: the vocabulary it
//!   arrived in.
//! - `default` on every field a client may leave out; `anyOf` with `null` for
//!   a value that may be absent; `x-json` for any JSON value.

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::action::{Action, Command, Pair, Present, State};
use crate::client::{ClientDescriptor, Contrast, FormFactor, Platform, Scheme};
use crate::device::{
    Accuracy, Alt, AppLock, AuthSession, Call, CancelNotification, CodeFormat, Confirm,
    ConfirmMethod, Confirmed, ConfirmedBy, CopyText, DeviceCall, Feature, Haptic, HapticKind,
    HttpsUrl, LocateOnce, Located, Notify, OpenSettings, OpenUrl, OriginPath, Passkey,
    PasskeyCredential, PasskeyMode, PlatformSignIn, PushEnvironment, PushProvider,
    PushRegistered, Refusal, RegisterPush, ReplyCall, ScanCode, Scanned, SetBadge, SettingsPage,
    Share, Shared, SignInProvider, SignedIn, Then, Ticket, When,
};
use crate::envelope::{Back, Envelope, Screen, Search, Upgrade, Web};
use crate::input::{
    ChoiceCreate, ChoiceInput, ChoiceKind, ChoiceOption, ChoiceSearch, ChoicesInput, ColourInput,
    DateInput, DateKind, FileInput, Input, SliderInput, TextInput, TextKind, ToggleInput,
    ToggleKind, Weekday,
};
use crate::node::{
    Align, Avatar, Axis, Badge, Banner, Block, BlockKind, Button, Chart, ChartKind, Code, Column,
    ColumnRole, Delivery, Dialog, Direction, Disclosure, Divider, Empty, Fact, Facts, FeedRow,
    Field, Figure, Fit, Form, Gauge, Grid, Hidden, Icon, IconShape, Image, Justify, Link, List,
    Live, Menu, Message, Method, Moment, NativeView, Node, Pane, PaneRole, Popover, Progress,
    Prominence, Region, Rich, Role, Row, RowState, Run, Section, SectionStyle, Series, Sheet,
    SheetSize, Sort, SortDir, Spacer, Split, Stack, Standard, Tab, Table, TableRow, Tabs, Text,
    TextStyle, Timeline, Tone, Toolbar, ToolbarItem, Trend, WebRegion,
};
use crate::op::{Op, Toast, ToastAction, UrlMode};
use crate::shell::{Device, Nav, NavGroup, NavItem, Push, Route, Shell, Theme, User};
use crate::vocab::{Kind, LATER_FIELDS};

/// A type the schema describes.
pub trait Describe {
    /// How a field of this type is written in the schema: a reference to the
    /// type's definition, which is added to `schema` the first time, or the
    /// shape of a plain value.
    fn describe(schema: &mut Schema) -> Value;
}

/// Reads a JSON value as an object's type and writes each of its fields, by
/// its name in the JSON.
type ReadFields = fn(Value) -> Result<BTreeMap<String, Value>, String>;

/// How a definition is held to serde in the tests.
#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Check {
    /// Reads a JSON value as the type and writes it back.
    pub(crate) round_trip: fn(Value) -> Result<Value, String>,
    /// Reads an object's fields.
    pub(crate) fields: Option<ReadFields>,
}

/// The schema being written: its definitions so far.
#[derive(Default)]
pub struct Schema {
    defs: BTreeMap<String, Value>,
    started: BTreeSet<String>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) checks: BTreeMap<String, Check>,
}

fn reference(name: &str) -> Value {
    json!({ "$ref": format!("#/$defs/{name}") })
}

fn round_trip<T: Serialize + DeserializeOwned>(value: Value) -> Result<Value, String> {
    let typed: T = serde_json::from_value(value).map_err(|error| error.to_string())?;
    serde_json::to_value(&typed).map_err(|error| error.to_string())
}

/// `value` with `tag` set to `name`, so a variant's own fields read as the
/// union they belong to.
fn tagged(mut value: Value, tag: &str, name: &str) -> Value {
    if let Value::Object(map) = &mut value {
        map.insert(tag.to_owned(), name.into());
    }
    value
}

/// A variant's tag: its name in snake case, as serde writes it.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (at, letter) in name.chars().enumerate() {
        if letter.is_ascii_uppercase() {
            if at > 0 {
                out.push('_');
            }
            out.push(letter.to_ascii_lowercase());
        } else {
            out.push(letter);
        }
    }
    out
}

/// The vocabulary a field arrived in: its entry in [`LATER_FIELDS`], or its
/// type's.
fn field_since(ty: &str, field: &str, own: u32) -> u32 {
    LATER_FIELDS
        .iter()
        .find(|(of, name, _)| *of == ty && *name == field)
        .map_or(own, |(_, _, since)| *since)
}

impl Schema {
    /// Adds the definition `name` with `build`, once, and refers to it.
    fn define(&mut self, name: &str, build: impl FnOnce(&mut Self) -> (Value, Check)) -> Value {
        if self.started.insert(name.to_owned()) {
            let (def, check) = build(self);
            self.defs.insert(name.to_owned(), def);
            self.checks.insert(name.to_owned(), check);
        }
        reference(name)
    }

    /// How a field of type `T` is written, defining `T` when it is new.
    pub fn of<T: Describe>(&mut self) -> Value {
        T::describe(self)
    }

    /// The definitions so far, by name.
    pub fn defs(&self) -> &BTreeMap<String, Value> {
        &self.defs
    }

    /// Records that the definition `name` arrived in vocabulary `since`, and
    /// with it every field of it not listed in [`LATER_FIELDS`].
    fn arrived(&mut self, name: &str, since: u32) {
        let Some(def) = self.defs.get_mut(name) else {
            return;
        };
        def["x-since"] = since.into();
        if let Some(Value::Object(properties)) = def.get_mut("properties") {
            for (field, property) in properties {
                property["x-since"] = field_since(name, field, since).into();
            }
        }
    }
}

/// The fields of an object being described.
struct Fields<S> {
    name: &'static str,
    properties: Map<String, Value>,
    required: Vec<Value>,
    of: PhantomData<fn(&S)>,
}

impl<S> Fields<S> {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            properties: Map::new(),
            required: Vec::new(),
            of: PhantomData,
        }
    }

    /// A field every value holds. `_field` says which, so its type is the
    /// field's own.
    fn required<T: Describe>(&mut self, schema: &mut Schema, wire: &str, _field: fn(&S) -> &T) {
        let mut property = T::describe(schema);
        property["x-since"] = field_since(self.name, wire, 0).into();
        self.properties.insert(wire.to_owned(), property);
        self.required.push(wire.into());
    }

    /// A field a value may leave out, which then holds its type's default.
    fn optional<T: Describe + Default + Serialize>(
        &mut self,
        schema: &mut Schema,
        wire: &str,
        field: fn(&S) -> &T,
    ) {
        let default = serde_json::to_value(T::default()).unwrap_or_default();
        self.optional_or(schema, wire, field, default);
    }

    /// A field a value may leave out, which then holds `default`.
    fn optional_or<T: Describe>(
        &mut self,
        schema: &mut Schema,
        wire: &str,
        _field: fn(&S) -> &T,
        default: Value,
    ) {
        let mut property = T::describe(schema);
        property["default"] = default;
        property["x-since"] = field_since(self.name, wire, 0).into();
        self.properties.insert(wire.to_owned(), property);
    }

    fn finish(self) -> Value {
        json!({
            "x-kind": "object",
            "type": "object",
            "x-since": 0,
            "properties": self.properties,
            "required": self.required,
        })
    }
}

/// The variants of a union being described.
struct Variants<U> {
    tag: &'static str,
    variants: Vec<Value>,
    one_of: Vec<Value>,
    of: PhantomData<fn(&U)>,
}

impl<U> Variants<U> {
    fn new(tag: &'static str) -> Self {
        Self {
            tag,
            variants: Vec::new(),
            one_of: Vec::new(),
            of: PhantomData,
        }
    }

    fn push(&mut self, variant: &str, payload: Option<Value>) {
        let tag = snake(variant);
        let mut entry = json!({ "tag": tag, "x-since": 0 });
        let mut one = json!({
            "properties": { self.tag: { "const": tag } },
            "required": [self.tag],
        });
        if let Some(payload) = payload {
            let name = payload["$ref"]
                .as_str()
                .and_then(|path| path.strip_prefix("#/$defs/"))
                .unwrap_or_default()
                .to_owned();
            entry["type"] = name.into();
            one["allOf"] = json!([payload]);
        }
        self.variants.push(entry);
        self.one_of.push(one);
    }

    /// A variant that holds a struct defined on its own: `_make` is the
    /// variant's constructor, so the struct's type is its own.
    fn newtype<P: Describe>(&mut self, schema: &mut Schema, variant: &str, _make: fn(P) -> U) {
        let payload = P::describe(schema);
        self.push(variant, Some(payload));
    }

    /// A variant with fields of its own, defined as `Union.Variant`.
    fn structure(&mut self, variant: &str, payload: Value) {
        self.push(variant, Some(payload));
    }

    /// A variant that holds nothing.
    fn unit(&mut self, variant: &str) {
        self.push(variant, None);
    }

    fn finish(self, unknown: Value) -> Value {
        json!({
            "x-kind": "union",
            "x-tag": self.tag,
            "x-since": 0,
            "oneOf": self.one_of,
            "x-variants": self.variants,
            "x-unknown": unknown,
        })
    }
}

/// A field's name in the JSON: its name in Rust, unless it is renamed.
macro_rules! wire {
    ($field:ident) => {
        stringify!($field)
    };
    ($field:ident = $wire:literal) => {
        $wire
    };
}

/// One field a value may leave out: with its type's default, or with the
/// default given after a colon.
macro_rules! optional {
    ($fields:ident, $schema:ident, $ty:ident, $field:ident $(= $wire:literal)?) => {
        $fields.optional($schema, wire!($field $(= $wire)?), |value: &$ty| &value.$field)
    };
    ($fields:ident, $schema:ident, $ty:ident, $field:ident $(= $wire:literal)? : $default:expr) => {
        $fields.optional_or(
            $schema,
            wire!($field $(= $wire)?),
            |value: &$ty| &value.$field,
            Value::from($default),
        )
    };
}

/// Describes a struct: `{ required fields } { fields a value may leave out }`,
/// each by its name in Rust, `= "name"` when the JSON renames it, and
/// `: default` when its default is not its type's.
macro_rules! object {
    (
        $ty:ident
        { $($req:ident $(= $rwire:literal)?),* $(,)? }
        { $($opt:ident $(= $owire:literal)? $(: $default:expr)?),* $(,)? }
    ) => {
        impl Describe for $ty {
            fn describe(schema: &mut Schema) -> Value {
                #[allow(dead_code)]
                fn every_field_is_described(value: &$ty) {
                    let $ty { $($req: _,)* $($opt: _,)* } = value;
                }
                schema.define(stringify!($ty), |schema| {
                    #[allow(unused_mut)]
                    let mut fields = Fields::<$ty>::new(stringify!($ty));
                    $(fields.required(schema, wire!($req $(= $rwire)?), |value: &$ty| &value.$req);)*
                    $(optional!(fields, schema, $ty, $opt $(= $owire)? $(: $default)?);)*
                    let check = Check {
                        round_trip: round_trip::<$ty>,
                        fields: Some(|value| {
                            let typed: $ty =
                                serde_json::from_value(value).map_err(|error| error.to_string())?;
                            Ok(BTreeMap::from([
                                $((
                                    wire!($req $(= $rwire)?).to_owned(),
                                    serde_json::to_value(&typed.$req).unwrap_or_default(),
                                ),)*
                                $((
                                    wire!($opt $(= $owire)?).to_owned(),
                                    serde_json::to_value(&typed.$opt).unwrap_or_default(),
                                ),)*
                            ]))
                        }),
                    };
                    (fields.finish(), check)
                })
            }
        }
    };
}

/// Describes an enum written as one of a closed list of names.
macro_rules! names {
    ($ty:ident { $($variant:ident),* $(,)? }) => {
        impl Describe for $ty {
            fn describe(schema: &mut Schema) -> Value {
                #[allow(dead_code)]
                fn every_value_is_described(value: $ty) {
                    match value {
                        $($ty::$variant => {})*
                    }
                }
                schema.define(stringify!($ty), |_| {
                    let values: Vec<Value> = vec![
                        $(serde_json::to_value($ty::$variant).unwrap_or_default(),)*
                    ];
                    let def = json!({
                        "x-kind": "enum",
                        "type": "string",
                        "x-since": 0,
                        "enum": values,
                    });
                    (def, Check { round_trip: round_trip::<$ty>, fields: None })
                })
            }
        }
    };
}

/// Describes a union: its tag field, the variants that hold a struct of their
/// own, those with fields of their own (`{ required } { may be left out }`),
/// those that hold nothing, and the pattern of the variant an unknown tag is
/// read as, with what it keeps.
macro_rules! union {
    (
        $ty:ident tag $tag:literal
        newtype { $($nv:ident),* $(,)? }
        structs { $($sv:ident { $($sr:ident),* } { $($so:ident),* }),* $(,)? }
        units { $($uv:ident),* $(,)? }
        unknown $unknown:pat => $keeps:expr
    ) => {
        impl Describe for $ty {
            fn describe(schema: &mut Schema) -> Value {
                #[allow(dead_code)]
                fn every_variant_is_described(value: &$ty) {
                    match value {
                        $($ty::$nv(_) => {})*
                        $($ty::$sv { $($sr: _,)* $($so: _,)* } => {})*
                        $($ty::$uv => {})*
                        $unknown => {}
                    }
                }
                schema.define(stringify!($ty), |schema| {
                    #[allow(unused_mut)]
                    let mut variants = Variants::<$ty>::new($tag);
                    $(variants.newtype(schema, stringify!($nv), $ty::$nv);)*
                    $(
                        let name = concat!(stringify!($ty), ".", stringify!($sv));
                        let payload = schema.define(name, |schema| {
                            #[allow(unused_mut)]
                            let mut fields = Fields::<$ty>::new(name);
                            $(fields.required(schema, stringify!($sr), |value: &$ty| match value {
                                $ty::$sv { $sr, .. } => $sr,
                                _ => unreachable!("only this variant is described here"),
                            });)*
                            $(fields.optional(schema, stringify!($so), |value: &$ty| match value {
                                $ty::$sv { $so, .. } => $so,
                                _ => unreachable!("only this variant is described here"),
                            });)*
                            let check = Check {
                                round_trip: |value| {
                                    let value = tagged(value, $tag, &snake(stringify!($sv)));
                                    round_trip::<$ty>(value)
                                },
                                fields: Some(|value| {
                                    let value = tagged(value, $tag, &snake(stringify!($sv)));
                                    let typed: $ty = serde_json::from_value(value)
                                        .map_err(|error| error.to_string())?;
                                    #[allow(unused_variables)]
                                    match typed {
                                        $ty::$sv { $($sr,)* $($so,)* } => Ok(BTreeMap::from([
                                            $((
                                                stringify!($sr).to_owned(),
                                                serde_json::to_value(&$sr).unwrap_or_default(),
                                            ),)*
                                            $((
                                                stringify!($so).to_owned(),
                                                serde_json::to_value(&$so).unwrap_or_default(),
                                            ),)*
                                        ])),
                                        other => Err(format!("read as {other:?}")),
                                    }
                                }),
                            };
                            (fields.finish(), check)
                        });
                        variants.structure(stringify!($sv), payload);
                    )*
                    $(variants.unit(stringify!($uv));)*
                    let keeps: fn(&mut Schema) -> Value = $keeps;
                    let unknown = keeps(schema);
                    (variants.finish(unknown), Check { round_trip: round_trip::<$ty>, fields: None })
                })
            }
        }
    };
}

/// Describes text with a rule of its own, named by `format`.
macro_rules! text {
    ($ty:ident, $format:literal) => {
        impl Describe for $ty {
            fn describe(schema: &mut Schema) -> Value {
                schema.define(stringify!($ty), |_| {
                    let def = json!({
                        "x-kind": "string",
                        "type": "string",
                        "format": $format,
                        "x-since": 0,
                    });
                    (def, Check { round_trip: round_trip::<$ty>, fields: None })
                })
            }
        }
    };
}

impl Describe for String {
    fn describe(_: &mut Schema) -> Value {
        json!({ "type": "string" })
    }
}

impl Describe for bool {
    fn describe(_: &mut Schema) -> Value {
        json!({ "type": "boolean" })
    }
}

impl Describe for u8 {
    fn describe(_: &mut Schema) -> Value {
        json!({ "type": "integer", "format": "uint8", "minimum": 0, "maximum": u8::MAX })
    }
}

impl Describe for u16 {
    fn describe(_: &mut Schema) -> Value {
        json!({ "type": "integer", "format": "uint16", "minimum": 0, "maximum": u16::MAX })
    }
}

impl Describe for u32 {
    fn describe(_: &mut Schema) -> Value {
        json!({ "type": "integer", "format": "uint32", "minimum": 0, "maximum": u32::MAX })
    }
}

impl Describe for i64 {
    fn describe(_: &mut Schema) -> Value {
        json!({ "type": "integer", "format": "int64" })
    }
}

impl Describe for f64 {
    fn describe(_: &mut Schema) -> Value {
        json!({ "type": "number", "format": "double" })
    }
}

impl Describe for serde_json::Number {
    fn describe(_: &mut Schema) -> Value {
        json!({ "type": "number" })
    }
}

impl Describe for Value {
    fn describe(_: &mut Schema) -> Value {
        json!({ "x-json": true })
    }
}

impl<T: Describe> Describe for Option<T> {
    fn describe(schema: &mut Schema) -> Value {
        json!({ "anyOf": [T::describe(schema), { "type": "null" }] })
    }
}

impl<T: Describe> Describe for Vec<T> {
    fn describe(schema: &mut Schema) -> Value {
        json!({ "type": "array", "items": T::describe(schema) })
    }
}

impl<T: Describe> Describe for Box<T> {
    fn describe(schema: &mut Schema) -> Value {
        T::describe(schema)
    }
}

impl<T: Describe> Describe for BTreeMap<String, T> {
    fn describe(schema: &mut Schema) -> Value {
        json!({ "type": "object", "additionalProperties": T::describe(schema) })
    }
}

// The envelope and the screen.
object!(Envelope { navree, build, vocab } { app, shell, screen, ops, web, upgrade, icons });
object!(Screen { title, body } { subtitle, id, back, trail, nav, actions, search, refresh });
object!(Back { title, href } {});
object!(Search { name } { value, placeholder, action, hidden });
object!(Web { url } { title, why });
object!(Upgrade { min_app, url } {});

// The vocabulary.
union!(Node tag "t"
    newtype {
        Stack, Grid, Section, List, Row, Split, Tabs, Disclosure, Divider, Spacer, Region,
        Text, Rich, Image, Icon, Avatar, Badge, Progress, Banner, Empty, Facts, Table, Chart,
        Code, Button, Link, Menu, Form, Field, Sheet, Dialog, Popover, Native, Web, Toolbar,
        Gauge, Figure, Timeline, FeedRow, Message, Pane,
    }
    structs {}
    units {}
    unknown Node::Unknown(_) => |schema| json!({
        "id": schema.of::<String>(),
        "alt": schema.of::<Node>(),
    })
);
names!(Role { None, Primary, Secondary, Destructive, Cancel, Confirm, Create, Navigation });
names!(Tone { Neutral, Accent, Success, Warning, Danger, Info });
names!(Prominence { High, Medium, Low });
names!(Axis { Vertical, Horizontal });
names!(Align { Start, Center, End, Stretch, Baseline });
names!(Justify { Start, Center, End, Between });
names!(Standard { Share, Delete, Add, Edit, Search, Done, Cancel, More });
object!(Stack {} { id, axis, gap, align, justify, wrap, hidden, children });
object!(Grid {} { id, cols, min, gap, children });
names!(SectionStyle { Plain, Card, Inset });
object!(Section {} { id, title, subtitle, style, actions, children, footer });
object!(List {} { id, children, more, ordered });
object!(Row { title } { id, leading, subtitle, detail, trailing, badge, state, action, swipe, menu });
names!(RowState { Done, Current, Upcoming });
object!(Split { primary, detail } { id });
object!(Tabs {} { id, items });
object!(Tab { label } { icon, selected, href, children });
object!(Disclosure { label } { id, open, children });
object!(Divider {} { id });
object!(Spacer {} { id });
object!(Region { id, src } { lazy, live, children });
object!(Live { src } { event });
names!(TextStyle { LargeTitle, Title, Headline, Body, Callout, Caption, Mono });
object!(Text { value } { id, style, tone, muted, lines, heading });
object!(Rich {} { id, runs, blocks, style, muted });
object!(Run { text } { bold, italic, code, href });
names!(BlockKind { Paragraph, Heading, Quote, Bullet, Numbered, Code });
object!(Block {} { kind, runs, level });
names!(Fit { Contain, Cover });
object!(Image { src } { id, alt, width, height, fit });
object!(Icon {} { id, name, paths, fills, view_box, label, tone });
object!(IconShape {} { paths, fills, view_box });
object!(Avatar { name } { id, src, initials });
object!(Badge { text } { id, tone, dot });
object!(Progress {} { id, value, label });
object!(Banner {} { id, tone, title, text, icon, actions });
object!(Empty { title } { id, icon, text, actions });
object!(Facts {} { id, items });
object!(Fact { label } { value, icon, tone, details });
names!(ColumnRole { Leading, Title, Subtitle, Status, Trailing, Actions });
names!(SortDir { Asc, Desc });
object!(Sort { href } { dir });
object!(Column { label } { key, numeric, align, sort, role, secondary });
object!(TableRow {} { id, cells, action, actions });
object!(Table {} { id, caption, columns, rows, query, more, prev, summary, empty });
names!(ChartKind { Line, Area, Bar, Donut, Sparkline, Funnel });
object!(Series { label } { values, shown });
object!(Chart {} { id, kind, title, labels, series });
object!(Code { text } { id, lang });
object!(Button { label } {
    id, icon, icon_only, role, standard, prominence, undoable, disabled, pending, action
});
object!(Link { label, action } { id, children });
object!(Menu { label } { id, icon, items });
names!(Method { Get, Post });
object!(Hidden { name } { value, id });
object!(Form {} { id, method, action, hidden, children });
object!(Field { name, input } { id, label, help, error, required, disabled });
names!(SheetSize { Medium, Large, Full });
object!(Sheet { id } { title, subtitle, size, open, children, actions });
object!(Dialog { id } { title, text, alert, open, children, actions });
object!(Popover { id } { children });
object!(NativeView { name } { id, props, alt });
object!(WebRegion { html } { id });
object!(Toolbar {} { id, items });
object!(ToolbarItem { title, icon } {
    id, standard, role, prominence, undoable, disabled, action, menu
});
object!(Gauge { value, min, max } { id, label, shown, tone });
names!(Direction { Up, Down, Flat });
object!(Trend { direction } { text, tone });
object!(Figure { label, value } { id, trend, detail, tone });
object!(Timeline {} { id, items });
object!(Moment { title } { id, subtitle, time, state, icon, tone, detail });
object!(FeedRow {} { id, actor, icon, runs, time, tone, detail, action });
names!(Delivery { Sending, Sent, Delivered, Read, Failed });
object!(Message { author } { id, avatar, mine, note, body, time, delivery });
names!(PaneRole { List, Content, Detail, Supporting, Inspector });
object!(Pane { role } { id, title, current, children });

// The controls of fields.
union!(Input tag "t"
    newtype { Text, Toggle, Choice, Choices, Date, Slider, Colour, File }
    structs {}
    units {}
    unknown Input::Unknown => |_| json!({})
);
names!(TextKind {
    Plain, Email, Password, Number, Decimal, Phone, Url, Search, Multiline, OneTimeCode
});
object!(TextInput {} {
    kind, value, placeholder, max_length, min, max, step, autofill, readonly
});
names!(ToggleKind { Switch, Checkbox });
object!(ToggleInput {} { kind, on, value });
names!(ChoiceKind { Menu, Segmented, Radio, List });
object!(ChoiceOption { value, label } { detail, disabled });
object!(ChoiceSearch { href, name } { min });
object!(ChoiceCreate { label } { href });
object!(ChoiceInput {} { kind, options, value, placeholder, search, create });
object!(ChoicesInput {} { options, values, search, create });
names!(DateKind { Date, Time, Datetime });
names!(Weekday { Monday, Tuesday, Wednesday, Thursday, Friday, Saturday, Sunday });
object!(DateInput {} { kind, value, min, max, off_days, off_weekdays });
object!(SliderInput { min, max, value } { step });
object!(ColourInput {} { value });
object!(FileInput {} { accept, multiple, capture, keep_location });

// What controls do.
union!(Action tag "t"
    newtype { Device }
    structs {
        Navigate { href } { present },
        Submit {} { form, href, with },
        Request { href } { method, fields },
        Commands { list } {},
    }
    units { Back, Refresh, Dismiss }
    unknown Action::Unknown => |_| json!({})
);
names!(Present { Push, Replace, Root, Sheet, Full, External });
object!(Pair { name } { value });
names!(State { Open, Hidden, Checked, Selected, Disabled, Active, Invalid, Pending });
union!(Command tag "c"
    newtype {}
    structs {
        Toggle { id } {},
        Show { id } {},
        Hide { id } {},
        Open { id } {},
        Close { id } {},
        Copy { text } {},
        Set { id, state, on } {},
        Flip { id, state } {},
        Focus { id } {},
        Submit { id } {},
        Text { id, text } {},
        Count { id } { max },
        Value { id, value } {},
        Toast { text } { tone },
    }
    units {}
    unknown Command::Unknown => |_| json!({})
);

// What the server changes.
union!(Op tag "op"
    newtype { Toast }
    structs {
        Morph { target, node } {},
        Replace { target, node } {},
        Append { target, nodes } { more },
        Prepend { target, nodes } {},
        Remove { target } {},
        Errors { form, fields } {},
        Open { target } {},
        Close { target } {},
        Focus { target } {},
        Url { href } { mode },
        Navigate { href } {},
        NavigateExternal { href } {},
        Invalidate { target } {},
        Device { call } {},
    }
    units { Reload }
    unknown Op::Unknown => |_| json!({})
);
names!(UrlMode { Push, Replace });
object!(ToastAction { label, href } { post });
object!(Toast { text } { tone, title, id, action });

// The shell.
object!(Shell { navree, build, name, home } {
    stamp, context, theme, nav, user, routes, device
});
object!(Nav {} { groups });
object!(NavGroup {} { label, items });
object!(NavItem { label, href } { icon, badge, items });
object!(User { name } { email, menu });
object!(Route { matches = "match" } { present, chrome: true });
object!(Theme {} { name, accent, accent_dark });
object!(Push { register } {});
object!(Device {} { caps, push, lock });

// The descriptor.
object!(ClientDescriptor { platform, form, vocab, text_percent } {
    os, app, lib, scheme, contrast, locale, tz, caps
});
names!(Platform { Ios, Ipados, Macos, Watchos, Tvos, Visionos, Android, Wearos, Windows, Linux });
names!(FormFactor { Phone, Tablet, Desktop, Watch, Tv, Spatial, Car });
names!(Scheme { Light, Dark });
names!(Contrast { Normal, More });

// Device calls and their results.
names!(Feature {
    Haptic, Clipboard, Share, Capture, AuthSession, OpenUrl, Push, LocalNotification, Passkey,
    Confirm, LocationOnce, ScanCode, AppLock, PlatformSignIn, RequestReview, OpenSettings,
});
names!(Refusal { Unsupported, NotBuilt, Denied, Restricted, Unavailable, Cancelled, Busy, Failed });
text!(OriginPath, "origin-path");
text!(HttpsUrl, "https-url");
text!(Ticket, "ticket");
object!(DeviceCall { call } { then, alt });
object!(Then { href, ticket, exp } {});
union!(Alt tag "t"
    newtype {}
    structs {
        Navigate { href } {},
        Text { text } {},
    }
    units { Hide }
    unknown Alt::Unknown => |_| json!({})
);
union!(Call tag "f"
    newtype {
        Haptic, Copy, Share, OpenUrl, AuthSession, RegisterPush, Notify, CancelNotification,
        SetBadge, OpenSettings, Passkey, Confirm, LocateOnce, ScanCode, PlatformSignIn,
    }
    structs {}
    units {}
    unknown Call::Unknown => |_| json!({})
);
union!(ReplyCall tag "f"
    newtype { Haptic, SetBadge, CancelNotification }
    structs {}
    units { RequestReview }
    unknown ReplyCall::Unknown => |_| json!({})
);
names!(HapticKind { Success, Warning, Error, Selection, Light, Medium, Heavy });
object!(Haptic { kind } {});
object!(CopyText { text } { sensitive });
object!(Share {} { title, text, href, url, file });
object!(Shared { completed } {});
object!(OpenUrl { url } { inside });
object!(AuthSession { start } { ephemeral });
object!(RegisterPush {} { categories });
names!(PushProvider { Apns, Fcm, Wns });
names!(PushEnvironment { Development, Production });
object!(PushRegistered { provider, environment, token, app_id, install_id } { locale });
union!(When tag "t"
    newtype {}
    structs {
        At { unix } {},
        After { secs } {},
        Daily { hour, minute } {},
    }
    units {}
    unknown When::Unknown => |_| json!({})
);
object!(Notify { id, title, at } { body, href, thread });
object!(CancelNotification { id } {});
object!(SetBadge {} { count });
names!(SettingsPage { App, Notifications });
object!(OpenSettings {} { page });
names!(PasskeyMode { Register, SignIn });
object!(Passkey { mode, options } { conditional });
object!(PasskeyCredential { credential } {});
names!(ConfirmMethod { Passkey, DeviceKey, Either });
object!(Confirm { reason } { detail, method });
names!(ConfirmedBy { Passkey, DeviceKey });
object!(Confirmed { challenge, method } { credential, key_id, signature });
names!(Accuracy { Approximate, Precise });
object!(LocateOnce {} { accuracy, max_age_secs, timeout_secs });
object!(Located { lat, lon, accuracy_m, at, precise } {});
names!(CodeFormat {
    Qr, Ean13, Ean8, UpcA, UpcE, Code128, Code39, Code93, Itf, Pdf417, Aztec, DataMatrix
});
object!(ScanCode {} { formats, prompt });
object!(Scanned { format, value } {});
object!(AppLock { after_secs } {});
names!(SignInProvider { Apple, Google });
object!(PlatformSignIn { provider, nonce } { client_id });
object!(SignedIn { provider, id_token } { code, given_name, family_name, email });

/// The definitions every other is reached from: what a server answers with,
/// what a client says about itself, and what it posts back.
const ROOTS: &[&str] = &[
    "Envelope",
    "Shell",
    "ClientDescriptor",
    "Feature",
    "Refusal",
    "Shared",
    "PushRegistered",
    "PasskeyCredential",
    "Confirmed",
    "Located",
    "Scanned",
    "SignedIn",
];

/// Every definition, from the roots.
pub(crate) fn described() -> Schema {
    let mut schema = Schema::default();
    schema.of::<Envelope>();
    schema.of::<Shell>();
    schema.of::<ClientDescriptor>();
    schema.of::<Feature>();
    schema.of::<Refusal>();
    schema.of::<Shared>();
    schema.of::<PushRegistered>();
    schema.of::<PasskeyCredential>();
    schema.of::<Confirmed>();
    schema.of::<Located>();
    schema.of::<Scanned>();
    schema.of::<SignedIn>();
    // A kind's struct and its fields arrived with the kind.
    for kind in Kind::ALL {
        schema.arrived(kind.type_name(), kind.since());
    }
    if let Some(Value::Array(variants)) = schema
        .defs
        .get_mut("Node")
        .and_then(|node| node.get_mut("x-variants"))
    {
        for variant in variants {
            let since = variant["tag"]
                .as_str()
                .and_then(Kind::parse)
                .map_or(0, Kind::since);
            variant["x-since"] = since.into();
        }
    }
    schema
}

/// The whole schema, as `schema/navree-native.schema.json` holds it.
pub fn document() -> Value {
    let schema = described();
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Navree Native",
        "description": "The protocol between a Navree app and a native client: the envelope a server answers with, the shell, the descriptor a client sends, the device calls and their results. Written by the navree-native crate; edit the Rust types, never this file.",
        "x-major": crate::MAJOR,
        "x-vocab": crate::VOCAB,
        "x-roots": ROOTS,
        "$ref": "#/$defs/Envelope",
        "$defs": schema.defs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values a field of this shape may hold, the least first.
    fn samples(defs: &BTreeMap<String, Value>, shape: &Value, depth: usize) -> Vec<Value> {
        assert!(depth < 32, "the samples of {shape} do not end");
        if let Some(path) = shape.get("$ref").and_then(Value::as_str) {
            let name = path.trim_start_matches("#/$defs/");
            let def = &defs[name];
            return match def["x-kind"].as_str() {
                Some("object") => {
                    let least = least(defs, def, depth + 1);
                    let mut more = least.clone();
                    let optional = def["properties"]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .find(|(field, _)| !required(def).contains(field.as_str()));
                    if let Some((field, property)) = optional
                        && let Some(sample) = samples(defs, property, depth + 1).into_iter().next()
                    {
                        more[field] = sample;
                    }
                    vec![least, more]
                }
                Some("enum") => def["enum"].as_array().cloned().unwrap_or_default(),
                Some("union") => {
                    let variant = &def["x-variants"][0];
                    let tag = def["x-tag"].as_str().unwrap();
                    let body = match variant["type"].as_str() {
                        Some(payload) => least(defs, &defs[payload], depth + 1),
                        None => json!({}),
                    };
                    vec![tagged(body, tag, variant["tag"].as_str().unwrap())]
                }
                Some("string") => match def["format"].as_str() {
                    Some("origin-path") => vec![json!("/orders/1042")],
                    Some("https-url") => vec![json!("https://pay.example/7")],
                    _ => vec![json!("sealed")],
                },
                kind => panic!("{name} is of no kind I know: {kind:?}"),
            };
        }
        if let Some(Value::Array(any)) = shape.get("anyOf") {
            return samples(defs, &any[0], depth + 1);
        }
        if shape.get("x-json").is_some() {
            return vec![json!({"lat": 30.9}), json!(7)];
        }
        match shape["type"].as_str() {
            Some("string") => vec![json!("x"), json!("")],
            Some("boolean") => vec![json!(true), json!(false)],
            Some("integer") => vec![json!(1), json!(0)],
            Some("number") => vec![json!(1.5), json!(0)],
            Some("array") => {
                let item = samples(defs, &shape["items"], depth + 1).remove(0);
                vec![json!([item]), json!([])]
            }
            Some("object") => {
                let value = samples(defs, &shape["additionalProperties"], depth + 1).remove(0);
                vec![json!({ "k": value }), json!({})]
            }
            other => panic!("a shape of no type I know: {other:?} in {shape}"),
        }
    }

    fn required(def: &Value) -> BTreeSet<&str> {
        def["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect()
    }

    /// The least value an object's definition allows: its required fields.
    fn least(defs: &BTreeMap<String, Value>, def: &Value, depth: usize) -> Value {
        let mut out = Map::new();
        for field in required(def) {
            let shape = &def["properties"][field];
            out.insert(field.to_owned(), samples(defs, shape, depth + 1).remove(0));
        }
        Value::Object(out)
    }

    #[test]
    fn every_definition_reads_and_writes_as_serde_does() {
        let schema = described();
        let defs = schema.defs();
        for (name, def) in defs {
            let check = schema.checks[name];
            match def["x-kind"].as_str().unwrap() {
                "object" => {
                    let read_fields = check.fields.expect("an object reads its fields");
                    let least = least(defs, def, 0);
                    let read = read_fields(least.clone()).unwrap_or_else(|error| {
                        panic!("{name}: the least value the schema allows, {least}, is refused: {error}")
                    });
                    let needed = required(def);
                    for (field, property) in def["properties"].as_object().unwrap() {
                        if needed.contains(field.as_str()) {
                            let mut without = least.clone();
                            without.as_object_mut().unwrap().remove(field);
                            assert!(
                                read_fields(without).is_err(),
                                "{name}.{field} is described as required, and serde reads a value without it"
                            );
                            continue;
                        }
                        assert_eq!(
                            read[field], property["default"],
                            "{name}.{field}: serde's default is not the described one"
                        );
                        let takes_a_value = samples(defs, property, 0).into_iter().any(|sample| {
                            let mut with = least.clone();
                            with[field] = sample;
                            read_fields(with)
                                .is_ok_and(|read| read[field] != property["default"])
                        });
                        assert!(
                            takes_a_value,
                            "{name}.{field}: no value of its described name and shape reaches it"
                        );
                    }
                }
                "enum" => {
                    for value in def["enum"].as_array().unwrap() {
                        assert_eq!((check.round_trip)(value.clone()).as_ref(), Ok(value), "{name}");
                    }
                    assert!((check.round_trip)(json!("not-a-value")).is_err(), "{name}");
                }
                "union" => {
                    let tag = def["x-tag"].as_str().unwrap();
                    for variant in def["x-variants"].as_array().unwrap() {
                        let label = variant["tag"].as_str().unwrap();
                        let body = match variant["type"].as_str() {
                            Some(payload) => least(defs, &defs[payload], 0),
                            None => json!({}),
                        };
                        let value = tagged(body, tag, label);
                        let back = (check.round_trip)(value.clone()).unwrap_or_else(|error| {
                            panic!("{name}: the variant {value} is refused: {error}")
                        });
                        assert_eq!(back[tag], label, "{name}: {value} came back as {back}");
                    }
                    let unknown = tagged(json!({}), tag, "from-a-newer-server");
                    assert!(
                        (check.round_trip)(unknown).is_ok(),
                        "{name}: a tag this build does not know is refused"
                    );
                }
                "string" => {
                    let sample = samples(defs, &reference(name), 0).remove(0);
                    assert_eq!((check.round_trip)(sample.clone()), Ok(sample), "{name}");
                }
                kind => panic!("{name} is of no kind I know: {kind}"),
            }
        }
    }

    #[test]
    fn the_schema_holds_every_kind_and_what_each_arrived_in() {
        let schema = described();
        let node = &schema.defs()["Node"];
        let tags: BTreeSet<&str> = node["x-variants"]
            .as_array()
            .unwrap()
            .iter()
            .map(|variant| variant["tag"].as_str().unwrap())
            .collect();
        let kinds: BTreeSet<&str> = Kind::ALL.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(tags, kinds);
        for (name, def) in schema.defs() {
            assert!(def["x-since"].is_u64(), "{name} records no vocabulary");
            for (field, property) in def["properties"].as_object().into_iter().flatten() {
                assert!(property["x-since"].is_u64(), "{name}.{field} records no vocabulary");
            }
            for variant in def["x-variants"].as_array().into_iter().flatten() {
                assert!(variant["x-since"].is_u64(), "{name}: {variant} records no vocabulary");
            }
        }
        for kind in Kind::ALL {
            assert!(schema.defs().contains_key(kind.type_name()), "{kind:?}");
        }
    }
}
