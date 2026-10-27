//! garde rules for zone names kept as strings.
//!
//! ```
//! use garde::Validate;
//!
//! #[derive(Validate)]
//! struct UpdateProfile {
//!     #[garde(custom(navree_time::rules::time_zone))]
//!     timezone: Option<String>,
//! }
//!
//! assert!(UpdateProfile { timezone: Some("Asia/Calcutta".into()) }.validate().is_ok());
//! assert!(UpdateProfile { timezone: None }.validate().is_ok());
//! assert!(UpdateProfile { timezone: Some("Mars/Base".into()) }.validate().is_err());
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

/// garde: the value is an IANA time zone name (legacy aliases included;
/// normalize with [`Zone::parse`] before storing it).
pub fn time_zone<T: ZoneInput + ?Sized, C: ?Sized>(value: &T, _ctx: &C) -> garde::Result {
    match value.zone_text() {
        Some(text) if Zone::parse(text).is_err() => Err(garde::Error::new(
            ":attribute must be a valid IANA time zone name",
        )),
        _ => Ok(()),
    }
}
