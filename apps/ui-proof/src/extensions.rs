//! Extensions: components of the app's own and of a plugin, in a real browser: a script that mounts,
//! gets new props and leaves, bodies it owns and the seal around them, calls to the app's own
//! routes, a script that asks for another site's file, a sheet scoped to its components, pages that
//! start clean, an inline editor with a control of a plugin's own, and the plugin's map.
//!
//! | Page | Shows |
//! |---|---|
//! | `/extensions?step=` | a pad in a region whose link changes the step, a link to a page with a pad of its own, the pad's stored twin, a sheet-only component beside the page's own sign-out form and dialog, and a form whose pad is written into by a library |
//! | `/extensions/other` | another pad, which a boosted link comes to and leaves |
//! | `/extensions/outside` | a component whose script asks for another site's file |
//! | `/extensions/fresh` | a page that starts clean, with a password field |
//! | `/extensions/security` | a pad and a lazy region whose source is the page that starts clean |
//! | `/extensions/deliveries` | three deliveries whose drop point is edited in place with the map plugin's control |
//! | `/extensions/map` | the map plugin's map, tiles from this app, and a pad that writes two thousand cells at once |
//!
//! `GET /extensions/hits` says how often the two routes a library must not reach were reached;
//! `POST /extensions/reset` clears the counts and the drop points. `GET /proof/tiles/{z}/{x}/{y}`
//! is one grey square for the map to draw.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use http::Method;
use http::header::CONTENT_TYPE;
use navree::AppContext;
use navree::http::extract::urlencoded;
use navree::http::response::{IntoResponse, Response};
use navree::http::{AppError, Controller, Extension, Form, Json, Path, Query, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::form::{Control, Language};
use navree::ui::table::{EditCell, NoFilter};
use navree::ui::*;
use navree_plugin_geo::{GeoPoint, LOCATION, MapView, Tiles, nearest};
use serde::{Deserialize, Serialize};

/// The pad's files: a script that draws lines into its own body and calls the routes under
/// `/extensions/pad/`, and the sheet that boxes it. It keeps the `data-count` mark it sets.
pub static PAD: UiExtension = UiExtension::new("app", "pad")
    .script(extension_script!("extensions/pad.js", 16 * 1024))
    .sheet(extension_sheet!("extensions/pad.css", "app", 8 * 1024))
    .contract(1)
    .keeps(&["data-count"])
    .routes(&["/extensions/pad/"]);

/// A component whose script imports a file of another site, which the page's policy refuses.
pub static OUTSIDE: UiExtension = UiExtension::new("app", "outside")
    .script(extension_script!("extensions/outside.js", 1024))
    .contract(1);

/// A component of a stylesheet only, whose rules name forms and dialogs the page also has.
pub static STYLED: UiExtension = UiExtension::new("app", "styled").sheet(extension_sheet!(
    "extensions/styled.css",
    "app",
    8 * 1024
));

/// What the pad's script is given.
#[derive(Serialize)]
struct PadProps {
    step: u32,
    depot: &'static str,
    /// An address the server did not name as a route of the pad: the props hold it, and the pad
    /// asks to call it.
    next: Option<&'static str>,
    /// Whether the pad writes a library's markup into its body.
    vendor: bool,
}

fn props(step: u32) -> PadProps {
    PadProps {
        step,
        depot: "Ludhiana",
        next: Some("/account/email"),
        vendor: false,
    }
}

/// The buttons a pad draws beside its body.
#[derive(Clone, Copy)]
enum Buttons {
    Nothing,
    /// One that saves, one whose save fails and one that asks for an address it was not given.
    Calls,
    /// One that writes two thousand cells at once.
    Cells,
}

/// The pad: a root with the pad's files, its owned body, and the buttons asked for. The ids of the
/// buttons are the page's, so only one pad of a page asks for them.
fn pad(id: &'static str, props: &PadProps, buttons: Buttons) -> Element {
    let row = match buttons {
        Buttons::Nothing => None,
        Buttons::Calls => Some(Row::gap(2).wrap().children((
            Button::outline("Save for Harpreet Sandhu").id("call-save"),
            Button::outline("Save that fails").id("call-fail"),
            Button::outline("Call an address the server did not name").id("call-planted"),
        ))),
        Buttons::Cells => Some(
            Row::gap(2)
                .wrap()
                .children(Button::outline("Write 2,000 cells").id("add-cells")),
        ),
    };
    el("div")
        .slot("app:pad")
        .id(id)
        .extension_with(&PAD, props)
        .children((
            el("div").slot("app:pad-body").flag("data-ui-keep", true),
            row,
        ))
}

/// The pad as stored content would carry it, beside the live one: nothing in it mounts.
fn stored_pad() -> Element {
    let copy: Node = pad("stored-pad", &props(1), Buttons::Nothing).into();
    el("div").id("stored").child(kit::stored_twin(&copy))
}

/// A form that posts to the sign-out address.
fn sign_out(id: &'static str) -> Element {
    el("form")
        .id(id)
        .attr("method", "post")
        .attr("action", "/sign-out")
        .child(Button::outline("Sign out").submit())
}

/// A box the toolkit's dialog slot names, which a stylesheet could reach by that name.
fn dialog(id: &'static str) -> Element {
    el("div").id(id).slot("dialog")
}

/// The sheet-only component, with a sign-out form and a dialog inside it: its rules apply there.
fn styled() -> Element {
    el("div")
        .id("styled")
        .slot("app:styled")
        .extension(&STYLED)
        .children((sign_out("styled-sign-out"), dialog("styled-dialog")))
}

/// The form of a new delivery. Its pad is the vendor's: a library writes into its body.
fn delivery_form() -> Element {
    let vendor = PadProps {
        step: 1,
        depot: "Jalandhar",
        next: None,
        vendor: true,
    };
    el("form")
        .id("new-delivery")
        .attr("method", "post")
        .attr("action", "/extensions/deliveries")
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .child(
            Stack::gap(3).children((
                pad("vendor-pad", &vendor, Buttons::Nothing),
                Field::new("recipient", "Recipient")
                    .control(Input::text("recipient").value("Harpreet Sandhu")),
                Field::new("charge", "Charge in ₹").control(Input::number("charge").value("40")),
                Button::primary("Add delivery").submit().id("add-delivery"),
            )),
        )
}

/// `GET /extensions`.
fn index(shell: &AppShell, step: u32) -> AppPage {
    let links: Vec<Element> = [1, 2]
        .into_iter()
        .map(|to| {
            el("a")
                .href(format!("/extensions?step={to}"))
                .attr("id", format!("step-{to}"))
                .flag("data-ui-in-region", true)
                .child(format!("Step {to}"))
        })
        .collect();
    shell.page("Extensions").body(
        Stack::gap(6).children((
            Heading::h1("Extensions"),
            Region::new("pad-region")
                .src(format!("/extensions?step={step}"))
                .children(Stack::gap(3).children((
                    Row::gap(3).children(links),
                    pad("pad", &props(step), Buttons::Calls),
                ))),
            Row::gap(4).wrap().children((
                el("a")
                    .href("/extensions/other")
                    .attr("id", "to-other")
                    .child("Another pad"),
                el("a")
                    .href("/extensions/fresh")
                    .attr("id", "to-fresh")
                    .child("Confirm your password"),
            )),
            stored_pad(),
            Stack::gap(3).children((
                styled(),
                sign_out("sign-out"),
                dialog("dialog").child(Text::new(
                    "Delete this workspace? Meera Sharma will lose access to it.",
                )),
            )),
            delivery_form(),
        )),
    )
}

/// `GET /extensions/other`.
fn other(shell: &AppShell) -> AppPage {
    shell.page("Another pad").body(
        Stack::gap(6).children((
            Heading::h1("Another pad"),
            pad("other-pad", &props(1), Buttons::Nothing),
            el("a")
                .href("/extensions")
                .attr("id", "to-index")
                .child("Back to the pad"),
        )),
    )
}

/// `GET /extensions/outside`.
fn outside(shell: &AppShell) -> AppPage {
    shell.page("A component from another site").body(
        Stack::gap(6).children((
            Heading::h1("A component from another site"),
            el("div")
                .id("outside")
                .slot("app:outside")
                .extension(&OUTSIDE)
                .child("Map unavailable"),
        )),
    )
}

/// `GET /extensions/fresh`: a page that takes a password, so it starts from a clean document.
fn fresh() -> View {
    View::new("Confirm your password").fresh().body(
        el("main").child(
            Stack::gap(3).children((
                Heading::h1("Confirm it is you"),
                el("form")
                    .attr("method", "post")
                    .attr("action", "/extensions/fresh")
                    .child(
                        Stack::gap(3).children((
                            Field::new("password", "Password")
                                .control(Input::password("password").required()),
                            Button::primary("Confirm").submit(),
                        )),
                    ),
            )),
        ),
    )
}

/// `GET /extensions/security`: a pad, and a lazy region whose source is the page above.
fn security(shell: &AppShell) -> AppPage {
    shell.page("Security").body(Stack::gap(6).children((
        Heading::h1("Security"),
        pad("security-pad", &props(1), Buttons::Nothing),
        Region::new("security").src("/extensions/fresh").lazy(),
    )))
}

/// The deliveries: where each one is to be dropped.
const DELIVERIES: [(i64, &str, &str, &str); 3] = [
    (1, "Harpreet Sandhu", "Ludhiana", "30.8890,75.8340"),
    (2, "Meera Sharma", "Jalandhar", "31.3080,75.5810"),
    (3, "Simran Kaur", "Amritsar", "31.6550,74.8590"),
];

/// A row of the deliveries table.
#[derive(Clone, TableRow)]
#[table(sort = "recipient", per = 25)]
struct DeliveryRow {
    #[col(skip)]
    id: i64,
    #[col(sortable, label = "Recipient")]
    recipient: String,
    #[col(label = "City")]
    city: String,
    #[col(label = "Drop point")]
    drop_at: String,
}

/// The drop points edited since the last reset, by delivery.
static DROPS: Mutex<BTreeMap<i64, String>> = Mutex::new(BTreeMap::new());

/// How often a library's address `/account/email` and the delivery form's post were reached.
static EMAIL: AtomicU64 = AtomicU64::new(0);
static POSTED: AtomicU64 = AtomicU64::new(0);

/// The fields of the last post of the delivery form.
static LAST: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// `GET /extensions/deliveries`: each drop point is edited in its cell by the map plugin's
/// control.
fn deliveries(shell: &AppShell) -> AppPage {
    let rows: Vec<DeliveryRow> = {
        let drops = DROPS.lock().unwrap_or_else(PoisonError::into_inner);
        DELIVERIES
            .iter()
            .map(|(id, recipient, city, at)| DeliveryRow {
                id: *id,
                recipient: (*recipient).to_owned(),
                city: (*city).to_owned(),
                drop_at: drops.get(id).cloned().unwrap_or_else(|| (*at).to_owned()),
            })
            .collect()
    };
    let query = TableQuery::<DeliveryRow, NoFilter>::parse("/extensions/deliveries", "");
    shell
        .page("Deliveries")
        .body(
            DataTable::new("deliveries", &query, &rows).cell("drop_at", |row| {
                EditCell::control(
                    row.drop_at.clone(),
                    format!("/extensions/deliveries/{}/drop", row.id),
                    Control::new(&LOCATION),
                )
                .label("Drop point")
            }),
        )
}

/// `POST /extensions/deliveries/{id}/drop`: a drop point edited in its cell, held to the delivery
/// area before it is kept.
async fn drop_point(id: i64, ctx: AppContext, edit: EditedValue<GeoPoint>) -> Reply {
    let language = Language::of(Some(&ctx)).await;
    match edit.read(Control::new(&LOCATION), &language) {
        Ok(at) => {
            DROPS
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id, at.to_string());
            Reply::ops(Vec::new())
        }
        Err(refused) => refused.reply("/extensions/deliveries"),
    }
}

/// `GET /extensions/map`: the map plugin's map, and a pad that writes a great many cells.
fn map(shell: &AppShell, tiles: &Tiles) -> AppPage {
    let at = GeoPoint {
        lat: 30.8890,
        lng: 75.8340,
    };
    shell.page("Map").body(Stack::gap(6).children((
        Heading::h1("Map"),
        MapView::new("drop-map", at, nearest(at), tiles),
        pad("map-pad", &props(1), Buttons::Cells),
    )))
}

/// What `POST /extensions/pad/save` reads: the step the pad was at.
#[derive(Deserialize)]
struct Save {
    n: u32,
}

/// `POST /extensions/pad/save`: the pad drawn again at the next step, and a toast.
async fn save(Form(form): Form<Save>) -> Reply {
    let step = form.n.saturating_add(1).min(99);
    Reply::ops(Vec::new())
        .morph("pad", pad("pad", &props(step), Buttons::Calls))
        .toast(ToastTone::Success, "Saved for Harpreet Sandhu")
}

/// `POST /extensions/pad/fail`: an answer that is a failure and nothing else.
async fn fail() -> Result<Json<u8>, AppError> {
    Err(AppError::internal("The save for Harpreet Sandhu failed"))
}

/// `POST /account/email`: the route a library must not reach with the page's token.
async fn account_email() -> Reply {
    EMAIL.fetch_add(1, Ordering::SeqCst);
    Reply::ops(Vec::new())
}

/// `POST /extensions/deliveries`: the delivery form; the fields it posted are kept for
/// `/extensions/hits`.
async fn new_delivery(body: String) -> Reply {
    POSTED.fetch_add(1, Ordering::SeqCst);
    *LAST.lock().unwrap_or_else(PoisonError::into_inner) = urlencoded::parse(body.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();
    Reply::ops(Vec::new()).toast(ToastTone::Success, "Delivery added for Meera Sharma")
}

/// A text as a JSON string: quotes and backslashes escaped, control characters made spaces.
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// `GET /extensions/hits`: `{"email": n, "deliveries": n, "fields": [[name, value], ..]}`.
fn hits() -> Response {
    let fields = LAST
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|(name, value)| format!("[{},{}]", quoted(name), quoted(value)))
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        r#"{{"email":{},"deliveries":{},"fields":[{}]}}"#,
        EMAIL.load(Ordering::SeqCst),
        POSTED.load(Ordering::SeqCst),
        fields
    );
    ([(CONTENT_TYPE, "application/json")], body).into_response()
}

/// `POST /extensions/reset`: the counts, the last post and the drop points as they began.
fn reset() -> http::StatusCode {
    EMAIL.store(0, Ordering::SeqCst);
    POSTED.store(0, Ordering::SeqCst);
    LAST.lock().unwrap_or_else(PoisonError::into_inner).clear();
    DROPS.lock().unwrap_or_else(PoisonError::into_inner).clear();
    http::StatusCode::NO_CONTENT
}

/// One grey square, which a map draws as every tile.
const TILE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256"><rect width="256" height="256" fill="#e8ecef" stroke="#c8cdd1"/></svg>"##;

/// `GET /proof/tiles/{z}/{x}/{y}`.
fn tile() -> Response {
    ([(CONTENT_TYPE, "image/svg+xml")], TILE).into_response()
}

/// What `GET /extensions` reads.
#[derive(Deserialize)]
struct Asked {
    step: Option<u32>,
}

pub struct ExtensionPages;

impl Injectable for ExtensionPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for ExtensionPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/extensions",
                |shell: AppShell, Query(asked): Query<Asked>| async move {
                    index(&shell, asked.step.unwrap_or(1).clamp(1, 99))
                },
            )
            .public()
            .page(),
            RouteDef::on(
                Method::GET,
                "!/extensions/other",
                |shell: AppShell| async move { other(&shell) },
            )
            .public()
            .page(),
            RouteDef::on(
                Method::GET,
                "!/extensions/outside",
                |shell: AppShell| async move { outside(&shell) },
            )
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/extensions/fresh", || async { fresh() })
                .public()
                .page(),
            RouteDef::on(Method::POST, "!/extensions/fresh", || async {
                Reply::ops_or_redirect("/extensions")
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/extensions/security",
                |shell: AppShell| async move { security(&shell) },
            )
            .public()
            .page(),
            RouteDef::on(
                Method::GET,
                "!/extensions/deliveries",
                |shell: AppShell| async move { deliveries(&shell) },
            )
            .public()
            .page(),
            RouteDef::on(
                Method::GET,
                "!/extensions/map",
                |shell: AppShell, tiles: Tiles| async move { map(&shell, &tiles) },
            )
            .public()
            .page(),
            RouteDef::on(Method::POST, "!/extensions/pad/save", save).public(),
            RouteDef::on(Method::POST, "!/extensions/pad/fail", fail)
                .public()
                .json_only(),
            RouteDef::on(Method::POST, "!/account/email", account_email).public(),
            RouteDef::on(Method::POST, "!/extensions/deliveries", new_delivery).public(),
            RouteDef::on(
                Method::POST,
                "!/extensions/deliveries/{id}/drop",
                |Path(id): Path<i64>,
                 Extension(ctx): Extension<AppContext>,
                 edit: EditedValue<GeoPoint>| async move {
                    drop_point(id, ctx, edit).await
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/extensions/hits", || async { hits() }).public(),
            RouteDef::on(Method::POST, "!/extensions/reset", || async { reset() }).public(),
            RouteDef::on(Method::GET, "!/proof/tiles/{z}/{x}/{y}", || async {
                tile()
            })
            .public(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pads_and_the_roots_around_them_follow_the_rules_of_a_component() {
        for buttons in [Buttons::Nothing, Buttons::Calls, Buttons::Cells] {
            kit::assert_component(&pad("pad", &props(1), buttons).into());
        }
        kit::assert_component(&styled().into());
        kit::assert_component(&stored_pad().into());
        kit::assert_component(&delivery_form().into());
    }

    #[test]
    fn the_proofs_own_files_are_served_under_the_app_namespace() {
        assert!(
            PAD.script_url()
                .is_some_and(|url| url.starts_with("/_navree/ui/ext/app.pad-"))
        );
        assert!(
            PAD.sheet_url()
                .is_some_and(|url| url.starts_with("/_navree/ui/ext/app.pad-"))
        );
        assert!(OUTSIDE.sheet_url().is_none());
        assert!(STYLED.script_url().is_none());
        assert!(
            STYLED
                .sheet_url()
                .is_some_and(|url| url.starts_with("/_navree/ui/ext/app.styled-"))
        );
    }
}
