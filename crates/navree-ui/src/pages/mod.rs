//! What the app shows when a request ends in an error, and when a place has nothing to
//! show.
//!
//! - [`ErrorPage`]: a whole page for a status, drawn by [`UiModule`](crate::UiModule) for
//!   every page route that ends in a 401, 403, 404, 405, 409, 410, 419, 422, 429, 500 or
//!   503 (and any other error status), inside the shell when the user can see it.
//!   An API route keeps its JSON error.
//! - [`State`]: the block under all of them, for a card, a tab, a region or a table row.
//!   [`Forbidden`], [`NotFound`], [`SignInRequired`] and [`EmptyState`] are it with the
//!   words and actions of each case.
//! - [`ReturnTo`]: the page to come back to after signing in, checked, so a `next` from
//!   a request can't be an open redirect.
//! - [`StateLabels`] holds every word, and [`PagesOptions`] the settings.

mod labels;
mod options;
mod render;
pub mod return_to;
mod state;

pub use labels::{KindWords, StateKind, StateLabels};
pub use options::{PagesOptions, UiOptions};
pub(crate) use render::Renderer;
pub(crate) use render::shell_of;
pub use render::{ErrorShell, FROM_HEADER, REGION_HEADER, STATE_HEADER, region_asked};
pub use return_to::ReturnTo;
pub use state::{
    EmptyKind, EmptyState, ErrorPage, Forbidden, NotFound, SessionEnded, SignInRequired, State,
    StateSize,
};
