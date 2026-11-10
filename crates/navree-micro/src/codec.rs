//! [`Codec`]: how payloads are encoded (`micro.codec`).

use std::fmt;

use bytes::Bytes;
use navree_kernel::BoxError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// How payloads are encoded: `json` (the default) or `msgpack`.
///
/// The receiving side decodes by the message's `content-type`, so services
/// with different codecs understand each other. MessagePack payloads are
/// written as maps with field names, so they decode into the same serde
/// types, and into `serde_json::Value`, like JSON.
///
/// ```
/// use navree_micro::Codec;
///
/// let bytes = Codec::Msgpack.encode(&serde_json::json!({ "id": 42 })).unwrap();
/// let back: serde_json::Value = Codec::Msgpack.decode(&bytes).unwrap();
/// assert_eq!(back["id"], 42);
/// assert_eq!(Codec::from_content_type("application/msgpack"), Some(Codec::Msgpack));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Codec {
    /// `application/json`: readable in every broker's tools.
    #[default]
    Json,
    /// `application/msgpack`: smaller and faster to encode.
    Msgpack,
}

impl Codec {
    /// The content type of payloads in this codec.
    pub fn content_type(self) -> &'static str {
        match self {
            Codec::Json => "application/json",
            Codec::Msgpack => "application/msgpack",
        }
    }

    /// The codec of a content type (parameters and case ignored):
    /// `application/json` (or `application/*+json`), `application/msgpack`
    /// or `application/x-msgpack`.
    pub fn from_content_type(content_type: &str) -> Option<Codec> {
        let essence = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        match essence.as_str() {
            "application/json" => Some(Codec::Json),
            "application/msgpack" | "application/x-msgpack" | "application/vnd.msgpack" => {
                Some(Codec::Msgpack)
            }
            other if other.starts_with("application/") && other.ends_with("+json") => {
                Some(Codec::Json)
            }
            _ => None,
        }
    }

    /// Encodes `value`.
    pub fn encode<T: Serialize + ?Sized>(self, value: &T) -> Result<Bytes, BoxError> {
        Ok(match self {
            Codec::Json => serde_json::to_vec(value)?.into(),
            Codec::Msgpack => rmp_serde::to_vec_named(value)?.into(),
        })
    }

    /// Decodes `bytes` into `T`.
    pub fn decode<T: DeserializeOwned>(self, bytes: &[u8]) -> Result<T, BoxError> {
        Ok(match self {
            Codec::Json => serde_json::from_slice(bytes)?,
            Codec::Msgpack => rmp_serde::from_slice(bytes)?,
        })
    }

    /// `json` or `msgpack`.
    pub fn as_str(self) -> &'static str {
        match self {
            Codec::Json => "json",
            Codec::Msgpack => "msgpack",
        }
    }
}

impl fmt::Display for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Deal {
        id: u64,
        title: String,
        #[serde(default)]
        owner: Option<String>,
    }

    #[test]
    fn both_codecs_round_trip_serde_types() {
        let deal = Deal {
            id: 7,
            title: "Agra".into(),
            owner: None,
        };
        for codec in [Codec::Json, Codec::Msgpack] {
            let bytes = codec.encode(&deal).unwrap();
            assert_eq!(codec.decode::<Deal>(&bytes).unwrap(), deal, "{codec}");
        }
    }

    #[test]
    fn msgpack_keeps_field_names() {
        let bytes = Codec::Msgpack
            .encode(&Deal {
                id: 1,
                title: "x".into(),
                owner: Some("anu".into()),
            })
            .unwrap();
        let value: Value = Codec::Msgpack.decode(&bytes).unwrap();
        assert_eq!(value, json!({ "id": 1, "title": "x", "owner": "anu" }));
    }

    #[test]
    fn unknown_fields_are_ignored_and_missing_ones_fail() {
        let deal: Deal = Codec::Json
            .decode(br#"{"id":1,"title":"x","extra":true}"#)
            .unwrap();
        assert_eq!(deal.owner, None);
        let error = Codec::Json.decode::<Deal>(br#"{"id":1}"#).unwrap_err();
        assert!(error.to_string().contains("title"), "{error}");
    }

    #[test]
    fn content_types_map_to_codecs() {
        assert_eq!(Codec::Json.content_type(), "application/json");
        assert_eq!(Codec::Msgpack.content_type(), "application/msgpack");
        for (content_type, codec) in [
            ("application/json", Some(Codec::Json)),
            ("Application/JSON; charset=utf-8", Some(Codec::Json)),
            ("application/problem+json", Some(Codec::Json)),
            ("application/msgpack", Some(Codec::Msgpack)),
            ("application/x-msgpack", Some(Codec::Msgpack)),
            ("text/plain", None),
        ] {
            assert_eq!(
                Codec::from_content_type(content_type),
                codec,
                "{content_type}"
            );
        }
    }

    #[test]
    fn deserializes_from_config_strings() {
        assert_eq!(
            serde_json::from_value::<Codec>(json!("msgpack")).unwrap(),
            Codec::Msgpack
        );
        assert!(serde_json::from_value::<Codec>(json!("xml")).is_err());
        assert_eq!(Codec::default(), Codec::Json);
    }
}
