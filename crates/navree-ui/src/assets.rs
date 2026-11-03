//! The toolkit's files, embedded in the binary with their brotli and gzip
//! forms, and served at content-hash URLs (`/_navree/ui/core-3f9a1c07b2de.js`)
//! as immutable.
//!
//! The files and their hashes come from `runtime/build.mjs`, which writes
//! `assets_gen.rs`, so no URL is computed at run time.

use bytes::Bytes;
use http::header::{
    ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, ETAG, HeaderValue,
    IF_NONE_MATCH, VARY,
};
use http::{HeaderMap, StatusCode};
use navree_http::response::{IntoResponse, Response};

pub use crate::assets_gen::*;

use crate::node::Uses;

/// Where the files are served.
pub const MOUNT: &str = "/_navree/ui";

/// One file.
#[derive(Debug)]
pub struct Asset {
    /// The name without its hash or extension (`core`).
    pub name: &'static str,
    /// The extension (`js`).
    pub ext: &'static str,
    /// The first 12 hex digits of the content hash.
    pub hash: &'static str,
    /// The `Content-Type`.
    pub content_type: &'static str,
    /// The file.
    pub bytes: &'static [u8],
    /// Its gzip form.
    pub gzip: Option<&'static [u8]>,
    /// Its brotli form.
    pub brotli: Option<&'static [u8]>,
}

/// The FNV-1a hash the file names carry, as 12 hex digits.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let mut out = format!("{hash:016x}");
    out.truncate(12);
    out
}

/// The file named `core`.
pub fn get(name: &str) -> Option<&'static Asset> {
    FILES.iter().find(|asset| asset.name == name)
}

/// The module scripts a page with `uses` loads.
pub fn scripts(uses: Uses) -> impl Iterator<Item = &'static str> {
    [
        (Uses::CORE, CORE_URL),
        (Uses::SIDEBAR, SIDEBAR_URL),
        (Uses::OVERLAY, OVERLAY_URL),
        (Uses::TABLE, TABLE_URL),
        (Uses::SELECT, SELECT_URL),
    ]
    .into_iter()
    .filter(move |(bit, _)| uses.has(*bit))
    .map(|(_, url)| url)
}

/// Finds the file behind `core-3f9a1c07b2de.js`: the file, and whether the
/// hash is current. An old hash still gets the file, uncached, so a page open
/// across a deploy keeps working.
fn lookup(file: &str) -> Option<(&'static Asset, bool)> {
    let (stem, ext) = file.rsplit_once('.')?;
    let (name, hash) = stem.rsplit_once('-')?;
    let asset = FILES
        .iter()
        .find(|asset| asset.name == name && asset.ext == ext)?;
    Some((asset, asset.hash == hash))
}

/// Whether `accept` takes `encoding` (an explicit `;q=0` refuses it).
pub(crate) fn accepts(accept: &str, encoding: &str) -> bool {
    accept.split(',').any(|item| {
        let mut parts = item.split(';');
        let name = parts.next().unwrap_or_default().trim();
        let refused = parts.any(|param| {
            let param = param.trim();
            param
                .strip_prefix("q=")
                .is_some_and(|q| q.trim().parse::<f32>().is_ok_and(|q| q == 0.0))
        });
        name.eq_ignore_ascii_case(encoding) && !refused
    })
}

/// Answers `GET {MOUNT}/{file}`: the precompressed form the client takes.
pub fn respond(file: &str, request: &HeaderMap) -> Response {
    let Some((asset, current)) = lookup(file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let cache = if current {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let etag = HeaderValue::from_str(&format!("\"{}\"", asset.hash))
        .unwrap_or(HeaderValue::from_static("\"navree-ui\""));
    if request
        .get(IF_NONE_MATCH)
        .is_some_and(|value| value == etag)
    {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (ETAG, etag),
                (CACHE_CONTROL, HeaderValue::from_static(cache)),
            ],
        )
            .into_response();
    }
    let accept = request
        .get(ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let (bytes, encoding) = match (asset.brotli, asset.gzip) {
        (Some(brotli), _) if accepts(accept, "br") => (brotli, Some("br")),
        (_, Some(gzip)) if accepts(accept, "gzip") => (gzip, Some("gzip")),
        _ => (asset.bytes, None),
    };
    let mut response = (
        [
            (CONTENT_TYPE, HeaderValue::from_static(asset.content_type)),
            (CACHE_CONTROL, HeaderValue::from_static(cache)),
            (ETAG, etag),
        ],
        Bytes::from_static(bytes),
    )
        .into_response();
    if asset.gzip.is_some() {
        response
            .headers_mut()
            .insert(VARY, HeaderValue::from_static("accept-encoding"));
    }
    if let Some(encoding) = encoding {
        response
            .headers_mut()
            .insert(CONTENT_ENCODING, HeaderValue::from_static(encoding));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_match_the_files() {
        for asset in FILES {
            assert_eq!(content_hash(asset.bytes), asset.hash, "{}", asset.name);
        }
    }

    #[test]
    fn urls_carry_the_hash() {
        let core = get("core").unwrap();
        assert_eq!(CORE_URL, format!("{MOUNT}/core-{}.js", core.hash));
    }

    #[test]
    fn stale_hashes_are_found_but_not_current() {
        assert!(
            lookup(&format!("core-{}.js", get("core").unwrap().hash))
                .unwrap()
                .1
        );
        assert!(!lookup("core-000000000000.js").unwrap().1);
        assert!(lookup("nothing-000000000000.js").is_none());
        assert!(lookup("core.js").is_none());
        assert!(lookup("../secret").is_none());
    }

    #[test]
    fn encodings_are_negotiated() {
        assert!(accepts("gzip, br", "br"));
        assert!(accepts("gzip;q=0.5", "gzip"));
        assert!(!accepts("br;q=0", "br"));
        assert!(!accepts("", "gzip"));
    }
}
