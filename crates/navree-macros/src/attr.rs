//! Helpers shared by the attribute parsers.

/// A path written as an attribute key (`scope`, `a::b`), for error messages.
pub(crate) fn key_name(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// Collects several errors so one expansion reports all of them at once.
#[derive(Default)]
pub(crate) struct Errors(Option<syn::Error>);

impl Errors {
    /// Returns the value, or records the error and returns `None`.
    pub(crate) fn keep<T>(&mut self, result: syn::Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.push(error);
                None
            }
        }
    }

    /// Records an error.
    pub(crate) fn push(&mut self, error: syn::Error) {
        match &mut self.0 {
            Some(all) => all.combine(error),
            None => self.0 = Some(error),
        }
    }

    /// Whether an error was recorded.
    pub(crate) fn has_errors(&self) -> bool {
        self.0.is_some()
    }

    /// `Err` with every recorded error, or `Ok` when there were none.
    pub(crate) fn finish(self) -> syn::Result<()> {
        self.0.map_or(Ok(()), Err)
    }
}
