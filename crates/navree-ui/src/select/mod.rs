//! Select, Combobox, MultiSelect and Command.
//!
//! - [`Select`]: one choice from a small, fixed list, in a styled listbox. A real
//!   `<select>` carries the value and shows when scripting is off.
//! - [`Combobox`]: one choice, found by typing. The options are in the page or are
//!   searched on the server (see [`OptionQuery`] and [`ChoiceList`]).
//! - [`MultiSelect`]: many choices, shown as chips.
//! - [`Command`]: a command palette in a modal dialog, opened by `Ctrl` or `Cmd` and `K`.
//!
//! All four are server-rendered markup that the `select` runtime module drives.
//! Focus stays on the control and the list is followed with `aria-activedescendant`,
//! as WAI-ARIA's combobox and listbox patterns say. The value always lives in a
//! native `<select>`, so forms, `required`, reset and `change` work as they do for
//! any select.

mod choice;
mod combobox;
mod command;
mod labels;
mod list;
mod multi;
mod picker;
mod query;
mod single;

#[cfg(feature = "orm")]
mod orm;

pub use choice::Choice;
pub use combobox::Combobox;
pub use command::{Command, CommandGroup, CommandItem};
pub use labels::SelectLabels;
pub use multi::MultiSelect;
pub use query::{ChoiceList, MAX_OFFSET, MAX_QUERY_TEXT, OptionQuery};
pub use single::Select;

#[cfg(feature = "orm")]
pub use orm::OptionSource;
