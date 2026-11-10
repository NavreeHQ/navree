//! What a record carries for the attribute layer: its own visibility and
//! appends (when they differ from the model's), the encrypted columns it
//! read under a previous key, and the cast context it was read or saved
//! under.

use std::borrow::Cow;
use std::sync::Arc;

use crate::attributes::CastContext;

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
    /// The cast context the record was last read or saved under (for
    /// models whose casts use one), so dirty checks, fills and
    /// serialization outside any scope use the same services.
    pub(crate) context: Option<Arc<CastContext>>,
}
