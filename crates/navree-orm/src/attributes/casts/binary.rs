//! `cast = "binary_uuid"` and `cast = "binary_ulid"`: ids stored as 16
//! bytes.

use uuid::Uuid;

use crate::attributes::CastContext;
use crate::cast::{Cast, CastError};
use crate::ulid::Ulid;
use crate::value::{Field, FieldKind};

fn sixteen_bytes(stored: &[u8], what: &str) -> Result<[u8; 16], CastError> {
    <[u8; 16]>::try_from(stored).map_err(|_| {
        CastError::new(format!(
            "{} bytes are not a binary {what} (16 bytes)",
            stored.len()
        ))
    })
}

/// A [`Uuid`] stored as its 16 bytes (`#[model(cast = "binary_uuid")]`):
/// `BYTEA` on PostgreSQL, `BINARY(16)` on MySQL, a blob on SQLite. Typed
/// conditions (`Deal::REF.eq(id)`) encode through the cast; `fill` and
/// serialized records use the text form.
///
/// `Uuid` fields without a cast are already 16 bytes on MySQL and SQLite
/// and native UUIDs on PostgreSQL: the cast is for columns that must be
/// binary on every database.
///
/// ```
/// use navree_orm::cast::{BinaryUuid, Cast};
/// use uuid::Uuid;
///
/// let id = Uuid::now_v7();
/// let stored = BinaryUuid::set(&id)?;
/// assert_eq!(stored, id.as_bytes().to_vec());
/// assert_eq!(<BinaryUuid as Cast<Uuid>>::get(stored)?, id);
/// # Ok::<(), navree_orm::cast::CastError>(())
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct BinaryUuid;

impl Cast<Uuid> for BinaryUuid {
    type Stored = Vec<u8>;

    const KIND: FieldKind = FieldKind::Uuid;

    fn get(stored: Vec<u8>) -> Result<Uuid, CastError> {
        sixteen_bytes(&stored, "UUID").map(Uuid::from_bytes)
    }

    fn set(value: &Uuid) -> Result<Vec<u8>, CastError> {
        Ok(value.as_bytes().to_vec())
    }

    fn from_json(input: serde_json::Value, context: &CastContext) -> Result<Uuid, CastError> {
        let _ = context;
        Uuid::from_json(input).map_err(CastError::from)
    }
}

/// A [`Ulid`] stored as its 16 bytes (`#[model(cast = "binary_ulid")]`),
/// big-endian, so byte order is time order. `fill` and serialized records
/// use the 26-character text form.
///
/// ```
/// use navree_orm::Ulid;
/// use navree_orm::cast::{BinaryUlid, Cast};
///
/// let id = Ulid::new();
/// let stored = BinaryUlid::set(&id)?;
/// assert_eq!(stored, id.to_u128().to_be_bytes().to_vec());
/// assert_eq!(<BinaryUlid as Cast<Ulid>>::get(stored)?, id);
/// # Ok::<(), navree_orm::cast::CastError>(())
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct BinaryUlid;

impl Cast<Ulid> for BinaryUlid {
    type Stored = Vec<u8>;

    const KIND: FieldKind = FieldKind::Ulid;

    fn get(stored: Vec<u8>) -> Result<Ulid, CastError> {
        sixteen_bytes(&stored, "ULID").map(|bytes| Ulid::from_u128(u128::from_be_bytes(bytes)))
    }

    fn set(value: &Ulid) -> Result<Vec<u8>, CastError> {
        Ok(value.to_u128().to_be_bytes().to_vec())
    }

    fn from_json(input: serde_json::Value, context: &CastContext) -> Result<Ulid, CastError> {
        let _ = context;
        Ulid::from_json(input).map_err(CastError::from)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wrong_lengths_are_cast_errors() {
        let error = <BinaryUuid as Cast<Uuid>>::get(vec![1, 2, 3]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "3 bytes are not a binary UUID (16 bytes)"
        );
        let error = <BinaryUlid as Cast<Ulid>>::get(Vec::new()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "0 bytes are not a binary ULID (16 bytes)"
        );
    }

    #[test]
    fn input_is_the_text_form() {
        let context = CastContext::new();
        let id = Uuid::now_v7();
        let read: Uuid = BinaryUuid::from_json(json!(id.to_string()), &context).unwrap();
        assert_eq!(read, id);
        let ulid = Ulid::new();
        let read: Ulid = BinaryUlid::from_json(json!(ulid.to_string()), &context).unwrap();
        assert_eq!(read, ulid);
        assert!(<BinaryUuid as Cast<Uuid>>::from_json(json!("not-an-id"), &context).is_err());
    }

    #[test]
    fn byte_order_is_time_order_for_ulids() {
        let earlier = Ulid::from_parts(1_000, 7);
        let later = Ulid::from_parts(2_000, 1);
        assert!(BinaryUlid::set(&earlier).unwrap() < BinaryUlid::set(&later).unwrap());
    }
}
