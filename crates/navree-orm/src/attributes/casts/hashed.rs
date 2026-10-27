//! `cast = "hashed"`: passwords and other secrets stored as Argon2id hashes
//! (navree-crypto's `Hasher`).

use argon2::{Algorithm, Params, PasswordHash};
use navree_kernel::BoxFuture;

use crate::attributes::CastContext;
use crate::cast::{Cast, CastError};
use crate::value::{Field, FieldKind};

/// What a string assigned to a hashed field is.
enum Kind {
    /// A well-formed Argon2 PHC string: kept as it is.
    Hash,
    /// A plain value: hashed before it is written.
    Plain,
    /// Something that claims to be a hash but cannot be used as one.
    Refused(String),
}

/// Hash formats of other algorithms, which Navree's hasher cannot verify.
const OTHER_FORMATS: [(&str, &str); 6] = [
    ("$2a$", "bcrypt"),
    ("$2b$", "bcrypt"),
    ("$2y$", "bcrypt"),
    ("$scrypt$", "scrypt"),
    ("$pbkdf2", "PBKDF2"),
    ("$balloon", "Balloon"),
];

fn classify(value: &str) -> Kind {
    if value.starts_with("$argon2") {
        return if is_password_hash(value) {
            Kind::Hash
        } else {
            Kind::Refused(
                "the value looks like an Argon2 hash but does not parse as one (algorithm, version, parameters, salt and hash are required)"
                    .to_owned(),
            )
        };
    }
    match OTHER_FORMATS
        .iter()
        .find(|(prefix, _)| value.starts_with(prefix))
    {
        Some((_, algorithm)) => Kind::Refused(format!(
            "the value is a {algorithm} hash, and Navree verifies Argon2 hashes only: assign the plain value to hash it with Argon2id"
        )),
        None => Kind::Plain,
    }
}

/// Whether `value` is an Argon2 hash in the PHC string format
/// (`$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>`) whose parameters parse,
/// which navree-crypto's `Hasher::verify` can check passwords against
/// (whatever its cost parameters: `Hasher::needs_rehash` tells when to
/// upgrade them).
///
/// ```
/// use navree_orm::cast::is_password_hash;
///
/// assert!(is_password_hash(
///     "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$Tr6ffxUqo5TVOlcIZeVY0pIBn6bv7HJbNSuDn9t1Bzo"
/// ));
/// assert!(!is_password_hash("correct horse battery staple"));
/// assert!(!is_password_hash("$argon2id$v=19$m=19456"));
/// ```
pub fn is_password_hash(value: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(value) else {
        return false;
    };
    Algorithm::try_from(parsed.algorithm.as_str()).is_ok()
        && parsed.salt.is_some()
        && parsed.hash.is_some()
        && Params::try_from(&parsed).is_ok()
}

/// A secret stored as an Argon2id hash (`#[model(cast = "hashed")]`), on
/// `String` fields: assign the plain value, save, and the record holds the
/// hash. Check a password with navree-crypto's `Hasher::verify`.
///
/// - Hashing runs when the record is saved (after the `saving` hooks),
///   with the app's `Hasher`: on its blocking pool, never on the async
///   threads.
/// - A value that already is an Argon2 hash is kept, whatever its cost
///   parameters (`Hasher::needs_rehash` tells when to upgrade at the next
///   login); a malformed Argon2 string or another algorithm's hash
///   (bcrypt, scrypt, PBKDF2) is refused, so a hash is never hashed again.
/// - Reading returns the hash; serialized records write it (hide the
///   field with `#[model(hidden)]`).
/// - Writing a plain value without saving the record (a mass update, a
///   condition) is an error: hash it first.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hashed;

impl Cast<String> for Hashed {
    type Stored = String;

    const CONTEXT: bool = true;

    const KIND: FieldKind = FieldKind::Text;

    fn get(stored: String) -> Result<String, CastError> {
        Ok(stored)
    }

    fn set(value: &String) -> Result<String, CastError> {
        match classify(value) {
            Kind::Hash => Ok(value.clone()),
            Kind::Plain => Err(CastError::new(
                "the value is not hashed yet: hashed fields are hashed when their record is saved; to write one another way, hash it first with navree_crypto::Hasher::hash",
            )),
            Kind::Refused(reason) => Err(CastError::new(reason)),
        }
    }

    fn same(a: &String, b: &String, context: &CastContext) -> bool {
        let _ = context;
        a == b
    }

    fn from_json(input: serde_json::Value, context: &CastContext) -> Result<String, CastError> {
        let _ = context;
        String::from_json(input).map_err(CastError::from)
    }

    fn prepare<'a>(
        value: &'a mut String,
        context: &'a CastContext,
    ) -> BoxFuture<'a, Result<(), CastError>> {
        Box::pin(async move {
            match classify(value) {
                Kind::Hash => Ok(()),
                Kind::Refused(reason) => Err(CastError::new(reason)),
                Kind::Plain => {
                    let hash = context
                        .hasher()?
                        .hash(value)
                        .await
                        .map_err(|error| CastError::new(error.to_string()))?;
                    *value = hash;
                    Ok(())
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use navree_crypto::{Hasher, HashingOptions};

    use super::*;

    fn fast_hasher() -> Hasher {
        Hasher::new(&HashingOptions {
            memory_kib: 1024,
            iterations: 1,
            parallelism: 1,
            max_concurrent_hashes: Some(2),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn plain_values_are_hashed_before_the_write_and_hashes_are_kept() {
        let hasher = fast_hasher();
        let context = CastContext::new().with_hasher(hasher.clone());
        let mut password = "correct horse".to_owned();
        assert!(
            Hashed::set(&password).is_err(),
            "never stores a plain value"
        );
        Hashed::prepare(&mut password, &context).await.unwrap();
        assert!(is_password_hash(&password), "{password}");
        assert!(hasher.verify("correct horse", &password).await.unwrap());
        let hash = password.clone();
        Hashed::prepare(&mut password, &context).await.unwrap();
        assert_eq!(password, hash, "a hash is not hashed again");
        assert_eq!(Hashed::set(&password).unwrap(), hash);
    }

    #[tokio::test]
    async fn other_algorithms_and_broken_hashes_are_refused() {
        let context = CastContext::new().with_hasher(fast_hasher());
        let mut bcrypt = "$2y$10$abcdefghijklmnopqrstuuJ8Wbm3gx9z5HFRz8wE2PxDZtjVYZxq2".to_owned();
        let error = Hashed::prepare(&mut bcrypt, &context).await.unwrap_err();
        assert!(error.to_string().contains("bcrypt hash"), "{error}");
        let mut broken = "$argon2id$v=19$m=1024".to_owned();
        let error = Hashed::prepare(&mut broken, &context).await.unwrap_err();
        assert!(error.to_string().contains("does not parse"), "{error}");
    }

    #[tokio::test]
    async fn hashing_needs_the_apps_hasher() {
        let mut password = "secret".to_owned();
        let error = Hashed::prepare(&mut password, &CastContext::new())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("no Hasher"), "{error}");
        assert_eq!(password, "secret");
    }
}
