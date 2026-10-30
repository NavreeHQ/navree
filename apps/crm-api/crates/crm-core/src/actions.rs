//! The CRM's custom actions, besides navree-access's `Read`, `Create`,
//! `Update`, `Delete` and `Manage`: use the markers in route attributes
//! (`#[can(Move, Deal, from = "id")]`) and extractors
//! (`Authorized<Deal, Move>`), and the names in rules.

use navree::access::ActionMarker;

/// `move`: change a deal's stage or position on the board.
pub const MOVE: &str = "move";
/// `export`: download contacts as CSV.
pub const EXPORT: &str = "export";
/// `complete`: mark an activity (a task) done.
pub const COMPLETE: &str = "complete";

/// The [`MOVE`] action.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Move;

impl ActionMarker for Move {
    const NAME: &'static str = MOVE;
}

/// The [`EXPORT`] action.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Export;

impl ActionMarker for Export {
    const NAME: &'static str = EXPORT;
}

/// The [`COMPLETE`] action.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Complete;

impl ActionMarker for Complete {
    const NAME: &'static str = COMPLETE;
}
