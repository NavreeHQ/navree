//! [`Kwargs`]: the keyword arguments of a call.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use super::{Repr, Value, ValueMap};
use crate::error::{Error, ErrorKind};
use crate::functions::ArgType;

/// The keyword arguments a function, filter or test was called with
/// (`t("greeting", name=user.name)`). Take it as the last parameter of a
/// function to receive them; [`assert_all_used`](Kwargs::assert_all_used)
/// rejects names the function does not know.
pub struct Kwargs {
    values: Arc<ValueMap>,
    used: RefCell<BTreeSet<String>>,
}

impl Kwargs {
    pub(crate) fn new(values: Arc<ValueMap>) -> Kwargs {
        Kwargs {
            values,
            used: RefCell::new(BTreeSet::new()),
        }
    }

    /// The keyword arguments a value holds, if it is the keyword argument
    /// value of a call.
    pub(crate) fn extract(value: &Value) -> Option<Kwargs> {
        match &value.0 {
            Repr::Kwargs(values, _) => Some(Kwargs::new(values.clone())),
            _ => None,
        }
    }

    pub(crate) fn values(&self) -> &ValueMap {
        &self.values
    }

    /// Argument `key`, converted like a positional argument (an `Option<T>`
    /// is `None` when it is missing), and marked as used.
    pub fn get<'a, T>(&'a self, key: &'a str) -> Result<T, Error>
    where
        T: ArgType<'a, Output = T>,
    {
        let value = self.values.get(&Value::from(key));
        let converted = T::from_value(value, false).map_err(|error| {
            if value.is_none() && error.kind() == ErrorKind::MissingArgument {
                Error::new(
                    ErrorKind::MissingArgument,
                    format!("missing keyword argument `{key}`"),
                )
            } else {
                error
            }
        })?;
        self.used.borrow_mut().insert(key.to_owned());
        Ok(converted)
    }

    /// Whether argument `key` was given.
    pub fn has(&self, key: &str) -> bool {
        self.values.contains_key(&Value::from(key))
    }

    /// The names of the arguments, in order.
    pub fn args(&self) -> impl Iterator<Item = &str> {
        self.values.keys().filter_map(Value::as_str)
    }

    /// Fails when an argument was given that [`get`](Kwargs::get) never
    /// read.
    pub fn assert_all_used(&self) -> Result<(), Error> {
        let used = self.used.borrow();
        for name in self.args() {
            if !used.contains(name) {
                return Err(Error::new(
                    ErrorKind::TooManyArguments,
                    format!("unknown keyword argument `{name}`"),
                ));
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Kwargs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.values.iter()).finish()
    }
}
