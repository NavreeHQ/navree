//! The places a drop point is named after: a short fixed list, standing in for a geocoder.

use crate::point::GeoPoint;

/// Each place with its name and where it is.
static PLACES: [(&str, GeoPoint); 4] = [
    (
        "Clock Tower, Ludhiana",
        GeoPoint {
            lat: 30.9010,
            lng: 75.8573,
        },
    ),
    (
        "Sarabha Nagar, Ludhiana",
        GeoPoint {
            lat: 30.8890,
            lng: 75.8340,
        },
    ),
    (
        "Model Town, Jalandhar",
        GeoPoint {
            lat: 31.3080,
            lng: 75.5810,
        },
    ),
    (
        "Ranjit Avenue, Amritsar",
        GeoPoint {
            lat: 31.6550,
            lng: 74.8590,
        },
    ),
];

/// How far `a` is from `b`, squared, in degrees of latitude: a degree of longitude is shortened
/// by how far from the equator the two are. Only the order of the distances matters here.
fn distance(a: GeoPoint, b: GeoPoint) -> f64 {
    let squeeze = f64::midpoint(a.lat, b.lat).to_radians().cos();
    let across = (a.lng - b.lng) * squeeze;
    let along = a.lat - b.lat;
    across * across + along * along
}

/// The name of the place nearest to `at`, from a fixed list of four: Clock Tower and Sarabha
/// Nagar in Ludhiana, Model Town in Jalandhar and Ranjit Avenue in Amritsar.
///
/// ```
/// use navree_plugin_geo::{GeoPoint, nearest};
///
/// let at: GeoPoint = "30.8890,75.8340".parse().unwrap();
/// assert_eq!(nearest(at), "Sarabha Nagar, Ludhiana");
/// ```
pub fn nearest(at: GeoPoint) -> &'static str {
    PLACES
        .iter()
        .min_by(|a, b| distance(at, a.1).total_cmp(&distance(at, b.1)))
        .map_or("", |place| place.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_place_is_nearest_to_itself() {
        for (name, at) in &PLACES {
            assert_eq!(nearest(*at), *name);
        }
    }

    #[test]
    fn a_point_is_named_after_the_place_it_is_closest_to() {
        let near = |text: &str| nearest(text.parse().unwrap());
        assert_eq!(near("30.8900,75.8360"), "Sarabha Nagar, Ludhiana");
        assert_eq!(near("30.9000,75.8500"), "Clock Tower, Ludhiana");
        assert_eq!(near("31.3000,75.6000"), "Model Town, Jalandhar");
        assert_eq!(near("31.6000,74.9000"), "Ranjit Avenue, Amritsar");
    }
}
