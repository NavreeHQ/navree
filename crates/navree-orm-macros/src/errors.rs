//! Collecting several errors, so one expansion reports all of them.

/// Errors collected while parsing; reported together.
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

    /// `Err` with every recorded error, or `Ok` when there were none.
    pub(crate) fn finish(self) -> syn::Result<()> {
        self.0.map_or(Ok(()), Err)
    }

    /// Every recorded error followed by `error` (for a problem that stops
    /// the expansion).
    pub(crate) fn with(self, error: syn::Error) -> syn::Error {
        match self.0 {
            Some(mut all) => {
                all.combine(error);
                all
            }
            None => error,
        }
    }
}

/// A path written as an attribute key (`table`, `a::b`), for messages.
pub(crate) fn key_name(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// The closest of `known` to `key` (a small edit distance), for "did you
/// mean" hints.
pub(crate) fn suggestion<'a>(key: &str, known: &[&'a str]) -> Option<&'a str> {
    known
        .iter()
        .map(|candidate| (distance(key, candidate), *candidate))
        .filter(|(distance, _)| *distance <= 2)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
}

/// The Levenshtein distance between `a` and `b`.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut current = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != *cb);
            current.push(substitution.min(previous[j + 1] + 1).min(current[j] + 1));
        }
        previous = current;
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_close_keys_only() {
        let known = ["table", "timestamps", "soft_deletes"];
        assert_eq!(suggestion("tabel", &known), Some("table"));
        assert_eq!(suggestion("timestamp", &known), Some("timestamps"));
        assert_eq!(suggestion("relations", &known), None);
    }
}
