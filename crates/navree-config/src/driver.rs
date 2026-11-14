//! Drivers a module registers under a name: [`DriverName`] and the set of
//! names the process knows.
//!
//! A driver name is a lower-case letter followed by up to 63 lower-case
//! letters, digits or `_` (`two_tier`, `sanduk`). Names live for the whole
//! process, each stored once, so a [`DriverName`] is a `Copy` handle that a
//! selector such as `CacheStoreKind` can hold without losing `Copy`.
//!
//! Only two things add a name to the set: a registration (and the built-in
//! names of a driver kind), with [`DriverName::register`], and a value read
//! from the app's configuration, with [`DriverName::new`]. A name read from
//! anywhere else (a request, a stored row, a job payload) goes through
//! [`DriverName::lookup`], which never adds one, so made-up names cannot
//! grow the set.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The most distinct driver names one process holds.
const LIMIT: usize = 1024;

/// The names of the process.
///
/// Names are never removed: registered and built-in names are in the set
/// before any configuration is read, so nothing read later can push them
/// out.
static NAMES: OnceLock<Mutex<Names>> = OnceLock::new();

/// The process's set of names, locked. A lock poisoned by a panic elsewhere
/// is taken over: the set is never left half-changed.
fn names() -> MutexGuard<'static, Names> {
    NAMES
        .get_or_init(|| Mutex::new(Names::with_limit(LIMIT)))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// A set of driver names, each stored once, holding at most `limit`.
struct Names {
    set: BTreeSet<&'static str>,
    limit: usize,
}

impl Names {
    fn with_limit(limit: usize) -> Names {
        Names {
            set: BTreeSet::new(),
            limit,
        }
    }

    /// Stores `name` itself (no copy); the stored name when it is there.
    fn register(&mut self, name: &'static str) -> Result<&'static str, DriverNameError> {
        if !DriverName::is_valid(name) {
            return Err(DriverNameError::Invalid);
        }
        if let Some(stored) = self.set.get(name).copied() {
            return Ok(stored);
        }
        if self.set.len() >= self.limit {
            return Err(DriverNameError::TooMany);
        }
        self.set.insert(name);
        Ok(name)
    }

    /// The stored name equal to `name`, or a copy of it, stored once.
    // Interns a name: a method of the set, not a constructor.
    #[allow(clippy::new_ret_no_self)]
    fn new(&mut self, name: &str) -> Result<&'static str, DriverNameError> {
        if !DriverName::is_valid(name) {
            return Err(DriverNameError::Invalid);
        }
        if let Some(stored) = self.set.get(name).copied() {
            return Ok(stored);
        }
        if self.set.len() >= self.limit {
            return Err(DriverNameError::TooMany);
        }
        let stored: &'static str = Box::leak(name.to_owned().into_boxed_str());
        self.set.insert(stored);
        Ok(stored)
    }

    /// The stored name equal to `name`; never stores one.
    fn lookup(&self, name: &str) -> Option<&'static str> {
        self.set.get(name).copied()
    }
}

/// The name of a driver: a lower-case letter followed by up to 63 lower-case
/// letters, digits or `_`. A `Copy` handle to a name the process stores
/// once; two names are equal when their text is.
///
/// A name written in code is [`from_static`](DriverName::from_static); a
/// name read from the app's configuration is [`new`](DriverName::new); any
/// other name (from a request, a stored row, a job payload) is
/// [`lookup`](DriverName::lookup), which only finds names the process
/// already knows.
///
/// ```
/// use navree_config::DriverName;
///
/// let name = DriverName::from_static("two_tier");
/// assert_eq!(name.as_str(), "two_tier");
/// assert_eq!(name.to_string(), "two_tier");
/// assert!(!DriverName::is_valid("Two-Tier"));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DriverName(&'static str);

impl DriverName {
    /// A name written in code. It does not touch the process's set of names.
    ///
    /// A name that is not a lower-case letter followed by up to 63
    /// lower-case letters, digits or `_` panics with `driver names are a
    /// lower-case letter followed by up to 63 lower-case letters, digits or
    /// _`; in a `const`, that fails the build.
    ///
    /// ```
    /// use navree_config::DriverName;
    ///
    /// const TWO_TIER: DriverName = DriverName::from_static("two_tier");
    /// assert_eq!(TWO_TIER.as_str(), "two_tier");
    /// ```
    ///
    /// ```compile_fail
    /// // a_bad_static_name_fails_in_a_const
    /// const BAD: navree_config::DriverName = navree_config::DriverName::from_static("Two-Tier");
    /// let _ = BAD;
    /// ```
    pub const fn from_static(name: &'static str) -> DriverName {
        if !DriverName::is_valid(name) {
            panic!(
                "driver names are a lower-case letter followed by up to 63 lower-case letters, digits or _"
            );
        }
        DriverName(name)
    }

    /// Puts a name written in code into the process's set, without copying
    /// it: what a registration method calls for the name it registers, and
    /// a driver kind for its built-in names, while modules register.
    pub fn register(name: &'static str) -> Result<DriverName, DriverNameError> {
        names().register(name).map(DriverName)
    }

    /// A name read from the app's configuration: checked, then stored once
    /// (the first time, as a copy). Only configuration values come through
    /// here.
    pub fn new(name: &str) -> Result<DriverName, DriverNameError> {
        names().new(name).map(DriverName)
    }

    /// The name equal to `name` that the process already holds, if any. It
    /// never adds a name: every name read from a request, a stored row or a
    /// job payload goes through here.
    pub fn lookup(name: &str) -> Option<DriverName> {
        names().lookup(name).map(DriverName)
    }

    /// Whether `name` is a lower-case letter followed by up to 63 lower-case
    /// letters, digits or `_`.
    pub const fn is_valid(name: &str) -> bool {
        let bytes = name.as_bytes();
        if bytes.is_empty() || bytes.len() > 64 || !bytes[0].is_ascii_lowercase() {
            return false;
        }
        let mut i = 1;
        while i < bytes.len() {
            let byte = bytes[i];
            if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') {
                return false;
            }
            i += 1;
        }
        true
    }

    /// The name.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for DriverName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// The name as a quoted string: `"two_tier"`.
impl fmt::Debug for DriverName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0, f)
    }
}

/// The name, as a string.
impl Serialize for DriverName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

/// A string naming a driver the process already knows, through
/// [`DriverName::lookup`]: reading one never adds a name, and the error for
/// an unknown or malformed one never repeats the value.
impl<'de> Deserialize<'de> for DriverName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        DriverName::lookup(&name)
            .ok_or_else(|| de::Error::custom("not the name of a registered driver"))
    }
}

/// Why a driver name was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DriverNameError {
    /// The name is not a lower-case letter followed by up to 63 lower-case
    /// letters, digits or `_`.
    #[error(
        "a driver name is a lower-case letter followed by up to 63 lower-case letters, digits or _"
    )]
    Invalid,
    /// The process already holds 1024 distinct driver names.
    #[error("too many driver names in one process (at most 1024)")]
    TooMany,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interned_names_are_stored_once_and_compare_by_text() {
        let read = DriverName::new("amritsar_cache").unwrap();
        let again = DriverName::new("amritsar_cache").unwrap();
        assert!(std::ptr::eq(read.as_str(), again.as_str()), "one copy");
        assert_eq!(
            read,
            DriverName::from_static("amritsar_cache"),
            "equality is by text"
        );
        assert_eq!(read.to_string(), "amritsar_cache");
        assert_eq!(format!("{read:?}"), "\"amritsar_cache\"");
        assert_eq!(DriverName::new("Amritsar"), Err(DriverNameError::Invalid));
        assert_eq!(
            DriverName::new("amritsar-cache"),
            Err(DriverNameError::Invalid)
        );
        assert_eq!(
            DriverName::new(&"a".repeat(65)),
            Err(DriverNameError::Invalid)
        );
        assert!(DriverName::is_valid(&"a".repeat(64)));
    }

    #[test]
    fn lookup_never_inserts_and_register_stores_without_copying() {
        assert_eq!(DriverName::lookup("amritsar_queue"), None);
        assert_eq!(
            DriverName::lookup("amritsar_queue"),
            None,
            "a miss adds nothing"
        );
        static NAME: &str = "amritsar_queue";
        let registered = DriverName::register(NAME).unwrap();
        assert!(
            std::ptr::eq(registered.as_str(), NAME),
            "the given text, not a copy"
        );
        assert_eq!(DriverName::lookup("amritsar_queue"), Some(registered));
        assert_eq!(
            DriverName::register("Amritsar_Queue"),
            Err(DriverNameError::Invalid)
        );
    }

    #[test]
    fn the_1025th_distinct_name_is_refused() {
        // A set of its own, so the process's names are not used up.
        let mut names = Names::with_limit(LIMIT);
        for i in 0..LIMIT {
            names.new(&format!("ludhiana_{i}")).unwrap();
        }
        assert_eq!(names.new("ludhiana_extra"), Err(DriverNameError::TooMany));
        assert_eq!(
            names.new("ludhiana_7").map(str::len),
            Ok(10),
            "a known name is still found"
        );
        assert_eq!(LIMIT, 1024);
        assert_eq!(
            DriverNameError::TooMany.to_string(),
            "too many driver names in one process (at most 1024)"
        );
    }

    #[test]
    fn driver_names_deserialize_without_repeating_a_bad_value() {
        DriverName::register("amritsar_mail").unwrap();
        let name: DriverName = serde_json::from_value(serde_json::json!("amritsar_mail")).unwrap();
        assert_eq!(name.as_str(), "amritsar_mail");
        assert_eq!(
            serde_json::to_value(name).unwrap(),
            serde_json::json!("amritsar_mail")
        );
        for bad in ["amritsar_unknown_9f3", "Not Valid!"] {
            let error = serde_json::from_value::<DriverName>(serde_json::json!(bad))
                .unwrap_err()
                .to_string();
            assert!(error.contains("not the name of a registered driver"), "{error}");
            assert!(!error.contains(bad), "{error}");
        }
        assert_eq!(
            DriverName::lookup("amritsar_unknown_9f3"),
            None,
            "deserializing never inserts"
        );
    }
}
