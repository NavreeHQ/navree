//! [`PdfError`].

/// Why a PDF cannot be read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PdfError {
    /// The bytes are not a PDF the reader can make sense of: not a PDF at
    /// all, cut short, or damaged past repair. The message says what is
    /// wrong.
    #[error("{0}")]
    Damaged(String),
    /// Reading would go over one of the limits it is held to (see
    /// [`ReadLimits`](crate::ReadLimits) and
    /// [`TextLimits`](crate::TextLimits)). The message names the limit.
    #[error("{0}")]
    TooLarge(String),
    /// The document is encrypted with a password to open it, or with a
    /// security handler other than the standard one.
    #[error("the document is encrypted: it needs a password to open")]
    Encrypted,
}

impl PdfError {
    pub(crate) fn damaged(message: impl Into<String>) -> PdfError {
        PdfError::Damaged(message.into())
    }
}

/// `bytes` in the largest unit that divides it: `128 MiB`, `8 KiB`,
/// `61445 bytes`.
pub(crate) fn size(bytes: usize) -> String {
    for (unit, name) in [(1 << 30, "GiB"), (1 << 20, "MiB"), (1 << 10, "KiB")] {
        if bytes >= unit && bytes.is_multiple_of(unit) {
            return format!("{} {name}", bytes / unit);
        }
    }
    format!("{bytes} bytes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_the_largest_unit_that_divides_them() {
        assert_eq!(size(128 * 1024 * 1024), "128 MiB");
        assert_eq!(size(8 * 1024), "8 KiB");
        assert_eq!(size(1 << 30), "1 GiB");
        assert_eq!(size(61_445), "61445 bytes");
        assert_eq!(size(10), "10 bytes");
    }
}
