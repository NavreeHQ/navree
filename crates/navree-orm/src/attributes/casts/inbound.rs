//! Inbound casts: transform a value on its way to the database, read the
//! stored value as it is.

use std::marker::PhantomData;

use crate::attributes::CastContext;
use crate::cast::{Cast, CastError};
use crate::value::{Field, FieldKind};

/// A transformation applied when a field is written and never when it is
/// read (`#[model(inbound = path::To::Cast)]`): normalizing, slugifying,
/// trimming, hashing with a custom scheme. The field is a [`Field`] stored
/// as itself.
///
/// The transformation runs every time the field is encoded (saves, dirty
/// checks, conditions), on the value the record holds, and a reloaded
/// record holds the stored, transformed value: make it idempotent
/// (transforming a transformed value changes nothing), as a slug or a
/// lower-cased email is.
///
/// ```
/// use navree_orm::cast::{Cast, CastError, Inbound, InboundCast};
///
/// /// Stores emails trimmed and lower-cased.
/// struct NormalizedEmail;
///
/// impl InboundCast<String> for NormalizedEmail {
///     fn set(value: &String) -> Result<String, CastError> {
///         Ok(value.trim().to_lowercase())
///     }
/// }
///
/// assert_eq!(Inbound::<NormalizedEmail>::set(&" Ada@Example.COM ".to_owned())?, "ada@example.com");
/// let read: String = Inbound::<NormalizedEmail>::get("Kept As Stored".to_owned())?;
/// assert_eq!(read, "Kept As Stored");
/// # Ok::<(), navree_orm::cast::CastError>(())
/// ```
pub trait InboundCast<T>: 'static {
    /// Whether the transformation reads services from a [`CastContext`].
    const CONTEXT: bool = false;

    /// The value to store for `value`.
    fn set(value: &T) -> Result<T, CastError>;

    /// [`set`](InboundCast::set) with the services of `context`.
    fn set_with(value: &T, context: &CastContext) -> Result<T, CastError> {
        let _ = context;
        Self::set(value)
    }
}

/// The [`Cast`] of an [`InboundCast`] `C` (what `#[model(inbound = C)]`
/// expands to).
pub struct Inbound<C>(PhantomData<fn() -> C>);

impl<C: InboundCast<T>, T: Field> Cast<T> for Inbound<C> {
    type Stored = T;

    const CONTEXT: bool = C::CONTEXT;

    const KIND: FieldKind = T::KIND;

    fn get(stored: T) -> Result<T, CastError> {
        Ok(stored)
    }

    fn set(value: &T) -> Result<T, CastError> {
        C::set(value)
    }

    fn set_with(value: &T, context: &CastContext) -> Result<T, CastError> {
        C::set_with(value, context)
    }

    fn from_json(input: serde_json::Value, context: &CastContext) -> Result<T, CastError> {
        let _ = context;
        T::from_json(input).map_err(CastError::from)
    }
}
