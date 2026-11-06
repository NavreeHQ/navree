//! The key of a stored file: what a form's file field posts in place of the file once the
//! upload module has sent it.
//!
//! A key is the file's path on its disk, signed with the app's [`UrlSigner`] and good for a
//! day, so a browser can hand back a file the server stored for it and no other: a path it
//! makes up, or one changed by a character, reads as no key at all.

use chrono::{Duration, Utc};
use navree_http::UrlSigner;
use navree_http::extract::urlencoded;

/// The address a key signs. It is never served: it only carries the path.
const KEY_PATH: &str = "/_navree/ui/file";
/// How long a key may be posted after its file was stored.
const KEY_LIFETIME_HOURS: i64 = 24;

/// The key of the stored file at `path`, good for a day.
///
/// ```
/// use navree_http::UrlSigner;
/// use navree_ui::upload::{file_key, file_path};
///
/// let signer = UrlSigner::new(b"a long random key");
/// let key = file_key(&signer, "avatars/k3v9q.png");
/// assert_eq!(file_path(&signer, &key).as_deref(), Some("avatars/k3v9q.png"));
/// assert_eq!(file_path(&signer, "avatars/k3v9q.png"), None);
/// ```
pub fn file_key(signer: &UrlSigner, path: &str) -> String {
    signer.sign(
        &format!("{KEY_PATH}?path={}", encode(path)),
        Utc::now() + Duration::hours(KEY_LIFETIME_HOURS),
    )
}

/// The path `key` names, when `signer` signed it and it has not expired.
pub fn file_path(signer: &UrlSigner, key: &str) -> Option<String> {
    let uri: http::Uri = key.trim().parse().ok()?;
    if uri.path() != KEY_PATH || !signer.verify(&uri, Utc::now()) {
        return None;
    }
    let mut paths = urlencoded::parse(uri.query()?.as_bytes())
        .filter(|(name, _)| name == "path")
        .map(|(_, value)| value.into_owned());
    match (paths.next(), paths.next()) {
        (Some(path), None) if !path.is_empty() => Some(path),
        _ => None,
    }
}

/// `text` for a query: letters, digits and `-._~/` as they are, every other byte as `%XX`.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> UrlSigner {
        UrlSigner::new(b"a key only these tests know")
    }

    #[test]
    fn a_key_names_its_path_and_nothing_else() {
        let signer = signer();
        for path in ["documents/aB3x.pdf", "a b/ç%&=.png"] {
            let key = file_key(&signer, path);
            assert_eq!(file_path(&signer, &key).as_deref(), Some(path), "{key}");
        }
    }

    #[test]
    fn a_changed_or_foreign_key_reads_as_none() {
        let signer = signer();
        let key = file_key(&signer, "documents/aB3x.pdf");
        let changed = key.replace("aB3x", "aB3y");
        assert_eq!(file_path(&signer, &changed), None);
        let other = UrlSigner::new(b"another app's key");
        assert_eq!(file_path(&other, &key), None);
        for made_up in [
            "",
            "documents/aB3x.pdf",
            "/_navree/ui/file?path=documents/aB3x.pdf",
            "https://example.com/_navree/ui/file?path=x",
        ] {
            assert_eq!(file_path(&signer, made_up), None, "{made_up}");
        }
        // A key whose path was given twice names no file.
        let twice = signer.sign(
            "/_navree/ui/file?path=a.pdf&path=b.pdf",
            Utc::now() + Duration::hours(1),
        );
        assert_eq!(file_path(&signer, &twice), None);
    }

    #[test]
    fn an_expired_key_reads_as_none() {
        let signer = signer();
        let old = signer.sign(
            "/_navree/ui/file?path=a.pdf",
            Utc::now() - Duration::seconds(1),
        );
        assert_eq!(file_path(&signer, &old), None);
    }
}
