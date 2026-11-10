//! What the page hands out signed: the post of a component's button, and the page's
//! context. Both are URLs the app's [`UrlSigner`] signed (a key derived from the app
//! key), so a model, a page of another site or another reader can't make one, change
//! one, or use one past its time.
//!
//! A button's URL names the reader, the conversation, the component's item, the
//! action, its input and a nonce used once: the server takes nothing about the button
//! from the post but the URL it signed, checks every part again, and records the nonce
//! in the action's own transaction.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use http::Uri;
use navree_http::{AppError, UrlSigner};
use serde_json::Value;
use url::form_urlencoded;
use uuid::Uuid;

/// How long a button's signature lasts: a page left open overnight asks again.
const BUTTON_HOURS: i64 = 12;

/// How long a page's context lasts.
const CONTEXT_HOURS: i64 = 12;

/// The most bytes of a button's input, as JSON.
pub(crate) const MAX_INPUT: usize = 1024;

/// A button's post, as the server signed it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ActionClaim {
    pub(crate) conversation: Uuid,
    pub(crate) item: Uuid,
    pub(crate) action: String,
    pub(crate) label: String,
    pub(crate) user: Uuid,
    pub(crate) nonce: String,
    pub(crate) input: Value,
}

/// `text`, encoded for a query.
fn enc(text: &str) -> String {
    form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

/// A fresh nonce.
pub(crate) fn nonce() -> String {
    Uuid::new_v4().simple().to_string()
}

/// The signed URL of a button's post under `base`, or `None` when its input is too big.
pub(crate) fn action_url(
    signer: &UrlSigner,
    base: &str,
    claim: &ActionClaim,
    now: DateTime<Utc>,
) -> Option<String> {
    let input = claim.input.to_string();
    if input.len() > MAX_INPUT {
        return None;
    }
    let label: String = claim.label.chars().take(80).collect();
    let path = format!(
        "{base}/act?c={}&i={}&a={}&l={}&u={}&n={}&in={}",
        claim.conversation,
        claim.item,
        enc(&claim.action),
        enc(&label),
        claim.user,
        enc(&claim.nonce),
        enc(&input),
    );
    Some(signer.sign(&path, now + Duration::hours(BUTTON_HOURS)))
}

fn refused() -> AppError {
    AppError::forbidden()
        .with_detail("This button has expired or was changed. Ask the assistant again.")
}

/// The fields of a signed query, refusing one that names a field twice.
fn fields(uri: &Uri) -> Result<BTreeMap<String, String>, AppError> {
    let mut fields = BTreeMap::new();
    for (key, value) in form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes()) {
        if fields
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            return Err(refused());
        }
    }
    Ok(fields)
}

/// Reads a button's post from the URL it was sent to: refused unless the server signed
/// exactly this URL under `base`, for now.
pub(crate) fn read_action(
    signer: &UrlSigner,
    base: &str,
    uri: &Uri,
    now: DateTime<Utc>,
) -> Result<ActionClaim, AppError> {
    if uri.path() != format!("{base}/act") || !signer.verify(uri, now) {
        return Err(refused());
    }
    let fields = fields(uri)?;
    let get = |name: &str| fields.get(name).cloned().ok_or_else(refused);
    let uuid = |name: &str| get(name).and_then(|text| text.parse::<Uuid>().map_err(|_| refused()));
    let input: Value = serde_json::from_str(&get("in")?).map_err(|_| refused())?;
    Ok(ActionClaim {
        conversation: uuid("c")?,
        item: uuid("i")?,
        action: get("a")?,
        label: get("l")?,
        user: uuid("u")?,
        nonce: get("n")?,
        input,
    })
}

/// What a page is about, as the server signed it for one reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContextClaim {
    pub(crate) kind: String,
    pub(crate) key: String,
}

/// The signed reference to record `key` of `kind`, for `user`, under `base`.
pub(crate) fn context_ref(
    signer: &UrlSigner,
    base: &str,
    user: Uuid,
    kind: &str,
    key: &str,
    now: DateTime<Utc>,
) -> String {
    signer.sign(
        &format!("{base}/context?k={}&id={}&u={user}", enc(kind), enc(key)),
        now + Duration::hours(CONTEXT_HOURS),
    )
}

/// Reads a page's context the composer sent: refused unless the server signed it under
/// `base` for this `user`, and it is still in its time.
pub(crate) fn read_context(
    signer: &UrlSigner,
    base: &str,
    text: &str,
    user: Uuid,
    now: DateTime<Utc>,
) -> Result<ContextClaim, AppError> {
    let wrong = || {
        AppError::forbidden()
            .with_detail("The page's context was changed or has expired. Open the page again.")
    };
    let uri: Uri = text.parse().map_err(|_| wrong())?;
    if uri.path() != format!("{base}/context") || !signer.verify(&uri, now) {
        return Err(wrong());
    }
    let fields = fields(&uri).map_err(|_| wrong())?;
    if fields.get("u").and_then(|u| u.parse::<Uuid>().ok()) != Some(user) {
        return Err(wrong());
    }
    match (fields.get("k"), fields.get("id")) {
        (Some(kind), Some(key)) => Ok(ContextClaim {
            kind: kind.clone(),
            key: key.clone(),
        }),
        _ => Err(wrong()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn claim() -> ActionClaim {
        ActionClaim {
            conversation: Uuid::now_v7(),
            item: Uuid::now_v7(),
            action: "refund_order".into(),
            label: "Refund ₹1,250 & more".into(),
            user: Uuid::now_v7(),
            nonce: nonce(),
            input: json!({ "order": "CS-1042" }),
        }
    }

    #[test]
    fn a_signed_button_reads_back_whole() {
        let signer = UrlSigner::new(b"test key");
        let now = Utc::now();
        let claim = claim();
        let url = action_url(&signer, "/assistant", &claim, now).unwrap();
        let read = read_action(&signer, "/assistant", &url.parse().unwrap(), now).unwrap();
        assert_eq!(read, claim);
    }

    #[test]
    fn a_changed_moved_or_old_button_is_refused() {
        let signer = UrlSigner::new(b"test key");
        let now = Utc::now();
        let claim = claim();
        let url = action_url(&signer, "/assistant", &claim, now).unwrap();
        let other = Uuid::now_v7().to_string();
        for forged in [
            url.replace(&claim.user.to_string(), &other),
            url.replace(&claim.item.to_string(), &other),
            url.replace(&claim.conversation.to_string(), &other),
            url.replace("refund_order", "delete_order"),
            url.replace("CS-1042", "CS-1043"),
            url.replacen("/assistant/act", "/admin/act", 1),
            format!("{url}&a=delete_order"),
        ] {
            let uri: Uri = forged.parse().unwrap();
            assert!(
                read_action(&signer, "/assistant", &uri, now).is_err(),
                "{forged}"
            );
        }
        let later = now + Duration::hours(BUTTON_HOURS) + Duration::seconds(1);
        assert!(read_action(&signer, "/assistant", &url.parse().unwrap(), later).is_err());
        let stranger = UrlSigner::new(b"another key");
        assert!(read_action(&stranger, "/assistant", &url.parse().unwrap(), now).is_err());
        let big = ActionClaim {
            input: json!({ "order": "x".repeat(2000) }),
            ..claim
        };
        assert!(action_url(&signer, "/assistant", &big, now).is_none());
    }

    #[test]
    fn a_context_is_one_readers_and_unchanged() {
        let signer = UrlSigner::new(b"test key");
        let now = Utc::now();
        let user = Uuid::now_v7();
        let signed = context_ref(&signer, "/assistant", user, "order", "1042", now);
        assert_eq!(
            read_context(&signer, "/assistant", &signed, user, now).unwrap(),
            ContextClaim {
                kind: "order".into(),
                key: "1042".into()
            }
        );
        assert!(read_context(&signer, "/assistant", &signed, Uuid::now_v7(), now).is_err());
        assert!(
            read_context(
                &signer,
                "/assistant",
                &signed.replace("1042", "1043"),
                user,
                now
            )
            .is_err()
        );
        assert!(
            read_context(
                &signer,
                "/assistant",
                "/assistant/context?k=order&id=1&u=x",
                user,
                now
            )
            .is_err()
        );
        assert!(read_context(&signer, "/assistant", "not a url at all", user, now).is_err());
    }
}
