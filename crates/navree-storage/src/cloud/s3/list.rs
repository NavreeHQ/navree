//! `ListObjectsV2` pagination (`GET ?list-type=2`) for [`super::S3Store`].

use std::collections::HashSet;

use ::http::Method;
use navree_datetime::Timestamp;

use super::{S3Store, enrich_error, sigv4};
use crate::cloud::http::Request;
use crate::cloud::keys::{check_key, check_prefix, encode_component};
use crate::cloud::retry::{Replay, with_backoff};
use crate::cloud::{Error, ErrorKind, MAX_LIST_ENTRIES, MAX_LIST_PAGES, Result, xml};
use crate::store::Meta;

/// Lists every object under the directory `prefix` (`""` lists the whole
/// bucket), following `NextContinuationToken` across pages.
pub(super) async fn list_objects(store: &S3Store, prefix: &str) -> Result<Vec<Meta>> {
    check_prefix(prefix)?;
    // A non-empty prefix names a directory (`docs` lists `docs/...`, never
    // `docs2/...` or `docs.txt`).
    let directory = if prefix.is_empty() {
        None
    } else if prefix.ends_with('/') {
        Some(prefix.to_owned())
    } else {
        Some(format!("{prefix}/"))
    };

    let mut files = Vec::new();
    let mut continuation_token: Option<String> = None;
    let mut seen_tokens = HashSet::new();
    let mut pages = 0usize;

    loop {
        pages += 1;
        if pages > MAX_LIST_PAGES {
            return Err(Error::new(
                ErrorKind::Answer,
                "an S3 listing exceeded the maximum page limit",
            ));
        }

        let mut query = String::from("list-type=2");
        if let Some(token) = &continuation_token {
            query.push_str("&continuation-token=");
            query.push_str(&encode_component(token));
        }
        if let Some(dir) = &directory {
            query.push_str("&prefix=");
            query.push_str(&encode_component(dir));
        }
        let mut url = store.bucket_url.clone();
        url.set_query(Some(&query));

        let answer = with_backoff(&store.retry, Replay::Safe, || {
            let url = url.clone();
            async move {
                let credential = store.credentials.credential().await?;
                let mut request = Request::new(Method::GET, url);
                sigv4::sign_request(&mut request, &credential.secret, &store.region, store.now())?;
                store
                    .client
                    .send(request)
                    .await?
                    .success()
                    .await
                    .map_err(enrich_error)
            }
        })
        .await?;

        let body = answer.bytes_up_to(xml::Limits::default().max_bytes).await?;
        let root = xml::parse(&body)?;

        for item in root.all("Contents") {
            let key = item.value("Key").ok_or_else(|| {
                Error::new(
                    ErrorKind::Answer,
                    "a listed S3 object is missing its Key element",
                )
            })?;
            check_key(key)?;
            if let Some(ref dir) = directory
                && (!key.starts_with(dir.as_str()) || key.len() <= dir.len())
            {
                continue;
            }
            if files.len() >= MAX_LIST_ENTRIES {
                return Err(Error::new(
                    ErrorKind::Answer,
                    "an S3 listing exceeded the maximum object count",
                ));
            }
            let size_text = item.value("Size").ok_or_else(|| {
                Error::new(
                    ErrorKind::Answer,
                    "a listed S3 object is missing its Size element",
                )
            })?;
            let size = size_text.trim().parse::<u64>().map_err(|_| {
                Error::new(ErrorKind::Answer, "a listed S3 object has an invalid Size")
            })?;
            let last_modified = match item.value("LastModified").map(str::trim) {
                Some(text) if !text.is_empty() => Timestamp::parse_rfc3339(text)
                    .map_err(|_| {
                        Error::new(
                            ErrorKind::Answer,
                            "a listed S3 object has an invalid LastModified timestamp",
                        )
                    })?,
                _ => Timestamp::UNIX_EPOCH,
            };
            files.push(Meta {
                path: key.to_owned(),
                size,
                last_modified,
            });
        }

        let truncated = root
            .value("IsTruncated")
            .is_some_and(|text| text.trim().eq_ignore_ascii_case("true"));
        if !truncated {
            break;
        }
        let next = root
            .value("NextContinuationToken")
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Answer,
                    "a truncated S3 listing has no NextContinuationToken",
                )
            })?;
        if !seen_tokens.insert(next.to_owned()) {
            return Err(Error::new(
                ErrorKind::Answer,
                "an S3 listing repeated its continuation token",
            ));
        }
        continuation_token = Some(next.to_owned());
    }

    Ok(files)
}
