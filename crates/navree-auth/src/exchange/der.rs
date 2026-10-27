//! Just enough DER to read a SubjectPublicKeyInfo (RFC 5280 §4.1): the
//! algorithm and the key of RSA, P-256 and Ed25519 public keys.

use super::jws::PublicKey;

/// `rsaEncryption` (1.2.840.113549.1.1.1).
const RSA_ENCRYPTION: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
/// `id-ecPublicKey` (1.2.840.10045.2.1).
const EC_PUBLIC_KEY: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
/// `prime256v1` (1.2.840.10045.3.1.7).
const PRIME256V1: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
/// `id-Ed25519` (1.3.101.112).
const ED25519: &[u8] = &[0x2B, 0x65, 0x70];

const SEQUENCE: u8 = 0x30;
const BIT_STRING: u8 = 0x03;
const OID: u8 = 0x06;

/// One TLV: its tag, its content and what follows it.
fn read(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7F);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (bytes, rest) = rest.split_at(count);
        let len = bytes
            .iter()
            .fold(0_usize, |len, byte| (len << 8) | usize::from(*byte));
        (len, rest)
    };
    if rest.len() < len {
        return None;
    }
    let (content, rest) = rest.split_at(len);
    Some((tag, content, rest))
}

/// The public key of a DER SubjectPublicKeyInfo.
pub(crate) fn public_key_from_spki(der: &[u8]) -> Option<PublicKey> {
    let (tag, spki, rest) = read(der)?;
    if tag != SEQUENCE || !rest.is_empty() {
        return None;
    }
    let (tag, algorithm, rest) = read(spki)?;
    if tag != SEQUENCE {
        return None;
    }
    let (tag, bits, rest) = read(rest)?;
    if tag != BIT_STRING || !rest.is_empty() {
        return None;
    }
    let (&unused, key) = bits.split_first()?;
    if unused != 0 {
        return None;
    }
    let (tag, oid, params) = read(algorithm)?;
    if tag != OID {
        return None;
    }
    match oid {
        RSA_ENCRYPTION => Some(PublicKey::RsaDer(key.to_vec())),
        EC_PUBLIC_KEY => {
            let (tag, curve, _) = read(params)?;
            (tag == OID && curve == PRIME256V1 && key.len() == 65 && key[0] == 0x04)
                .then(|| PublicKey::P256(key.to_vec()))
        }
        ED25519 => (key.len() == 32).then(|| PublicKey::Ed25519(key.to_vec())),
        _ => None,
    }
}

/// A DER TLV of `tag` around `content` (short lengths and two-byte ones).
#[cfg(test)]
fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    if content.len() < 0x80 {
        out.push(u8::try_from(content.len()).unwrap());
    } else {
        out.push(0x82);
        out.extend_from_slice(&u16::try_from(content.len()).unwrap().to_be_bytes());
    }
    out.extend_from_slice(content);
    out
}

/// The SubjectPublicKeyInfo of an uncompressed P-256 point.
#[cfg(test)]
pub(crate) fn p256_spki(point: &[u8]) -> Vec<u8> {
    let algorithm = [tlv(OID, EC_PUBLIC_KEY), tlv(OID, PRIME256V1)].concat();
    let bits = [&[0_u8][..], point].concat();
    tlv(
        SEQUENCE,
        &[tlv(SEQUENCE, &algorithm), tlv(BIT_STRING, &bits)].concat(),
    )
}

/// The SubjectPublicKeyInfo of an Ed25519 key.
#[cfg(test)]
pub(crate) fn ed25519_spki(key: &[u8]) -> Vec<u8> {
    let bits = [&[0_u8][..], key].concat();
    tlv(
        SEQUENCE,
        &[tlv(SEQUENCE, &tlv(OID, ED25519)), tlv(BIT_STRING, &bits)].concat(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spki_documents_give_their_keys() {
        let point = [[0x04_u8].as_slice(), &[7; 64]].concat();
        assert_eq!(
            public_key_from_spki(&p256_spki(&point)),
            Some(PublicKey::P256(point.clone()))
        );
        assert_eq!(
            public_key_from_spki(&ed25519_spki(&[9; 32])),
            Some(PublicKey::Ed25519(vec![9; 32]))
        );
        let rsa_key = tlv(SEQUENCE, &[1, 2, 3]);
        let algorithm = [tlv(OID, RSA_ENCRYPTION), vec![0x05, 0x00]].concat();
        let spki = tlv(
            SEQUENCE,
            &[
                tlv(SEQUENCE, &algorithm),
                tlv(BIT_STRING, &[&[0_u8][..], &rsa_key].concat()),
            ]
            .concat(),
        );
        assert_eq!(
            public_key_from_spki(&spki),
            Some(PublicKey::RsaDer(rsa_key))
        );
    }

    #[test]
    fn malformed_documents_are_refused() {
        let good = ed25519_spki(&[9; 32]);
        for bad in [
            Vec::new(),
            good[..good.len() - 1].to_vec(),
            [good.as_slice(), &[0]].concat(),
            ed25519_spki(&[9; 31]),
            p256_spki(&[0x04; 33]),
            vec![0x30, 0x85, 0, 0, 0, 0, 0],
        ] {
            assert_eq!(public_key_from_spki(&bad), None, "{bad:?}");
        }
    }
}
