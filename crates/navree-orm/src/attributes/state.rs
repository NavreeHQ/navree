//! What a record carries for the attribute layer: its own visibility and
//! appends (when they differ from the model's), and the encrypted columns
//! it read under a previous key.

use std::borrow::Cow;

/// A list of names (hidden, visible, appended).
pub(crate) type Names = Vec<Cow<'static, str>>;

/// The attribute state of one record.
#[derive(Clone, Debug, Default)]
pub(crate) struct RecordState {
    /// The hidden names, when the record changed them.
    pub(crate) hidden: Option<Names>,
    /// The visible names, when the record changed them.
    pub(crate) visible: Option<Names>,
    /// The appended accessors, when the record changed them.
    pub(crate) appends: Option<Names>,
    /// Columns whose stored values must be rewritten even though the
    /// fields did not change (encrypted with a previous key).
    pub(crate) stale: Vec<&'static str>,
}
