//! Flow: the parts of a task that goes in order or over a range, a hierarchy or a
//! row of tools.
//!
//! - [`Stepper`]: a list of steps with their states, horizontal or vertical. It
//!   stands on its own, or inside a [`Wizard`].
//! - [`Wizard`]: a form in steps. Each step is a `#[derive(Form)]` struct that the
//!   server checks before the next opens, the progress is a [`Draft`] kept on
//!   the server (it belongs to a person, not to a tab), and the last step checks
//!   every step again. Every move is a plain post, so it all works with
//!   JavaScript off.
//! - [`Slider`]: one value or a range, on native range inputs. The server holds
//!   a posted value to the same grid the browser draws ([`SliderSpec`]).
//! - [`Tree`]: a tree of links or choices, with children loaded from the server.
//! - [`Toolbar`]: a row of tools with one tab stop, that moves what does not
//!   fit into a menu.
//!
//! Each of these is markup that works with no script. The slider, tree and toolbar
//! modules add the patterns of WAI-ARIA's Authoring Practices when they run.

mod draft;
mod number;
mod slider;
mod stepper;
mod wizard;

pub use draft::{
    Draft, DraftError, DraftStore, MAX_DRAFT_BYTES, MemoryDrafts, StepRecord, is_draft_id, is_step_id,
};
pub use number::{Decimal, DecimalError};
pub use slider::{Slider, SliderError, SliderLabels, SliderSpec};
pub use stepper::{Step, StepState, Stepper, StepperLabels};
pub use wizard::{
    Wizard, WizardAction, WizardError, WizardLabels, WizardOutcome, WizardPage, WizardRefusal,
    WizardStep, WizardStepOf,
};
