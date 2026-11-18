//! The map: [`MapView`] draws one place on a Leaflet map, and [`Tiles`] says where the map may
//! load its tiles from.
//!
//! The server draws the place's name and point as text, which is all a reader sees when the page
//! may not load tiles or has no script. The map's script (`assets/map.js`, Leaflet bundled with
//! the glue in `assets/src/map.js`) draws the tiles into the element's owned body, and the
//! stylesheet (`assets/map.css`, Leaflet's rules and the plugin's own) is scoped to this plugin's
//! components when the crate compiles.

use http::request::Parts;
use navree::AppContext;
use navree::http::AppError;
use navree::http::extract::FromRequestParts;
use navree::ui::{
    Directive, Element, Node, PagePolicy, UiExtension, el, extension_script, extension_sheet,
};
use serde::Serialize;

use crate::options::GeoOptions;
use crate::point::GeoPoint;

/// The map component's files: its script and its stylesheet, served by Navree UI under the
/// `geo` namespace and loaded on a page only where an element names them. A map's script calls
/// the plugin's own routes (`/geo/`), and keeps the `data-moving` mark it sets on the element
/// while the map moves.
///
/// The files are read, scoped and hashed when the crate compiles: Leaflet's stylesheet is the one
/// it ships, with the three rules that name pictures taken out (the map draws a circle for its
/// pin and has no layers control), followed by the plugin's own.
pub static MAP: UiExtension = UiExtension::new("geo", "map")
    .script(extension_script!("assets/map.js", 200 * 1024))
    .sheet(extension_sheet!("assets/map.css", "geo", 32 * 1024))
    .contract(1)
    .keeps(&["data-moving"])
    .routes(&["/geo/"]);

/// What the map's script is given, as JSON on the element.
#[derive(Clone, Debug, Serialize)]
pub struct MapProps {
    /// Where the map opens and where its pin stands. A control leaves it out when its field is
    /// empty, and the script then opens the map on the delivery area's centre.
    pub center: Option<GeoPoint>,
    /// How far in the map is zoomed.
    pub zoom: u8,
    /// The tile address when the page may load tiles from it, else none: the map is then not
    /// drawn, and the server's fallback stays.
    pub tiles: Option<String>,
    /// What the map says about where its tiles come from.
    pub attribution: String,
    /// The name of the form field the map moves, when the map is a control.
    pub pick: Option<String>,
    /// The id of the line under the map that names the place.
    pub place_id: String,
    /// Counts the times the server moved the map: a higher number than the map had makes it fly
    /// to the new centre, and the same number leaves its view alone.
    pub seq: u32,
}

/// What a person reads in the place of a map that is not drawn.
const FALLBACK: &str = "Map unavailable";

/// A map of one place with a pin on it, and under it the place's name and point as text.
///
/// ```
/// use navree_plugin_geo::{MapView, Tiles};
///
/// let tiles = Tiles::from_parts(None, "© OpenStreetMap contributors".into());
/// let map = MapView::new(
///     "drop-map",
///     "30.8890,75.8340".parse().unwrap(),
///     "Sarabha Nagar, Ludhiana",
///     &tiles,
/// );
/// let html = navree::ui::render(&map.into());
/// assert!(html.contains("Sarabha Nagar, Ludhiana (30.8890,75.8340)"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct MapView {
    el: Element,
    props: MapProps,
    place: String,
}

navree::ui::component_methods!(MapView);
navree::ui::component_node!(MapView);

impl MapView {
    /// A map with the element id `id`, opened on `center` at street level with a pin there,
    /// under which `place` names the point. The map is drawn with `tiles` when the page may load
    /// them.
    ///
    /// The line under the map carries the id `id` followed by `-place`.
    pub fn new(id: &str, center: GeoPoint, place: impl Into<String>, tiles: &Tiles) -> MapView {
        let props = MapProps {
            center: Some(center),
            zoom: 15,
            tiles: tiles.address(),
            attribution: tiles.attribution(),
            pick: None,
            place_id: format!("{id}-place"),
            seq: 0,
        };
        MapView {
            el: el("div").slot("geo:map").id(id.to_owned()),
            props,
            place: place.into(),
        }
    }

    /// Sets the count of times the server moved this map: a map on the page flies to the new
    /// centre when the count is higher than its own.
    pub fn seq(mut self, seq: u32) -> MapView {
        self.props.seq = seq;
        self
    }

    fn into_node(self) -> Node {
        let place = place_line(&self.props.place_id, &self.place, self.props.center);
        self.el
            .extension_with(&MAP, &self.props)
            .children((map_body(), place))
            .into()
    }
}

/// The body the map's script draws into. It is the script's own: the page seals it before the
/// script loads, so a library that draws into it can write nothing the page acts on, and a
/// morph from the server leaves it alone. What it holds until the map is drawn, or when it is
/// not, is the fallback.
pub(crate) fn map_body() -> Element {
    el("div")
        .slot("geo:map-body")
        .flag("data-ui-keep", true)
        .child(FALLBACK)
}

/// The line that names a place: `Sarabha Nagar, Ludhiana (30.8890,75.8340)`, or only the point
/// when `name` is empty, or only the name when there is no point.
pub(crate) fn place_line(id: &str, name: &str, at: Option<GeoPoint>) -> Element {
    let line = match at {
        Some(at) if name.is_empty() => at.to_string(),
        Some(at) => format!("{name} ({at})"),
        None => name.to_owned(),
    };
    el("p").slot("geo:place").id(id.to_owned()).child(line)
}

/// Where a map may load its tiles from on this app: the tile address when the app's page policy
/// allows its host for pictures, else none, and what the map says about its tiles.
///
/// A handler takes one as an argument, and it reads the app's `geo` settings and page policy
/// from the request's app.
#[derive(Clone, Debug)]
pub struct Tiles {
    address: Option<String>,
    attribution: String,
}

impl Tiles {
    /// The tile address of `options` when `policy` allows its origin for pictures, else none. An
    /// address starting with a single slash (not `//` or `/\`) is this app's own, and always
    /// allowed.
    pub fn new(options: &GeoOptions, policy: &PagePolicy) -> Tiles {
        let own = options.tiles.starts_with('/')
            && !options.tiles.starts_with("//")
            && !options.tiles.starts_with("/\\");
        let allowed = own || policy.allows(Directive::ImgSrc, &options.tiles);
        Tiles {
            address: allowed.then(|| options.tiles.clone()),
            attribution: options.attribution.clone(),
        }
    }

    /// Tiles from what is already known: for a test, and for a page that decided the address
    /// itself.
    pub fn from_parts(address: Option<String>, attribution: String) -> Tiles {
        Tiles {
            address,
            attribution,
        }
    }

    /// The tile address, when the page may load tiles from it.
    pub fn address(&self) -> Option<String> {
        self.address.clone()
    }

    /// What the map says about where its tiles come from.
    pub fn attribution(&self) -> String {
        self.attribution.clone()
    }
}

impl FromRequestParts for Tiles {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, AppError> {
        let ctx = parts
            .extensions
            .get::<AppContext>()
            .cloned()
            .ok_or_else(|| {
                AppError::internal("Tiles is only available in a route mounted by the HTTP module")
            })?;
        let options = ctx.get::<GeoOptions>().await.map_err(AppError::internal)?;
        let policy = ctx.get::<PagePolicy>().await.map_err(AppError::internal)?;
        Ok(Tiles::new(&options, &policy))
    }
}
