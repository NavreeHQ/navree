//! The plugin's two routes, both for a signed-in reader and both under `/geo/`, the prefix the
//! map's script is allowed to call:
//!
//! - `GET /geo/tiles.json` says where the map's tiles are, what it says about them, and where a
//!   map with no place of its own opens. A control is drawn with no request, so its script asks
//!   here.
//! - `POST /geo/reverse` takes a point and the id of a place line, and answers with that line
//!   redrawn to name the nearest place.

use std::sync::Arc;

use http::Method;
use navree::http::{AppError, Controller, Form, Json, RouteDef, Scoped};
use navree::ui::Reply;
use navree::{BoxFuture, Injectable, KernelError, Resolver};
use serde::Deserialize;

use crate::options::GeoOptions;
use crate::places::nearest;
use crate::point::GeoPoint;
use crate::ui::map::{Tiles, place_line};

/// What `POST /geo/reverse` reads: the point, and the id of the line that names it.
#[derive(Deserialize)]
struct Reverse {
    at: GeoPoint,
    target: String,
}

/// Whether `id` is the id of a place line: lower-case letters, digits, `_` and `-`, at most 64
/// of them, ending with `-place`. A post names the element it redraws, so only an id of that
/// form is taken.
fn is_place_id(id: &str) -> bool {
    id.len() > "-place".len()
        && id.len() <= 64
        && id.ends_with("-place")
        && id
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
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

/// `POST /geo/reverse`: morphs the place line `target` into the name of the place nearest `at`.
async fn reverse(Form(form): Form<Reverse>) -> Result<Reply, AppError> {
    if !is_place_id(&form.target) {
        return Err(AppError::bad_request(
            "The target is not the id of a place line.",
        ));
    }
    let line = place_line(&form.target, nearest(form.at), Some(form.at));
    Ok(Reply::ops(Vec::new()).morph(form.target, line))
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
            RouteDef::on(Method::POST, "!/geo/reverse", reverse).authenticated(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_id_of_a_place_line_is_redrawn() {
        for good in [
            "drop-map-place",
            "newdelivery-drop_at-place",
            "edit-0123456789ab-place",
        ] {
            assert!(is_place_id(good), "{good}");
        }
        let too_long = format!("{}-place", "a".repeat(60));
        for bad in [
            "",
            "-place",
            "drop-map",
            "Drop-Map-place",
            "drop map-place",
            "drop-map-place ",
            "geo:map-place",
            "#drop-map-place",
            too_long.as_str(),
        ] {
            assert!(!is_place_id(bad), "{bad}");
        }
    }
}
