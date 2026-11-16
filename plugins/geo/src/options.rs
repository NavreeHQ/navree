//! [`GeoOptions`]: the `geo` section of an app's configuration.

use navree::config::ConfigSection;
use serde::Deserialize;

use crate::point::GeoPoint;

/// The settings of the map and of the delivery area, from the `geo` section of the app's
/// configuration. Every key has a value, so an app that sets none draws OpenStreetMap's tiles
/// around Ludhiana's clock tower.
///
/// ```toml
/// [geo]
/// tiles = "https://tile.openstreetmap.org/{z}/{x}/{y}.png"
/// zoom = 13
///
/// [[geo.area]]
/// name = "Ludhiana"
/// south_west = "30.80,75.70"
/// north_east = "31.00,75.98"
/// ```
///
/// ```
/// use navree_plugin_geo::GeoOptions;
///
/// let options = GeoOptions::default();
/// assert!(options.holds("30.8890,75.8340".parse().unwrap()));
/// assert!(!options.holds("28.6139,77.2090".parse().unwrap()));
/// ```
#[derive(Clone, Debug, Deserialize, ConfigSection)]
#[config(key = "geo")]
#[serde(default)]
pub struct GeoOptions {
    /// The address of the map's tiles, a template in which z, x and y in braces stand for the
    /// zoom, the column and the row of the tile asked for. An address starting with a slash is
    /// this app's own and always allowed; any other host is loaded only where the app allows it
    /// for pictures in ui.policy.
    pub tiles: String,
    /// What the map says about where its tiles come from.
    pub attribution: String,
    /// Where a map with no place of its own opens: the centre of the delivery area.
    pub centre: GeoPoint,
    /// How far in that map is zoomed, from 0 (the world) to 19 (a street).
    pub zoom: u8,
    /// The places deliveries go to. A drop point outside every area is refused, in a form and in
    /// a table cell alike; with no area listed, no point is refused.
    pub area: Vec<Area>,
}

/// One place deliveries go to: a box on the map between two corners.
#[derive(Clone, Debug, Deserialize)]
pub struct Area {
    /// What the area is called, as the refusal names it.
    pub name: String,
    /// The south-western corner: the smallest latitude and longitude the area holds.
    pub south_west: GeoPoint,
    /// The north-eastern corner: the largest latitude and longitude the area holds.
    pub north_east: GeoPoint,
}

impl Area {
    /// Whether `at` is inside the area, its edges included.
    pub fn holds(&self, at: GeoPoint) -> bool {
        (self.south_west.lat..=self.north_east.lat).contains(&at.lat)
            && (self.south_west.lng..=self.north_east.lng).contains(&at.lng)
    }
}

impl Default for GeoOptions {
    fn default() -> Self {
        let area = |name: &str, south_west: (f64, f64), north_east: (f64, f64)| Area {
            name: name.to_owned(),
            south_west: GeoPoint {
                lat: south_west.0,
                lng: south_west.1,
            },
            north_east: GeoPoint {
                lat: north_east.0,
                lng: north_east.1,
            },
        };
        GeoOptions {
            tiles: "https://tile.openstreetmap.org/{z}/{x}/{y}.png".to_owned(),
            attribution: "© OpenStreetMap contributors".to_owned(),
            // Ludhiana's clock tower.
            centre: GeoPoint {
                lat: 30.9010,
                lng: 75.8573,
            },
            zoom: 13,
            area: vec![
                area("Ludhiana", (30.80, 75.70), (31.00, 75.98)),
                area("Jalandhar", (31.27, 75.52), (31.38, 75.65)),
                area("Amritsar", (31.58, 74.80), (31.70, 74.95)),
            ],
        }
    }
}

impl GeoOptions {
    /// Whether a delivery can go to `at`: it is inside one of the areas, or no area is listed.
    pub fn holds(&self, at: GeoPoint) -> bool {
        self.area.is_empty() || self.area.iter().any(|area| area.holds(at))
    }

    /// What a person is told when `at` is outside every area: the names of the areas, in the
    /// order listed (`Pick a place inside the delivery area: Ludhiana, Jalandhar or Amritsar`).
    pub fn refusal(&self) -> String {
        let names: Vec<&str> = self.area.iter().map(|area| area.name.as_str()).collect();
        match names.as_slice() {
            [] => "Pick a place inside the delivery area".to_owned(),
            [only] => format!("Pick a place inside the delivery area: {only}"),
            [first @ .., last] => format!(
                "Pick a place inside the delivery area: {} or {last}",
                first.join(", ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(text: &str) -> GeoPoint {
        text.parse().unwrap()
    }

    #[test]
    fn the_default_area_is_ludhiana_jalandhar_and_amritsar() {
        let options = GeoOptions::default();
        for inside in [
            "30.9010,75.8573",
            "30.8890,75.8340",
            "31.3080,75.5810",
            "31.6550,74.8590",
        ] {
            assert!(options.holds(point(inside)), "{inside}");
        }
        for outside in ["28.6139,77.2090", "18.5204,73.8567", "31.1000,75.8000"] {
            assert!(!options.holds(point(outside)), "{outside}");
        }
        assert_eq!(
            options.refusal(),
            "Pick a place inside the delivery area: Ludhiana, Jalandhar or Amritsar"
        );
        assert_eq!(options.centre.to_string(), "30.9010,75.8573");
    }

    #[test]
    fn the_refusal_names_one_two_or_no_area() {
        let mut options = GeoOptions::default();
        options.area.truncate(2);
        assert_eq!(
            options.refusal(),
            "Pick a place inside the delivery area: Ludhiana or Jalandhar"
        );
        options.area.truncate(1);
        assert_eq!(
            options.refusal(),
            "Pick a place inside the delivery area: Ludhiana"
        );
        options.area.clear();
        assert_eq!(options.refusal(), "Pick a place inside the delivery area");
        assert!(options.holds(point("28.6139,77.2090")), "no area, no limit");
    }

    #[test]
    fn the_section_reads_what_it_is_given_and_keeps_the_rest() {
        let options: GeoOptions = serde_json::from_value(serde_json::json!({
            "zoom": 12,
            "area": [{ "name": "Pune", "south_west": "18.40,73.75", "north_east": "18.65,73.98" }],
        }))
        .unwrap();
        assert_eq!(options.zoom, 12);
        assert_eq!(options.tiles, GeoOptions::default().tiles);
        assert!(options.holds(point("18.5204,73.8567")));
        assert!(!options.holds(point("30.8890,75.8340")));
        assert_eq!(GeoOptions::KEY, "geo");
    }
}
