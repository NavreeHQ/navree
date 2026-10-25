//! [`Secret`]: a value that never shows up in logs.

use std::fmt;

use serde::Deserialize;

/// Wraps a sensitive value (password, API key, token). `Debug` and `Display`
/// print `[redacted]`; there is deliberately no `Serialize` impl, so a secret
/// cannot leak into JSON responses or logs by accident. Deserializes as the
/// inner value, so config structs can hold `Secret<String>` fields.
#[derive(Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(transparent)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    /// Wraps `value`.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// The secret value. Call this only where the value is actually used.
    pub fn expose(&self) -> &T {
        &self.0
    }

    /// Unwraps the secret value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for Secret<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}
