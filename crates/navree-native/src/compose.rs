//! Free composition: the parts a screen with a design of its own is laid out
//! with.
//!
//! Most screens are lists, records and forms, and say only what their parts
//! are: the client places them as its system does. A screen that deserves a
//! design of its own (a day at a glance, a wall of tiles) is composed: parts
//! one after another or side by side, in cards, over each other, in a run
//! that scrolls across, with a shape behind a symbol and a number set large.
//! These are the parts for that. Every one of them has a drawing of its own in
//! each system's toolkit, built from that system's own styles, so a composed
//! screen is still its system's screen.
//!
//! What a composition may say is small on purpose:
//!
//! - Room is in steps of the system's spacing scale, never in points.
//! - A length, where one is needed at all ([`Size`]), is in points: the unit
//!   both systems lay out in, independent of the screen's density. It can
//!   follow the person's text size.
//! - Colour is a [`Colour`] by name; type is a
//!   [`TextStyle`](crate::node::TextStyle) with a [`Weight`]; a corner is the
//!   system's own; motion is a word ([`Appear`], [`Change`], [`Transition`])
//!   and its timing is the system's.
//! - What depends on the room a screen has travels with the screen and is
//!   chosen on the device: a [`Stack`](crate::node::Stack) names the axis it
//!   takes in a wide room, a [`Grid`](crate::node::Grid) and a [`Carousel`]
//!   show as many parts as fit, and [`Adaptive`] holds the parts for a compact
//!   room beside the parts for a wide one.
//!
//! The kinds live in [`node`](crate::node) as well: they are the parts a
//! [`Node`] holds.

use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::charts::Colour;
use crate::node::{Node, is_default, is_false, is_zero};

/// How heavy text is set. Each system has these four in its own face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Weight {
    /// The usual weight of running text.
    #[default]
    Regular,
    /// A little heavier: a label.
    Medium,
    /// Heavier: a heading among text.
    Semibold,
    /// The heaviest: a figure, a title.
    Bold,
}

/// One of the nine places of a box: its corners, the middles of its edges and
/// its centre. Start and end follow the direction text runs in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Place {
    /// The top corner at the start.
    TopStart,
    /// The middle of the top edge.
    Top,
    /// The top corner at the end.
    TopEnd,
    /// The middle of the edge at the start.
    Start,
    /// The centre.
    #[default]
    Center,
    /// The middle of the edge at the end.
    End,
    /// The bottom corner at the start.
    BottomStart,
    /// The middle of the bottom edge.
    Bottom,
    /// The bottom corner at the end.
    BottomEnd,
}

/// A length a composed part is given, in points: the unit both systems lay
/// out in, the same on every screen density. A side left out is the part's
/// own.
///
/// A size is for what has no size of its own: a shape, the box a symbol or a
/// ring stands in. Text is never given one: its style sizes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Size {
    /// How wide, in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u16>,
    /// How high, in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u16>,
    /// Whether it grows and shrinks with the person's text size, as the text
    /// beside it does.
    #[serde(default, skip_serializing_if = "is_false")]
    pub scaled: bool,
}

/// How a part that joins a screen already drawn comes in: a card an answer
/// adds, what a region holds once it has loaded. The timing is the system's
/// own, and a person who asked for less motion gets a plain fade or none. A
/// screen's first drawing is never animated part by part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Appear {
    /// It fades in.
    Fade,
    /// It rises a little as it fades in.
    Rise,
    /// It grows from a little smaller as it fades in.
    Scale,
}

/// How text shows that its words changed, when an answer changes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Change {
    /// As a number counting to its new value: its figures roll.
    Count,
    /// The old words fade into the new.
    Fade,
}

/// How a part leads into the screen its action opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Transition {
    /// The part grows into the screen, and the screen shrinks back into the
    /// part when it is left.
    Zoom,
}

/// The form of a [`Shape`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ShapeForm {
    /// A rectangle with the system's own corner.
    #[default]
    Rounded,
    /// A circle.
    Circle,
    /// A rectangle whose short sides are half circles.
    Capsule,
    /// A rectangle with square corners.
    Rect,
}

/// A surface that holds parts, one under another: a tile of a dashboard, the
/// summary at the head of a record. The client draws it as its system draws a
/// card in content (its ground, its corner, its padding), never as a bar or a
/// sheet is drawn.
///
/// A card may be tinted by a hue, softly: the tint says which thing the card
/// is about and never carries meaning alone. A card with an `action` is one
/// control: the whole card is tapped, and `label` names it for a reader who
/// can't see it. `menu` is offered on a long press or a secondary click, and
/// every item of it is also reachable some other way.
///
/// A card does not stand inside another card.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The hue its ground is tinted with. A card with none has the system's
    /// own ground.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
    /// The room between its edge and its parts, in steps of the system's
    /// spacing scale. Left out, it is the system's own for a card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad: Option<u8>,
    /// What a tap on the card does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
    /// What the card is called as a control: `Sleep, 87 of 100`. A card with
    /// an action has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// What a long press or a secondary click offers: buttons, links and
    /// dividers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub menu: Vec<Node>,
    /// How the card leads into the screen its action opens.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub transition: Option<Transition>,
    /// How the card comes in when it joins a screen already drawn.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub appear: Option<Appear>,
    /// The parts, one under another.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// Parts over each other, the first at the back: a symbol on a shape, a word
/// in a corner of a picture. The largest part gives the box its size, unless
/// `size` does, and every part sits at `place`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layers {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Where the parts sit in the box.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub place: Place,
    /// The size of the box, when the parts do not give it: a part that has no
    /// size of its own (a shape, a ring) fills it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// The room between its edge and its parts, in steps of the system's
    /// spacing scale.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pad: u8,
    /// Whether it takes the room left along the stack it stands in.
    #[serde(default, skip_serializing_if = "is_false")]
    pub grow: bool,
    /// The parts, the first at the back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// A plain shape, filled or outlined in a colour by name: the ground behind a
/// symbol, a dot in a key, a bar at the edge of a card. It fills the box it
/// stands in unless it has a `size`.
///
/// A shape is decoration unless it has a `label`: a colour is never the only
/// thing that says something.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shape {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Its form.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub form: ShapeForm,
    /// The colour it is filled with. A blend runs along it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Colour>,
    /// Whether the fill is the soft tint of its hue, a ground for what stands
    /// on it, and not the hue at full strength.
    #[serde(default, skip_serializing_if = "is_false")]
    pub soft: bool,
    /// The colour of its outline, a hairline. A shape with none has no
    /// outline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Colour>,
    /// Its size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// What it says to a reader who can't see it, when it says anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Parts side by side that scroll across, stopping at a part: a run of cards
/// that does not fit the screen. Each part is at least `min` points wide and
/// as many as fit are shown, so a wide room may show them all and scroll
/// nothing. `paged` shows one at a time with the system's marks for which.
///
/// A carousel does not stand inside another one, and what it holds is also
/// reachable without the gesture: a client lets a reader of assistive
/// technology step through its parts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carousel {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The least a part may be wide, in points. Left out, each part has its
    /// own width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u16>,
    /// The room between parts, in steps of the system's spacing scale.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gap: u8,
    /// Whether one part shows at a time, each the width of the carousel.
    #[serde(default, skip_serializing_if = "is_false")]
    pub paged: bool,
    /// What the run is, for a reader who can't see it: `Sales charts`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

/// The parts for a compact room and the parts for a wide one. The client
/// draws one of the two, by the room the screen has where it is drawn, and
/// changes over when the room does (a phone turned on its side, a window
/// made wider) with no request.
///
/// A room is wide where its system says so: a regular width on Apple systems,
/// 600 points and more on Android. A client that can't tell draws `compact`.
///
/// A part with an id, a form and a field stand in one of the two only: an
/// answer that names an id finds one node. What is the same in both rooms
/// stands outside, and a [`Stack`](crate::node::Stack) that only turns says so
/// itself with `wide`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adaptive {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The parts for a compact room.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compact: Vec<Node>,
    /// The parts for a wide room.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wide: Vec<Node>,
}
