//! SASL for Kafka: PLAIN, SCRAM-SHA-256 and SCRAM-SHA-512 (RFC 5802).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256, Sha512};

use super::client::{Conn, sasl_authenticate, sasl_handshake};
use crate::MicroError;
use crate::options::SaslOptions;

/// Authenticates `conn` with `credentials`.
pub(crate) async fn authenticate(conn: &Conn, credentials: &SaslOptions) -> Result<(), MicroError> {
    let mechanism = credentials.mechanism.as_str();
    sasl_handshake(conn, mechanism).await?;
    match mechanism {
        "PLAIN" => {
            sasl_authenticate(
                conn,
                plain(&credentials.username, credentials.password.expose()),
            )
            .await?;
            Ok(())
        }
        "SCRAM-SHA-256" | "SCRAM-SHA-512" => {
            let hash = if mechanism == "SCRAM-SHA-256" {
                Hash::Sha256
            } else {
                Hash::Sha512
            };
            let nonce = uuid::Uuid::new_v4().simple().to_string();
            let mut scram = Scram::new(
                hash,
                &credentials.username,
                credentials.password.expose(),
                &nonce,
            );
            let server_first = sasl_authenticate(conn, Bytes::from(scram.client_first())).await?;
            let client_final = scram.client_final(&server_first)?;
            let server_final = sasl_authenticate(conn, Bytes::from(client_final)).await?;
            scram.verify(&server_final)
        }
        other => Err(MicroError::transport(format!(
            "micro.kafka.sasl.mechanism must be PLAIN, SCRAM-SHA-256 or SCRAM-SHA-512, not `{other}`"
        ))),
    }
}

/// `\0user\0password`.
pub(crate) fn plain(username: &str, password: &str) -> Bytes {
    Bytes::from(format!("\0{username}\0{password}"))
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Hash {
    Sha256,
    Sha512,
}

impl Hash {
    fn hmac(self, key: &[u8], data: &[u8]) -> Vec<u8> {
        match self {
            Hash::Sha256 => {
                let mut mac =
                    <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key");
                mac.update(data);
                mac.finalize().into_bytes().to_vec()
            }
            Hash::Sha512 => {
                let mut mac =
                    <Hmac<Sha512> as KeyInit>::new_from_slice(key).expect("HMAC takes any key");
                mac.update(data);
                mac.finalize().into_bytes().to_vec()
            }
        }
    }

    fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            Hash::Sha256 => Sha256::digest(data).to_vec(),
            Hash::Sha512 => Sha512::digest(data).to_vec(),
        }
    }

    /// PBKDF2 with one block (the hash's output length).
    fn salted_password(self, password: &[u8], salt: &[u8], iterations: u32) -> Vec<u8> {
        let mut first = salt.to_vec();
        first.extend_from_slice(&1u32.to_be_bytes());
        let mut u = self.hmac(password, &first);
        let mut out = u.clone();
        for _ in 1..iterations {
            u = self.hmac(password, &u);
            for (o, b) in out.iter_mut().zip(&u) {
                *o ^= b;
            }
        }
        out
    }
}

/// One SCRAM exchange.
pub(crate) struct Scram {
    hash: Hash,
    password: String,
    nonce: String,
    client_first_bare: String,
    server_signature: Vec<u8>,
}

impl Scram {
    pub(crate) fn new(hash: Hash, username: &str, password: &str, nonce: &str) -> Self {
        let user = username.replace('=', "=3D").replace(',', "=2C");
        Self {
            hash,
            password: password.to_owned(),
            nonce: nonce.to_owned(),
            client_first_bare: format!("n={user},r={nonce}"),
            server_signature: Vec::new(),
        }
    }

    /// `n,,n=<user>,r=<nonce>`.
    pub(crate) fn client_first(&self) -> String {
        format!("n,,{}", self.client_first_bare)
    }

    /// The client's final message, with its proof, from the server's
    /// first message.
    pub(crate) fn client_final(&mut self, server_first: &[u8]) -> Result<String, MicroError> {
        let server_first = std::str::from_utf8(server_first)
            .map_err(|_| MicroError::transport("Kafka sent a SCRAM message that is not UTF-8"))?;
        let field = |name: char| {
            server_first.split(',').find_map(|part| {
                part.strip_prefix(name)
                    .and_then(|rest| rest.strip_prefix('='))
            })
        };
        let nonce = field('r')
            .ok_or_else(|| MicroError::transport("Kafka's SCRAM message has no nonce"))?;
        if !nonce.starts_with(&self.nonce) {
            return Err(MicroError::transport(
                "Kafka's SCRAM nonce does not extend ours",
            ));
        }
        let salt = field('s')
            .and_then(|salt| STANDARD.decode(salt).ok())
            .ok_or_else(|| MicroError::transport("Kafka's SCRAM message has no salt"))?;
        let iterations: u32 = field('i')
            .and_then(|i| i.parse().ok())
            .filter(|i| *i > 0)
            .ok_or_else(|| MicroError::transport("Kafka's SCRAM message has no iteration count"))?;
        let salted = self
            .hash
            .salted_password(self.password.as_bytes(), &salt, iterations);
        let client_key = self.hash.hmac(&salted, b"Client Key");
        let stored_key = self.hash.digest(&client_key);
        let without_proof = format!("c=biws,r={nonce}");
        let auth_message = format!("{},{server_first},{without_proof}", self.client_first_bare);
        let signature = self.hash.hmac(&stored_key, auth_message.as_bytes());
        let proof: Vec<u8> = client_key
            .iter()
            .zip(&signature)
            .map(|(k, s)| k ^ s)
            .collect();
        let server_key = self.hash.hmac(&salted, b"Server Key");
        self.server_signature = self.hash.hmac(&server_key, auth_message.as_bytes());
        Ok(format!("{without_proof},p={}", STANDARD.encode(proof)))
    }

    /// Checks the server's final message.
    pub(crate) fn verify(&self, server_final: &[u8]) -> Result<(), MicroError> {
        let text = String::from_utf8_lossy(server_final);
        if let Some(error) = text.strip_prefix("e=") {
            return Err(MicroError::transport(format!(
                "Kafka refused the SCRAM credentials: {error}"
            )));
        }
        let signature = text
            .strip_prefix("v=")
            .and_then(|v| STANDARD.decode(v.trim()).ok())
            .ok_or_else(|| MicroError::transport("Kafka's final SCRAM message has no signature"))?;
        if signature == self.server_signature {
            Ok(())
        } else {
            Err(MicroError::transport(
                "Kafka's SCRAM signature does not match: the server is not who it claims",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7677's SCRAM-SHA-256 example.
    #[test]
    fn scram_sha_256_matches_rfc_7677() {
        let mut scram = Scram::new(Hash::Sha256, "user", "pencil", "rOprNGfwEbeRWgbNEkqO");
        assert_eq!(scram.client_first(), "n,,n=user,r=rOprNGfwEbeRWgbNEkqO");
        let server_first = b"r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096";
        let client_final = scram.client_final(server_first).unwrap();
        assert_eq!(
            client_final,
            "c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,p=dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ="
        );
        scram
            .verify(b"v=6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=")
            .unwrap();
        assert!(scram.verify(b"v=AAAA").is_err());
        assert!(
            scram
                .verify(b"e=invalid-proof")
                .unwrap_err()
                .to_string()
                .contains("invalid-proof")
        );
    }

    #[test]
    fn plain_and_name_escaping() {
        assert_eq!(plain("deals", "secret").as_ref(), b"\0deals\0secret");
        let scram = Scram::new(Hash::Sha512, "a=b,c", "x", "n");
        assert_eq!(scram.client_first(), "n,,n=a=3Db=2Cc,r=n");
    }

    #[test]
    fn a_server_nonce_that_does_not_extend_ours_is_refused() {
        let mut scram = Scram::new(Hash::Sha256, "user", "pencil", "abc");
        let error = scram.client_final(b"r=xyz,s=AAAA,i=1").unwrap_err();
        assert!(error.to_string().contains("nonce"), "{error}");
    }
}
