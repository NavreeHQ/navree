//! # navree-ui
//!
//! Navree's UI toolkit: typed Rust components render to HTML on the server, a
//! small owned runtime handles behaviour, and a page with nothing interactive
//! ships no JavaScript at all.
//!
//! - [`Node`], [`el`]: the tree every component builds. [`render`] writes it
//!   in one pass to one `String`.
//! - [`Cmd`]: what an element does in the browser with no request. [`Op`] and
//!   [`Reply`]: what the server sends back from an action.
//! - [`View`]: a page, with its strict content security policy.
//! - [`assets`]: one stylesheet and the runtime's modules, embedded with
//!   brotli and gzip forms, at content-hash URLs.
//! - [`tokens`]: the design tokens' one source. [`theme`]: the built-in themes,
//!   themes of your own, light, dark and system, and the theme builder.
//! - [`form`]: `#[derive(Form)]`, [`FormView`], [`Submit`]. [`table`]:
//!   `#[derive(TableRow)]`, [`DataTable`], [`TableQuery`], [`Region`].
//! - [`sortable`]: [`Sortable`] lists and boards, and the [`Move`] each drop posts.
//! - [`TestView`]: drive a page in a plain test, with no browser.
//! - [`UiModule`]: serves the files and picks ops or a page for each reply.
//! - [`pages`]: what the app shows when a request ends in an error, or a place has
//!   nothing to show: [`ErrorPage`], [`EmptyState`], [`Forbidden`], [`NotFound`],
//!   [`SignInRequired`].
//!
//! The browser side lives in `runtime/`: plain ES modules with JSDoc types.
//! Its build output is committed, so `cargo build` never needs Node.
#![forbid(unsafe_code)]

/// What pages import: every component, the shell, forms, tables and the wire.
pub mod prelude {
    pub use crate::components::*;
    pub use crate::dates::{Calendar, DatePicker, DateRangePicker, TimePicker};
    pub use crate::flow::{
        Draft, DraftStore, MemoryDrafts, Slider, SliderError, SliderLabels, SliderSpec, Step,
        StepState, Stepper, StepperLabels, Toolbar, ToolbarItem, Tree, TreeChildren, TreeNode,
        TreeSelect, TreeSelection, TreeState, Wizard, WizardOutcome, WizardPage, WizardStep,
    };
    pub use crate::form::{
        Form as FormFields, FormView, Many, Money, Options as OptionList, Rejected, Submit,
    };
    pub use crate::icons;
    pub use crate::op::{Op, Reply, ToastTone, UrlMode};
    #[cfg(feature = "phone")]
    pub use crate::phone::{PhoneInput, PhoneLabels};
    pub use crate::select::{
        Choice, ChoiceList, Combobox, Command, CommandGroup, CommandItem, MultiSelect, OptionQuery,
        Select, SelectLabels,
    };
    pub use crate::sortable::{
        Landing, ListState, Move, Refusal, SortItem, SortLabels, SortLayout, SortList, Sortable,
    };
    pub use crate::table::{
        BulkAction, BulkRequest, DataTable, FilterOption, Region, RowAction, TableDensity,
        TableHeight, TableLabels, TablePage, TableQuery,
    };
    pub use crate::theme::{Appearance, Mode, Theme, ThemeMenu, ThemeSetting};
    pub use crate::{
        AppPage, AppShell, Cmd, Collapse, Element, FilterBar, Form, IntoNodes, NavGroup, NavItem,
        Node, Options, Shell, ShellLayout, TableRow, TestView, UiModule, UserMenu, Validate, View,
        el,
    };
}

pub mod assets;
#[rustfmt::skip]
mod assets_gen;
pub mod cmd;
pub mod components;
pub mod compress;
pub mod dates;
pub mod flags;
pub mod flow;
pub mod form;
pub mod html;
pub mod icons;
mod module;
pub mod node;
pub mod op;
pub mod pages;
#[cfg(feature = "phone")]
pub mod phone;
pub mod render;
pub mod select;
pub mod shell;
pub mod sortable;
pub mod table;
pub mod test_view;
pub mod theme;
pub mod tokens;
pub mod view;

pub use cmd::{Cmd, Commands, Trigger};
pub use components::*;
pub use flow::{
    Draft, DraftStore, MemoryDrafts, Slider, SliderError, SliderLabels, SliderSpec, Step,
    StepState, Stepper, StepperLabels, Toolbar, ToolbarItem, Tree, TreeChildren, TreeNode,
    TreeSelect, TreeSelection, TreeState, Wizard, WizardOutcome, WizardPage, WizardStep,
};
pub use form::{FormView, Money, Submit};
pub use html::{Href, HrefError, LocalHref, SafeHtml};
pub use module::{UiModule, UiNeed, UiOnDemand};
pub use navree_ui_macros::{FilterBar, Form, Options, TableRow, icon_file, icon_set};
pub use navree_validation::Validate;
pub use node::{Attr, AttrValue, Element, IntoNodes, Node, Uses, el};
pub use op::{Envelope, Op, Reply, ToastTone, UrlMode, is_prefetch};
pub use pages::{
    EmptyState, ErrorPage, ErrorShell, Forbidden, NotFound, PagesOptions, ReturnTo, SessionEnded,
    SignInRequired, State, StateKind, StateLabels, StateSize,
};
#[cfg(feature = "phone")]
pub use phone::{PhoneInput, PhoneLabels};
pub use render::{Measure, measure, render, render_into};
#[cfg(feature = "orm")]
pub use select::OptionSource;
pub use select::{
    Choice, ChoiceList, Combobox, Command, CommandGroup, CommandItem, MultiSelect, OptionQuery,
    Select, SelectLabels,
};
pub use shell::{AppPage, AppShell, Collapse, NavGroup, NavItem, Shell, ShellLayout, UserMenu};
pub use sortable::{
    Landing, ListState, Move, MoveError, MoveRejected, Placement, RankedLists, Refusal, SortItem,
    SortLabels, SortLayout, SortList, Sortable, Spot,
};
pub use table::{
    BulkAction, BulkRequest, DataTable, FilterOption, Region, RowAction, TableDensity, TableHeight,
    TableLabels, TablePage, TableQuery,
};
pub use test_view::TestView;
pub use view::{PAGE_CSP, View};
