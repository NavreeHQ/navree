//! [`GeoPoint`]: a place on the map, written as latitude and longitude with a comma between
//! them (`30.8890,75.8340`).

use std::fmt;
use std::str::FromStr;

use navree::ui::form::FieldValue;
use serde::de::{self, Deserializer};
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};

/// What a person is told when a text is not a point.
const EXPECTED: &str = "Enter the point as latitude,longitude, such as 30.9010,75.8573";

/// A place on the earth: degrees of latitude (within 90 either way) and of longitude (within
/// 180 either way).
///
/// It is written as two numbers with a comma between them. It displays with four decimals, which
/// is about eleven metres, and it serializes as `{"lat": .., "lng": ..}`, the shape the map's
/// script reads.
///
/// ```
/// use navree_plugin_geo::GeoPoint;
///
/// let at: GeoPoint = "30.8890,75.8340".parse().unwrap();
/// assert_eq!(at.to_string(), "30.8890,75.8340");
/// assert!("north".parse::<GeoPoint>().is_err());
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeoPoint {
    /// Degrees north of the equator; south is negative.
    pub lat: f64,
    /// Degrees east of Greenwich; west is negative.
    pub lng: f64,
}

/// A text that is not a point: it says how to write one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsePointError;

impl fmt::Display for ParsePointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(EXPECTED)
    }
}

impl std::error::Error for ParsePointError {}

impl FromStr for GeoPoint {
    type Err = ParsePointError;

    /// Reads `latitude,longitude`: two numbers with a comma between them, spaces around either
    /// allowed, the latitude within 90 and the longitude within 180.
    fn from_str(text: &str) -> Result<Self, ParsePointError> {
        let (lat, lng) = text.split_once(',').ok_or(ParsePointError)?;
        let lat: f64 = lat.trim().parse().map_err(|_| ParsePointError)?;
        let lng: f64 = lng.trim().parse().map_err(|_| ParsePointError)?;
        if lat.is_finite() && lng.is_finite() && lat.abs() <= 90.0 && lng.abs() <= 180.0 {
            Ok(GeoPoint { lat, lng })
        } else {
            Err(ParsePointError)
        }
    }
}

impl fmt::Display for GeoPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.4},{:.4}", self.lat, self.lng)
    }
}

impl<'de> Deserialize<'de> for GeoPoint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

impl Serialize for GeoPoint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut point = serializer.serialize_struct("GeoPoint", 2)?;
        point.serialize_field("lat", &self.lat)?;
        point.serialize_field("lng", &self.lng)?;
        point.end()
    }
}

impl FieldValue for GeoPoint {
    fn field_value(&self) -> Option<String> {
        Some(self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_is_two_numbers_within_range() {
        let point: GeoPoint = "30.8890,75.8340".parse().unwrap();
        assert_eq!(
            point,
            GeoPoint {
                lat: 30.8890,
                lng: 75.8340
            }
        );
        assert_eq!(point.to_string(), "30.8890,75.8340");

        // Spaces are allowed, and more decimals than are kept are cut when it is written.
        let loose: GeoPoint = " 30.90101 , 75.85734 ".parse().unwrap();
        assert_eq!(loose.to_string(), "30.9010,75.8573");

        for bad in [
            "",
            "north",
            "30.8890",
            "91,75",
            "30,181",
            "NaN,75",
            "30,inf",
            "30.88,75.83,1",
        ] {
            assert!(bad.parse::<GeoPoint>().is_err(), "{bad:?} is not a point");
        }
        assert_eq!(
            "north".parse::<GeoPoint>().unwrap_err().to_string(),
            "Enter the point as latitude,longitude, such as 30.9010,75.8573"
        );
    }

    #[test]
    fn a_point_is_read_from_one_string_and_written_as_lat_and_lng() {
        let point: GeoPoint = serde_json::from_str("\"31.3080,75.5810\"").unwrap();
        assert_eq!(point.lat, 31.3080);
        assert_eq!(
            serde_json::to_value(point).unwrap(),
            serde_json::json!({ "lat": 31.308, "lng": 75.581 })
        );
        let error = serde_json::from_str::<GeoPoint>("\"north\"").unwrap_err();
        assert!(error.to_string().contains("Enter the point as"), "{error}");
        assert!(serde_json::from_str::<GeoPoint>("{\"lat\":1,\"lng\":2}").is_err());
    }

    #[test]
    fn a_form_shows_a_point_as_it_displays() {
        let point: GeoPoint = "31.6550,74.8590".parse().unwrap();
        assert_eq!(point.field_value().as_deref(), Some("31.6550,74.8590"));
    }
}
