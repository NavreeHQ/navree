//! Error pages, sign-in and the states inside a page, in every case the conformance
//! suite runs against.
//!
//! | Page | Shows |
//! |---|---|
//! | `/pages` | every state, in every size, and the empty states |
//! | `/pages/status/{code}` | a page route that ends in that status |
//! | `/pages/user` | a page that needs a signed-in user |
//! | `/pages/leak`, `/pages/panic` | a failure with secrets in it |
//! | `/pages/regions` | regions whose fetches end in each status; ones that poll a page that is gone, a server that keeps failing (500), one that asks to wait (429 with `Retry-After`) and a shut door that opens; a feed that gets 403; a region cut off, and one that gets an HTTP error that is not a state |
//! | `/pages/actions` | forms whose posts end in each status |
//! | `/pages/rows` | a table with locked, missing and empty rows |
//! | `/sign-in` | the sign-in page: it reads `next` and goes back there |
//! | `/pages/frag/{code}`, `/pages/frag/{name}` | the fragments the regions fetch: a status, or `ok`, `flaky`, `offline`, `poll500`, `poll429`, `door`, `feed403`, `json500` |
//! | `/pages/status/{code}`, `/pages/user`, `/pages/leak`, `/pages/panic`, `/pages/data` | page routes that end in an error (`/pages/data` is `json_only`) |
//! | `/pages/act/{code}` | the posts of `/pages/actions` |
//!
//! `GET /pages/hits` says how many times each fragment was asked for; `POST /pages/reset` clears
//! the counts and the fragments that change as they are asked.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use http::request::Parts;
use http::{Method, StatusCode};
use navree::contracts::{AuthVia, Principal};
use navree::http::extract::{Path, Query, RawQuery};
use navree::http::response::{IntoResponse, Response};
use navree::http::{AppError, Authenticator, Controller, Form, Json, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::*;
use serde::Deserialize;

/// What a failure of the app has inside it. None of this may reach a page.
const SECRET: &str =
    "db password=hunter2; SELECT * FROM users WHERE token='abc'; /home/ubuntu/app/src/db.rs:42:7";

/// The statuses a page route here can end in.
const STATUSES: [u16; 14] = [
    401, 403, 404, 405, 409, 410, 419, 422, 429, 500, 502, 503, 400, 418,
];

/// The cookie that makes a request come from a signed-in user.
pub const COOKIE: &str = "proof_user";

/// Signs a request in when it carries the cookie `proof_user`.
pub struct CookieAuth;

impl Authenticator for CookieAuth {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let signed_in = parts
            .headers
            .get_all(http::header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .any(|pair| {
                pair.trim()
                    .strip_prefix("proof_user=")
                    .is_some_and(|v| !v.is_empty())
            });
        Box::pin(async move {
            Ok(signed_in.then(|| Principal {
                user_id: uuid::Uuid::from_u128(0xada),
                via: AuthVia::Session,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}

/// How often each fragment was asked for.
static HITS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
/// How many times the flaky fragment has been asked for.
static FLAKY: AtomicU64 = AtomicU64::new(0);
/// How many times the door has been asked for.
static DOOR: AtomicU64 = AtomicU64::new(0);

fn hit(name: &str) -> u64 {
    let mut hits = HITS.lock().unwrap_or_else(PoisonError::into_inner);
    let count = hits.entry(name.to_owned()).or_insert(0);
    *count += 1;
    *count
}

fn failure(code: u16) -> AppError {
    match code {
        401 => AppError::unauthenticated(),
        403 => AppError::forbidden(),
        404 => AppError::not_found(),
        409 => AppError::conflict("Someone else renamed this deal a minute ago."),
        419 => AppError::page_expired(),
        429 => AppError::too_many_requests(Duration::from_secs(12)),
        500 => AppError::internal(SECRET),
        503 => AppError::service_unavailable("Back at noon."),
        other => {
            AppError::from_status(StatusCode::from_u16(other).unwrap_or(StatusCode::BAD_REQUEST))
        }
    }
}

fn known(code: u16) -> Result<u16, AppError> {
    if STATUSES.contains(&code) {
        Ok(code)
    } else {
        Err(AppError::not_found())
    }
}

fn section(id: &'static str, title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("id", id)
        .children((Heading::h2(title), Stack::gap(3).children(content)))
}

fn kinds() -> Vec<Node> {
    StateKind::ALL
        .into_iter()
        .map(|kind| {
            el("div")
                .attr("data-case", kind.name())
                .child(
                    ErrorPage::of(kind)
                        .sign_in("/sign-in?next=%2Fpages")
                        .retry_to("/pages")
                        .reference("req-01hx7")
                        .state(None, false)
                        .size(StateSize::Panel)
                        .id(format!("kind-{}", kind.name())),
                )
                .into()
        })
        .collect()
}

fn inline() -> Vec<Node> {
    let here = ReturnTo::parse("/pages?tab=notes");
    let tabs = Tabs::new("states-tabs", "A record")
        .param("tab")
        .tab(Tab::new("overview", "Overview").panel(Text::new("The overview panel.")))
        .tab(Tab::new("notes", "Notes").panel(Forbidden::new().what("notes").id("tab-forbidden")))
        .tab(Tab::new("files", "Files").panel(NotFound::new().what("file list").id("tab-missing")));
    vec![
        el("div")
            .attr("data-case", "card-forbidden")
            .child(
                Card::titled("Revenue").body(Forbidden::new().what("report").id("card-forbidden")),
            )
            .into(),
        el("div")
            .attr("data-case", "card-forbidden-access")
            .child(
                Card::titled("Contracts").body(
                    Forbidden::new()
                        .what("contract")
                        .request_access("/pages?request=1")
                        .id("card-access"),
                ),
            )
            .into(),
        el("div")
            .attr("data-case", "card-not-found")
            .child(Card::titled("Owner").body(NotFound::new().what("deal").id("card-missing")))
            .into(),
        el("div")
            .attr("data-case", "card-sign-in")
            .child(Card::titled("Your deals").body({
                let state = SignInRequired::new("/sign-in")
                    .what("your deals")
                    .id("card-sign-in");
                match &here {
                    Some(here) => state.return_to(here),
                    None => state,
                }
            }))
            .into(),
        el("div").attr("data-case", "tabs").child(tabs).into(),
        el("div")
            .attr("data-case", "compact")
            .child(
                Card::titled("Compact").body(
                    Stack::gap(2).children((
                        Forbidden::new()
                            .what("deal")
                            .compact()
                            .id("compact-forbidden"),
                        NotFound::new().what("deal").compact().id("compact-missing"),
                        EmptyState::caught_up().compact().id("compact-empty"),
                    )),
                ),
            )
            .into(),
    ]
}

fn empties() -> Vec<Node> {
    vec![
        el("div")
            .attr("data-case", "nothing-yet")
            .child(
                EmptyState::nothing_yet("deals")
                    .description("Create a deal to start tracking your pipeline.")
                    .action(Button::primary("New deal").icon(icons::PLUS))
                    .id("empty-nothing"),
            )
            .into(),
        el("div")
            .attr("data-case", "no-results")
            .child(
                EmptyState::no_results()
                    .clear_filters("/pages")
                    .id("empty-none"),
            )
            .into(),
        el("div")
            .attr("data-case", "caught-up")
            .child(EmptyState::caught_up().id("empty-done"))
            .into(),
    ]
}

fn table() -> Node {
    let row = |name: &'static str, amount: &'static str| {
        TableRowEl::new((TableCell::new(name), TableCell::new(amount).numeric()))
            .key(name.to_ascii_lowercase())
    };
    Table::new()
        .caption("Deals, one of which you can no longer open")
        .head((TableHead::new("Name"), TableHead::new("Amount").numeric()))
        .rows((
            row("Agra", "1,200.00"),
            Forbidden::new().row(2),
            row("Gulati", "3,400.00"),
            NotFound::new().what("deal").row(2),
            EmptyState::no_results().row(2),
            row("Ilaichi", "9,900.00"),
        ))
        .into()
}

/// The words of a locale that reads right to left, to show that the states follow the page.
fn right_to_left() -> Node {
    Direction::rtl(
        State::custom("empty", icons::INBOX, "لا توجد عناصر بعد")
            .description("أنشئ عنصراً للبدء.")
            .id("rtl"),
    )
    .into()
}

fn gallery(shell: &AppShell) -> AppPage {
    shell.page("States").body(
        Stack::gap(8).children((
            section(
                "kinds",
                "Every kind",
                Grid::cols(2).gap(4).children(kinds()),
            ),
            section(
                "inline",
                "In a card, a tab and a list",
                Grid::cols(2).gap(4).children(inline()),
            ),
            section("empty", "Empty", Grid::cols(2).gap(4).children(empties())),
            section("rows", "In a table", table()),
            section("direction", "Right to left", right_to_left()),
            section(
                "links",
                "Pages that end in an error",
                Row::gap(2).wrap().children(
                    [401, 403, 404, 409, 410, 419, 422, 429, 500, 503]
                        .map(|code| {
                            Button::outline(code.to_string())
                                .size(Size::Sm)
                                .href(format!("/pages/status/{code}"))
                        })
                        .into_iter()
                        .collect::<Vec<_>>(),
                ),
            ),
        )),
    )
}

fn regions(shell: &AppShell) -> AppPage {
    let region = |code: u16| {
        Region::new(format!("r-{code}"))
            .src(format!("/pages/frag/{code}"))
            .lazy()
    };
    shell.page("Regions").body(
        Stack::gap(6).children((
            section(
                "region-cases",
                "Regions that fail",
                Grid::cols(2).gap(4).children(
                    [401, 403, 404, 410, 429, 500, 503]
                        .map(|code| Card::titled(format!("Region {code}")).body(region(code)))
                        .into_iter()
                        .collect::<Vec<_>>(),
                ),
            ),
            section(
                "region-poll",
                "A region that polls a page that is gone",
                Card::titled("Polling").body(
                    Region::new("r-poll")
                        .src("/pages/frag/404?poll=1")
                        .poll(Duration::from_millis(400))
                        .children(Text::new("Waiting for the first answer.")),
                ),
            ),
            section(
                "region-flaky",
                "A region that fails twice, then works",
                Card::titled("Flaky").body(Region::new("r-flaky").src("/pages/frag/flaky").lazy()),
            ),
            section(
                "region-offline",
                "A region that can be cut off",
                Card::titled("Offline").body(
                    Region::new("r-offline")
                        .src("/pages/frag/offline")
                        .lazy()
                        .when_offline(),
                ),
            ),
            section(
                "region-poll500",
                "A polled region whose server keeps failing",
                Card::titled("Poll 500").body(
                    Region::new("r-poll500")
                        .src("/pages/frag/poll500")
                        .poll(Duration::from_millis(300))
                        .children(Text::new("Waiting.")),
                ),
            ),
            section(
                "region-poll429",
                "A polled region that is asked to wait",
                Card::titled("Poll 429").body(
                    Region::new("r-poll429")
                        .src("/pages/frag/poll429")
                        .poll(Duration::from_millis(300))
                        .children(Text::new("Waiting.")),
                ),
            ),
            section(
                "region-door",
                "A polled region that is shut once, then opens",
                Card::titled("Door").body(
                    Region::new("r-door")
                        .src("/pages/frag/door")
                        .poll(Duration::from_millis(300))
                        .children(Text::new("Waiting.")),
                ),
            ),
            section(
                "region-feed403",
                "A polled feed that gets a 403",
                Card::titled("Feed").body(
                    Region::new("r-feed403")
                        .src("/pages/frag/feed403")
                        .poll(Duration::from_millis(300))
                        .feed("feed-rows", "0", None)
                        .children(
                            el("ul")
                                .attr("id", "feed-rows")
                                .child(el("li").child("A row")),
                        ),
                ),
            ),
            section(
                "region-http",
                "Two regions that get an HTTP error that is not a state",
                Grid::cols(2).gap(4).children((
                    Card::titled("Offline wording").body(
                        Region::new("r-http-offline")
                            .src("/pages/frag/json500")
                            .lazy()
                            .when_offline(),
                    ),
                    Card::titled("Any failure").body(
                        Region::new("r-http-any")
                            .src("/pages/frag/json500")
                            .lazy()
                            .when_failed(State::of(StateKind::Failed)),
                    ),
                )),
            ),
            section(
                "region-ok",
                "A region that works",
                Card::titled("Fine").body(Region::new("r-ok").src("/pages/frag/ok").lazy()),
            ),
        )),
    )
}

fn actions(shell: &AppShell) -> AppPage {
    let form = |code: u16| {
        el("form")
            .attr("method", "post")
            .attr("action", format!("/pages/act/{code}"))
            .attr("id", format!("act-{code}"))
            .flag("data-ui-form", true)
            .child(
                Row::gap(2).children((
                    Input::text("note")
                        .value("typed text")
                        .attr("aria-label", format!("Note {code}")),
                    Button::new(format!("Post {code}")).kind(ButtonKind::Submit),
                )),
            )
    };
    shell.page("Actions").body(section(
        "action-cases",
        "Posts that fail",
        Stack::gap(3).children(
            [401, 403, 404, 409, 419, 422, 429, 500, 503]
                .map(form)
                .into_iter()
                .collect::<Vec<_>>(),
        ),
    ))
}

#[derive(Deserialize)]
struct SignInQuery {
    next: Option<String>,
}

#[derive(Deserialize)]
struct SignInForm {
    name: String,
    next: Option<String>,
}

fn sign_in(shell: &AppShell, next: Option<&str>, ended: bool) -> AppPage {
    let kept = next.and_then(|raw| ReturnTo::parse_for(raw, "/sign-in"));
    let form = el("form")
        .attr("method", "post")
        .attr("action", "/sign-in")
        .attr("id", "sign-in")
        .child(
            Stack::gap(3).children((
                ended.then(SessionEnded::new),
                Field::new("name", "Name").control(Input::text("name").value("Anu").required()),
                el("input")
                    .attr("type", "hidden")
                    .attr("name", "next")
                    .attr(
                        "value",
                        kept.as_ref().map(ReturnTo::as_str).unwrap_or("").to_owned(),
                    ),
                Button::primary("Sign in").kind(ButtonKind::Submit),
            )),
        );
    shell
        .page("Sign in")
        .body(Card::titled("Sign in").body(form))
}

fn page(path: &'static str, shell: &AppShell) -> AppPage {
    shell
        .page("A page")
        .body(Text::new(format!("You are on {path}.")))
}

pub struct StatePages;

impl Injectable for StatePages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

fn fragment(inner: impl Into<Node>) -> Response {
    View::new("Fragment").body(inner).into_response()
}

fn cookie(value: &str, max_age: i64) -> String {
    format!("{COOKIE}={value}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax")
}

impl Controller for StatePages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/pages", |shell: AppShell| async move {
                gallery(&shell)
            })
            .public()
            .page(),
            RouteDef::on(
                Method::GET,
                "!/pages/regions",
                |shell: AppShell| async move { regions(&shell) },
            )
            .public()
            .page(),
            RouteDef::on(
                Method::GET,
                "!/pages/actions",
                |shell: AppShell| async move { actions(&shell) },
            )
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/pages/rows", |shell: AppShell| async move {
                shell.page("Rows").body(table())
            })
            .public()
            .page(),
            RouteDef::on(
                Method::GET,
                "!/pages/status/{code}",
                |Path(code): Path<u16>| async move { Err::<AppPage, _>(failure(known(code)?)) },
            )
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/pages/user", |shell: AppShell| async move {
                page("/pages/user", &shell)
            })
            .authenticated()
            .page(),
            RouteDef::on(Method::GET, "!/pages/leak", || async {
                Err::<AppPage, _>(AppError::internal(SECRET).with_detail(SECRET))
            })
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/pages/panic", || async {
                if std::hint::black_box(true) {
                    panic!("{SECRET}");
                }
                View::new("never")
            })
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/pages/data", || async {
                Err::<Json<u8>, _>(AppError::forbidden())
            })
            .public()
            .json_only(),
            RouteDef::on(Method::GET, "!/pages/frag/ok", || async {
                hit("ok");
                fragment(Alert::new(AlertTone::Success).title("This region loaded."))
            })
            .public(),
            RouteDef::on(Method::GET, "!/pages/frag/poll500", || async {
                hit("poll500");
                Err::<Response, _>(AppError::internal(SECRET))
            })
            .public(),
            RouteDef::on(Method::GET, "!/pages/frag/poll429", || async {
                hit("poll429");
                Err::<Response, _>(AppError::too_many_requests(Duration::from_secs(4)))
            })
            .public(),
            RouteDef::on(Method::GET, "!/pages/frag/door", || async {
                hit("door");
                if DOOR.fetch_add(1, Ordering::SeqCst) < 1 {
                    return Err(AppError::forbidden());
                }
                Ok(fragment(
                    Region::new("r-door")
                        .src("/pages/frag/door")
                        .poll(Duration::from_millis(300))
                        .children(Alert::new(AlertTone::Success).title("The door is open.")),
                ))
            })
            .public(),
            RouteDef::on(Method::GET, "!/pages/frag/feed403", || async {
                hit("feed403");
                Err::<Response, _>(AppError::forbidden())
            })
            .public(),
            RouteDef::on(Method::GET, "!/pages/frag/json500", || async {
                hit("json500");
                Err::<Response, _>(AppError::internal(SECRET))
            })
            .public()
            .json_only(),
            RouteDef::on(Method::GET, "!/pages/frag/offline", || async {
                hit("offline");
                fragment(Alert::new(AlertTone::Success).title("This region is back."))
            })
            .public(),
            RouteDef::on(Method::GET, "!/pages/frag/flaky", || async {
                hit("flaky");
                if FLAKY.fetch_add(1, Ordering::SeqCst) < 2 {
                    return Err(AppError::service_unavailable("Not just yet."));
                }
                Ok(fragment(
                    Alert::new(AlertTone::Success).title("It worked on the third try."),
                ))
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/pages/frag/{code}",
                |Path(code): Path<u16>, RawQuery(_): RawQuery| async move {
                    hit(&code.to_string());
                    Err::<Response, _>(failure(known(code)?))
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/pages/act/{code}",
                |Path(code): Path<u16>| async move { Err::<Reply, _>(failure(known(code)?)) },
            )
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/pages/hits", || async {
                Json(HITS.lock().unwrap_or_else(PoisonError::into_inner).clone())
            })
            .public(),
            RouteDef::on(Method::POST, "!/pages/reset", || async {
                HITS.lock().unwrap_or_else(PoisonError::into_inner).clear();
                FLAKY.store(0, Ordering::SeqCst);
                DOOR.store(0, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/sign-in",
                |shell: AppShell, Query(query): Query<SignInQuery>| async move {
                    let ended = query.next.is_some();
                    sign_in(&shell, query.next.as_deref(), ended)
                },
            )
            .public()
            .page(),
            RouteDef::on(
                Method::POST,
                "!/sign-in",
                |Form(form): Form<SignInForm>| async move {
                    let name: String = form
                        .name
                        .chars()
                        .filter(char::is_ascii_alphanumeric)
                        .take(16)
                        .collect();
                    let mut reply = Reply::redirect(ReturnTo::resolve_for(
                        form.next.as_deref(),
                        "/sign-in",
                        "/",
                    ))
                    .into_response();
                    if let Ok(value) = http::HeaderValue::from_str(&cookie(&name, 3600)) {
                        reply.headers_mut().append(http::header::SET_COOKIE, value);
                    }
                    reply
                },
            )
            .public()
            .page(),
        ]
    }
}

pub use StatePages as Pages;
