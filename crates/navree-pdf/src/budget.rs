//! [`ReadLimits`], and the budget a document is read within.

use std::cell::Cell;

use crate::error::{PdfError, size};

/// What reading one document may take. Every figure is per document.
///
/// Objects are read when they are first asked for, and kept: the memory
/// they hold, the streams that are inflated to find them, and the bytes the
/// reader scans are counted as they are read, before they are kept. A file
/// whose objects would take more is refused with [`PdfError::TooLarge`],
/// however small the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadLimits {
    /// Memory the objects read may hold (256 MiB), counted as the reader
    /// builds them: each object, string, name and dictionary entry, each
    /// object stream and cross-reference stream inflated, and each entry
    /// of the cross-reference. The bytes of a stream are the file's own and
    /// are not counted again.
    pub memory: usize,
    /// Bytes the reader may scan (1 GiB), a byte counting again each time
    /// it is read: objects parsed, streams inflated, a damaged file scanned
    /// for its objects.
    pub work: usize,
    /// What one object stream or cross-reference stream may inflate to (8
    /// MiB). Writers put 100 to 200 objects in an object stream, tens of
    /// kilobytes; 8 MiB holds tens of thousands. A stream over the limit is
    /// read as a damaged one: the objects in it are missing.
    pub object_stream: usize,
    /// Cross-reference sections followed back through `/Prev` (256): one
    /// for each incremental update. A section seen before ends the chain.
    pub sections: usize,
}

impl Default for ReadLimits {
    fn default() -> Self {
        Self {
            memory: 256 * 1024 * 1024,
            work: 1024 * 1024 * 1024,
            object_stream: 8 * 1024 * 1024,
            sections: 256,
        }
    }
}

/// What reading a document has spent of its [`ReadLimits`].
#[derive(Debug)]
pub(crate) struct Budget {
    pub(crate) limits: ReadLimits,
    memory: Cell<usize>,
    work: Cell<usize>,
}

impl Budget {
    pub(crate) fn new(limits: ReadLimits) -> Budget {
        Budget {
            limits,
            memory: Cell::new(0),
            work: Cell::new(0),
        }
    }

    /// Counts `bytes` of memory, or fails when the document has used all it
    /// may hold.
    pub(crate) fn memory(&self, bytes: usize) -> Result<(), PdfError> {
        let used = self.memory.get().saturating_add(bytes);
        self.memory.set(used);
        if used > self.limits.memory {
            return Err(PdfError::TooLarge(format!(
                "a PDF holds more objects than the limit of {} of memory for a document",
                size(self.limits.memory)
            )));
        }
        Ok(())
    }

    /// Counts `bytes` scanned, or fails when the document has used all the
    /// work it may.
    pub(crate) fn work(&self, bytes: usize) -> Result<(), PdfError> {
        let used = self.work.get().saturating_add(bytes);
        self.work.set(used);
        if used > self.limits.work {
            return Err(PdfError::TooLarge(format!(
                "a PDF takes more work to read than the limit of {} of bytes scanned for a document",
                size(self.limits.work)
            )));
        }
        Ok(())
    }

    /// The error for memory that ran out: counts all of it as used.
    pub(crate) fn out_of_memory(&self) -> PdfError {
        self.memory(usize::MAX)
            .err()
            .unwrap_or_else(|| PdfError::TooLarge(String::new()))
    }

    /// The memory counted so far.
    pub(crate) fn memory_used(&self) -> usize {
        self.memory.get()
    }

    /// The memory still allowed.
    pub(crate) fn memory_left(&self) -> usize {
        self.limits.memory.saturating_sub(self.memory.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn going_over_a_limit_names_it() {
        let budget = Budget::new(ReadLimits {
            memory: 1024,
            work: 2048,
            ..ReadLimits::default()
        });
        budget.memory(1000).unwrap();
        assert_eq!(budget.memory_left(), 24);
        let error = budget.memory(25).unwrap_err();
        assert!(error.to_string().contains("1 KiB"), "{error}");
        budget.work(2048).unwrap();
        let error = budget.work(1).unwrap_err();
        assert!(error.to_string().contains("2 KiB"), "{error}");
    }
}
