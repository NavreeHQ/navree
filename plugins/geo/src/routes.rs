//! The plugin's two routes, both for a signed-in user and both under `/geo/`, the prefix the
//! map's script is allowed to call:
//!
//! - `GET /geo/tiles.json` says where the map's tiles are, what it says about them, and where a
//!   map with no place of its own opens. A control is drawn with no request, so its script asks
//!   here; on a guest page it answers 401 and the control keeps its plain coordinate input and
//!   fallback text.
//! - `GET /geo/reverse` takes a point in the query (`?at=lat,lng`) and answers JSON with the
//!   nearest place's name and formatted line, changing no element on the page.

use std::sync::Arc;

use http::Method;
use navree::http::{Controller, Json, Query, RouteDef, Scoped};
use navree::{BoxFuture, Injectable, KernelError, Resolver};
use serde::Deserialize;

use crate::options::GeoOptions;
use crate::places::nearest;
use crate::point::GeoPoint;
use crate::ui::map::Tiles;

/// What `GET /geo/reverse` reads: the point to name.
#[derive(Deserialize)]
struct Reverse {
    at: GeoPoint,
}

/// `GET /geo/tiles.json`: `{"tiles": .., "attribution": .., "centre": {"lat": .., "lng": ..},
/// "zoom": ..}`. The address is `null` when the app's page policy does not allow its host.
async fn tile_address(
    tiles: Tiles,
    Scoped(options): Scoped<GeoOptions>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "tiles": tiles.address(),
        "attribution": tiles.attribution(),
        "centre": options.centre,
        "zoom": options.zoom,
    }))
}

/// `GET /geo/reverse`: answers JSON data naming the place nearest `at`, with no DOM operation.
async fn reverse(Query(query): Query<Reverse>) -> Json<serde_json::Value> {
    let place = nearest(query.at);
    let at = query.at.to_string();
    let line = if place.is_empty() {
        at.clone()
    } else {
        format!("{place} ({at})")
    };
    Json(serde_json::json!({
        "place": place,
        "at": at,
        "line": line,
    }))
}

/// The controller of the two routes above.
pub(crate) struct GeoController;

impl Injectable for GeoController {
    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(GeoController) })
    }
}

impl Controller for GeoController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/geo/tiles.json", tile_address).authenticated(),
            RouteDef::on(Method::GET, "!/geo/reverse", reverse).authenticated(),
        ]
    }
}
