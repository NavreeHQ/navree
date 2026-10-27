//! `cast = "encrypted"` and `cast = "encrypted_json"`: values encrypted at
//! rest with the app's key (navree-crypto's AES-256-GCM `Encrypter`).

use navree_crypto::{CryptoError, Encrypter};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::attributes::CastContext;
use crate::cast::{Cast, CastError};
use crate::row::Row;
use crate::value::{Field, FieldError, FieldKind};

/// A field type the [`Encrypted`] cast stores: text (`String`) or bytes
/// (`Vec<u8>`). Other types use [`EncryptedJson`].
pub trait EncryptedValue: Sized + 'static {
    /// What the plain value is in serialized records.
    const KIND: FieldKind;

    /// Encrypts the value.
    fn encrypt(&self, encrypter: &Encrypter) -> Result<String, CryptoError>;

    /// Decrypts a payload into the value.
    fn decrypt(payload: &str, encrypter: &Encrypter) -> Result<Self, CryptoError>;

    /// The value from untyped input (the plain value).
    fn from_input(input: serde_json::Value) -> Result<Self, FieldError>;

    /// Whether two values are equal.
    fn same(&self, other: &Self) -> bool;
}

impl EncryptedValue for String {
    const KIND: FieldKind = FieldKind::Text;

    fn encrypt(&self, encrypter: &Encrypter) -> Result<String, CryptoError> {
        encrypter.encrypt_string(self)
    }

    fn decrypt(payload: &str, encrypter: &Encrypter) -> Result<Self, CryptoError> {
        encrypter.decrypt_string(payload)
    }

    fn from_input(input: serde_json::Value) -> Result<Self, FieldError> {
        String::from_json(input)
    }

    fn same(&self, other: &Self) -> bool {
        self == other
    }
}

impl EncryptedValue for Vec<u8> {
    const KIND: FieldKind = FieldKind::Bytes;

    fn encrypt(&self, encrypter: &Encrypter) -> Result<String, CryptoError> {
        encrypter.encrypt(self)
    }

    fn decrypt(payload: &str, encrypter: &Encrypter) -> Result<Self, CryptoError> {
        encrypter.decrypt(payload)
    }

    fn from_input(input: serde_json::Value) -> Result<Self, FieldError> {
        Vec::<u8>::from_json(input)
    }

    fn same(&self, other: &Self) -> bool {
        self == other
    }
}

/// What a failed encryption or decryption says (never the value: crypto
/// errors carry no plaintext).
fn crypto_error(error: CryptoError) -> CastError {
    match error {
        CryptoError::Decrypt => CastError::new(
            "the stored value could not be decrypted: it was modified, or it was encrypted with a key that is no longer configured (keep retired keys in `app.previous_keys`)",
        ),
        other => CastError::new(other.to_string()),
    }
}

/// The key id of an encrypted payload (`v1.<kid>.<nonce>.<ciphertext>`).
fn payload_kid(payload: &str) -> Option<&str> {
    let mut segments = payload.split('.');
    match (segments.next(), segments.next()) {
        (Some("v1"), Some(kid)) => Some(kid),
        _ => None,
    }
}

/// Whether the payload stored in `column` of `row` was encrypted with
/// another key than `context`'s current one (a previous key: the next save
/// re-encrypts it).
fn stale_payload(row: &Row, column: &str, context: &CastContext) -> bool {
    let Ok(encrypter) = context.encrypter() else {
        return false;
    };
    let Some(current) = encrypter.current_kid() else {
        return false;
    };
    match row.field::<Option<String>>(column) {
        Ok(Some(payload)) => payload_kid(&payload).is_some_and(|kid| kid != current),
        _ => false,
    }
}

/// Text or bytes encrypted at rest (`#[model(cast = "encrypted")]`) with
/// the app's [`Encrypter`] (AES-256-GCM, a fresh nonce every write), or the
/// encrypter the model names (`#[model(encrypter = "...")]`).
///
/// - Reading decrypts with the current key or any key in
///   `app.previous_keys`, so keys can rotate; a record read with a previous
///   key counts as dirty, and its next save re-encrypts it with the current
///   key.
/// - Dirty tracking compares the plain values (each encryption differs).
/// - `fill` and serialized records use the plain value; conditions cannot
///   compare encrypted columns (every payload differs), so query them by
///   another column.
/// - Errors never contain the value.
///
/// ```
/// use navree_crypto::{AppKey, Encrypter};
/// use navree_orm::attributes::CastContext;
/// use navree_orm::cast::{Cast, Encrypted};
///
/// let context = CastContext::new().with_encrypter(Encrypter::new(AppKey::generate()));
/// let note = "allergic to penicillin".to_owned();
/// let stored = Encrypted::set_with(&note, &context)?;
/// assert_ne!(stored, note);
/// let read: String = Encrypted::get_with(stored, &context)?;
/// assert_eq!(read, note);
/// # Ok::<(), navree_orm::cast::CastError>(())
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct Encrypted;

impl<T: EncryptedValue> Cast<T> for Encrypted {
    type Stored = String;

    const CONTEXT: bool = true;

    const KIND: FieldKind = T::KIND;

    fn get(stored: String) -> Result<T, CastError> {
        CastContext::with_current(|context| Self::get_with(stored, context))
    }

    fn set(value: &T) -> Result<String, CastError> {
        CastContext::with_current(|context| Self::set_with(value, context))
    }

    fn get_with(stored: String, context: &CastContext) -> Result<T, CastError> {
        T::decrypt(&stored, context.encrypter()?).map_err(crypto_error)
    }

    fn set_with(value: &T, context: &CastContext) -> Result<String, CastError> {
        value.encrypt(context.encrypter()?).map_err(crypto_error)
    }

    fn same(a: &T, b: &T, context: &CastContext) -> bool {
        let _ = context;
        a.same(b)
    }

    fn from_json(input: serde_json::Value, context: &CastContext) -> Result<T, CastError> {
        let _ = context;
        T::from_input(input).map_err(CastError::from)
    }

    fn is_stale(row: &Row, column: &str, context: &CastContext) -> bool {
        stale_payload(row, column, context)
    }
}

/// Any serde type stored as encrypted JSON (`#[model(cast =
/// "encrypted_json")]`): lists, maps, structs, collections. It behaves like
/// [`Encrypted`] (key rotation, plain-value dirty tracking, plain input and
/// output); the JSON is what is encrypted.
///
/// ```
/// use navree_crypto::{AppKey, Encrypter};
/// use navree_orm::attributes::CastContext;
/// use navree_orm::cast::{Cast, EncryptedJson};
///
/// let context = CastContext::new().with_encrypter(Encrypter::new(AppKey::generate()));
/// let codes = vec!["A-1".to_owned(), "B-2".to_owned()];
/// let stored = EncryptedJson::set_with(&codes, &context)?;
/// let read: Vec<String> = EncryptedJson::get_with(stored, &context)?;
/// assert_eq!(read, codes);
/// # Ok::<(), navree_orm::cast::CastError>(())
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct EncryptedJson;

impl<T: Serialize + DeserializeOwned + 'static> Cast<T> for EncryptedJson {
    type Stored = String;

    const CONTEXT: bool = true;

    const KIND: FieldKind = FieldKind::Json;

    fn get(stored: String) -> Result<T, CastError> {
        CastContext::with_current(|context| Self::get_with(stored, context))
    }

    fn set(value: &T) -> Result<String, CastError> {
        CastContext::with_current(|context| Self::set_with(value, context))
    }

    fn get_with(stored: String, context: &CastContext) -> Result<T, CastError> {
        context
            .encrypter()?
            .decrypt_json(&stored)
            .map_err(crypto_error)
    }

    fn set_with(value: &T, context: &CastContext) -> Result<String, CastError> {
        context
            .encrypter()?
            .encrypt_json(value)
            .map_err(crypto_error)
    }

    fn same(a: &T, b: &T, context: &CastContext) -> bool {
        let _ = context;
        match (serde_json::to_value(a), serde_json::to_value(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }

    fn from_json(input: serde_json::Value, context: &CastContext) -> Result<T, CastError> {
        let _ = context;
        serde_json::from_value(input).map_err(|error| {
            CastError::new(format!(
                "the input is not the JSON of the field's type: {error}"
            ))
        })
    }

    fn is_stale(row: &Row, column: &str, context: &CastContext) -> bool {
        stale_payload(row, column, context)
    }
}

#[cfg(test)]
mod tests {
    use navree_crypto::AppKey;
    use serde_json::json;

    use super::*;
    use crate::value::Value;

    fn context(key: AppKey) -> CastContext {
        CastContext::new().with_encrypter(Encrypter::new(key))
    }

    #[test]
    fn every_write_differs_but_the_plain_values_compare() {
        let context = context(AppKey::generate());
        let secret = "s3cret".to_owned();
        let first: String = Encrypted::set_with(&secret, &context).unwrap();
        let second: String = Encrypted::set_with(&secret, &context).unwrap();
        assert_ne!(first, second, "a fresh nonce each time");
        assert!(<Encrypted as Cast<String>>::same(
            &secret,
            &secret.clone(),
            &context
        ));
        assert!(!<Encrypted as Cast<String>>::same(
            &secret,
            &"other".to_owned(),
            &context
        ));
        let bytes = vec![0_u8, 159, 255];
        let stored = Encrypted::set_with(&bytes, &context).unwrap();
        let read: Vec<u8> = Encrypted::get_with(stored, &context).unwrap();
        assert_eq!(read, bytes);
    }

    #[test]
    fn decryption_failures_explain_key_rotation() {
        let stored: String =
            Encrypted::set_with(&"x".to_owned(), &context(AppKey::generate())).unwrap();
        let error = <Encrypted as Cast<String>>::get_with(stored, &context(AppKey::generate()))
            .unwrap_err();
        assert!(error.to_string().contains("app.previous_keys"), "{error}");
        let error =
            <Encrypted as Cast<String>>::get_with("plain".into(), &context(AppKey::generate()))
                .unwrap_err();
        assert!(
            error.to_string().starts_with("invalid encrypted payload"),
            "{error}"
        );
        let error = <Encrypted as Cast<String>>::set_with(&"x".to_owned(), &CastContext::new())
            .unwrap_err();
        assert!(error.to_string().contains("no Encrypter"), "{error}");
    }

    #[test]
    fn payloads_under_a_previous_key_are_stale() {
        let old = AppKey::generate();
        let stored: String = Encrypted::set_with(&"x".to_owned(), &context(old.clone())).unwrap();
        let rotated = CastContext::new()
            .with_encrypter(Encrypter::new(AppKey::generate()).with_previous_keys([old.clone()]));
        let row = Row::from_values(vec![("note".into(), Value::from(stored.clone()))]);
        assert!(<Encrypted as Cast<String>>::is_stale(
            &row, "note", &rotated
        ));
        assert!(!<Encrypted as Cast<String>>::is_stale(
            &row,
            "note",
            &context(old)
        ));
        let read: String = Encrypted::get_with(stored, &rotated).unwrap();
        assert_eq!(read, "x", "previous keys still decrypt");
        let nulls = Row::from_values(vec![("note".into(), Value::String(None))]);
        assert!(!<Encrypted as Cast<String>>::is_stale(
            &nulls, "note", &rotated
        ));
    }

    #[test]
    fn json_values_round_trip_and_take_plain_input() {
        let context = context(AppKey::generate());
        let codes = json!({"backup": ["a", "b"], "used": 1});
        let stored = EncryptedJson::set_with(&codes, &context).unwrap();
        let read: serde_json::Value = EncryptedJson::get_with(stored, &context).unwrap();
        assert_eq!(read, codes);
        let input: Vec<u32> = EncryptedJson::from_json(json!([1, 2]), &context).unwrap();
        assert_eq!(input, [1, 2]);
        let error =
            <EncryptedJson as Cast<Vec<u32>>>::from_json(json!("nope"), &context).unwrap_err();
        assert!(
            error.to_string().starts_with("the input is not the JSON"),
            "{error}"
        );
        let plain: String = Encrypted::from_json(json!("plain"), &context).unwrap();
        assert_eq!(plain, "plain");
    }
}
