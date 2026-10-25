//! Field redaction: any field whose name contains a redact key
//! (ASCII case-insensitively) is written as `[redacted]`.

use std::error::Error;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use tracing::field::{Field, Visit};
use tracing_subscriber::field::{MakeVisitor, VisitFmt, VisitOutput};
use tracing_subscriber::fmt::format::Writer;

/// What a redacted value is written as.
pub(crate) const REDACTED: &str = "[redacted]";

/// Writes [`REDACTED`] without quotes in both `Debug` and `Display`.
pub(crate) struct Redacted;

impl fmt::Debug for Redacted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// Decides which field names are redacted.
#[derive(Clone, Debug, Default)]
pub(crate) struct Redactor {
    /// Lower-cased, non-empty key fragments.
    keys: Arc<[String]>,
}

impl Redactor {
    /// A redactor for `keys`; blank keys are ignored (they would match every field).
    pub(crate) fn new<S: AsRef<str>>(keys: &[S]) -> Self {
        Self {
            keys: keys
                .iter()
                .map(|key| key.as_ref().trim().to_ascii_lowercase())
                .filter(|key| !key.is_empty())
                .collect(),
        }
    }

    /// Whether the value of field `name` must be redacted.
    pub(crate) fn is_redacted(&self, name: &str) -> bool {
        self.keys
            .iter()
            .any(|key| contains_ignore_ascii_case(name, key))
    }
}

/// Whether `haystack` contains `needle` (already lower-case), ignoring ASCII case.
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let (haystack, needle) = (haystack.as_bytes(), needle.as_bytes());
    needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
}

/// A field formatter (`FormatFields`) that wraps another one and redacts
/// matching fields before they reach it.
///
/// Formatting layers cache formatted span fields per field-formatter type,
/// so layers that format differently (coloured stdout, plain file) must use
/// different `Output` markers, or one would print the other's colour codes.
pub(crate) struct RedactingFields<M, Output = ()> {
    inner: M,
    redactor: Redactor,
    output: PhantomData<fn(Output)>,
}

impl<M, Output> RedactingFields<M, Output> {
    pub(crate) fn new(inner: M, redactor: Redactor) -> Self {
        Self {
            inner,
            redactor,
            output: PhantomData,
        }
    }
}

impl<M: fmt::Debug, Output> fmt::Debug for RedactingFields<M, Output> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedactingFields")
            .field("inner", &self.inner)
            .field("redactor", &self.redactor)
            .finish()
    }
}

impl<'a, M, Output> MakeVisitor<Writer<'a>> for RedactingFields<M, Output>
where
    M: MakeVisitor<Writer<'a>>,
{
    type Visitor = RedactingVisitor<M::Visitor>;

    fn make_visitor(&self, target: Writer<'a>) -> Self::Visitor {
        RedactingVisitor {
            inner: self.inner.make_visitor(target),
            redactor: self.redactor.clone(),
        }
    }
}

/// The visitor behind [`RedactingFields`].
pub(crate) struct RedactingVisitor<V> {
    inner: V,
    redactor: Redactor,
}

impl<V: Visit> RedactingVisitor<V> {
    /// Records `[redacted]` and returns true when `field` must be redacted.
    fn redacted(&mut self, field: &Field) -> bool {
        let redacted = self.redactor.is_redacted(field.name());
        if redacted {
            self.inner.record_debug(field, &Redacted);
        }
        redacted
    }
}

impl<V: Visit> Visit for RedactingVisitor<V> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        if !self.redacted(field) {
            self.inner.record_f64(field, value);
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        if !self.redacted(field) {
            self.inner.record_i64(field, value);
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if !self.redacted(field) {
            self.inner.record_u64(field, value);
        }
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        if !self.redacted(field) {
            self.inner.record_i128(field, value);
        }
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        if !self.redacted(field) {
            self.inner.record_u128(field, value);
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        if !self.redacted(field) {
            self.inner.record_bool(field, value);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if !self.redacted(field) {
            self.inner.record_str(field, value);
        }
    }

    fn record_bytes(&mut self, field: &Field, value: &[u8]) {
        if !self.redacted(field) {
            self.inner.record_bytes(field, value);
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        if !self.redacted(field) {
            self.inner.record_error(field, value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if !self.redacted(field) {
            self.inner.record_debug(field, value);
        }
    }
}

impl<V: VisitOutput<fmt::Result>> VisitOutput<fmt::Result> for RedactingVisitor<V> {
    fn finish(self) -> fmt::Result {
        self.inner.finish()
    }
}

impl<V: VisitFmt> VisitFmt for RedactingVisitor<V> {
    fn writer(&mut self) -> &mut dyn fmt::Write {
        self.inner.writer()
    }
}
