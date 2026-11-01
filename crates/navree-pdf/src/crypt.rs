//! Files encrypted with the standard security handler and an empty
//! password to open them (an owner password that only restricts printing
//! or copying): RC4 and AES-128 (revisions 2 to 4) and AES-256
//! (revisions 5 and 6). A file that needs a password to open is
//! [`PdfError::Encrypted`].
//!
//! The ciphers and hashes are RustCrypto's.

use aes::{Aes128, Aes256};
use cbc::cipher::array::Array;
use cbc::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use md5::{Digest, Md5};
use rc4::{KeyInit, Rc4, StreamCipher};
use sha2::{Sha256, Sha384, Sha512};

use crate::error::PdfError;
use crate::object::{Dictionary, Object, ObjectId};

/// The padding that stands for an empty password (Algorithm 2).
const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// How strings or streams are encrypted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    Identity,
    Rc4,
    Aes128,
    Aes256,
}

/// The file's key and how it is used.
#[derive(Debug)]
pub(crate) struct Crypt {
    key: Vec<u8>,
    strings: Method,
    streams: Method,
    /// Whether metadata streams are encrypted.
    metadata: bool,
}

/// The method a crypt filter named `name` in `encrypt`'s `CF` uses.
fn filter_method(encrypt: &Dictionary, name: Option<&[u8]>) -> Result<Method, PdfError> {
    let name = name.unwrap_or(b"Identity");
    if name == b"Identity" {
        return Ok(Method::Identity);
    }
    let filter = encrypt
        .get(b"CF")
        .and_then(Object::as_dict)
        .and_then(|filters| filters.get(name))
        .and_then(Object::as_dict)
        .ok_or(PdfError::Encrypted)?;
    match filter.get(b"CFM").and_then(Object::as_name) {
        None | Some(b"None") => Ok(Method::Identity),
        Some(b"V2") => Ok(Method::Rc4),
        Some(b"AESV2") => Ok(Method::Aes128),
        Some(b"AESV3") => Ok(Method::Aes256),
        Some(_) => Err(PdfError::Encrypted),
    }
}

impl Crypt {
    /// The decryption of a file whose `Encrypt` dictionary is `encrypt` and
    /// whose first `ID` is `id`, if the empty password opens it, as the
    /// user's password or the owner's.
    pub(crate) fn open(encrypt: &Dictionary, id: &[u8]) -> Result<Crypt, PdfError> {
        if encrypt.get(b"Filter").and_then(Object::as_name) != Some(b"Standard") {
            return Err(PdfError::Encrypted);
        }
        let int = |key: &[u8]| encrypt.get(key).and_then(Object::as_i64);
        let bytes = |key: &[u8]| {
            encrypt
                .get(key)
                .and_then(Object::as_str)
                .unwrap_or_default()
        };
        let version = int(b"V").unwrap_or(0);
        let revision = int(b"R").unwrap_or(0);
        let metadata = encrypt
            .get(b"EncryptMetadata")
            .and_then(Object::as_bool)
            .unwrap_or(true);
        let (strings, streams) = match version {
            1 | 2 => (Method::Rc4, Method::Rc4),
            4 | 5 => (
                filter_method(encrypt, encrypt.get(b"StrF").and_then(Object::as_name))?,
                filter_method(encrypt, encrypt.get(b"StmF").and_then(Object::as_name))?,
            ),
            _ => return Err(PdfError::Encrypted),
        };
        let (owner, user) = (bytes(b"O"), bytes(b"U"));
        let key = if revision >= 5 {
            let (owner_key, user_key) = (bytes(b"OE"), bytes(b"UE"));
            aes256_key(revision, owner, user, owner_key, user_key).ok_or(PdfError::Encrypted)?
        } else {
            let length = match version {
                1 => 5,
                // In bits; a few writers give it in bytes.
                _ => {
                    let given = int(b"Length").unwrap_or(128);
                    usize::try_from(if given < 40 { given } else { given / 8 })
                        .unwrap_or(16)
                        .clamp(5, 16)
                }
            };
            let permissions = int(b"P").unwrap_or(0) as i32;
            let compute = |password: &[u8; 32]| {
                rc4_key(password, owner, permissions, id, revision, length, metadata)
            };
            let user_key = compute(&PAD);
            if user_valid(&user_key, revision, user, id) {
                user_key
            } else {
                // The owner password's key opens O, which holds the user's
                // password.
                let password = owner_to_user(owner, revision, length).ok_or(PdfError::Encrypted)?;
                let key = compute(&password);
                if !user_valid(&key, revision, user, id) {
                    return Err(PdfError::Encrypted);
                }
                key
            }
        };
        Ok(Crypt {
            key,
            strings,
            streams,
            metadata,
        })
    }

    /// Decrypts the strings and stream of object `id` in place; `identity`
    /// says the stream opts out (see [`opts_out`]). Returns how many it
    /// decrypted: each makes a key.
    pub(crate) fn decrypt(&self, id: ObjectId, object: &mut Object, identity: bool) -> usize {
        let mut count = 0;
        match object {
            Object::Stream(stream) => {
                self.decrypt_strings(id, &mut stream.dict, &mut count);
                let skip = identity
                    || stream.dict.has_type(b"XRef")
                    || (!self.metadata && stream.dict.has_type(b"Metadata"));
                if !skip {
                    count += 1;
                    stream.content = self.apply(self.streams, id, &stream.content).into();
                }
            }
            other => self.decrypt_object_strings(id, other, 0, &mut count),
        }
        count
    }

    fn decrypt_strings(&self, id: ObjectId, dict: &mut Dictionary, count: &mut usize) {
        for (_, value) in dict.iter_mut() {
            self.decrypt_object_strings(id, value, 1, count);
        }
    }

    fn decrypt_object_strings(
        &self,
        id: ObjectId,
        object: &mut Object,
        depth: usize,
        count: &mut usize,
    ) {
        if depth > crate::parse::MAX_DEPTH {
            return;
        }
        match object {
            Object::String(bytes, _) => {
                *count += 1;
                *bytes = self.apply(self.strings, id, bytes);
            }
            Object::Array(items) => {
                for item in items {
                    self.decrypt_object_strings(id, item, depth + 1, count);
                }
            }
            Object::Dictionary(dict) => {
                for (_, value) in dict.iter_mut() {
                    self.decrypt_object_strings(id, value, depth + 1, count);
                }
            }
            _ => {}
        }
    }

    /// `data` decrypted with `method` for object `id`. Data that does not
    /// decrypt (AES of a length that is not whole blocks) is kept.
    fn apply(&self, method: Method, (number, generation): ObjectId, data: &[u8]) -> Vec<u8> {
        if method == Method::Identity {
            return data.to_vec();
        }
        let key = if method == Method::Aes256 {
            self.key.clone()
        } else {
            // Algorithm 1: the file key, the object's number and generation.
            let mut hasher = Md5::new();
            hasher.update(&self.key);
            hasher.update(&number.to_le_bytes()[..3]);
            hasher.update(generation.to_le_bytes());
            if method == Method::Aes128 {
                hasher.update(b"sAlT");
            }
            let digest = hasher.finalize();
            digest
                .get(..(self.key.len() + 5).min(16))
                .unwrap_or_default()
                .to_vec()
        };
        match method {
            Method::Rc4 => rc4(&key, data),
            Method::Aes128 | Method::Aes256 => {
                aes_cbc_decrypt(&key, data).unwrap_or_else(|| data.to_vec())
            }
            Method::Identity => data.to_vec(),
        }
    }
}

/// Whether a stream's first filter is `Crypt` with the `Identity` crypt
/// filter: named in its parameters, or by having no name there, which is what
/// the name defaults to.
pub(crate) fn opts_out<'a>(
    dict: &'a Dictionary,
    resolve: &dyn Fn(&'a Object) -> Option<&'a Object>,
) -> bool {
    let first = |object: &'a Object| -> Option<&'a Object> {
        match resolve(object)? {
            Object::Array(items) => resolve(items.first()?),
            other => Some(other),
        }
    };
    let crypt = dict
        .get(b"Filter")
        .and_then(first)
        .and_then(Object::as_name)
        == Some(b"Crypt");
    if !crypt {
        return false;
    }
    let name = dict
        .get(b"DecodeParms")
        .or_else(|| dict.get(b"DP"))
        .and_then(first)
        .and_then(Object::as_dict)
        .and_then(|params| params.get(b"Name"))
        .and_then(resolve)
        .and_then(Object::as_name);
    name.is_none_or(|name| name == b"Identity")
}

fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    if let Ok(mut cipher) = Rc4::new_from_slice(key) {
        cipher.apply_keystream(&mut out);
    }
    out
}

/// AES-CBC with the initialization vector in the first 16 bytes, and
/// PKCS#7 padding.
fn aes_cbc_decrypt(key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let (iv, rest) = data.split_at_checked(16)?;
    if rest.is_empty() {
        return Some(Vec::new());
    }
    if !rest.len().is_multiple_of(16) {
        return None;
    }
    let mut out = rest.to_vec();
    let (blocks, _) = Array::slice_as_chunks_mut(&mut out);
    match key.len() {
        16 => cbc::Decryptor::<Aes128>::new_from_slices(key, iv)
            .ok()?
            .decrypt_blocks(blocks),
        32 => cbc::Decryptor::<Aes256>::new_from_slices(key, iv)
            .ok()?
            .decrypt_blocks(blocks),
        _ => return None,
    }
    let pad = usize::from(*out.last()?);
    if (1..=16).contains(&pad) && pad <= out.len() {
        out.truncate(out.len() - pad);
    }
    Some(out)
}

/// Algorithm 2: the key from a padded password.
fn rc4_key(
    password: &[u8; 32],
    owner: &[u8],
    permissions: i32,
    id: &[u8],
    revision: i64,
    length: usize,
    metadata: bool,
) -> Vec<u8> {
    let mut hasher = Md5::new();
    hasher.update(password);
    hasher.update(owner.get(..32).unwrap_or(owner));
    hasher.update(permissions.to_le_bytes());
    hasher.update(id);
    if revision >= 4 && !metadata {
        hasher.update([0xFF; 4]);
    }
    let mut hash = hasher.finalize().to_vec();
    if revision >= 3 {
        for _ in 0..50 {
            hash = Md5::digest(hash.get(..length).unwrap_or(&hash)).to_vec();
        }
    }
    hash.truncate(if revision == 2 { 5 } else { length });
    hash
}

/// Algorithms 4 and 5: whether `key` opens the file, checked against `U`.
fn user_valid(key: &[u8], revision: i64, user: &[u8], id: &[u8]) -> bool {
    if revision == 2 {
        return user.get(..32).is_some_and(|user| rc4(key, &PAD) == user);
    }
    let mut hasher = Md5::new();
    hasher.update(PAD);
    hasher.update(id);
    let mut value = rc4(key, &hasher.finalize());
    for round in 1..=19u8 {
        let round_key: Vec<u8> = key.iter().map(|byte| byte ^ round).collect();
        value = rc4(&round_key, &value);
    }
    user.get(..16)
        .is_some_and(|user| value.get(..16) == Some(user))
}

/// Algorithm 7: the user's padded password kept in `O`, decrypted with the
/// key of an empty owner password.
fn owner_to_user(owner: &[u8], revision: i64, length: usize) -> Option<[u8; 32]> {
    let mut hash = Md5::digest(PAD).to_vec();
    if revision >= 3 {
        for _ in 0..50 {
            hash = Md5::digest(&hash).to_vec();
        }
    }
    let key = hash.get(..if revision == 2 { 5 } else { length })?.to_vec();
    let mut value = owner.get(..32)?.to_vec();
    if revision == 2 {
        value = rc4(&key, &value);
    } else {
        for round in (0..=19u8).rev() {
            let round_key: Vec<u8> = key.iter().map(|byte| byte ^ round).collect();
            value = rc4(&round_key, &value);
        }
    }
    value.try_into().ok()
}

/// The hash of revisions 5 and 6 of an empty password with `salt` and
/// `extra` (Algorithm 2.B for revision 6).
fn hash(revision: i64, salt: &[u8], extra: &[u8]) -> Option<Vec<u8>> {
    let mut k = Sha256::new()
        .chain_update(salt)
        .chain_update(extra)
        .finalize()
        .to_vec();
    if revision == 5 {
        return Some(k);
    }
    let mut round = 0u32;
    loop {
        let mut k1 = Vec::with_capacity(64 * (k.len() + extra.len()));
        for _ in 0..64 {
            k1.extend_from_slice(&k);
            k1.extend_from_slice(extra);
        }
        let (key, iv) = (k.get(..16)?, k.get(16..32)?);
        let (blocks, rest) = Array::slice_as_chunks_mut(&mut k1);
        if !rest.is_empty() {
            return None;
        }
        cbc::Encryptor::<Aes128>::new_from_slices(key, iv)
            .ok()?
            .encrypt_blocks(blocks);
        let e = k1;
        let sum: u32 = e.get(..16)?.iter().map(|&byte| u32::from(byte)).sum();
        k = match sum % 3 {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        round += 1;
        if round >= 64 && u32::from(*e.last()?) <= round - 32 {
            break;
        }
    }
    k.truncate(32);
    Some(k)
}

/// The file key of revisions 5 and 6, from `UE` with the empty user
/// password or from `OE` with the empty owner password.
fn aes256_key(
    revision: i64,
    owner: &[u8],
    user: &[u8],
    owner_key: &[u8],
    user_key: &[u8],
) -> Option<Vec<u8>> {
    let user48 = user.get(..48)?;
    let candidates = [
        (
            user.get(..32)?,
            user.get(32..40)?,
            user.get(40..48)?,
            &[][..],
            user_key,
        ),
        (
            owner.get(..32)?,
            owner.get(32..40)?,
            owner.get(40..48)?,
            user48,
            owner_key,
        ),
    ];
    for (check, validation, key_salt, extra, wrapped) in candidates {
        if hash(revision, validation, extra)?.as_slice() != check {
            continue;
        }
        let intermediate = hash(revision, key_salt, extra)?;
        let mut key = wrapped.get(..32)?.to_vec();
        let (blocks, _) = Array::slice_as_chunks_mut(&mut key);
        cbc::Decryptor::<Aes256>::new_from_slices(&intermediate, &[0u8; 16])
            .ok()?
            .decrypt_blocks(blocks);
        return Some(key);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crypt_filter_opts_out_by_name_in_a_list_or_with_no_name() {
        let parsed = |text: &str| {
            let Some(Object::Dictionary(dict)) = crate::parse::object(text.as_bytes()) else {
                panic!("{text}");
            };
            dict
        };
        let opted = |text: &str| opts_out(&parsed(text), &Some);
        assert!(opted("<</Filter/Crypt>>"));
        assert!(opted("<</Filter[/Crypt]>>"));
        assert!(opted("<</Filter/Crypt/DecodeParms<</Name/Identity>>>>"));
        assert!(opted("<</Filter[/Crypt]/DecodeParms[<</Name/Identity>>]>>"));
        assert!(opted(
            "<</Filter[/Crypt/FlateDecode]/DecodeParms[<<>> null]>>"
        ));
        assert!(opted("<</Filter/Crypt/DP<</Name/Identity>>>>"));
        assert!(!opted("<</Filter/Crypt/DecodeParms<</Name/StdCF>>>>"));
        assert!(!opted("<</Filter[/Crypt]/DecodeParms[<</Name/StdCF>>]>>"));
        assert!(!opted("<</Filter/FlateDecode>>"));
        assert!(!opted("<</Filter[/FlateDecode/Crypt]>>"));
        assert!(!opted("<<>>"));
    }

    #[test]
    fn a_stream_that_opts_out_is_kept_as_it_is_written() {
        let crypt = Crypt {
            key: vec![1; 16],
            strings: Method::Rc4,
            streams: Method::Rc4,
            metadata: true,
        };
        let stream = |identity| {
            let mut object: Object =
                crate::object::Stream::new(Dictionary::new(), b"plain".to_vec()).into();
            crypt.decrypt((4, 0), &mut object, identity);
            object.as_stream().unwrap().content.to_vec()
        };
        assert_eq!(stream(true), b"plain");
        assert_ne!(stream(false), b"plain");
    }

    #[test]
    fn rc4_matches_known_vectors() {
        assert_eq!(
            rc4(b"Key", b"Plaintext"),
            [0xBB, 0xF3, 0x16, 0xE8, 0xD9, 0x40, 0xAF, 0x0A, 0xD3]
        );
        assert_eq!(rc4(b"Wiki", b"pedia"), [0x10, 0x21, 0xBF, 0x04, 0x20]);
    }

    #[test]
    fn aes_cbc_strips_its_padding_and_keeps_what_does_not_decrypt() {
        // Encrypt "hello" with a known key and IV, padded to one block.
        let key = [0x42u8; 16];
        let iv = [0x24u8; 16];
        let mut block = *b"hello\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b";
        let (blocks, _) = Array::slice_as_chunks_mut(&mut block);
        cbc::Encryptor::<Aes128>::new_from_slices(&key, &iv)
            .unwrap()
            .encrypt_blocks(blocks);
        let mut data = iv.to_vec();
        data.extend_from_slice(&block);
        assert_eq!(aes_cbc_decrypt(&key, &data).unwrap(), b"hello");
        assert_eq!(aes_cbc_decrypt(&key, &data[..20]), None);
        assert_eq!(aes_cbc_decrypt(&key, &iv), Some(Vec::new()));
    }

    #[test]
    fn a_handler_other_than_the_standard_one_is_refused() {
        let encrypt = crate::dictionary! { "Filter" => "Adobe.PubSec", "V" => 2 };
        assert_eq!(
            Crypt::open(&encrypt, b"id").unwrap_err(),
            PdfError::Encrypted
        );
    }
}
