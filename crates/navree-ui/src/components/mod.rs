//! The components. Each is a plain struct with builder methods that turns into
//! a [`Node`](crate::Node). Variants are Rust enums, so a typo can't compile.
//! Components carry no outer margin: layout belongs to [`Stack`], [`Row`] and
//! [`Grid`].
//!
//! Provenance: the anatomy, slot names and variant names follow shadcn/ui
//! (MIT, Copyright (c) 2023 shadcn). See THIRD_PARTY.md.

mod actions;
mod display;
mod inputs;
mod layout;
mod overlays;

pub use actions::{Button, ButtonGroup, ButtonKind, ButtonVariant, Toggle};
pub use display::{
    Alert, AlertTone, AspectRatio, Attachment, Avatar, Badge, BadgeVariant, Bars, Breadcrumb, Bubble, BubbleSide, Card, Direction, Empty,
    Heading, Item, ItemVariant, Kbd, Marker, MarkerVariant, Message, Pagination, Progress, Ratio, Separator, Skeleton, SkeletonShape,
    Spinner, Stat, Table, TableCell, TableHead, TableRowEl, Text, TextVariant,
};
pub use inputs::{
    Checkbox, Field, FieldSet, Input, InputGroup, InputKind, Label, NativeSelect, OptionItem, RadioGroup, Switch, Textarea,
};
pub use layout::{Align, Container, ContainerSize, Grid, Justify, Row, Stack};
pub use overlays::{
    Accordion, AlertDialog, Collapsible, Dialog, Drawer, HoverCard, Popover, PopupAlign, ScrollArea, Sheet, SheetSide, Side, Tooltip,
};

use crate::icons::Icon;
use crate::node::{Node, el};

/// A control's size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Size {
    /// 24px.
    Xs,
    /// 28px.
    Sm,
    /// 32px: the default.
    #[default]
    Md,
    /// 36px.
    Lg,
}

impl Size {
    pub(crate) const fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Xs => Some("xs"),
            Self::Sm => Some("sm"),
            Self::Md => None,
            Self::Lg => Some("lg"),
        }
    }
}

/// An icon as a node, hidden from assistive technology: what it means is in
/// the text beside it.
#[inline]
pub fn icon(icon: Icon) -> Node {
    Node::Element(el("svg").slot("icon").attr("viewBox", "0 0 24 24").attr("aria-hidden", "true").child(Node::Static(icon.0)))
}

/// Adds `id`, `attr`, `flag` and `span` to a component whose `el` field is its
/// root element.
macro_rules! root_methods {
    ($($type:ident),+ $(,)?) => {
        $(impl $type {
            /// Sets the root element's `id`.
            #[must_use]
            pub fn id(mut self, id: impl Into<$crate::node::AttrValue>) -> Self {
                self.el = self.el.id(id);
                self
            }

            /// Sets an attribute on the root element.
            #[must_use]
            pub fn attr(mut self, name: &'static str, value: impl Into<$crate::node::AttrValue>) -> Self {
                self.el = self.el.attr(name, value);
                self
            }

            /// Takes `columns` columns of a [`Grid`](crate::Grid).
            #[must_use]
            pub fn span(mut self, columns: u8) -> Self {
                self.el = self.el.attr("data-span", $crate::components::layout::span_name(columns));
                self
            }
        })+
    };
}
pub(crate) use root_methods;
