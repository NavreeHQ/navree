//! The geo plugin booted in an app: the rules its map and its field keep, the delivery area held
//! wherever a drop point is read with the app (and every point refused where it is read with
//! none), and what the plugin asks the page policy for. A test binary of its own: it reads the
//! lines the boot logs.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use http::{Method, StatusCode};
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, Extension, HttpModule, ModuleBuilderHttpExt as _, Path, RouteDef};
use navree::testing::{TestApp, TestAppBuilder, TestResponse, principal};
use navree::ui::form::{Control, Language, Rejected, decode, decode_for};
use navree::ui::{
    Directive, EditedValue, Form, FormView, Money, Node, PAGE_CSP, PagePolicy, Reply, Submit,
    TestView, UiModule, Validate, View, kit, render,
};
use navree::validation::ValidationModule;
use navree::{AppContext, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_plugin_geo::{GeoModule, GeoOptions, GeoPoint, LOCATION, MAP, MapView, Tiles};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

const FORM: &str = "application/x-www-form-urlencoded";

/// A browser's `Accept`.
const BROWSER: &str = "text/html,application/xhtml+xml,*/*;q=0.8";

/// What a person is told when a point is outside the delivery area, and when it is not a point.
const REFUSAL: &str = "Pick a place inside the delivery area: Ludhiana, Jalandhar or Amritsar";
const NOT_A_POINT: &str = "Enter the point as latitude,longitude, such as 30.9010,75.8573";

/// What a person is told when the check has no delivery area to read.
const UNREADABLE: &str = "The delivery area cannot be checked right now";

/// A tile of the map, as OpenStreetMap's host would be asked for it.
const TILE: &str = "https://tile.openstreetmap.org/13/5890/3400.png";

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

/// The drop point stored for each delivery.
static DROPS: Mutex<BTreeMap<u64, GeoPoint>> = Mutex::new(BTreeMap::new());

fn point(text: &str) -> GeoPoint {
    text.parse().expect("a point")
}

/// `POST /deliveries`: a new delivery, with its drop point held to the delivery area.
async fn new_delivery(submit: Result<Submit<NewDelivery>, Rejected<NewDelivery>>) -> Response {
    match submit {
        Ok(Submit(delivery)) => format!(
            "taken {} at {} for {}",
            delivery.recipient,
            delivery.drop_at,
            delivery.charge.display()
        )
        .into_response(),
        Err(rejected) => rejected.into_response(),
    }
}

/// `POST /deliveries/{id}/drop`: a new drop point typed or picked in a table cell, held to the
/// delivery area before it is stored.
async fn edit_drop(
    Path(id): Path<u64>,
    Extension(ctx): Extension<AppContext>,
    edit: EditedValue<GeoPoint>,
) -> Reply {
    let language = Language::of(Some(&ctx)).await;
    match edit.read(Control::new(&LOCATION), &language) {
        Ok(at) => {
            DROPS.lock().expect("the stored drop points").insert(id, at);
            Reply::ops(Vec::new())
        }
        Err(refused) => refused.reply("/deliveries"),
    }
}

struct Pages;

impl Injectable for Pages {
    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "!/deliveries", new_delivery).authenticated(),
            RouteDef::on(Method::POST, "!/deliveries/{id}/drop", edit_drop).authenticated(),
            RouteDef::on(Method::GET, "!/map", |tiles: Tiles| async move {
                View::new("Map").body(MapView::new(
                    "drop-map",
                    point("30.8890,75.8340"),
                    "Sarabha Nagar, Ludhiana",
                    &tiles,
                ))
            })
            .public()
            .page(),
        ]
    }
}

struct App;

impl Module for App {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
    }
}

/// An app with the toolkit, the plugin and the pages above; requests are signed in with the test
/// authenticator.
fn builder() -> TestAppBuilder {
    TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(HttpModule::for_root(Default::default()))
        .module(ValidationModule)
        .module(UiModule)
        .module(GeoModule)
        .module(App)
}

/// A post by Harpreet Sandhu, as the runtime sends it when `runtime` is set and as a browser
/// without JavaScript does when it is not.
async fn post(app: &TestApp, path: &str, body: &str, runtime: bool) -> TestResponse {
    let mut request = app
        .acting_as(principal(Uuid::from_u128(7)))
        .post(path)
        .header("content-type", FORM);
    if runtime {
        request = request.header("x-navree-ui", "1");
    }
    request.body(body.to_owned()).send().await
}

/// A read by Harpreet Sandhu.
async fn get_as_member(app: &TestApp, path: &str) -> TestResponse {
    app.acting_as(principal(Uuid::from_u128(7)))
        .get(path)
        .send()
        .await
}

/// The page at `path`, asked for as a browser does.
async fn page(app: &TestApp, path: &str) -> TestResponse {
    app.client()
        .get(path)
        .header("accept", BROWSER)
        .send()
        .await
        .assert_status(200)
}

/// Captures formatted tracing output written while the guard is alive (on this thread: use a
/// current-thread runtime).
#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_ansi(false)
            .with_writer(self.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }

    fn lines(&self) -> Vec<String> {
        String::from_utf8_lossy(&self.0.lock().unwrap())
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl std::io::Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// One boot at a time in this binary. A test below reads the lines a boot logs on its own thread,
/// and a boot running on another thread at the same moment, with nobody listening there, can
/// leave one of those lines unheard.
static ONE_BOOT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Whether the log holds a line of `level` with `text` in it.
fn logged(capture: &LogCapture, level: &str, text: &str) -> bool {
    capture
        .lines()
        .iter()
        .any(|line| line.contains(level) && line.contains(text))
}

// The map and the field draw what Navree UI's rules for a component and for a field control ask
// for, and name the plugin's own files.
#[test]
fn the_map_and_the_location_control_follow_the_rules() {
    let tiles = Tiles::from_parts(None, "© OpenStreetMap contributors".into());
    let map: Node = MapView::new(
        "drop-map",
        point("30.8890,75.8340"),
        "Sarabha Nagar, Ludhiana",
        &tiles,
    )
    .into();
    kit::assert_component(&map);
    kit::assert_control(&LOCATION);

    let html = render(&map);
    assert!(html.contains(MAP.script_url().expect("a script")), "{html}");
    assert!(
        html.contains(MAP.sheet_url().expect("a stylesheet")),
        "{html}"
    );
    assert!(html.contains(r#"data-slot="geo:map""#), "{html}");
    assert!(
        html.contains(r#"data-slot="geo:map-body" data-ui-keep"#),
        "the body is the script's own: {html}"
    );
    assert!(
        html.contains("Sarabha Nagar, Ludhiana (30.8890,75.8340)"),
        "the place as text: {html}"
    );
    assert!(TestView::of(&View::new("Delivery").body(map)).uses_extension(&MAP));

    // The field, drawn by a form: the point's text field first, then the rest of the control.
    let form = render(&FormView::<NewDelivery>::new().post("/deliveries").into());
    assert!(form.contains(r#"data-slot="geo:location""#), "{form}");
    assert!(form.contains(r#"name="drop_at""#), "{form}");
    assert!(form.contains(r#"placeholder="30.9010,75.8573""#), "{form}");
    assert!(form.contains(r#"data-slot="geo:locate""#), "{form}");
    assert!(form.contains(MAP.script_url().expect("a script")), "{form}");
}

// The delivery area is held to on every path the point arrives by: the form, with and without a
// script, and the inline edit of a table cell.
#[tokio::test]
async fn a_point_outside_the_delivery_area_is_refused_in_the_form_and_in_the_table() {
    let _one = ONE_BOOT.lock().await;
    let app = builder().build().await;

    // The form: Delhi is outside, and the refusal stands on the field.
    let delhi = "recipient=Harpreet+Sandhu&drop_at=28.6139%2C77.2090&charge=40";
    let response = post(&app, "/deliveries", delhi, true).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = response.json_value();
    assert_eq!(body["ops"][0]["op"], "errors", "{body}");
    assert_eq!(body["ops"][0]["fields"], json!({ "drop_at": [REFUSAL] }));

    // With JavaScript off the form is drawn again with the refusal under the field.
    let response = post(&app, "/deliveries", delhi, false).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.text().contains(REFUSAL), "{}", response.text());

    // A text that is not a point says how to write one.
    let response = post(
        &app,
        "/deliveries",
        "recipient=Harpreet+Sandhu&drop_at=north&charge=40",
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.json_value()["ops"][0]["fields"],
        json!({ "drop_at": [NOT_A_POINT] })
    );

    // Sarabha Nagar is inside: the delivery is taken, in rupees.
    let sarabha = "recipient=Harpreet+Sandhu&drop_at=30.8890%2C75.8340&charge=40";
    let response = post(&app, "/deliveries", sarabha, true).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .text()
            .contains("taken Harpreet Sandhu at 30.8890,75.8340 for 40.00"),
        "{}",
        response.text()
    );

    // The table: the same point is refused with a toast, and nothing is stored.
    let response = post(&app, "/deliveries/42/drop", "value=28.6139%2C77.2090", true).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = response.json_value();
    assert_eq!(body["ops"][0]["op"], "toast", "{body}");
    assert_eq!(body["ops"][0]["tone"], "danger");
    assert_eq!(body["ops"][0]["text"], REFUSAL);
    assert!(
        DROPS.lock().expect("the stored drop points").is_empty(),
        "a refused point was stored"
    );

    // A value that is not a point is refused the same way, with how to write one.
    let response = post(&app, "/deliveries/42/drop", "value=north", true).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.json_value()["ops"][0]["text"], NOT_A_POINT);

    // A point inside the area is stored.
    let response = post(&app, "/deliveries/42/drop", "value=30.8890%2C75.8340", true).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        DROPS
            .lock()
            .expect("the stored drop points")
            .get(&42)
            .copied(),
        Some(point("30.8890,75.8340"))
    );
}

/// A delivery to Harpreet Sandhu with its drop point at `drop_at`, as a form posts it.
fn delivery_at(drop_at: &str) -> Vec<(String, String)> {
    [
        ("recipient", "Harpreet Sandhu"),
        ("drop_at", drop_at),
        ("charge", "40"),
    ]
    .iter()
    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
    .collect()
}

// With no app the check has no delivery area to read, and refuses every point, one inside the
// default area too. In an app that names an area of its own, a point of the default area outside
// it is refused wherever it is read with the app.
#[tokio::test]
async fn the_drop_point_check_refuses_without_an_app_and_holds_to_the_apps_own_area() {
    let sarabha = delivery_at("30.8890,75.8340");
    assert_eq!(
        decode::<NewDelivery>(&sarabha).unwrap_err()["drop_at"],
        [UNREADABLE]
    );
    assert_eq!(
        decode_for::<NewDelivery>(&sarabha, &Language::english()).unwrap_err()["drop_at"],
        [UNREADABLE]
    );

    let _one = ONE_BOOT.lock().await;
    let ludhiana = json!([{
        "name": "Ludhiana",
        "south_west": "30.80,75.70",
        "north_east": "31.00,75.98",
    }]);
    let app = builder().config("geo.area", ludhiana).build().await;
    let language = Language::of(Some(app.ctx())).await;
    let refusal = ["Pick a place inside the delivery area: Ludhiana"];
    let jalandhar = delivery_at("31.3080,75.5810");
    assert_eq!(
        decode_for::<NewDelivery>(&jalandhar, &language).unwrap_err()["drop_at"],
        refusal
    );
    assert!(decode_for::<NewDelivery>(&sarabha, &language).is_ok());

    let body = "recipient=Harpreet+Sandhu&drop_at=31.3080%2C75.5810&charge=40";
    let response = post(&app, "/deliveries", body, true).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.json_value()["ops"][0]["fields"],
        json!({ "drop_at": refusal })
    );
    app.shutdown().await.unwrap();
}

// The plugin boots with the toolkit, asks for the tile host and gets nothing until the app says
// yes; what it serves and answers is the same either way, but the tiles.
#[tokio::test(flavor = "current_thread")]
async fn geo_boots_in_test_app_and_asks_for_the_tile_host() {
    let _one = ONE_BOOT.lock().await;
    let capture = LogCapture::default();
    let _guard = capture.install();

    // Asked and not allowed: a warning that says how to allow it, and no tile address.
    let app = builder().build().await;
    assert!(logged(
        &capture,
        "WARN",
        "page policy: plugin `geo` asks for img-src https://tile.openstreetmap.org (map tiles), which the app does not allow; what needs it shows its fallback (allow it with ui.policy.allow.geo.img_src)"
    ));
    let policy = app.get::<PagePolicy>().await;
    assert!(!policy.allows(Directive::ImgSrc, TILE));
    let options = app.get::<GeoOptions>().await;
    assert_eq!(Tiles::new(&options, &policy).address(), None);

    let script = app
        .client()
        .get(MAP.script_url().expect("a script"))
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        script.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );

    // The routes are for a signed-in reader.
    app.client()
        .get("/geo/tiles.json")
        .send()
        .await
        .assert_status(401);
    let tiles = get_as_member(&app, "/geo/tiles.json")
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(tiles["tiles"], Value::Null, "{tiles}");
    assert_eq!(tiles["attribution"], "© OpenStreetMap contributors");
    assert_eq!(tiles["centre"], json!({ "lat": 30.901, "lng": 75.8573 }));
    assert_eq!(tiles["zoom"], 13);

    // A point's place is named by a morph of the line the page named.
    let reply = post(
        &app,
        "/geo/reverse",
        "at=30.8890%2C75.8340&target=drop-place",
        true,
    )
    .await
    .assert_status(200)
    .json_value();
    assert_eq!(reply["ops"][0]["op"], "morph", "{reply}");
    assert_eq!(reply["ops"][0]["target"], "drop-place");
    let html = reply["ops"][0]["html"].as_str().expect("the new line");
    assert!(html.contains("Sarabha Nagar, Ludhiana"), "{html}");
    assert!(html.contains(r#"id="drop-place""#), "{html}");
    // Only the id of a place line is redrawn.
    let response = post(
        &app,
        "/geo/reverse",
        "at=30.8890%2C75.8340&target=account",
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // A page with a map keeps the toolkit's policy, and names the plugin's files.
    let map = page(&app, "/map").await;
    assert_eq!(map.header("content-security-policy"), Some(PAGE_CSP));
    assert!(map.text().contains(MAP.script_url().expect("a script")));
    app.shutdown().await.unwrap();

    // Allowed: an info line, the host in the policy and in the page header, and a tile address.
    let app = builder()
        .config(
            "ui.policy.allow.geo.img_src",
            json!(["https://tile.openstreetmap.org"]),
        )
        .build()
        .await;
    assert!(logged(
        &capture,
        "INFO",
        "page policy: plugin `geo` asks for img-src https://tile.openstreetmap.org (map tiles): allowed"
    ));
    let policy = app.get::<PagePolicy>().await;
    assert!(policy.allows(Directive::ImgSrc, TILE));
    assert!(
        policy
            .header()
            .contains("img-src 'self' data: https://tile.openstreetmap.org")
    );
    let options = app.get::<GeoOptions>().await;
    assert_eq!(
        Tiles::new(&options, &policy).address().as_deref(),
        Some("https://tile.openstreetmap.org/{z}/{x}/{y}.png")
    );
    let tiles = get_as_member(&app, "/geo/tiles.json")
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        tiles["tiles"],
        "https://tile.openstreetmap.org/{z}/{x}/{y}.png"
    );
    let map = page(&app, "/map").await;
    assert!(
        map.header("content-security-policy").is_some_and(
            |policy| policy.contains("img-src 'self' data: https://tile.openstreetmap.org")
        ),
        "{:?}",
        map.header("content-security-policy")
    );
    app.shutdown().await.unwrap();

    // Tiles served by the app itself need nothing allowed: a path of this app is its own origin.
    let app = builder()
        .config("geo.tiles", "/geo-test/tiles/{z}/{x}/{y}")
        .build()
        .await;
    let tiles = get_as_member(&app, "/geo/tiles.json")
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(tiles["tiles"], "/geo-test/tiles/{z}/{x}/{y}");
    app.shutdown().await.unwrap();
}
