//! Properties over generated forms and over bytes shaped like forms.

use http::StatusCode;
use navree_http::extract::{FromRequest, Multipart, MultipartError, MultipartLimits};
use proptest::prelude::*;

use crate::support::*;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}

/// `body` split at `cuts`.
fn split(body: &[u8], cuts: &[usize]) -> Vec<Vec<u8>> {
    let mut at: Vec<usize> = cuts.iter().map(|cut| cut % (body.len() + 1)).collect();
    at.sort_unstable();
    at.dedup();
    let mut chunks = Vec::new();
    let mut from = 0;
    for cut in at {
        chunks.push(body[from..cut].to_vec());
        from = cut;
    }
    chunks.push(body[from..].to_vec());
    chunks
}

/// The whole of a part: what limits do not depend on.
fn parts() -> impl Strategy<Value = Vec<(String, Option<String>, Vec<u8>)>> {
    prop::collection::vec(
        (
            "[a-z]{1,8}",
            prop::option::of("[a-zA-Z0-9._/-]{1,16}"),
            // Bytes a delimiter is made of, so near-misses are common.
            prop::collection::vec(prop::sample::select(&b"\r\n-navre\x00x"[..]), 0..64),
        ),
        0..6,
    )
}

fn built(parts: &[(String, Option<String>, Vec<u8>)]) -> Vec<u8> {
    let parts: Vec<_> = parts
        .iter()
        .map(|(name, file_name, value)| match file_name {
            Some(file_name) => file_part(name, file_name, value),
            None => text_part(name, value),
        })
        .collect();
    form(&parts)
}

/// Bytes that look enough like a form to reach every stage of the parser.
fn shaped() -> impl Strategy<Value = Vec<u8>> {
    let pieces = prop::collection::vec(
        prop_oneof![
            Just(format!("--{BOUNDARY}").into_bytes()),
            Just(format!("\r\n--{BOUNDARY}").into_bytes()),
            Just(format!("--{BOUNDARY}--").into_bytes()),
            Just(b"\r\n".to_vec()),
            Just(b"\r\n\r\n".to_vec()),
            Just(b" \t".to_vec()),
            Just(b"-".to_vec()),
            Just(b"Content-Disposition: form-data; name=\"a\"".to_vec()),
            Just(b"Content-Disposition: form-data; name=\"a\"; filename=\"../b\"".to_vec()),
            Just(b"Content-Type: text/plain; charset=utf-16".to_vec()),
            Just(
                b"Content-Disposition: form-data; NAME=a; filename*=UTF-8''%C3%A9%2F.txt".to_vec()
            ),
            Just(b"; name=\"b\"; filename=\"c\\\"d\"".to_vec()),
            Just(b": no name".to_vec()),
            prop::collection::vec(any::<u8>(), 0..32),
        ],
        0..24,
    );
    pieces.prop_map(|pieces| pieces.concat())
}

/// What a read comes to: a form, or that it failed.
fn outcome(read: Result<Vec<Read>, MultipartError>) -> Option<Vec<Read>> {
    read.ok()
}

/// The form read whole by the reference, as a handler would have read it.
fn reference(body: &[u8]) -> Option<Vec<Read>> {
    let parts = crate::reference::parts(body, BOUNDARY)?;
    Some(
        parts
            .into_iter()
            .map(|(name, file_name, bytes)| Read {
                name,
                file_name,
                bytes,
            })
            .collect(),
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Every form reads back as it was built, however it arrives.
    #[test]
    fn forms_read_back_however_they_are_split(
        parts in parts(),
        cuts in prop::collection::vec(any::<usize>(), 0..16),
    ) {
        let body = built(&parts);
        let read = runtime()
            .block_on(read(request_of(&form_type(), split(&body, &cuts)), wide()))
            .unwrap();
        prop_assert_eq!(read.len(), parts.len());
        for (read, (name, file_name, value)) in read.iter().zip(&parts) {
            prop_assert_eq!(read.name.as_deref(), Some(name.as_str()));
            prop_assert_eq!(&read.file_name, file_name);
            prop_assert_eq!(&read.bytes, value);
        }
    }

    /// Whether a body is accepted, and what it holds, does not depend on how
    /// it was split, with limits or without, on bytes that are mostly not a
    /// form; and every failure is the client's.
    #[test]
    fn the_outcome_does_not_depend_on_the_chunks(
        body in shaped(),
        cuts in prop::collection::vec(any::<usize>(), 0..16),
        max_header_bytes in prop_oneof![Just(usize::MAX), 8..256_usize],
    ) {
        let limits = MultipartLimits { max_header_bytes, ..wide() };
        let runtime = runtime();
        let whole = runtime.block_on(read(request(&body), limits));
        let chunked = runtime.block_on(read(request_of(&form_type(), split(&body, &cuts)), limits));
        let dripped = runtime.block_on(read(dripped(&body), limits));
        for read in [&whole, &chunked, &dripped] {
            if let Err(error) = read {
                prop_assert!(error.status().is_client_error(), "{:?}", error);
            }
        }
        let whole = outcome(whole);
        prop_assert_eq!(&whole, &outcome(chunked));
        prop_assert_eq!(&whole, &outcome(dripped));
    }

    /// With limits that cannot apply, the extractor reads what the grammar
    /// reads: the reference, reading the body whole, finds the same form.
    #[test]
    fn with_no_limit_reached_the_parser_decides(
        body in shaped(),
        cuts in prop::collection::vec(any::<usize>(), 0..16),
    ) {
        let runtime = runtime();
        let read = runtime.block_on(read(request_of(&form_type(), split(&body, &cuts)), wide()));
        prop_assert_eq!(outcome(read), reference(&body));
    }

    /// The head limit is exact for any head: the same form passes at its
    /// head's length and fails one below it.
    #[test]
    fn the_head_limit_is_exact(
        value in "[a-z]{0,400}",
        cuts in prop::collection::vec(any::<usize>(), 0..8),
    ) {
        let headers = format!("Content-Disposition: form-data; name=\"a\"\r\nX-Value: {value}");
        let length = headers.len() + "\r\n\r\n".len();
        let mut part = head(&headers);
        part.extend_from_slice(b"1\r\n");
        let body = form(&[part]);
        let runtime = runtime();
        let at = |max_header_bytes: usize| {
            let limits = MultipartLimits { max_header_bytes, ..wide() };
            runtime.block_on(read(request_of(&form_type(), split(&body, &cuts)), limits))
        };
        prop_assert!(at(length).is_ok());
        prop_assert_eq!(
            at(length - 1).unwrap_err(),
            MultipartError::HeadTooLarge { limit: length - 1 }
        );
    }

    /// No Content-Type a client can send gets past the 400.
    #[test]
    fn any_content_type_is_a_form_or_a_400(content_type in "[ -~]{0,80}") {
        let request = request_of(&content_type, vec![]);
        if let Err(rejection) = runtime().block_on(Multipart::from_request(request)) {
            prop_assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
        }
    }
}
