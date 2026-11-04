//! The components. Each is a plain struct with builder methods that turns into
//! a [`Node`](crate::Node). Variants are Rust enums, so a typo can't compile.
//! Components carry no outer margin: layout belongs to [`Stack`], [`Row`] and
//! [`Grid`].
//!
//! Provenance: the anatomy, slot names and variant names follow shadcn/ui
//! (MIT, Copyright (c) 2023 shadcn). See THIRD_PARTY.md.

mod actions;
mod activity;
mod clock;
mod display;
mod dropdown;
mod fact;
mod flag;
mod inputs;
mod layout;
mod lazy;
mod overlays;
mod split;
mod tabs;
mod timeline;
mod toast;
mod toggle_group;
mod tone;

pub use actions::{Button, ButtonGroup, ButtonKind, ButtonVariant, Toggle};
pub use activity::{ActivityFeed, ActivityItem, FeedHeight};
pub use display::{
    Alert, AlertTone, AspectRatio, Attachment, Avatar, Badge, BadgeVariant, Bars, Breadcrumb,
    Bubble, BubbleSide, Card, Direction, Empty, Heading, Item, ItemVariant, Kbd, Marker,
    MarkerVariant, Message, Pagination, Progress, Ratio, Separator, Skeleton, SkeletonShape,
    Spinner, Stat, Table, TableCell, TableHead, TableRowEl, Text, TextVariant,
};
pub use dropdown::{DropdownMenu, MenuCheckbox, MenuGroup, MenuItem, MenuRadios, MenuSub};
pub use fact::{FactCard, FactGrid};
pub use flag::{Flag, FlagShape, FlagSize};
pub use inputs::{
    Checkbox, Field, FieldSet, Input, InputGroup, InputKind, Label, NativeSelect, OptionItem,
    RadioGroup, Switch, Textarea,
};
pub use layout::{
    Align, Container, ContainerSize, Grid, Justify, Row, Stack, only_with_script,
    only_without_script,
};
pub use lazy::{
    LazyAsk, LazyEnd, LazyItem, LazyLabels, LazyList, LazyOrder, LazyPage, LazyQuery, MAX_CURSOR,
};
pub use overlays::{
    Accordion, AlertDialog, Collapsible, Dialog, Drawer, HoverCard, Popover, PopupAlign,
    ScrollArea, ScrollHeight, Sheet, SheetBackdrop, SheetSide, SheetWidth, Side, Tooltip,
};
pub use split::{Pane, SplitHeight, SplitPane, SplitSave, SplitSizes, SplitWidth};
pub use tabs::{Activation, Orientation, Tab, Tabs, TabsVariant};
pub use timeline::{Timeline, TimelineEvent, TimelineState};
pub use toast::{Toast, ToastAction, ToastDuration, ToastPosition, Toaster};
pub use toggle_group::{ToggleGroup, ToggleItem, ToggleVariant};
pub use tone::{DisplayLabels, Meta, Tone};

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
/// the text beside it. When an icon stands alone, use [`icon_labelled`].
#[inline]
pub fn icon(icon: Icon) -> Node {
    Node::Element(
        el("svg")
            .slot("icon")
            .attr("viewBox", icon.view_box())
            .attr("aria-hidden", "true")
            .child(Node::Static(icon.shapes())),
    )
}

/// An icon that stands alone and means something, such as a status mark in a
/// table cell: an image with `label` as its accessible name.
pub fn icon_labelled(icon: Icon, label: impl Into<std::borrow::Cow<'static, str>>) -> Node {
    Node::Element(
        el("svg")
            .slot("icon")
            .attr("viewBox", icon.view_box())
            .attr("role", "img")
            .attr("aria-label", label.into())
            .child(Node::Static(icon.shapes())),
    )
}

/// Adds `id`, `attr`, `on` and `span` to a component whose `el` field is its root
/// element.
macro_rules! root_methods {
    ($($type:ident),+ $(,)?) => {
        $(impl $type {
            /// Sets the root element's `id`.
            pub fn id(mut self, id: impl Into<$crate::node::AttrValue>) -> Self {
                self.el = self.el.id(id);
                self
            }

            /// Sets an attribute on the root element.
            pub fn attr(mut self, name: &'static str, value: impl Into<$crate::node::AttrValue>) -> Self {
                self.el = self.el.attr(name, value);
                self
            }

            /// Runs `commands` in the browser when `trigger` happens on the root element,
            /// with no request: a counter as a field is typed in, a label as a choice is made.
            pub fn on(mut self, trigger: $crate::Trigger, commands: impl Into<$crate::Commands>) -> Self {
                self.el = self.el.on(trigger, commands);
                self
            }

            /// Makes `key` a shortcut that clicks this, or focuses it if it is a field:
            /// see [`Element::hotkey`](crate::Element::hotkey).
            pub fn hotkey(mut self, key: &str) -> Self {
                self.el = self.el.hotkey(key);
                self
            }

            /// Takes `columns` columns of a [`Grid`](crate::Grid).
            pub fn span(mut self, columns: u8) -> Self {
                self.el = self.el.attr("data-span", $crate::components::layout::span_name(columns));
                self
            }
        })+
    };
}
pub(crate) use root_methods;
