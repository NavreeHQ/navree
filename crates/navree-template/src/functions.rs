//! Rust functions as template functions, filters and tests.
//!
//! [`Environment::add_function`](crate::Environment::add_function),
//! `add_filter` and `add_test` take plain closures; their parameters
//! convert from the template's arguments through [`ArgType`], and their
//! return value into a [`Value`] through [`FunctionResult`]:
//!
//! ```
//! use navree_template::{Environment, Error, ErrorKind, Kwargs};
//!
//! let mut env = Environment::new();
//! env.add_filter("shout", |text: &str| text.to_uppercase());
//! env.add_function("greet", |name: &str, kwargs: Kwargs| -> Result<String, Error> {
//!     let punctuation: Option<&str> = kwargs.get("end")?;
//!     kwargs.assert_all_used()?;
//!     Ok(format!("Hello, {name}{}", punctuation.unwrap_or(".")))
//! });
//! let out = env
//!     .render_str("hi.txt", "{{ greet(name|shout, end='!') }}", serde_json::json!({ "name": "anu" }))
//!     .unwrap();
//! assert_eq!(out, "Hello, ADA!");
//! ```

use std::fmt;
use std::sync::Arc;

use crate::error::{Error, ErrorKind};
use crate::value::{Kwargs, Repr, Value};
use crate::vm::State;

/// A function the engine calls: a filter, test or global function.
pub(crate) type Callable = dyn Fn(&State<'_>, &[Value]) -> Result<Value, Error> + Send + Sync;

/// A function value (what `add_function` stores as a global).
pub(crate) struct NativeFunction {
    pub(crate) name: Arc<str>,
    pub(crate) call: Arc<Callable>,
}

impl fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<function {}>", self.name)
    }
}

impl Value {
    pub(crate) fn from_function(name: &str, call: Arc<Callable>) -> Value {
        Value(Repr::Function(Arc::new(NativeFunction {
            name: Arc::from(name),
            call,
        })))
    }
}

/// A value a function returns: anything that converts into a [`Value`],
/// or a `Result` of one.
pub trait FunctionResult {
    #[doc(hidden)]
    fn into_result(self) -> Result<Value, Error>;
}

impl<I: Into<Value>> FunctionResult for Result<I, Error> {
    fn into_result(self) -> Result<Value, Error> {
        self.map(Into::into)
    }
}

impl<I: Into<Value>> FunctionResult for I {
    fn into_result(self) -> Result<Value, Error> {
        Ok(self.into())
    }
}

/// A parameter type a function can take: [`Value`], `&Value`, `&str`,
/// `String`, `bool`, integers, floats, `Option<T>` (none or undefined
/// become `None`), `Vec<T>` and, last, [`Kwargs`].
pub trait ArgType<'a> {
    /// The type the argument converts into.
    type Output;

    #[doc(hidden)]
    fn from_value(value: Option<&'a Value>, strict: bool) -> Result<Self::Output, Error>;

    #[doc(hidden)]
    fn from_values(
        values: &'a [Value],
        offset: usize,
        strict: bool,
    ) -> Result<(Self::Output, usize), Error> {
        Ok((Self::from_value(values.get(offset), strict)?, 1))
    }

    #[doc(hidden)]
    fn is_trailing() -> bool {
        false
    }
}

/// The longest text a value converts into for a `String` parameter.
pub(crate) const MAX_STRING_ARG: usize = 64 << 20;

fn missing() -> Error {
    Error::from(ErrorKind::MissingArgument)
}

fn unexpected_kwargs() -> Error {
    Error::new(ErrorKind::TooManyArguments, "unexpected keyword arguments")
}

impl<'a> ArgType<'a> for Value {
    type Output = Value;

    fn from_value(value: Option<&'a Value>, _strict: bool) -> Result<Value, Error> {
        match value {
            Some(value) if value.is_kwargs() => Err(unexpected_kwargs()),
            Some(value) => Ok(value.clone()),
            None => Err(missing()),
        }
    }
}

impl<'a> ArgType<'a> for &Value {
    type Output = &'a Value;

    fn from_value(value: Option<&'a Value>, _strict: bool) -> Result<&'a Value, Error> {
        match value {
            Some(value) if value.is_kwargs() => Err(unexpected_kwargs()),
            Some(value) => Ok(value),
            None => Err(missing()),
        }
    }
}

impl<'a> ArgType<'a> for &str {
    type Output = &'a str;

    fn from_value(value: Option<&'a Value>, _strict: bool) -> Result<&'a str, Error> {
        match value {
            Some(value) => value
                .as_str()
                .ok_or_else(|| Error::invalid("value is not a string")),
            None => Err(missing()),
        }
    }
}

impl<'a> ArgType<'a> for String {
    type Output = String;

    fn from_value(value: Option<&'a Value>, strict: bool) -> Result<String, Error> {
        match value {
            Some(value) if value.is_kwargs() => {
                Err(Error::invalid("cannot convert kwargs to string"))
            }
            Some(value) if strict && value.is_undefined() && !value.is_silent_undefined() => {
                Err(Error::undefined())
            }
            Some(value) => value.to_string_capped(MAX_STRING_ARG),
            None => Err(missing()),
        }
    }
}

impl<'a, T: ArgType<'a>> ArgType<'a> for Option<T> {
    type Output = Option<T::Output>;

    fn from_value(value: Option<&'a Value>, strict: bool) -> Result<Self::Output, Error> {
        match value {
            Some(value) if value.is_undefined() || value.is_none() => Ok(None),
            Some(value) => T::from_value(Some(value), strict).map(Some),
            None => Ok(None),
        }
    }
}

impl<'a, T: ArgType<'a, Output = T> + TryFrom<Value, Error = Error>> ArgType<'a> for Vec<T> {
    type Output = Vec<T>;

    fn from_value(value: Option<&'a Value>, _strict: bool) -> Result<Vec<T>, Error> {
        match value {
            Some(value) => value.iter_items()?.into_iter().map(T::try_from).collect(),
            None => Ok(Vec::new()),
        }
    }
}

macro_rules! primitive_arg {
    ($($ty:ty),*) => {
        $(
            impl<'a> ArgType<'a> for $ty {
                type Output = $ty;

                fn from_value(value: Option<&'a Value>, _strict: bool) -> Result<$ty, Error> {
                    match value {
                        Some(value) => <$ty>::try_from(value.clone()),
                        None => Err(missing()),
                    }
                }
            }
        )*
    };
}

primitive_arg!(
    bool, i8, i16, i32, i64, i128, u8, u16, u32, u64, u128, usize, isize, f32, f64
);

impl<'a> ArgType<'a> for Kwargs {
    type Output = Kwargs;

    fn from_value(value: Option<&'a Value>, _strict: bool) -> Result<Kwargs, Error> {
        match value {
            Some(value) => Kwargs::extract(value).ok_or_else(missing),
            None => Ok(Kwargs::new(Arc::default())),
        }
    }

    fn from_values(
        values: &'a [Value],
        offset: usize,
        _strict: bool,
    ) -> Result<(Kwargs, usize), Error> {
        Ok(match values.get(offset).and_then(Kwargs::extract) {
            Some(kwargs) => (kwargs, 1),
            None => (Kwargs::new(Arc::default()), 0),
        })
    }

    fn is_trailing() -> bool {
        true
    }
}

/// A tuple of [`ArgType`]s: the parameters of a function.
pub trait FunctionArgs<'a> {
    /// The converted arguments.
    type Output;

    #[doc(hidden)]
    fn from_values(values: &'a [Value], strict: bool) -> Result<Self::Output, Error>;
}

impl<'a> FunctionArgs<'a> for () {
    type Output = ();

    fn from_values(values: &'a [Value], _strict: bool) -> Result<(), Error> {
        if values.is_empty() {
            Ok(())
        } else {
            Err(Error::from(ErrorKind::TooManyArguments))
        }
    }
}

macro_rules! tuple_args {
    ($($name:ident)* ; $last:ident) => {
        impl<'a, $($name: ArgType<'a>,)* $last: ArgType<'a>> FunctionArgs<'a> for ($($name,)* $last,) {
            type Output = ($($name::Output,)* $last::Output,);

            #[allow(non_snake_case, unused_mut, unused_assignments)]
            fn from_values(values: &'a [Value], strict: bool) -> Result<Self::Output, Error> {
                let mut values = values;
                let mut last = None;
                // A trailing `Kwargs` takes the last value, if that holds
                // keyword arguments, before the positional ones convert.
                let trailing_first = $last::is_trailing() && !values.is_empty();
                if trailing_first {
                    let (value, used) = $last::from_values(values, values.len() - 1, strict)?;
                    last = Some(value);
                    values = &values[..values.len() - used];
                }
                let mut index = 0;
                $(
                    let ($name, used) = $name::from_values(values, index, strict)?;
                    index += used;
                )*
                if !trailing_first {
                    let (value, used) = $last::from_values(values, index, strict)?;
                    last = Some(value);
                    index += used;
                }
                if values.get(index).is_some() {
                    return Err(Error::from(ErrorKind::TooManyArguments));
                }
                match last {
                    Some(last) => Ok(($($name,)* last,)),
                    None => Err(missing()),
                }
            }
        }
    };
}

tuple_args!(; A);
tuple_args!(A ; B);
tuple_args!(A B ; C);
tuple_args!(A B C ; D);
tuple_args!(A B C D ; E);

/// A Rust closure the engine can call with converted arguments. Implemented
/// for `Fn` closures of up to five [`ArgType`] parameters returning a
/// [`FunctionResult`].
pub trait Function<Rv, Args>: Send + Sync + 'static {
    #[doc(hidden)]
    fn invoke(&self, values: &[Value], strict: bool) -> Result<Value, Error>;
}

/// Calls a closure with already converted arguments; the bounds of
/// [`Function`] go through it so closures may borrow from the arguments.
#[doc(hidden)]
pub trait FunctionHelper<Rv, Args> {
    fn invoke_nested(&self, args: Args) -> Rv;
}

macro_rules! function_impls {
    ($($name:ident)*) => {
        impl<Func, Rv, $($name),*> FunctionHelper<Rv, ($($name,)*)> for Func
        where
            Func: Fn($($name),*) -> Rv,
        {
            #[allow(non_snake_case)]
            fn invoke_nested(&self, args: ($($name,)*)) -> Rv {
                let ($($name,)*) = args;
                (self)($($name,)*)
            }
        }

        impl<Func, Rv, $($name),*> Function<Rv, ($($name,)*)> for Func
        where
            Func: Send + Sync + 'static,
            Func: Fn($($name),*) -> Rv
                + for<'a> FunctionHelper<Rv, ($(<$name as ArgType<'a>>::Output,)*)>,
            Rv: FunctionResult,
            $($name: for<'a> ArgType<'a>,)*
        {
            fn invoke(&self, values: &[Value], strict: bool) -> Result<Value, Error> {
                let args = <($($name,)*) as FunctionArgs<'_>>::from_values(values, strict)?;
                self.invoke_nested(args).into_result()
            }
        }
    };
}

function_impls!();
function_impls!(A);
function_impls!(A B);
function_impls!(A B C);
function_impls!(A B C D);
function_impls!(A B C D E);

/// Wraps a closure as an engine function.
pub(crate) fn boxed<F, Rv, Args>(f: F) -> Arc<Callable>
where
    F: Function<Rv, Args>,
{
    Arc::new(move |state: &State<'_>, values: &[Value]| f.invoke(values, state.strict_undefined()))
}
