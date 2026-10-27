//! An LDAPv3 fixture server for the LDAP source's tests: simple binds,
//! searches (base, one level and subtree scopes; and, or, not, equality,
//! presence and substring filters), requested attributes, binary values,
//! the paged results control, and LDAPS. Messages are encoded and decoded
//! with `lber`, the BER codec ldap3 itself uses.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use bytes::BytesMut;
use lber::common::TagClass;
use lber::structure::{PL, StructureTag};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;

/// The paged results control (RFC 2696).
const PAGED_RESULTS: &str = "1.2.840.113556.1.4.319";

/// A directory entry.
#[derive(Clone, Debug)]
pub struct Entry {
    pub dn: String,
    pub attrs: Vec<(String, Vec<Vec<u8>>)>,
}

impl Entry {
    pub fn new(dn: &str) -> Self {
        Self {
            dn: dn.to_owned(),
            attrs: Vec::new(),
        }
    }

    /// Adds text values.
    pub fn attr(mut self, name: &str, values: &[&str]) -> Self {
        self.attrs.push((
            name.to_owned(),
            values.iter().map(|v| v.as_bytes().to_vec()).collect(),
        ));
        self
    }

    /// Adds a binary value.
    pub fn binary(mut self, name: &str, value: Vec<u8>) -> Self {
        self.attrs.push((name.to_owned(), vec![value]));
        self
    }

    fn values(&self, name: &str) -> Vec<&[u8]> {
        self.attrs
            .iter()
            .filter(|(attr, _)| attr.eq_ignore_ascii_case(name))
            .flat_map(|(_, values)| values.iter().map(Vec::as_slice))
            .collect()
    }
}

/// What a search asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchSeen {
    pub base: String,
    pub page_size: Option<i64>,
    pub returned: usize,
}

#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    credentials: Option<(String, String)>,
    searches: Vec<SearchSeen>,
    binds: Vec<String>,
}

/// A running fixture server.
pub struct LdapServer {
    addr: SocketAddr,
    tls: bool,
    state: Arc<Mutex<State>>,
}

impl LdapServer {
    /// Serves `entries` over plain LDAP; with `credentials`, only that
    /// simple bind is accepted and searches need it.
    pub async fn start(entries: Vec<Entry>, credentials: Option<(&str, &str)>) -> Self {
        Self::serve(entries, credentials, None).await
    }

    /// Serves over LDAPS with a certificate for 127.0.0.1; returns the
    /// server and the certificate's PEM.
    pub async fn start_tls(
        entries: Vec<Entry>,
        credentials: Option<(&str, &str)>,
    ) -> (Self, String) {
        let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_owned()]).unwrap();
        let pem = certified.cert.pem();
        let cert = CertificateDer::from(certified.cert.der().to_vec());
        let key =
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der()));
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        (Self::serve(entries, credentials, Some(acceptor)).await, pem)
    }

    async fn serve(
        entries: Vec<Entry>,
        credentials: Option<(&str, &str)>,
        tls: Option<tokio_rustls::TlsAcceptor>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(State {
            entries,
            credentials: credentials.map(|(dn, pw)| (dn.to_owned(), pw.to_owned())),
            ..State::default()
        }));
        let shared = state.clone();
        let is_tls = tls.is_some();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let state = shared.clone();
                let tls = tls.clone();
                tokio::spawn(async move {
                    match tls {
                        Some(acceptor) => {
                            if let Ok(stream) = acceptor.accept(stream).await {
                                let _ = session(stream, state).await;
                            }
                        }
                        None => {
                            let _ = session(stream, state).await;
                        }
                    }
                });
            }
        });
        LdapServer {
            addr,
            tls: is_tls,
            state,
        }
    }

    /// `ldap://127.0.0.1:<port>` (or `ldaps://`).
    pub fn url(&self) -> String {
        format!(
            "{}://127.0.0.1:{}",
            if self.tls { "ldaps" } else { "ldap" },
            self.addr.port()
        )
    }

    /// The searches served.
    pub fn searches(&self) -> Vec<SearchSeen> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .searches
            .clone()
    }

    /// The DNs of the binds tried.
    pub fn binds(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binds
            .clone()
    }

    /// Replaces the entries.
    pub fn set_entries(&self, entries: Vec<Entry>) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries = entries;
    }
}

// -- the session --------------------------------------------------------------

async fn session<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    state: Arc<Mutex<State>>,
) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    let mut bound = false;
    loop {
        let message = loop {
            if let Some(len) = frame_len(&buffer)
                && buffer.len() >= len
            {
                let frame: Vec<u8> = buffer.drain(..len).collect();
                break frame;
            }
            let mut chunk = [0_u8; 4096];
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Ok(());
            }
            buffer.extend_from_slice(&chunk[..read]);
        };
        let Ok((_, tag)) = lber::parse::parse_tag(&message) else {
            return Ok(());
        };
        let Some(mut parts) = tag.expect_constructed() else {
            return Ok(());
        };
        if parts.len() < 2 {
            return Ok(());
        }
        let id = integer(&parts[0]);
        let op = parts.remove(1);
        let controls = parts
            .into_iter()
            .find(|t| t.class == TagClass::Context && t.id == 0);
        let mut replies = Vec::new();
        match (op.class, op.id) {
            (TagClass::Application, 0) => {
                let (code, message) = bind(&op, &state);
                bound = code == 0;
                replies.push(envelope(id, result(1, code, &message), None));
            }
            (TagClass::Application, 2) => return Ok(()),
            (TagClass::Application, 3) => {
                if !bound
                    && state
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .credentials
                        .is_some()
                {
                    replies.push(envelope(id, result(5, 50, "bind first"), None));
                } else {
                    replies.extend(search(id, op, controls, &state));
                }
            }
            _ => replies.push(envelope(id, result(24, 2, "unsupported operation"), None)),
        }
        for reply in replies {
            let mut out = BytesMut::new();
            lber::write::encode_into(&mut out, reply)?;
            stream.write_all(&out).await?;
        }
        stream.flush().await?;
    }
}

/// The length of the BER element at the start of `buffer`, once its
/// header is there.
fn frame_len(buffer: &[u8]) -> Option<usize> {
    if buffer.len() < 2 {
        return None;
    }
    let first = buffer[1];
    if first & 0x80 == 0 {
        return Some(2 + usize::from(first));
    }
    let count = usize::from(first & 0x7f);
    if buffer.len() < 2 + count {
        return None;
    }
    let len = buffer[2..2 + count]
        .iter()
        .fold(0_usize, |len, byte| (len << 8) | usize::from(*byte));
    Some(2 + count + len)
}

fn bind(op: &StructureTag, state: &Arc<Mutex<State>>) -> (i64, String) {
    let parts = op.clone().expect_constructed().unwrap_or_default();
    let dn = parts.get(1).map(text).unwrap_or_default();
    let password = parts.get(2).map(text).unwrap_or_default();
    let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
    state.binds.push(dn.clone());
    match &state.credentials {
        Some((expected_dn, expected_pw)) => {
            if dn.eq_ignore_ascii_case(expected_dn) && &password == expected_pw {
                (0, String::new())
            } else {
                (49, "80090308: LdapErr: DSID-0C09044E, comment: AcceptSecurityContext error, data 52e".to_owned())
            }
        }
        None => (0, String::new()),
    }
}

fn search(
    id: i64,
    op: StructureTag,
    controls: Option<StructureTag>,
    state: &Arc<Mutex<State>>,
) -> Vec<StructureTag> {
    let parts = op.expect_constructed().unwrap_or_default();
    let base = parts.first().map(text).unwrap_or_default();
    let scope = parts.get(1).map(integer).unwrap_or(2);
    let filter = parts.get(6).cloned();
    let attrs: Vec<String> = parts
        .get(7)
        .cloned()
        .and_then(StructureTag::expect_constructed)
        .unwrap_or_default()
        .iter()
        .map(text)
        .collect();
    // The paged results control: (size, offset as the cookie).
    let paging = controls.and_then(|controls| {
        controls
            .expect_constructed()?
            .into_iter()
            .find_map(|control| {
                let parts = control.expect_constructed()?;
                if text(parts.first()?) != PAGED_RESULTS {
                    return None;
                }
                let value = parts.last()?.clone().expect_primitive()?;
                let (_, inner) = lber::parse::parse_tag(&value).ok()?;
                let inner = inner.expect_constructed()?;
                let size = integer(inner.first()?);
                let cookie = text(inner.get(1)?);
                Some((size, cookie.parse::<usize>().unwrap_or(0)))
            })
    });
    let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
    let matching: Vec<Entry> = state
        .entries
        .iter()
        .filter(|entry| in_scope(&entry.dn, &base, scope))
        .filter(|entry| filter.as_ref().is_none_or(|f| matches(f, entry)))
        .cloned()
        .collect();
    let (page, next): (Vec<Entry>, Option<usize>) = match paging {
        Some((size, offset)) => {
            let size = usize::try_from(size.max(1)).unwrap_or(1);
            let end = (offset + size).min(matching.len());
            let next = (end < matching.len()).then_some(end);
            (matching[offset.min(matching.len())..end].to_vec(), next)
        }
        None => (matching, None),
    };
    state.searches.push(SearchSeen {
        base: base.clone(),
        page_size: paging.map(|(size, _)| size),
        returned: page.len(),
    });
    let mut replies: Vec<StructureTag> = page
        .iter()
        .map(|entry| envelope(id, search_entry(entry, &attrs), None))
        .collect();
    let control = paging.map(|(size, _)| {
        let cookie = next.map(|n| n.to_string()).unwrap_or_default();
        paged_control(size, &cookie)
    });
    replies.push(envelope(id, result(5, 0, ""), control));
    replies
}

fn in_scope(dn: &str, base: &str, scope: i64) -> bool {
    let dn = normalize(dn);
    let base = normalize(base);
    match scope {
        0 => dn == base,
        1 => dn
            .strip_suffix(&base)
            .and_then(|rest| rest.strip_suffix(','))
            .is_some_and(|rdn| !rdn.contains(',')),
        _ => dn == base || dn.ends_with(&format!(",{base}")),
    }
}

fn normalize(dn: &str) -> String {
    dn.split(',')
        .map(|rdn| {
            rdn.trim()
                .to_ascii_lowercase()
                .replace(" =", "=")
                .replace("= ", "=")
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn matches(filter: &StructureTag, entry: &Entry) -> bool {
    let children = || filter.clone().expect_constructed().unwrap_or_default();
    match (filter.class, filter.id) {
        (TagClass::Context, 0) => children().iter().all(|f| matches(f, entry)),
        (TagClass::Context, 1) => children().iter().any(|f| matches(f, entry)),
        (TagClass::Context, 2) => !children().first().is_some_and(|f| matches(f, entry)),
        (TagClass::Context, 3) => {
            let parts = children();
            let (Some(attr), Some(value)) = (parts.first(), parts.get(1)) else {
                return false;
            };
            let value = text(value).to_ascii_lowercase();
            entry
                .values(&text(attr))
                .iter()
                .any(|v| String::from_utf8_lossy(v).to_ascii_lowercase() == value)
        }
        (TagClass::Context, 4) => {
            let parts = children();
            let Some(attr) = parts.first() else {
                return false;
            };
            let pieces = parts
                .get(1)
                .cloned()
                .and_then(StructureTag::expect_constructed)
                .unwrap_or_default();
            entry.values(&text(attr)).iter().any(|value| {
                let value = String::from_utf8_lossy(value).to_ascii_lowercase();
                let mut at = 0;
                pieces.iter().all(|piece| {
                    let wanted = text(piece).to_ascii_lowercase();
                    match piece.id {
                        0 => {
                            let ok = value.starts_with(&wanted);
                            at = wanted.len();
                            ok
                        }
                        2 => value.ends_with(&wanted),
                        _ => match value[at..].find(&wanted) {
                            Some(found) => {
                                at += found + wanted.len();
                                true
                            }
                            None => false,
                        },
                    }
                })
            })
        }
        (TagClass::Context, 7) => {
            let attr =
                String::from_utf8_lossy(&filter.clone().expect_primitive().unwrap_or_default())
                    .into_owned();
            attr.eq_ignore_ascii_case("objectClass") || !entry.values(&attr).is_empty()
        }
        _ => false,
    }
}

// -- encoding -----------------------------------------------------------------

fn text(tag: &StructureTag) -> String {
    String::from_utf8_lossy(&tag.clone().expect_primitive().unwrap_or_default()).into_owned()
}

fn integer(tag: &StructureTag) -> i64 {
    let bytes = tag.clone().expect_primitive().unwrap_or_default();
    let mut value: i64 = if bytes.first().is_some_and(|b| b & 0x80 != 0) {
        -1
    } else {
        0
    };
    for byte in bytes {
        value = (value << 8) | i64::from(byte);
    }
    value
}

fn universal(id: u64, payload: PL) -> StructureTag {
    StructureTag {
        class: TagClass::Universal,
        id,
        payload,
    }
}

fn octets(value: &[u8]) -> StructureTag {
    universal(4, PL::P(value.to_vec()))
}

fn int(value: i64, id: u64) -> StructureTag {
    let mut bytes = value.to_be_bytes().to_vec();
    while bytes.len() > 1
        && ((bytes[0] == 0 && bytes[1] & 0x80 == 0) || (bytes[0] == 0xff && bytes[1] & 0x80 != 0))
    {
        bytes.remove(0);
    }
    universal(id, PL::P(bytes))
}

fn sequence(items: Vec<StructureTag>) -> StructureTag {
    universal(16, PL::C(items))
}

fn envelope(id: i64, op: StructureTag, control: Option<StructureTag>) -> StructureTag {
    let mut parts = vec![int(id, 2), op];
    if let Some(control) = control {
        parts.push(StructureTag {
            class: TagClass::Context,
            id: 0,
            payload: PL::C(vec![control]),
        });
    }
    sequence(parts)
}

/// An LDAPResult under application tag `op`.
fn result(op: u64, code: i64, message: &str) -> StructureTag {
    StructureTag {
        class: TagClass::Application,
        id: op,
        payload: PL::C(vec![int(code, 10), octets(b""), octets(message.as_bytes())]),
    }
}

fn search_entry(entry: &Entry, wanted: &[String]) -> StructureTag {
    let all = wanted.is_empty() || wanted.iter().any(|a| a == "*");
    let attributes: Vec<StructureTag> = entry
        .attrs
        .iter()
        .filter(|(name, _)| all || wanted.iter().any(|w| w.eq_ignore_ascii_case(name)))
        .map(|(name, values)| {
            sequence(vec![
                octets(name.as_bytes()),
                universal(17, PL::C(values.iter().map(|v| octets(v)).collect())),
            ])
        })
        .collect();
    StructureTag {
        class: TagClass::Application,
        id: 4,
        payload: PL::C(vec![octets(entry.dn.as_bytes()), sequence(attributes)]),
    }
}

fn paged_control(size: i64, cookie: &str) -> StructureTag {
    let mut value = BytesMut::new();
    lber::write::encode_into(
        &mut value,
        sequence(vec![int(size, 2), octets(cookie.as_bytes())]),
    )
    .unwrap();
    sequence(vec![octets(PAGED_RESULTS.as_bytes()), octets(&value)])
}
