# navree-plugin-geo

Maps and places for a Navree app: a map component and a drop-point form field. It is the
repository's worked example of a plugin that ships components of its own, and it is built from the
public API of the `navree` crate alone: nothing in it reaches into Navree UI's own crates. It is
not published; a plugin of your own is written the same way.

What it shows, in one small crate:

- **A component with a script and a stylesheet.** `MapView` draws one place on a Leaflet map. The
  server writes the place's name and point as text, so a page that may not load tiles, or has no
  script, still says where the place is. The script draws the tiles into the element's owned
  body, and the stylesheet is scoped to the plugin's components when the crate compiles.
- **A form field of its own.** `LOCATION` is a field control: a text field for a point, a map to
  click on, and a button that picks where the device is. A derived form names it with
  `#[form(control = navree_plugin_geo::LOCATION)]`, and a table cell with `EditCell::control`.
  The server holds the point to the delivery area in the `geo` settings wherever Navree UI reads
  the field with the app: a form read with `Submit`, a wizard step, a draft read back with
  `Draft::read_for` and an inline edit read with `EditedValue::read`. Where there is no app to
  read the area from, such as `decode` in a test, every point is refused.
- **A source the app has to allow.** The map's tiles come from OpenStreetMap. The plugin asks for
  that host, and the app allows it in `ui.policy.allow.geo.img_src`. Until it does, the map shows
  its fallback and the page loads nothing from the host. The plugin's two JSON routes under `/geo/`
  (`GET /geo/tiles.json` and `GET /geo/reverse`) answer signed-in requests with data; on a guest
  page `MapView` receives its `Tiles` on the server, while `LOCATION` keeps its coordinate input
  and `"Map unavailable"` fallback.

## Use it

In an app of this repository that has `UiModule`, add the crate by its path and the module to the
app:

```toml
navree-plugin-geo = { path = "../../plugins/geo" }
```

```rust
Navree::app()
    // ..
    .module(UiModule)
    .module(GeoModule)
```

Then say, in the app's configuration, that its pages may load the tiles:

```toml
# config/ui.policy.toml
[allow.geo]
img_src = ["https://tile.openstreetmap.org"]
```

The delivery area, the tile address and where an empty map opens are the `geo` section; every key
has a value, so an app that sets none gets OpenStreetMap's tiles around Ludhiana and the area of
Ludhiana, Jalandhar and Amritsar.

The crate reaches `navree` by a path, which is why its `Cargo.toml` differs from the line a plugin
outside the repository writes:
`navree = { version = "1.3", features = ["ui", "ui-extension-files"] }`.

## Building the browser files

The script and the stylesheet are built from `assets/src` and Leaflet 1.9.4 into `assets/map.js`
and `assets/map.css`. Both are committed, so building the crate needs no Node.

```sh
cd plugins/geo/assets
npm install
node build.mjs          # writes map.js and map.css
node build.mjs --check  # fails when they differ from a fresh build
```

`THIRD_PARTY.md` carries Leaflet's licence.

## Tests

```sh
cargo test -p navree-plugin-geo
cargo test -p navree-plugin-geo --test browser -- --ignored   # needs Chrome or Chromium
```
