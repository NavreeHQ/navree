//! Fetching and caching Azure user delegation keys (`POST
//! ?restype=service&comp=userdelegationkey`) for user delegation SAS links.

use std::time::{Duration, SystemTime};

use ::http::Method;
use navree_datetime::{Precision, Timestamp};
use tokio::sync::RwLock;

use super::AzureInner;
use crate::azure_sas::UserDelegationKey;
use crate::cloud::http::Request;
use crate::cloud::retry::{Replay, with_backoff};
use crate::cloud::{Error, ErrorKind, Result, xml};

/// Azure allows at most seven days between a delegation key's start and
/// expiry, and a SAS signed with it cannot outlive the key.
pub(crate) const MAX_DELEGATION_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Default lifetime asked for a cached delegation key (12 hours), so short
/// SAS links reuse one key without calling `GetUserDelegationKey` per link.
const DEFAULT_KEY_VALIDITY: Duration = Duration::from_secs(12 * 60 * 60);

/// Margin required beyond the requested SAS expiry when reusing a cached
/// delegation key.
const REUSE_MARGIN: Duration = Duration::from_secs(5 * 60);

/// How long a newly fetched delegation key stays reusable even when Azure
/// clamped its expiry inside [`REUSE_MARGIN`] (as long as it still covers
/// the requested SAS expiry).
const REFETCH_PAUSE: Duration = Duration::from_millis(100);

const DELEGATION_XML_LIMIT: usize = 64 * 1024;

struct CachedKey {
    key: UserDelegationKey,
    starts_at: Timestamp,
    expires_at: Timestamp,
    fetched_at: SystemTime,
}

/// Cache of [`UserDelegationKey`]s for one [`AzureStore`](super::AzureStore).
#[derive(Default)]
pub(crate) struct DelegationKeys {
    kept: RwLock<Option<CachedKey>>,
}

impl DelegationKeys {
    /// Returns a delegation key valid across `[effective_start, sas_expiry]`,
    /// reusing the cached key when it covers the window and fetching a new one
    /// under a write lock otherwise.
    pub(crate) async fn key_for(
        &self,
        inner: &AzureInner,
        start: Timestamp,
        sas_expiry: Timestamp,
        expires_in: Duration,
    ) -> Result<(UserDelegationKey, Timestamp)> {
        let now = (inner.clock)();
        if let Some(found) = usable(self.kept.read().await.as_ref(), now, start, sas_expiry) {
            return Ok(found);
        }
        let mut kept = self.kept.write().await;
        if let Some(found) = usable(kept.as_ref(), now, start, sas_expiry) {
            return Ok(found);
        }

        let validity = expires_in.max(DEFAULT_KEY_VALIDITY).min(MAX_DELEGATION_TTL);
        let requested_expiry = start
            .checked_add(
                navree_datetime::Span::try_from(validity).unwrap_or(navree_datetime::Span::days(7)),
            )
            .unwrap_or(sas_expiry);

        let body_xml = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <KeyInfo><Start>{}</Start><Expiry>{}</Expiry></KeyInfo>",
            start.to_rfc3339_with(Precision::Secs, true),
            requested_expiry.to_rfc3339_with(Precision::Secs, true),
        );
        let mut url = inner.service_url.clone();
        url.set_query(Some("restype=service&comp=userdelegationkey"));

        let answer = with_backoff(&inner.policy, Replay::Safe, || async {
            let cred = inner.credentials.credential().await?;
            let request = Request::new(Method::POST, url.clone())
                .header("content-type", "application/xml")?
                .with_body(body_xml.clone());
            let request = inner.authorize(request, &cred.secret)?;
            let response = inner.send_checked(request).await?;
            response.bytes_up_to(DELEGATION_XML_LIMIT).await
        })
        .await?;

        let (key, starts_at, expires_at) = parse_delegation_key(&answer)?;
        let effective_start = start.max(starts_at);
        if effective_start >= sas_expiry || expires_at < sas_expiry {
            return Err(Error::new(
                ErrorKind::Answer,
                "the user delegation key does not cover the requested SAS window",
            ));
        }
        *kept = Some(CachedKey {
            key: key.clone(),
            starts_at,
            expires_at,
            fetched_at: (inner.clock)(),
        });
        Ok((key, effective_start))
    }
}

fn usable(
    cached: Option<&CachedKey>,
    now: SystemTime,
    start: Timestamp,
    sas_expiry: Timestamp,
) -> Option<(UserDelegationKey, Timestamp)> {
    let cached = cached?;
    let effective_start = start.max(cached.starts_at);
    if effective_start >= sas_expiry || cached.expires_at < sas_expiry {
        return None;
    }
    let margin = navree_datetime::Span::try_from(REUSE_MARGIN).unwrap_or_default();
    let plenty_left = cached.expires_at.duration_since(sas_expiry) >= margin;
    let just_fetched = now
        .duration_since(cached.fetched_at)
        .is_ok_and(|age| age < REFETCH_PAUSE);
    (plenty_left || just_fetched).then(|| (cached.key.clone(), effective_start))
}

fn parse_delegation_key(body: &[u8]) -> Result<(UserDelegationKey, Timestamp, Timestamp)> {
    let root = xml::parse(body)?;
    if root.name() != "UserDelegationKey" {
        return Err(Error::new(
            ErrorKind::Answer,
            "the user delegation key answer does not have a UserDelegationKey root",
        ));
    }
    let field = |name: &'static str| -> Result<&str> {
        let value = root
            .value(name)
            .map(str::trim_ascii)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Answer,
                    format!("the user delegation key answer is missing `{name}`"),
                )
            })?;
        if value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
        {
            return Err(Error::new(
                ErrorKind::Answer,
                format!("the user delegation key field `{name}` contains invalid characters"),
            ));
        }
        Ok(value)
    };

    let signed_oid = field("SignedOid")?;
    let signed_tid = field("SignedTid")?;
    let signed_start = field("SignedStart")?;
    let signed_expiry = field("SignedExpiry")?;
    let signed_service = field("SignedService")?;
    let signed_version = field("SignedVersion")?;
    let value = root
        .value("Value")
        .map(str::trim_ascii)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Answer,
                "the user delegation key answer is missing `Value`",
            )
        })?;

    let starts_at = Timestamp::parse_rfc3339(signed_start).map_err(|_| {
        Error::new(
            ErrorKind::Answer,
            "the user delegation key has an invalid SignedStart timestamp",
        )
    })?;
    let expires_at = Timestamp::parse_rfc3339(signed_expiry).map_err(|_| {
        Error::new(
            ErrorKind::Answer,
            "the user delegation key has an invalid SignedExpiry timestamp",
        )
    })?;
    if starts_at >= expires_at {
        return Err(Error::new(
            ErrorKind::Answer,
            "the user delegation key expires before it starts",
        ));
    }

    let key = UserDelegationKey::new(
        signed_oid,
        signed_tid,
        signed_start,
        signed_expiry,
        signed_service,
        signed_version,
        value,
    )
    .map_err(|_| {
        Error::new(
            ErrorKind::Answer,
            "the user delegation key value is not valid base64",
        )
    })?;

    Ok((key, starts_at, expires_at))
}
