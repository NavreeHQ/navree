//! The decoders against references.
//!
//! - Percent-decoding gives the same bytes as `percent-encoding`.
//! - A parameter value that was percent-encoded decodes back to itself,
//!   unless it holds what parameters may not (then the problem is named).
//! - The dot-segment check refuses exactly the segments that decode to `.`
//!   or `..` with `%2e` as the only escape.
//! - The urlencoded parser gives the same pairs as `form_urlencoded` for any
//!   input, and the deserializer the same values and messages as
//!   `serde_urlencoded`.

use std::collections::{BTreeMap, HashMap};

use navree_http::extract::urlencoded;
use navree_http::routing::{ParamProblem, PathProblem, check_path, decode_param, percent_decode};
use proptest::prelude::*;
use serde::Deserialize;

/// Bytes rich in `%`, hex digits, `+`, `&`, `=` and UTF-8.
fn encodedish() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        any::<Vec<u8>>(),
        prop::collection::vec(
            prop::sample::select(b"%%%2eEfF0aZ+&=/.\\\xc3\xa9\xff ".to_vec()),
            0..24
        ),
    ]
}

fn encode(text: &str) -> String {
    text.bytes().map(|byte| format!("%{byte:02X}")).collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn percent_decoding_matches_the_reference(input in encodedish()) {
        let ours = percent_decode(&input).into_owned();
        let reference: Vec<u8> = percent_encoding::percent_decode(&input).collect();
        prop_assert_eq!(ours, reference);
    }

    #[test]
    fn encoded_parameters_decode_back(value in any::<String>(), catch_all in any::<bool>()) {
        let encoded = encode(&value);
        let decoded = decode_param(&encoded, catch_all);
        let expected = if value.contains('\0') {
            Err(ParamProblem::Nul)
        } else if value.contains('\\') {
            Err(ParamProblem::Backslash)
        } else if !catch_all && value.contains('/') {
            Err(ParamProblem::EncodedSlash)
        } else if (catch_all && value.split('/').any(|part| part == "." || part == ".."))
            || (!catch_all && (value == "." || value == ".."))
        {
            Err(ParamProblem::DotSegment)
        } else {
            Ok(value.clone())
        };
        prop_assert_eq!(decoded.map(|value| value.into_owned()), expected);
    }

    #[test]
    fn invalid_utf8_is_refused(bytes in any::<Vec<u8>>()) {
        let raw: String = bytes.iter().map(|byte| format!("%{byte:02x}")).collect();
        let decoded = decode_param(&raw, true);
        if std::str::from_utf8(&bytes).is_err() {
            prop_assert_eq!(decoded, Err(ParamProblem::InvalidUtf8));
        }
    }

    #[test]
    fn dot_segments_are_found_like_the_reference(segments in prop::collection::vec("(\\.|%2e|%2E|a|%2f|%25){0,4}", 1..5)) {
        let path = format!("/{}", segments.join("/"));
        let reference = segments.iter().any(|segment| {
            let dots = segment.replace("%2e", ".").replace("%2E", ".");
            dots == "." || dots == ".."
        });
        let ours = check_path(&path) == Err(PathProblem::DotSegment);
        prop_assert_eq!(ours, reference, "path {:?}", path);
    }

    #[test]
    fn urlencoded_pairs_match_the_reference(input in encodedish()) {
        let ours: Vec<(String, String)> = urlencoded::parse(&input)
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let reference: Vec<(String, String)> = form_urlencoded::parse(&input)
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        prop_assert_eq!(ours, reference);
    }

    #[test]
    fn urlencoded_values_match_the_reference(input in query()) {
        same::<HashMap<String, String>>(&input)?;
        same::<BTreeMap<String, i64>>(&input)?;
        same::<Vec<(String, String)>>(&input)?;
        same::<Filter>(&input)?;
        same::<Flags>(&input)?;
    }
}

/// Query strings over a few names and typed values.
fn query() -> impl Strategy<Value = Vec<u8>> {
    let name = prop::sample::select(vec!["q", "page", "active", "sort", "ratio", "id", "x", ""]);
    let value = prop::sample::select(vec![
        "",
        "1",
        "-3",
        "42",
        "1.5",
        "true",
        "false",
        "asc",
        "desc",
        "a+b",
        "%20",
        "%FF",
        "é",
        "9999999999999999999999",
    ]);
    prop::collection::vec((name, value, any::<bool>()), 0..6).prop_map(|pairs| {
        pairs
            .into_iter()
            .map(|(name, value, with_equals)| {
                if with_equals {
                    format!("{name}={value}")
                } else {
                    name.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("&")
            .into_bytes()
    })
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Sort {
    Asc,
    Desc,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Filter {
    q: Option<String>,
    page: Option<u32>,
    active: Option<bool>,
    sort: Option<Sort>,
    ratio: Option<f64>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Flags {
    #[serde(default)]
    active: bool,
    id: Option<i8>,
}

/// The same value, or errors with the same message.
fn same<T>(input: &[u8]) -> Result<(), TestCaseError>
where
    T: for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let ours = urlencoded::from_bytes::<T>(input).map_err(|error| error.to_string());
    let reference = serde_urlencoded::from_bytes::<T>(input).map_err(|error| error.to_string());
    prop_assert_eq!(
        ours,
        reference,
        "{} from {:?}",
        std::any::type_name::<T>(),
        String::from_utf8_lossy(input)
    );
    Ok(())
}

#[test]
fn encoded_slashes_are_data_only_in_catch_alls() {
    assert_eq!(
        decode_param("a%2Fb", false),
        Err(ParamProblem::EncodedSlash)
    );
    assert_eq!(decode_param("a%2Fb", true).unwrap(), "a/b");
}
