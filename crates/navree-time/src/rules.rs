//! Validation rules for zone names kept as strings, for
//! `#[validate(custom(navree_time::rules::time_zone))]` on a `String` or
//! `Option<String>` field.
//!
//! ```
//! use navree_time::rules::time_zone;
//!
//! assert!(time_zone(&Some("Asia/Calcutta".to_owned())).is_ok());
//! assert!(time_zone(&None::<String>).is_ok());
//! assert!(time_zone("Mars/Base").is_err());
//! ```
//!
//! A field of type [`Zone`](crate::Zone) needs no rule: it deserializes
//! only from a zone name (and normalizes it).

use std::borrow::Cow;

use crate::Zone;

/// A value the rules accept: text, or optional text (`None` passes).
pub trait ZoneInput {
    /// The text to check, if any.
    fn zone_text(&self) -> Option<&str>;
}

impl ZoneInput for str {
    fn zone_text(&self) -> Option<&str> {
        Some(self)
    }
}

impl ZoneInput for String {
    fn zone_text(&self) -> Option<&str> {
        Some(self)
    }
}

impl ZoneInput for Cow<'_, str> {
    fn zone_text(&self) -> Option<&str> {
        Some(self)
    }
}

impl<T: ZoneInput + ?Sized> ZoneInput for &T {
    fn zone_text(&self) -> Option<&str> {
        (**self).zone_text()
    }
}

impl<T: ZoneInput> ZoneInput for Option<T> {
    fn zone_text(&self) -> Option<&str> {
        self.as_ref().and_then(ZoneInput::zone_text)
    }
}

/// The value is an IANA time zone name (legacy aliases included;
/// normalize with [`Zone::parse`] before storing it). It fails with the
/// message `:attribute must be a valid IANA time zone name`, which
/// navree-validation renders with the field's name.
pub fn time_zone<T: ZoneInput + ?Sized>(value: &T) -> Result<(), &'static str> {
    match value.zone_text() {
        Some(text) if Zone::parse(text).is_err() => {
            Err(":attribute must be a valid IANA time zone name")
        }
        _ => Ok(()),
    }
}
