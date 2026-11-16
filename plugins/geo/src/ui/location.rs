//! The drop-point field: a text field for a point, with a map to pick it on and a button that
//! picks where the device is.
//!
//! A form names it with `#[form(control = navree_plugin_geo::LOCATION)]`, and a table cell with
//! `EditCell::control(.., Control::new(&navree_plugin_geo::LOCATION))`. The field posts the text
//! of a [`GeoPoint`] under the field's name, and the server holds that text to the delivery area
//! wherever Navree UI reads it with the app: a form read with `Submit`, a step of a wizard given
//! the app's language, a draft read back with `Draft::read_for`, and an inline edit read with
//! `EditedValue::read`. Read with no app, the point is refused, since the area can't be read;
//! read where no check runs (`navree::http::Form`), it is not held to the area.

use navree::config::ConfigSection;
use navree::ui::form::{ControlCx, ControlField, FieldControl};
use navree::ui::{Node, el};

use crate::options::GeoOptions;
use crate::places::nearest;
use crate::point::{GeoPoint, ParsePointError};
use crate::ui::map::{MAP, MapProps, map_body, place_line};

/// What the field's text box shows while it is empty, unless the form says otherwise.
const PLACEHOLDER: &str = "30.9010,75.8573";

/// What the check says when it cannot read the delivery area.
const UNREADABLE: &str = "The delivery area cannot be checked right now";

/// The drop-point field of a form or an inline edit: a field for the point, a button for the
/// device's own place, a map to click on, and the name of the place picked.
///
/// The map's tiles, its attribution and the centre it opens on when the field is empty come from
/// `GET /geo/tiles.json`, since a control is drawn with no request to read them from.
#[derive(Clone, Copy, Debug)]
pub struct LocationControl;

/// The drop-point control, for a form or a table cell to name.
pub static LOCATION: LocationControl = LocationControl;

impl FieldControl for LocationControl {
    fn name(&self) -> &'static str {
        "geo.location"
    }

    fn draw(&self, field: &ControlField<'_>) -> Node {
        let at = field.value.and_then(|text| text.parse::<GeoPoint>().ok());
        let place_id = format!("{}-place", field.id);
        let props = MapProps {
            center: at,
            zoom: 15,
            tiles: None,
            attribution: String::new(),
            pick: Some(field.name.to_owned()),
            place_id: place_id.clone(),
            seq: 0,
        };
        // The field the form posts, and the part a person types in. In a table cell it has no
        // label to point at, so it carries its own name.
        let input = el("input")
            .slot("input")
            .attr("type", "text")
            .attr("name", field.name.to_owned())
            .attr("autocomplete", "off")
            .attr(
                "placeholder",
                field.placeholder.unwrap_or(PLACEHOLDER).to_owned(),
            )
            .attr_opt("value", field.value.map(str::to_owned))
            .attr_opt(
                "aria-label",
                field.label_id.is_none().then(|| field.label.to_owned()),
            )
            .flag("required", field.required)
            .flag("disabled", field.disabled);
        // A button that submits nothing: it picks where the device is. Without a script it does
        // nothing, so the stylesheet hides it then.
        let locate = el("button")
            .slot("geo:locate")
            .attr("type", "button")
            .flag("disabled", field.disabled)
            .child("Use my location");
        let name = at.map_or("", nearest);
        el("div")
            .slot("geo:location")
            .extension_with(&MAP, &props)
            .children((input, locate, map_body(), place_line(&place_id, name, at)))
            .into()
    }

    /// Holds a posted point to the delivery area in the app's `geo` settings. A text that is not a
    /// point is refused with how to write one. Read with no app (`decode`, `Draft::read`, a wizard
    /// left in English), the check has no area to read and refuses every point: a point that
    /// could not be held to the app's area is not taken. An app with no configuration at all has
    /// the default area, Ludhiana, Jalandhar and Amritsar, as [`GeoModule`](crate::GeoModule)
    /// gives it.
    fn check(&self, value: &str, cx: &ControlCx<'_>) -> Result<(), String> {
        let at: GeoPoint = value
            .parse()
            .map_err(|error: ParsePointError| error.to_string())?;
        let options = match (cx.config(), cx.app()) {
            (Some(config), _) => config
                .section_or_default::<GeoOptions>(GeoOptions::KEY)
                .map_err(|_| UNREADABLE.to_owned())?,
            // An app with no configuration: its area is the one the module gives it.
            (None, Some(_)) => GeoOptions::default(),
            (None, None) => return Err(UNREADABLE.to_owned()),
        };
        if options.holds(at) {
            Ok(())
        } else {
            Err(options.refusal())
        }
    }
}
