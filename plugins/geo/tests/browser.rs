//! The geo plugin in a real browser (Chrome or Chromium, through navree-browser): a map and a
//! drop-point field drawn by a page, picked on, edited in place and moved by the server, under
//! the toolkit's strict content security policy.
//!
//! Every test needs a browser, so each is ignored by default:
//!
//! `cargo test -p navree-plugin-geo --test browser -- --ignored`
//!
//! The app serves its map tiles itself, at `/geo-test/tiles/{z}/{x}/{y}`, each one a grey SVG
//! square: a path of the app is its own origin, so the page policy lets pictures load from it
//! with nothing allowed. A browser signs in at `/test/sign-in`, which sets the cookie the app's
//! authenticator reads and starts a session, so every post after it carries the CSRF token as in
//! production.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::request::Parts;
use http::{HeaderValue, Method, header};
use navree::browser::{Browser, BrowserError, Page, Target, TestServer};
use navree::contracts::{AuthVia, Principal};
use navree::crypto::{CryptoModule, generate_key};
use navree::http::body::Body;
use navree::http::response::{IntoResponse, Response};
use navree::http::{
    AppError, Authenticator, Controller, Extension, ModuleBuilderHttpExt as _, Path, RouteDef,
};
use navree::session::{Session, SessionModule, SessionOptions};
use navree::testing::{TestApp, TestAppBuilder};
use navree::ui::form::{Control, Language, Rejected};
use navree::ui::table::{EditCell, NoFilter};
use navree::ui::{
    DataTable, EditedValue, Element, Form, FormView, Money, Node, Reply, Submit, TableQuery,
    TableRow, UiModule, Validate, View, el, kit,
};
use navree::validation::ValidationModule;
use navree::{
    AppContext, BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_plugin_geo::{GeoModule, GeoPoint, LOCATION, MAP, MapView, Tiles, nearest};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

/// The cookie that signs a browser in.
const COOKIE: &str = "geo_user";

/// The app's own tiles: a path of this app, so nothing has to be allowed for them.
const OWN_TILES: &str = "/geo-test/tiles/{z}/{x}/{y}";

/// OpenStreetMap's tiles, the plugin's default, which a page may load only when the app allows
/// the host.
const OSM_TILES: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";

/// One grey square.
const TILE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256"><rect width="256" height="256" fill="#e8ecef" stroke="#c8cdd1"/></svg>"##;

/// The map once Leaflet drew it, and a field's input.
const DRAWN_MAP: &str = r#"css=[data-slot="geo:map-body"].leaflet-container"#;
const FIELD: &str = "css=input[name=drop_at]";

/// The first drop point of a table, in its cell's own input.
const FIRST_CELL: &str = "css=input[name=value]";

/// Signs a request in when it carries the cookie.
struct CookieAuth;

impl Authenticator for CookieAuth {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let signed_in = parts
            .headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .any(|pair| {
                pair.trim()
                    .strip_prefix("geo_user=")
                    .is_some_and(|value| !value.is_empty())
            });
        Box::pin(async move {
            Ok(signed_in.then(|| Principal {
                user_id: Uuid::from_u128(7),
                via: AuthVia::Session,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}

/// A delivery to a place on the map.
#[derive(Debug, Deserialize, Validate, Form)]
struct NewDelivery {
    /// Who receives it
    #[validate(length(max = 80))]
    recipient: String,
    /// Drop point
    #[validate(skip)]
    #[form(control = navree_plugin_geo::LOCATION)]
    drop_at: GeoPoint,
    /// Delivery charge
    #[validate(skip)]
    charge: Money,
}

/// A row of the deliveries table.
#[derive(Clone, TableRow)]
#[table(sort = "recipient", per = 25)]
struct Delivery {
    #[col(skip)]
    id: i64,
    #[col(sortable, label = "Recipient")]
    recipient: String,
    #[col(label = "City")]
    city: String,
    #[col(label = "Drop point")]
    drop_at: String,
}

/// The drop point stored for each delivery, as it is written.
type Drops = Arc<Mutex<BTreeMap<i64, String>>>;

fn point(text: &str) -> GeoPoint {
    text.parse().expect("a point")
}

/// `POST /deliveries`: a new delivery.
async fn new_delivery(submit: Result<Submit<NewDelivery>, Rejected<NewDelivery>>) -> Response {
    match submit {
        Ok(Submit(delivery)) => format!("taken {}", delivery.recipient).into_response(),
        Err(rejected) => rejected.into_response(),
    }
}

/// `POST /deliveries/{id}/drop`: a new drop point from a table cell, held to the delivery area
/// before it is stored.
async fn edit_drop(
    drops: Drops,
    id: i64,
    ctx: AppContext,
    edit: EditedValue<GeoPoint>,
) -> Reply {
    let language = Language::of(Some(&ctx)).await;
    match edit.read(Control::new(&LOCATION), &language) {
        Ok(at) => {
            drops
                .lock()
                .expect("the stored drop points")
                .insert(id, at.to_string());
            Reply::ops(Vec::new())
        }
        Err(refused) => refused.reply("/deliveries"),
    }
}

/// `GET /deliveries`: three deliveries, each drop point editable in its cell.
fn deliveries_page(drops: &Drops) -> View {
    let drops = drops.lock().expect("the stored drop points");
    let rows: Vec<Delivery> = [
        (1, "Harpreet Sandhu", "Ludhiana"),
        (2, "Meera Sharma", "Jalandhar"),
        (3, "Simran Kaur", "Amritsar"),
    ]
    .iter()
    .map(|(id, recipient, city)| Delivery {
        id: *id,
        recipient: (*recipient).to_owned(),
        city: (*city).to_owned(),
        drop_at: drops.get(id).cloned().unwrap_or_default(),
    })
    .collect();
    let query = TableQuery::<Delivery, NoFilter>::parse("/deliveries", "");
    View::new("Deliveries").body(DataTable::new("deliveries", &query, &rows).cell(
        "drop_at",
        |row| {
            EditCell::control(
                row.drop_at.clone(),
                format!("/deliveries/{}/drop", row.id),
                Control::new(&LOCATION),
            )
            .label("Drop point")
        },
    ))
}

/// `GET /deliveries/new`: the form of a new delivery, its drop point already picked.
fn new_delivery_page() -> View {
    let values = [
        ("recipient", "Harpreet Sandhu"),
        ("drop_at", "30.8890,75.8340"),
        ("charge", "40"),
    ]
    .iter()
    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
    .collect();
    View::new("New delivery").body(
        FormView::<NewDelivery>::new()
            .post("/deliveries")
            .with_values(values),
    )
}

/// The button that has the server move the map.
fn fly_form() -> Element {
    el("form")
        .attr("method", "post")
        .attr("action", "/map/fly")
        .flag("data-ui-form", true)
        .child(
            el("button")
                .attr("type", "submit")
                .child("Fly to Model Town"),
        )
}

/// `GET /geo-test/tiles/{z}/{x}/{y}`.
async fn tile() -> Response {
    http::Response::builder()
        .header(header::CONTENT_TYPE, "image/svg+xml")
        .body(Body::from(TILE_SVG))
        .expect("a response")
}

/// `GET /test/sign-in`: sets the cookie that signs the browser in, and starts its session with a
/// CSRF token, as a page with a form does: every post after it has to carry the token.
async fn sign_in(session: Session) -> Response {
    let _token = session.csrf_token();
    let mut response = "Signed in as Harpreet Sandhu".into_response();
    if let Ok(value) = HeaderValue::from_str(&format!(
        "{COOKIE}=harpreet; Path=/; HttpOnly; SameSite=Lax"
    )) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

/// The pages and posts of the test app.
struct Site {
    drops: Drops,
}

impl Injectable for Site {
    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        let drops = [
            (1, "30.8890,75.8340"),
            (2, "31.3080,75.5810"),
            (3, "31.6550,74.8590"),
        ]
        .into_iter()
        .map(|(id, at)| (id, at.to_owned()))
        .collect();
        Box::pin(async move {
            Ok(Site {
                drops: Arc::new(Mutex::new(drops)),
            })
        })
    }
}

impl Controller for Site {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let table = self.drops.clone();
        let edit = self.drops.clone();
        vec![
            RouteDef::on(Method::GET, "!/test/sign-in", sign_in).public(),
            RouteDef::on(Method::GET, "!/geo-test/tiles/{z}/{x}/{y}", tile).public(),
            RouteDef::on(Method::GET, "!/deliveries/new", || async {
                new_delivery_page()
            })
            .authenticated()
            .page(),
            RouteDef::on(Method::POST, "!/deliveries", new_delivery).authenticated(),
            RouteDef::on(Method::GET, "!/deliveries", move || {
                let page = deliveries_page(&table);
                async move { page }
            })
            .authenticated()
            .page(),
            RouteDef::on(
                Method::POST,
                "!/deliveries/{id}/drop",
                move |Path(id): Path<i64>,
                      Extension(ctx): Extension<AppContext>,
                      value: EditedValue<GeoPoint>| {
                    let drops = edit.clone();
                    async move { edit_drop(drops, id, ctx, value).await }
                },
            )
            .authenticated(),
            RouteDef::on(Method::GET, "!/map", |tiles: Tiles| async move {
                View::new("Map").body((
                    MapView::new(
                        "drop-map",
                        point("30.8890,75.8340"),
                        "Sarabha Nagar, Ludhiana",
                        &tiles,
                    ),
                    fly_form(),
                ))
            })
            .authenticated()
            .page(),
            RouteDef::on(Method::POST, "!/map/fly", |tiles: Tiles| async move {
                Reply::ops(Vec::new()).morph(
                    "drop-map",
                    MapView::new(
                        "drop-map",
                        point("31.3080,75.5810"),
                        "Model Town, Jalandhar",
                        &tiles,
                    )
                    .seq(1),
                )
            })
            .authenticated(),
            RouteDef::on(Method::GET, "!/twin", |tiles: Tiles| async move {
                let live = MapView::new(
                    "live-map",
                    point("30.8890,75.8340"),
                    "Sarabha Nagar, Ludhiana",
                    &tiles,
                );
                let twin: Node = MapView::new(
                    "twin-map",
                    point("30.8890,75.8340"),
                    "Sarabha Nagar, Ludhiana",
                    &tiles,
                )
                .into();
                View::new("Twin").body((live, kit::stored_twin(&twin)))
            })
            .authenticated()
            .page(),
        ]
    }
}

struct SiteModule;

impl Module for SiteModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(Factory::<dyn Authenticator>::new(|_| async {
            Ok(Arc::new(CookieAuth) as Arc<dyn Authenticator>)
        }))
        .exports::<dyn Authenticator>();
        m.controller::<Site>();
    }
}

/// The app: sessions, the toolkit, the plugin and the pages above, with the map's tiles at
/// `tiles` and, when given, a delivery area of `area`.
fn builder(tiles: &str, area: Option<Value>) -> TestAppBuilder {
    let builder = TestApp::builder()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("geo.tiles", tiles)
        .module(CryptoModule)
        .module(SessionModule::for_root(SessionOptions {
            // The test server speaks plain HTTP on 127.0.0.1.
            secure: false,
            ..SessionOptions::default()
        }))
        .module(ValidationModule)
        .module(UiModule)
        .module(GeoModule)
        .module(SiteModule);
    match area {
        Some(area) => builder.config("geo.area", area),
        None => builder,
    }
}

/// The app on a real port, and the browser that visits it.
struct Shop {
    _app: TestApp,
    server: TestServer,
    browser: Browser,
}

impl Shop {
    async fn start(builder: TestAppBuilder) -> Result<Shop, BrowserError> {
        let app = builder.build().await;
        let server = TestServer::start(app.ctx()).await?;
        let browser = Browser::launch().await?;
        Ok(Shop {
            _app: app,
            server,
            browser,
        })
    }

    /// The page at `path`, with the browser signed in as Harpreet Sandhu.
    async fn open(&self, path: &str) -> Result<Page, BrowserError> {
        let page = self
            .browser
            .visit(self.server.url("/test/sign-in"))
            .await?;
        page.visit(self.server.url(path)).await?;
        Ok(page)
    }
}

/// Clicks the first map `dx` pixels right of its middle and `dy` below it. The click is one the
/// page's own script receives, as Leaflet does a person's, at a place the map did not open on.
async fn click_map(page: &Page, dx: i32, dy: i32) -> Result<(), BrowserError> {
    page.script::<bool>(format!(
        r#"const map = document.querySelector('[data-slot="geo:map-body"]');
        const box = map.getBoundingClientRect();
        map.dispatchEvent(new MouseEvent("click", {{
            bubbles: true,
            cancelable: true,
            view: window,
            clientX: box.left + box.width / 2 + {dx},
            clientY: box.top + box.height / 2 + {dy},
        }}));
        return true;"#
    ))
    .await
    .map(drop)
}

/// The value of `target` once it is not `before` any more.
async fn changed_from(page: &Page, target: &Target, before: &str) -> Result<String, BrowserError> {
    for _ in 0..50 {
        let now = page.value(target.clone()).await?;
        if now != before {
            return Ok(now);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{target} stayed {before}");
}

/// How far the first map's pin is from the middle of the map, in pixels; -1 when it has none.
async fn pin_offset(page: &Page) -> Result<f64, BrowserError> {
    page.script(
        r#"const map = document.querySelector('[data-slot="geo:map-body"]');
        const pin = map.querySelector('path.leaflet-interactive');
        if (!pin) return -1;
        const a = pin.getBoundingClientRect(), b = map.getBoundingClientRect();
        return Math.hypot(a.left + a.width / 2 - (b.left + b.width / 2), a.top + a.height / 2 - (b.top + b.height / 2));"#,
    )
    .await
}

/// How many requests the page made that are not for a tile.
async fn requests(page: &Page) -> Result<u64, BrowserError> {
    page.script(
        "return performance.getEntriesByType('resource').filter((e) => !e.name.includes('/geo-test/tiles/')).length",
    )
    .await
}

/// The column and row of the tile that holds `at` at `zoom`, as a map of Web Mercator tiles
/// numbers them.
fn tile_of(at: GeoPoint, zoom: u32) -> (u64, u64) {
    let tiles = f64::from(1_u32 << zoom);
    let lat = at.lat.to_radians();
    let x = ((at.lng + 180.0) / 360.0 * tiles).floor();
    let y = ((1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * tiles)
        .floor();
    (x as u64, y as u64)
}

// A click on the map is the field's new value, and the line under the map names the place the
// server found nearest to it.
#[tokio::test]
#[ignore = "needs Chrome or Chromium"]
async fn harpreet_moves_the_drop_pin_and_the_address_follows() -> Result<(), BrowserError> {
    let shop = Shop::start(builder(OWN_TILES, None)).await?;
    let page = shop.open("/deliveries/new").await?;
    page.wait_for(DRAWN_MAP).await?;
    assert_eq!(page.value(FIELD).await?, "30.8890,75.8340");
    page.assert_see("Sarabha Nagar, Ludhiana (30.8890,75.8340)")
        .await?;

    click_map(&page, 90, -40).await?;
    let picked = changed_from(&page, &Target::from(FIELD), "30.8890,75.8340").await?;
    let at = point(&picked);
    assert_eq!(at.to_string(), picked, "the field holds a point as it displays");
    page.assert_see(format!("{} ({picked})", nearest(at))).await?;
    page.assert_no_console_errors().await?;
    Ok(())
}

// Nothing is allowed for OpenStreetMap's host: no tile is asked for, the server's fallback stays
// in the map's place, and the page reports no policy violation.
#[tokio::test]
#[ignore = "needs Chrome or Chromium"]
async fn without_the_tile_host_allowed_the_map_shows_its_fallback_and_no_violation()
-> Result<(), BrowserError> {
    let shop = Shop::start(builder(OSM_TILES, None)).await?;
    let page = shop.open("/deliveries/new").await?;
    // The control's script asked where the tiles are, and was told none may be loaded.
    page.wait_for_request("/geo/tiles.json").await?;
    page.assert_see("Map unavailable").await?;
    page.assert_missing("css=.leaflet-container").await?;
    let asked: u64 = page
        .script(
            "return performance.getEntriesByType('resource').filter((e) => e.name.includes('tile.openstreetmap.org')).length",
        )
        .await?;
    assert_eq!(asked, 0, "a tile was asked for");
    page.assert_no_policy_violations().await?;
    page.assert_no_console_errors().await?;
    Ok(())
}

// The table's first load names the map's stylesheet in its head, a pick in a cell posts the cell
// once, and what was picked is what the table shows after a reload.
#[tokio::test]
#[ignore = "needs Chrome or Chromium"]
async fn the_drop_point_edits_in_place_with_its_styles_on_first_load() -> Result<(), BrowserError> {
    let shop = Shop::start(builder(OWN_TILES, None)).await?;
    let page = shop.open("/deliveries").await?;

    // The server wrote the link: it is in the answer a request gets, before any script ran.
    let sheet = MAP.sheet_url().expect("a stylesheet");
    let linked: bool = page
        .script(format!(
            "const html = await (await fetch(location.href)).text(); return html.includes('<link rel=\"stylesheet\" href=\"{sheet}\" data-ui-ext-link>');"
        ))
        .await?;
    assert!(linked, "the table's head does not link {sheet}");

    page.wait_for(DRAWN_MAP).await?;
    let first = Target::from(FIRST_CELL).nth(0);
    assert_eq!(page.value(first.clone()).await?, "30.8890,75.8340");
    click_map(&page, 90, 0).await?;
    let picked = changed_from(&page, &first, "30.8890,75.8340").await?;
    page.wait_for_request("/deliveries/1/drop").await?;
    let posts: u64 = page
        .script(
            "return performance.getEntriesByType('resource').filter((e) => e.name.endsWith('/deliveries/1/drop')).length",
        )
        .await?;
    assert_eq!(posts, 1, "a pick posts its cell once");

    page.refresh().await?;
    assert_eq!(page.value(first).await?, picked);
    Ok(())
}

// A pick outside the delivery area is refused by the server: the cell goes back to the point it
// had, with the refusal as a toast, and so does the pin.
#[tokio::test]
#[ignore = "needs Chrome or Chromium"]
async fn a_refused_edit_puts_the_pin_back() -> Result<(), BrowserError> {
    // Only a few streets of Sarabha Nagar are inside: a click a short way off is outside.
    let area = json!([{
        "name": "Sarabha Nagar",
        "south_west": "30.8880,75.8330",
        "north_east": "30.8900,75.8350",
    }]);
    let shop = Shop::start(builder(OWN_TILES, Some(area))).await?;
    let page = shop.open("/deliveries").await?;
    page.wait_for(DRAWN_MAP).await?;
    let first = Target::from(FIRST_CELL).nth(0);
    assert_eq!(page.value(first.clone()).await?, "30.8890,75.8340");

    click_map(&page, 90, 0).await?;
    page.assert_see("Pick a place inside the delivery area: Sarabha Nagar")
        .await?;
    page.assert_value(first, "30.8890,75.8340").await?;
    for _ in 0..50 {
        let offset = pin_offset(&page).await?;
        if (0.0..2.0).contains(&offset) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the pin did not come back to the middle of the map");
}

// The server moves a map by morphing its element with a higher count: the script is given the new
// props and flies to the new centre.
#[tokio::test]
#[ignore = "needs Chrome or Chromium"]
async fn a_new_pin_code_flies_the_map_through_new_props() -> Result<(), BrowserError> {
    let shop = Shop::start(builder(OWN_TILES, None)).await?;
    let page = shop.open("/map").await?;
    page.wait_for(DRAWN_MAP).await?;

    page.press("Fly to Model Town").await?;
    let (x, y) = tile_of(point("31.3080,75.5810"), 15);
    page.wait_for_request(format!("/geo-test/tiles/15/{x}/{y}"))
        .await?;
    page.assert_see("Model Town, Jalandhar (31.3080,75.5810)")
        .await?;
    page.assert_no_console_errors().await?;
    Ok(())
}

// The same map drawn as stored content, beside the real one, is never mounted: nothing in it
// starts a script, and no map is drawn in it.
#[tokio::test]
#[ignore = "needs Chrome or Chromium"]
async fn the_stored_twin_of_the_map_is_never_mounted() -> Result<(), BrowserError> {
    let shop = Shop::start(builder(OWN_TILES, None)).await?;
    let page = shop.open("/twin").await?;
    page.wait_for("css=#live-map[data-ui-mounted]").await?;
    page.wait_for(DRAWN_MAP).await?;

    let mounted: u64 = page
        .script(r#"return document.querySelectorAll('[data-slot="geo:map"][data-ui-mounted]').length"#)
        .await?;
    assert_eq!(mounted, 1, "only the real map is mounted");
    let in_twin: u64 = page
        .script(
            r#"return document.querySelectorAll('[data-ui-stored] [data-slot="geo:map"][data-ui-mounted], [data-ui-stored] .leaflet-container').length"#,
        )
        .await?;
    assert_eq!(in_twin, 0, "the twin was mounted");
    Ok(())
}

// "Use my location" is a button that submits nothing, and a page without a script does not show
// it.
#[tokio::test]
#[ignore = "needs Chrome or Chromium"]
async fn use_my_location_is_a_button_that_submits_nothing() -> Result<(), BrowserError> {
    let shop = Shop::start(builder(OWN_TILES, None)).await?;
    let page = shop.open("/deliveries/new").await?;
    page.wait_for(r#"css=[data-slot="geo:location"][data-ui-mounted]"#)
        .await?;
    page.assert_visible(r#"css=[data-slot="geo:locate"]"#).await?;

    let before = requests(&page).await?;
    page.press("Use my location").await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    page.assert_path_is("/deliveries/new").await?;
    assert_eq!(requests(&page).await?, before, "the button sent a request");

    // The rule that hides it waits for the runtime's mark on the page: without it, the button is
    // not shown.
    page.script::<bool>("document.documentElement.removeAttribute('data-ui-js'); return true")
        .await?;
    page.assert_missing(r#"css=[data-slot="geo:locate"]"#).await?;
    Ok(())
}
