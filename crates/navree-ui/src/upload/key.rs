//! The key of a stored file: what a form's file field posts in place of the file once the
//! upload module has sent it.
//!
//! A key is the file's path on its disk, signed with the app's [`UrlSigner`] and good for a
//! day, so a browser can hand back a path the app signed and no other: a path it makes up, or
//! one changed by a character, reads as no key at all.
//!
//! A key made with [`file_key_for`] also says who it was made for and which folder of the disk
//! its file is in. A form's file field takes only that kind ([`file_path_for`]): a key is read
//! back by the person it was made for, for a file in the folder of the field's own upload, so a
//! key held by someone else, or one made for another field's folder, is no key here.
//! [`file_key`] names the path alone, for an app that reads it back itself.

use std::convert::Infallible;

use chrono::{Duration, Utc};
use http::request::Parts;
use navree_http::extract::{FromRequestParts, urlencoded};
use navree_http::{CurrentPrincipal, UrlSigner};
use sha2::{Digest, Sha256};

/// The address a key signs. It is never served: it only carries the path.
const KEY_PATH: &str = "/_navree/ui/file";
/// The address a key made for a person signs: another one, so that neither kind reads as the
/// other.
const OWNED_KEY_PATH: &str = "/_navree/ui/file/for";
/// How long a key may be posted after its file was stored.
const KEY_LIFETIME_HOURS: i64 = 24;

/// The key of the stored file at `path`, good for a day. It names the path and nothing else:
/// whoever holds it can post it. A form's file field reads only [`file_key_for`]'s keys.
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

/// The key of the stored file at `path`, made for `owner` and good for a day. It carries who it
/// was made for and the folder the file is in, so [`file_path_for`] gives the path back only to
/// the same owner, for the same folder.
///
/// `owner` is one person, as it is for the parts of a chunked upload: for a signed-in person
/// their id as text ([`UploadOwner`] says it, here and in a form), and for a guest nothing at
/// all, since a guest has no identity of its own to bind a key to.
///
/// ```
/// use navree_http::UrlSigner;
/// use navree_ui::upload::{file_key_for, file_path_for};
///
/// let signer = UrlSigner::new(b"a long random key");
/// let key = file_key_for(&signer, "user-7", "avatars/k3v9q.png");
/// let path = file_path_for(&signer, "user-7", "avatars", &key);
/// assert_eq!(path.as_deref(), Some("avatars/k3v9q.png"));
/// // Another person, or another folder, reads no file from it.
/// assert_eq!(file_path_for(&signer, "user-8", "avatars", &key), None);
/// assert_eq!(file_path_for(&signer, "user-7", "receipts", &key), None);
/// ```
pub fn file_key_for(signer: &UrlSigner, owner: &str, path: &str) -> String {
    signer.sign(
        &format!(
            "{OWNED_KEY_PATH}?path={}&for={}&dir={}",
            encode(path),
            owner_key(owner),
            encode(folder_of(path)),
        ),
        Utc::now() + Duration::hours(KEY_LIFETIME_HOURS),
    )
}

/// The path `key` names, when `signer` signed it for `owner` and for `folder`, the file is
/// directly in that folder, and it has not expired. A key from [`file_key`] is not one of these,
/// nor is a key made for another owner or another folder.
pub fn file_path_for(signer: &UrlSigner, owner: &str, folder: &str, key: &str) -> Option<String> {
    let uri: http::Uri = key.trim().parse().ok()?;
    if uri.path() != OWNED_KEY_PATH || !signer.verify(&uri, Utc::now()) {
        return None;
    }
    let (mut path, mut made_for, mut dir) = (None, None, None);
    for (name, value) in urlencoded::parse(uri.query()?.as_bytes()) {
        let slot = match &*name {
            "path" => &mut path,
            "for" => &mut made_for,
            "dir" => &mut dir,
            _ => continue,
        };
        // A name given twice is no key.
        if slot.replace(value.into_owned()).is_some() {
            return None;
        }
    }
    let (path, made_for, dir): (String, String, String) = (path?, made_for?, dir?);
    let folder = folder.trim_matches('/');
    (made_for == owner_key(owner) && dir == folder && is_file_in(folder, &path)).then_some(path)
}

/// Who a file key is made for: the signed-in person's id as text, and nothing for a guest. A
/// form's file field reads a posted key as this owner, so the route that stores the file and
/// answers with its key takes the same one:
/// [`Received::into_keyed_response_for`](crate::Received::into_keyed_response_for).
///
/// It is an extractor of the request's head, which always succeeds. A guest's owner is empty,
/// so a key made for a guest is read back by any guest: bind a guest's upload to something of
/// its own and the key is only as private as that.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UploadOwner(String);

impl UploadOwner {
    /// The owner of the request whose head is `parts`.
    pub fn of(parts: &Parts) -> Self {
        Self(
            CurrentPrincipal::from_parts(parts)
                .map(|principal| principal.0.user_id.to_string())
                .unwrap_or_default(),
        )
    }

    /// The owner as the text a key is made with.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromRequestParts for UploadOwner {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(Self::of(parts))
    }
}

/// What a key says an owner is: 32 hexadecimal digits of the SHA-256 of `owner`, as the parts of
/// a chunked upload are kept under its owner, so a key never carries a person's id.
pub(crate) fn owner_key(owner: &str) -> String {
    Sha256::digest(owner.as_bytes())[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The folder a stored file's path is in: what comes before its last `/`, nothing for a file at
/// the root of its disk.
fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// Whether `path` is a file directly in `folder`, a name of its own and not a way up or down.
pub(crate) fn is_file_in(folder: &str, path: &str) -> bool {
    if folder_of(path) != folder.trim_matches('/') {
        return false;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('\\')
        && !name.chars().any(char::is_control)
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

    #[test]
    fn a_key_made_for_a_person_is_read_by_that_person_for_that_folder() {
        let signer = signer();
        for (owner, path) in [
            ("0191a2c4-7b3e-7a52-9c1d-5e8f3a2b4c6d", "receipts/aB3x.pdf"),
            ("", "receipts/ç%&=.png"),
        ] {
            let key = file_key_for(&signer, owner, path);
            assert_eq!(
                file_path_for(&signer, owner, "receipts", &key).as_deref(),
                Some(path),
                "{key}"
            );
            // A folder is the same with or without its slashes.
            assert_eq!(
                file_path_for(&signer, owner, "/receipts/", &key).as_deref(),
                Some(path)
            );
        }
        let at_root = file_key_for(&signer, "u", "aB3x.pdf");
        assert_eq!(
            file_path_for(&signer, "u", "", &at_root).as_deref(),
            Some("aB3x.pdf")
        );
    }

    #[test]
    fn a_key_made_for_a_person_is_no_key_for_another_owner_or_folder() {
        let signer = signer();
        let key = file_key_for(&signer, "user-7", "receipts/aB3x.pdf");
        assert_eq!(file_path_for(&signer, "user-8", "receipts", &key), None);
        assert_eq!(file_path_for(&signer, "", "receipts", &key), None);
        assert_eq!(file_path_for(&signer, "user-7", "avatars", &key), None);
        assert_eq!(file_path_for(&signer, "user-7", "receipts/old", &key), None);
        assert_eq!(file_path_for(&signer, "user-7", "", &key), None);
        // The key does not carry the person's id.
        assert!(!key.contains("user-7"), "{key}");
    }

    #[test]
    fn the_two_kinds_of_key_do_not_read_as_each_other() {
        let signer = signer();
        let plain = file_key(&signer, "receipts/aB3x.pdf");
        assert_eq!(file_path_for(&signer, "", "receipts", &plain), None);
        let bound = file_key_for(&signer, "", "receipts/aB3x.pdf");
        assert_eq!(file_path(&signer, &bound), None);
    }

    #[test]
    fn a_changed_made_up_or_expired_key_for_a_person_reads_as_none() {
        let signer = signer();
        let key = file_key_for(&signer, "u", "receipts/aB3x.pdf");
        for changed in [
            key.replace("aB3x", "aB3y"),
            key.replace("receipts", "avatars"),
            key.replace(&owner_key("u"), &owner_key("v")),
        ] {
            assert_eq!(file_path_for(&signer, "u", "receipts", &changed), None);
        }
        let other = UrlSigner::new(b"another app's key");
        assert_eq!(file_path_for(&other, "u", "receipts", &key), None);
        let soon = Utc::now() + Duration::hours(1);
        let tag = owner_key("u");
        let sign = |query: &str, at| signer.sign(&format!("{OWNED_KEY_PATH}?{query}"), at);
        // A made-up key with every part, signed by the app, reads; each broken one does not.
        let whole = format!("path=receipts/a.pdf&for={tag}&dir=receipts");
        assert!(file_path_for(&signer, "u", "receipts", &sign(&whole, soon)).is_some());
        for broken in [
            "path=receipts/a.pdf&dir=receipts".to_owned(),
            format!("path=receipts/a.pdf&for={tag}"),
            format!("for={tag}&dir=receipts"),
            format!("{whole}&path=receipts/b.pdf"),
            format!("{whole}&for={tag}"),
            format!("path=receipts/a.pdf&for={tag}&dir=avatars"),
            format!("path=avatars/a.pdf&for={tag}&dir=receipts"),
        ] {
            assert_eq!(
                file_path_for(&signer, "u", "receipts", &sign(&broken, soon)),
                None,
                "{broken}"
            );
        }
        assert_eq!(
            file_path_for(
                &signer,
                "u",
                "receipts",
                &sign(&whole, Utc::now() - Duration::seconds(1))
            ),
            None
        );
        assert_eq!(
            file_path_for(&signer, "u", "receipts", "receipts/a.pdf"),
            None
        );
    }

    #[test]
    fn a_file_is_directly_in_its_folder() {
        assert!(is_file_in("receipts", "receipts/aB3x.pdf"));
        assert!(is_file_in("/receipts/", "receipts/aB3x.pdf"));
        assert!(is_file_in("a/b", "a/b/aB3x.pdf"));
        assert!(is_file_in("", "aB3x.pdf"));
        for (folder, path) in [
            ("receipts", "receipts"),
            ("receipts", "receipts/"),
            ("receipts", "receipts2/aB3x.pdf"),
            ("receipts", "receipts/_parts/u/id/00000000"),
            ("receipts", "receipts/sub/aB3x.pdf"),
            ("receipts", "receipts/.."),
            ("receipts", "receipts/."),
            ("receipts", "receipts/a\\b.pdf"),
            ("receipts", "receipts/a\nb.pdf"),
            ("receipts", "avatars/aB3x.pdf"),
            ("receipts/old", "receipts/aB3x.pdf"),
            ("", "receipts/aB3x.pdf"),
            ("", ""),
        ] {
            assert!(!is_file_in(folder, path), "{folder} {path}");
        }
    }
}
