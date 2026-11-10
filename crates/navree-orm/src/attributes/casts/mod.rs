//! The built-in casts beyond JSON and enums: fixed-scale decimals, text and
//! integer date-times, encryption, hashing, binary ids, vectors, inbound
//! casts, value types that pick their own cast, and fields stored across
//! several columns. They are re-exported from [`crate::cast`], where the
//! table of every cast is.

mod binary;
mod castable;
mod datetime;
mod decimal;
mod encrypted;
mod hashed;
mod inbound;
mod multi;
#[cfg(feature = "phone")]
mod phone;
mod url;
mod vector;

pub use binary::{BinaryUlid, BinaryUuid};
pub use castable::Castable;
pub use datetime::{
    DateTimeFormat, DateTimeFormatSpec, FormattedDateTime, UnixTimestamp, UnixTimestampMs,
};
pub use decimal::DecimalScale;
pub use encrypted::{Encrypted, EncryptedJson, EncryptedValue};
pub use hashed::{Hashed, is_password_hash};
pub use inbound::{Inbound, InboundCast};
pub use multi::{MultiColumnCast, StoredColumns};
pub use vector::{StoredVector, Vector};
